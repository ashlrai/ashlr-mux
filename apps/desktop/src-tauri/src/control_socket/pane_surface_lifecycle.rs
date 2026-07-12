use cmux_core::session::AppSessionSnapshot;
use cmux_ipc::ControlCallResult;
use serde_json::{Map, Value};

#[derive(Debug, Clone, PartialEq)]
pub(super) struct LifecycleEvent {
    pub name: &'static str,
    pub payload: Value,
}

#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)] // Variants are introduced together; dispatch branches consume them incrementally.
pub(super) enum LifecycleEffect {
    TerminalCreate {
        surface_id: String,
        generation: u64,
        command: Option<String>,
        working_directory: Option<String>,
    },
    TerminalReplace {
        surface_id: String,
        previous_generation: u64,
        generation: u64,
        command: String,
        working_directory: Option<String>,
    },
    BrowserAttach {
        surface_id: String,
        generation: u64,
        url: Option<String>,
    },
    RuntimeTeardown {
        surface_id: String,
        generation: u64,
    },
    DockCreate {
        dock_surface_id: String,
        kind: String,
        url: Option<String>,
    },
    RemoteCreate {
        remote_session_id: String,
        kind: String,
    },
    ActivateWindow {
        window_id: String,
    },
    PersistSession,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct LifecycleDispatchContext {
    pub viewport_size: Option<(f64, f64)>,
    pub browser_enabled: bool,
    pub dock_available: bool,
}

#[derive(Debug)]
pub(super) struct LifecycleTransition {
    pub snapshot: AppSessionSnapshot,
    pub result: ControlCallResult,
    pub changed: bool,
    pub events: Vec<LifecycleEvent>,
    pub effects: Vec<LifecycleEffect>,
}

pub(super) trait LifecycleEffectExecutor {
    type Error;

    /// Validate and acquire all resources needed by an effect without making
    /// it externally visible. The transition is committed only after every
    /// effect has staged successfully.
    fn stage(&mut self, effect: &LifecycleEffect) -> Result<(), Self::Error>;
    fn commit_staged(&mut self) -> Result<(), Self::Error>;
    fn rollback_staged(&mut self);
}

pub(super) fn commit_lifecycle_transition<E: LifecycleEffectExecutor>(
    target: &mut AppSessionSnapshot,
    transition: LifecycleTransition,
    executor: &mut E,
) -> Result<ControlCallResult, E::Error> {
    for effect in &transition.effects {
        if let Err(error) = executor.stage(effect) {
            executor.rollback_staged();
            return Err(error);
        }
    }
    if let Err(error) = executor.commit_staged() {
        executor.rollback_staged();
        return Err(error);
    }
    if transition.changed {
        *target = transition.snapshot;
    }
    Ok(transition.result)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct RuntimeArrival {
    pub window_id: String,
    pub workspace_id: String,
    pub pane_id: String,
    pub surface_id: String,
    pub remote_session_id: String,
    pub generation: u64,
}

impl RuntimeArrival {
    pub fn remote(
        window_id: impl Into<String>,
        workspace_id: impl Into<String>,
        pane_id: impl Into<String>,
        surface_id: impl Into<String>,
        remote_session_id: impl Into<String>,
        generation: u64,
    ) -> Self {
        Self {
            window_id: window_id.into(),
            workspace_id: workspace_id.into(),
            pane_id: pane_id.into(),
            surface_id: surface_id.into(),
            remote_session_id: remote_session_id.into(),
            generation,
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct RuntimeReconciliation {
    pub snapshot: AppSessionSnapshot,
}

impl RuntimeReconciliation {
    pub fn directory_apply_count(&self, _surface_id: &str) -> usize {
        0
    }
}

pub(super) fn reconcile_runtime_arrival(
    snapshot: &AppSessionSnapshot,
    _arrival: RuntimeArrival,
) -> RuntimeReconciliation {
    RuntimeReconciliation {
        snapshot: snapshot.clone(),
    }
}

pub(super) fn dispatch_lifecycle_request(
    snapshot: &AppSessionSnapshot,
    _method: &str,
    _params: &Map<String, Value>,
    _context: &LifecycleDispatchContext,
) -> LifecycleTransition {
    LifecycleTransition {
        snapshot: snapshot.clone(),
        result: ControlCallResult::Err {
            code: "internal_error".into(),
            message: "Lifecycle dispatcher is not implemented".into(),
            data: None,
        },
        changed: false,
        events: Vec::new(),
        effects: Vec::new(),
    }
}
