//! Cancel-before-launch coordination.
//!
//! Port of the macOS `ProcessTerminationGate` (`Sources/AgentForkSupport.swift:9-36`).
//! A kill can be requested before the child actually exists — between the
//! supervisor deciding to spawn and the OS returning a live handle. On macOS
//! Foundation raises if termination APIs touch a `Process` before launch; on
//! Windows there is simply no handle to `TerminateJobObject` yet. Either way the
//! intent must be remembered and honored the instant the child appears.
//!
//! The gate is a tiny lock-guarded three-flag state machine, identical in shape
//! to the Swift original so the race semantics match exactly. It is pure and
//! OS-independent, so it is unit-tested on every platform.

use std::sync::Mutex;

#[derive(Debug, Default)]
struct GateState {
    did_launch: bool,
    did_finish: bool,
    termination_requested: bool,
}

/// Coordinates a termination request against an in-flight spawn.
///
/// Usage mirrors the Swift gate:
/// * the spawn path calls [`mark_launched`](Self::mark_launched) once the child
///   handle exists; if it returns `true`, termination was already requested and
///   the caller must kill immediately.
/// * a concurrent killer calls [`request_termination`](Self::request_termination);
///   if it returns `true`, the child already launched and the caller may kill
///   now, otherwise the intent is latched for the spawn path to honor.
/// * the supervisor calls [`mark_finished`](Self::mark_finished) on exit so a
///   late request is dropped rather than acting on a dead handle.
#[derive(Debug, Default)]
pub struct ProcessTerminationGate {
    state: Mutex<GateState>,
}

impl ProcessTerminationGate {
    /// Create a gate in the initial (not launched, not finished, no request)
    /// state.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record that termination was requested. Returns `true` if the child has
    /// already launched (so the caller should terminate now); `false` if the
    /// child does not exist yet (the request is latched for `mark_launched`) or
    /// the process has already finished.
    pub fn request_termination(&self) -> bool {
        let mut state = self.state.lock().expect("gate mutex poisoned");
        if state.did_finish {
            return false;
        }
        state.termination_requested = true;
        state.did_launch
    }

    /// Record that the child has launched. Returns `true` if termination was
    /// already requested while the spawn was in flight (so the caller must kill
    /// the freshly-launched child immediately); `false` otherwise or if already
    /// finished.
    pub fn mark_launched(&self) -> bool {
        let mut state = self.state.lock().expect("gate mutex poisoned");
        if state.did_finish {
            return false;
        }
        state.did_launch = true;
        state.termination_requested
    }

    /// Record that the process has finished. After this, `request_termination`
    /// and `mark_launched` are no-ops returning `false`.
    pub fn mark_finished(&self) {
        let mut state = self.state.lock().expect("gate mutex poisoned");
        state.did_finish = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_before_launch_latches_and_launch_observes_it() {
        let gate = ProcessTerminationGate::new();
        // Killer arrives first: child does not exist yet.
        assert!(!gate.request_termination(), "no handle yet → cannot kill now");
        // Spawn completes: it must see the latched request and kill.
        assert!(gate.mark_launched(), "launch must observe the pending request");
    }

    #[test]
    fn request_after_launch_returns_true_immediately() {
        let gate = ProcessTerminationGate::new();
        assert!(!gate.mark_launched(), "no request pending at launch");
        assert!(
            gate.request_termination(),
            "child is live → caller may kill now"
        );
    }

    #[test]
    fn requests_after_finish_are_ignored() {
        let gate = ProcessTerminationGate::new();
        gate.mark_finished();
        assert!(!gate.request_termination(), "finished → nothing to kill");
        assert!(!gate.mark_launched(), "finished → launch is moot");
    }

    #[test]
    fn launch_after_finish_does_not_resurrect_request() {
        let gate = ProcessTerminationGate::new();
        assert!(!gate.request_termination());
        gate.mark_finished();
        // A spurious late launch must not report the stale request.
        assert!(!gate.mark_launched());
    }
}
