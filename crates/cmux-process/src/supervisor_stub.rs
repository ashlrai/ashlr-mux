//! Non-Windows stub supervisor.
//!
//! The Job-Object supervisor is Windows-only (M3 is the Windows port). On other
//! targets every operation returns [`ProcessError::Unsupported`] so the
//! workspace still compiles and the contract types stay exercised on macOS/Linux
//! CI. The pure `ProcessTerminationGate` is tested on all platforms regardless.

use crate::{
    AgentIo, ProcessError, ProcessSupervisor, SessionHandle, SessionId, SpawnSpec, TerminateMode,
};

/// Stub supervisor for non-Windows builds.
#[derive(Debug, Default)]
pub struct JobObjectSupervisor;

impl JobObjectSupervisor {
    /// Create a stub supervisor.
    pub fn new() -> Self {
        Self
    }

    /// No-op on non-Windows (no session is ever created).
    pub fn reap(&self, _id: SessionId) {}
}

impl ProcessSupervisor for JobObjectSupervisor {
    fn spawn(&self, _spec: SpawnSpec) -> Result<SessionHandle, ProcessError> {
        Err(ProcessError::Unsupported)
    }

    fn spawn_captured(
        &self,
        _spec: SpawnSpec,
    ) -> Result<(SessionHandle, AgentIo), ProcessError> {
        Err(ProcessError::Unsupported)
    }

    fn terminate(&self, _id: SessionId, _mode: TerminateMode) -> Result<(), ProcessError> {
        Err(ProcessError::Unsupported)
    }
}
