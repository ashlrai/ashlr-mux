//! Process supervisor for the cmux Windows port (M3 WS1).
//!
//! Owns the keystroke-to-agent process boundary: spawn a child, confine its
//! entire descendant tree to one Win32 **Job Object**, and tear the whole tree
//! down on demand with **zero orphans**. Windows has neither POSIX process
//! groups for lifecycle nor parent-death propagation, so a per-session Job
//! Object with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` is the only reliable
//! analogue (M3 §"Key technical decisions").
//!
//! ## Layering
//!
//! This crate is deliberately agnostic about *what* it launches — it takes a
//! [`SpawnSpec`] (program + args + env + cwd), not an agent launch plan. The
//! agent-specific resolution/env policy lives in `cmux-agent`; a higher layer
//! converts an `AgentSessionLaunchPlan` into a [`SpawnSpec`]. This keeps the
//! supervisor reusable for plain shells (M2/M4) too.
//!
//! ## Portability
//!
//! The [`ProcessTerminationGate`] and all contract types build on every OS and
//! are unit-tested there. The Job-Object supervisor itself is `#[cfg(windows)]`;
//! on other targets [`JobObjectSupervisor`] is a stub whose methods return
//! [`ProcessError::Unsupported`], so the workspace still compiles on macOS/Linux
//! CI.

use std::{collections::BTreeMap, path::PathBuf};

use uuid::Uuid;

mod gate;
pub use gate::ProcessTerminationGate;

#[cfg(windows)]
#[path = "supervisor_windows.rs"]
mod supervisor;
#[cfg(not(windows))]
#[path = "supervisor_stub.rs"]
mod supervisor;

pub use supervisor::JobObjectSupervisor;

/// Stable per-session identifier. Wraps a UUID so it is cheap to copy, hash, and
/// serialize, and never collides across relaunches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SessionId(pub Uuid);

impl SessionId {
    /// Mint a fresh random session id.
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for SessionId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for SessionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// How forcefully to take down a session, mirroring the macOS interrupt /
/// terminate / SIGKILL ladder (`AgentForkSupport.swift:225-247`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminateMode {
    /// Best-effort cooperative interrupt — `GenerateConsoleCtrlEvent`
    /// (Ctrl-Break) to the process group. The agent observes it like Ctrl-C.
    Interrupt,
    /// Graceful: signal the group, wait a bounded grace window for clean exit,
    /// then force-kill any survivors via the job.
    Graceful,
    /// Immediate whole-tree kill via `TerminateJobObject`, no grace period.
    Force,
}

/// What to launch. Program + args + a full replacement environment + optional
/// working directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpawnSpec {
    /// Absolute path to the executable to run.
    pub program: PathBuf,
    /// Arguments, excluding `argv[0]`.
    pub args: Vec<String>,
    /// The complete child environment (the supervisor does not inherit the
    /// parent block — the caller assembles the curated env, e.g. via
    /// `cmux-agent`'s `selected_environment`).
    pub env: BTreeMap<String, String>,
    /// Working directory for the child; inherits the supervisor's cwd if `None`.
    pub current_dir: Option<PathBuf>,
    /// When `true`, the session's job is created *without*
    /// `KILL_ON_JOB_CLOSE` so it survives the launching connection (M4 daemon
    /// sessions, M3 cross-cutting rule 6). Default `false`: tree dies with the
    /// supervisor.
    pub survive_disconnect: bool,
}

impl SpawnSpec {
    /// Construct a spec for `program` with no args, empty env, inherited cwd,
    /// and tree-dies-with-supervisor semantics.
    pub fn new(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            env: BTreeMap::new(),
            current_dir: None,
            survive_disconnect: false,
        }
    }

    /// Builder: set the argument vector.
    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.args = args.into_iter().map(Into::into).collect();
        self
    }

    /// Builder: set the full child environment.
    pub fn env(mut self, env: BTreeMap<String, String>) -> Self {
        self.env = env;
        self
    }

    /// Builder: set the working directory.
    pub fn current_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.current_dir = Some(dir.into());
        self
    }
}

/// A live (or recently-live) supervised session.
///
/// The OS job handle is owned internally by the supervisor; this handle exposes
/// only the cross-platform identity needed by callers and the orphan-recovery
/// ledger (WS4). `root_pid` plus a creation timestamp defeat PID reuse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionHandle {
    /// The session identity.
    pub id: SessionId,
    /// PID of the root child process the supervisor spawned.
    pub root_pid: u32,
}

/// Errors from the supervisor.
#[derive(Debug, thiserror::Error)]
pub enum ProcessError {
    /// The child could not be spawned (resolution succeeded but
    /// `CreateProcess` failed, bad path, etc.).
    #[error("failed to spawn {program}: {source}")]
    Spawn {
        /// The program that failed to launch.
        program: String,
        /// Underlying OS error.
        source: std::io::Error,
    },
    /// A Win32 call failed; carries the API name and the OS error.
    #[error("win32 {api} failed: {source}")]
    Os {
        /// The Win32 API that failed.
        api: &'static str,
        /// Underlying OS error.
        source: std::io::Error,
    },
    /// No session with this id is known to the supervisor.
    #[error("unknown session {0}")]
    UnknownSession(SessionId),
    /// The operation is not supported on this platform (non-Windows build).
    #[error("process supervision is only implemented on Windows")]
    Unsupported,
}

/// The supervisor contract (M3 §"Interfaces & contracts produced").
pub trait ProcessSupervisor {
    /// Spawn `spec` into a fresh per-session Job Object and return its handle.
    /// The child is confined to the job *before* it can spawn grandchildren.
    fn spawn(&self, spec: SpawnSpec) -> Result<SessionHandle, ProcessError>;

    /// Terminate the session's entire descendant tree with the given force.
    /// Idempotent: terminating an already-dead session is `Ok(())`.
    fn terminate(&self, id: SessionId, mode: TerminateMode) -> Result<(), ProcessError>;

    /// Deliver a cooperative interrupt (Ctrl-Break) to the session's process
    /// group without tearing it down. Equivalent to
    /// `terminate(id, TerminateMode::Interrupt)`.
    fn interrupt(&self, id: SessionId) -> Result<(), ProcessError> {
        self.terminate(id, TerminateMode::Interrupt)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_ids_are_unique() {
        assert_ne!(SessionId::new(), SessionId::new());
    }

    #[test]
    fn spawn_spec_builder_sets_fields() {
        let spec = SpawnSpec::new("C:/bin/codex.exe")
            .args(["app-server", "--listen", "stdio://"])
            .current_dir("C:/work");
        assert_eq!(spec.program, PathBuf::from("C:/bin/codex.exe"));
        assert_eq!(spec.args, vec!["app-server", "--listen", "stdio://"]);
        assert_eq!(spec.current_dir, Some(PathBuf::from("C:/work")));
        assert!(!spec.survive_disconnect);
    }
}
