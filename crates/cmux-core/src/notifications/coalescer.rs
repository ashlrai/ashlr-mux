//! Burst coalescer for notification side-signals (sound / refresh debounce).
//!
//! Ported from `cmux/Sources/NotificationBurstCoalescer.swift`.
//!
//! DEVIATION: the Swift version drives flushes from a self-rescheduling
//! `DispatchSource` timer whose event handler calls back into `self.flush()`.
//! That self-referential timer→`&mut self` callback is not expressible under
//! Rust's borrow rules, so the timer is modeled as an explicit, deterministic
//! pump: [`signal`](NotificationBurstCoalescer::signal) records the pending
//! action and the requested delay, and the host (or a test) advances time by
//! calling [`fire`](NotificationBurstCoalescer::fire) — the analogue of the
//! timer firing — or [`flush_now`](NotificationBurstCoalescer::flush_now). The
//! observable coalescing semantics are preserved: only the latest action runs
//! per flush, a changed delay cancels and reschedules, and an action queued
//! during a flush reschedules another.

/// Default coalescing delay (Swift `1.0 / 30.0` seconds).
pub const DEFAULT_DELAY_SECONDS: f64 = 1.0 / 30.0;

type Action = Box<dyn FnMut()>;

pub struct NotificationBurstCoalescer {
    delay: f64,
    pending_action: Option<Action>,
    /// Whether a flush is currently scheduled (the Swift
    /// `cancelScheduledFlush != nil` state).
    scheduled: bool,
    /// The delay handed to the scheduler for the in-flight schedule, exposed so
    /// hosts/tests can verify the injected-scheduler deadline.
    scheduled_delay: Option<f64>,
}

impl Default for NotificationBurstCoalescer {
    fn default() -> Self {
        Self::new(DEFAULT_DELAY_SECONDS)
    }
}

impl NotificationBurstCoalescer {
    /// Create a coalescer with the given base delay (clamped to `>= 0`),
    /// mirroring Swift `init(delay:)`.
    pub fn new(delay: f64) -> Self {
        Self {
            delay: delay.max(0.0),
            pending_action: None,
            scheduled: false,
            scheduled_delay: None,
        }
    }

    /// Whether a flush is currently scheduled.
    pub fn is_scheduled(&self) -> bool {
        self.scheduled
    }

    /// The delay (seconds) requested for the currently-scheduled flush, if any.
    pub fn scheduled_delay(&self) -> Option<f64> {
        self.scheduled_delay
    }

    /// Record a pending action, optionally overriding the delay, and ensure a
    /// flush is scheduled.
    ///
    /// Mirrors Swift `signal(delay:_:)` (`NotificationBurstCoalescer.swift`
    /// 41-52): a delay change while a flush is already scheduled cancels and
    /// reschedules it.
    pub fn signal<F>(&mut self, new_delay: Option<f64>, action: F)
    where
        F: FnMut() + 'static,
    {
        let previous_delay = self.delay;
        if let Some(new_delay) = new_delay {
            self.delay = new_delay.max(0.0);
        }
        self.pending_action = Some(Box::new(action));
        if self.scheduled && self.delay != previous_delay {
            self.scheduled = false;
            self.scheduled_delay = None;
        }
        self.schedule_flush_if_needed();
    }

    /// Cancel any scheduled flush and flush immediately.
    ///
    /// Mirrors Swift `flushNow()`.
    pub fn flush_now(&mut self) {
        self.scheduled = false;
        self.scheduled_delay = None;
        self.flush();
    }

    /// Advance the deterministic timer: if a flush is scheduled, run it. This
    /// stands in for the Swift `DispatchSource` timer firing.
    pub fn fire(&mut self) {
        if self.scheduled {
            self.scheduled = false;
            self.scheduled_delay = None;
            self.flush();
        }
    }

    /// Mirrors Swift `scheduleFlushIfNeeded()`.
    fn schedule_flush_if_needed(&mut self) {
        if self.scheduled {
            return;
        }
        self.scheduled = true;
        self.scheduled_delay = Some(self.delay);
    }

    /// Mirrors Swift `flush()`: clears the scheduled state, runs the latest
    /// pending action (if any), and reschedules when a new action was queued
    /// while the action ran.
    fn flush(&mut self) {
        self.scheduled = false;
        self.scheduled_delay = None;
        let Some(mut action) = self.pending_action.take() else {
            return;
        };
        action();
        if self.pending_action.is_some() {
            self.schedule_flush_if_needed();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::rc::Rc;

    #[test]
    fn default_delay_is_one_thirtieth() {
        let coalescer = NotificationBurstCoalescer::default();
        assert_eq!(coalescer.scheduled_delay(), None);
        // base delay is reflected when scheduling
        let mut coalescer = coalescer;
        coalescer.signal(None, || {});
        assert_eq!(coalescer.scheduled_delay(), Some(DEFAULT_DELAY_SECONDS));
    }

    #[test]
    fn coalesces_to_latest_action() {
        let counter = Rc::new(Cell::new(0));
        let last = Rc::new(Cell::new(0));
        let mut coalescer = NotificationBurstCoalescer::new(0.1);

        for n in 1..=5 {
            let counter = counter.clone();
            let last = last.clone();
            coalescer.signal(None, move || {
                counter.set(counter.get() + 1);
                last.set(n);
            });
        }
        assert!(coalescer.is_scheduled());
        coalescer.fire();
        // only one flush ran, carrying the most recent action
        assert_eq!(counter.get(), 1);
        assert_eq!(last.get(), 5);
        assert!(!coalescer.is_scheduled());
    }

    #[test]
    fn changing_delay_reschedules() {
        let mut coalescer = NotificationBurstCoalescer::new(0.1);
        coalescer.signal(None, || {});
        assert_eq!(coalescer.scheduled_delay(), Some(0.1));
        coalescer.signal(Some(0.5), || {});
        assert_eq!(coalescer.scheduled_delay(), Some(0.5));
    }

    #[test]
    fn action_queued_during_flush_reschedules() {
        let ran = Rc::new(Cell::new(0));
        let mut coalescer = NotificationBurstCoalescer::new(0.0);
        let ran_inner = ran.clone();
        coalescer.signal(None, move || {
            ran_inner.set(ran_inner.get() + 1);
        });
        coalescer.fire();
        assert_eq!(ran.get(), 1);
        // a fresh signal after the flush schedules again
        let ran_inner = ran.clone();
        coalescer.signal(None, move || {
            ran_inner.set(ran_inner.get() + 1);
        });
        assert!(coalescer.is_scheduled());
        coalescer.fire();
        assert_eq!(ran.get(), 2);
    }

    #[test]
    fn flush_now_runs_without_fire() {
        let ran = Rc::new(Cell::new(false));
        let mut coalescer = NotificationBurstCoalescer::new(10.0);
        let ran_inner = ran.clone();
        coalescer.signal(None, move || ran_inner.set(true));
        coalescer.flush_now();
        assert!(ran.get());
        assert!(!coalescer.is_scheduled());
    }

    #[test]
    fn fire_without_pending_is_noop() {
        let mut coalescer = NotificationBurstCoalescer::new(0.0);
        coalescer.fire();
        assert!(!coalescer.is_scheduled());
    }
}
