//! Windows Job-Object process supervisor (M3 WS1).
//!
//! Each session owns one Job Object created with
//! `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`. The child is spawned
//! `CREATE_SUSPENDED` so it cannot fork a grandchild before we
//! `AssignProcessToJobObject`; only then do we `ResumeThread`. This closes the
//! assign-before-spawn race the milestone calls out, and — because child
//! processes automatically inherit their parent's job — guarantees the *entire*
//! descendant tree is confined. `TerminateJobObject` (or dropping the job
//! handle, via `KILL_ON_JOB_CLOSE`) then tears the whole tree down with zero
//! orphans.
//!
//! Raw `HANDLE`s are stored as `isize` in the session table so the supervisor is
//! `Send + Sync` without wrapping every handle; they are reconstituted at the
//! Win32 call sites.

use std::{
    collections::{BTreeMap, HashMap},
    ffi::c_void,
    sync::Mutex,
};

use windows::{
    core::{PCWSTR, PWSTR},
    Win32::{
        Foundation::{CloseHandle, HANDLE},
        System::{
            Console::{GenerateConsoleCtrlEvent, CTRL_BREAK_EVENT},
            JobObjects::{
                AssignProcessToJobObject, CreateJobObjectW, QueryInformationJobObject,
                SetInformationJobObject, TerminateJobObject,
                JobObjectBasicAccountingInformation, JobObjectExtendedLimitInformation,
                JOBOBJECT_BASIC_ACCOUNTING_INFORMATION, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
                JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            },
            Threading::{
                CreateProcessW, ResumeThread, WaitForSingleObject, CREATE_NEW_PROCESS_GROUP,
                CREATE_NO_WINDOW, CREATE_SUSPENDED, CREATE_UNICODE_ENVIRONMENT,
                PROCESS_INFORMATION, STARTUPINFOW,
            },
        },
    },
};

use crate::{ProcessError, ProcessSupervisor, SessionHandle, SessionId, SpawnSpec, TerminateMode};

/// Grace window (ms) before a `Graceful` terminate escalates to a forced
/// `TerminateJobObject`. Mirrors the macOS two-stage `terminate()`→`SIGKILL`
/// teardown (`AgentForkSupport.swift:225-247`).
const GRACEFUL_TERMINATE_GRACE_MS: u32 = 500;

struct Session {
    /// `HANDLE.0 as isize` for the session's Job Object.
    job: isize,
    /// `HANDLE.0 as isize` for the root child process.
    process: isize,
    /// PID of the root child (also the process-group id, since the child is
    /// spawned `CREATE_NEW_PROCESS_GROUP`).
    root_pid: u32,
    /// Whether this job omits `KILL_ON_JOB_CLOSE` (M4 daemon survive-disconnect).
    survive_disconnect: bool,
}

/// Win32 Job-Object supervisor.
#[derive(Default)]
pub struct JobObjectSupervisor {
    sessions: Mutex<HashMap<SessionId, Session>>,
}

impl JobObjectSupervisor {
    /// Create an empty supervisor.
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of live processes currently confined to the session's job. Used
    /// by the no-orphan integration test as the authoritative oracle: after a
    /// terminate this must drop to zero.
    pub fn active_process_count(&self, id: SessionId) -> Result<u32, ProcessError> {
        let job = {
            let sessions = self.sessions.lock().expect("sessions mutex poisoned");
            sessions
                .get(&id)
                .map(|session| handle(session.job))
                .ok_or(ProcessError::UnknownSession(id))?
        };
        let mut accounting = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
        unsafe {
            QueryInformationJobObject(
                Some(job),
                JobObjectBasicAccountingInformation,
                &mut accounting as *mut _ as *mut c_void,
                std::mem::size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
                None,
            )
            .map_err(|_| os_error("QueryInformationJobObject"))?;
        }
        Ok(accounting.ActiveProcesses)
    }
}

impl ProcessSupervisor for JobObjectSupervisor {
    fn spawn(&self, spec: SpawnSpec) -> Result<SessionHandle, ProcessError> {
        let id = SessionId::new();

        // 1. Create the per-session NAMED job and arm kill-on-close (unless this
        //    is a survive-disconnect daemon session). The name is derived from
        //    the session id so the orphan sweep can reopen it after a crash.
        let job_name = to_wide(&crate::job_object_name(id));
        let job = unsafe { CreateJobObjectW(None, PCWSTR(job_name.as_ptr())) }
            .map_err(|_| os_error("CreateJobObjectW"))?;
        if !spec.survive_disconnect {
            if let Err(error) = arm_kill_on_job_close(job) {
                unsafe { close(job) };
                return Err(error);
            }
        }

        // 2. Spawn the child SUSPENDED so it cannot fork before we confine it.
        let mut command_line = build_command_line(&spec);
        let program_wide = to_wide(&spec.program.to_string_lossy());
        let current_dir_wide = spec
            .current_dir
            .as_ref()
            .map(|dir| to_wide(&dir.to_string_lossy()));
        let mut env_block = environment_block(&spec.env);

        let mut creation_flags =
            CREATE_SUSPENDED | CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW;
        let environment_ptr = match env_block.as_mut() {
            Some(block) => {
                creation_flags |= CREATE_UNICODE_ENVIRONMENT;
                Some(block.as_ptr() as *const c_void)
            }
            None => None,
        };

        let startup_info = STARTUPINFOW {
            cb: std::mem::size_of::<STARTUPINFOW>() as u32,
            ..Default::default()
        };
        let mut process_info = PROCESS_INFORMATION::default();

        let spawn_result = unsafe {
            CreateProcessW(
                PCWSTR(program_wide.as_ptr()),
                Some(PWSTR(command_line.as_mut_ptr())),
                None,
                None,
                false,
                creation_flags,
                environment_ptr,
                current_dir_wide
                    .as_ref()
                    .map_or(PCWSTR::null(), |dir| PCWSTR(dir.as_ptr())),
                &startup_info,
                &mut process_info,
            )
        };
        if spawn_result.is_err() {
            unsafe { close(job) };
            return Err(ProcessError::Spawn {
                program: spec.program.to_string_lossy().into_owned(),
                source: std::io::Error::last_os_error(),
            });
        }

        // 3. Confine BEFORE resuming, then resume.
        let assign = unsafe { AssignProcessToJobObject(job, process_info.hProcess) };
        if assign.is_err() {
            // The child is suspended and unconfined; kill it directly and bail.
            let error = os_error("AssignProcessToJobObject");
            unsafe {
                let _ = TerminateJobObject(job, 1);
                terminate_loose_process(process_info.hProcess);
                close(process_info.hThread);
                close(process_info.hProcess);
                close(job);
            }
            return Err(error);
        }

        let resume = unsafe { ResumeThread(process_info.hThread) };
        // ResumeThread returns u32::MAX (-1) on failure.
        if resume == u32::MAX {
            let error = os_error("ResumeThread");
            unsafe {
                let _ = TerminateJobObject(job, 1);
                close(process_info.hThread);
                close(process_info.hProcess);
                close(job);
            }
            return Err(error);
        }

        // The thread handle is no longer needed once the process runs.
        unsafe { close(process_info.hThread) };

        let root_pid = process_info.dwProcessId;
        self.sessions.lock().expect("sessions mutex poisoned").insert(
            id,
            Session {
                job: job.0 as isize,
                process: process_info.hProcess.0 as isize,
                root_pid,
                survive_disconnect: spec.survive_disconnect,
            },
        );

        Ok(SessionHandle { id, root_pid })
    }

    fn terminate(&self, id: SessionId, mode: TerminateMode) -> Result<(), ProcessError> {
        let (job, process, root_pid) = {
            let sessions = self.sessions.lock().expect("sessions mutex poisoned");
            match sessions.get(&id) {
                Some(session) => (handle(session.job), handle(session.process), session.root_pid),
                None => return Err(ProcessError::UnknownSession(id)),
            }
        };

        match mode {
            TerminateMode::Interrupt => {
                // Best-effort cooperative interrupt; leave the tree running.
                unsafe {
                    let _ = GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, root_pid);
                }
                return Ok(());
            }
            TerminateMode::Graceful => {
                unsafe {
                    let _ = GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, root_pid);
                    // Bounded grace window for the root to exit cleanly.
                    let _ = WaitForSingleObject(process, GRACEFUL_TERMINATE_GRACE_MS);
                }
                // Fall through to force-kill any survivors.
            }
            TerminateMode::Force => {}
        }

        unsafe {
            TerminateJobObject(job, 1).map_err(|_| os_error("TerminateJobObject"))?;
        }
        Ok(())
    }
}

impl Drop for JobObjectSupervisor {
    fn drop(&mut self) {
        let mut sessions = self.sessions.lock().expect("sessions mutex poisoned");
        for (_, session) in sessions.drain() {
            unsafe {
                // Closing the job handle triggers KILL_ON_JOB_CLOSE for normal
                // sessions, tearing down the tree; survive-disconnect sessions
                // are merely detached.
                let _ = WaitForSingleObject(handle(session.process), 0);
                let _ = session.survive_disconnect; // documented intent; close handles either way
                close(handle(session.process));
                close(handle(session.job));
            }
        }
    }
}

/// Reconstruct a `HANDLE` from a stored `isize`.
fn handle(value: isize) -> HANDLE {
    HANDLE(value as *mut c_void)
}

/// Close a handle, ignoring failure (best-effort cleanup).
unsafe fn close(value: HANDLE) {
    let _ = CloseHandle(value);
}

/// Build a `ProcessError::Os` from the current thread's last OS error.
fn os_error(api: &'static str) -> ProcessError {
    ProcessError::Os {
        api,
        source: std::io::Error::last_os_error(),
    }
}

/// Set `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` on the job's extended limits.
fn arm_kill_on_job_close(job: HANDLE) -> Result<(), ProcessError> {
    let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    unsafe {
        SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            &info as *const _ as *const c_void,
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )
        .map_err(|_| os_error("SetInformationJobObject"))
    }
}

/// Force-kill a process that escaped confinement, ignoring failure.
unsafe fn terminate_loose_process(process: HANDLE) {
    use windows::Win32::System::Threading::TerminateProcess;
    let _ = TerminateProcess(process, 1);
}

/// Encode a `&str` as a null-terminated UTF-16 buffer.
fn to_wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Build the mutable, null-terminated UTF-16 command line: the (quoted) program
/// path as `argv[0]` followed by each (quoted) argument. `CreateProcessW`
/// requires a writable buffer for `lpCommandLine`.
fn build_command_line(spec: &SpawnSpec) -> Vec<u16> {
    let mut line = String::new();
    append_quoted(&spec.program.to_string_lossy(), &mut line);
    for arg in &spec.args {
        line.push(' ');
        append_quoted(arg, &mut line);
    }
    to_wide(&line)
}

/// Append `arg` to `out`, quoted per the `CommandLineToArgvW` rules (the same
/// escaping the MSVC CRT uses) so the child parses back the exact argument.
fn append_quoted(arg: &str, out: &mut String) {
    let needs_quotes = arg.is_empty()
        || arg
            .chars()
            .any(|c| c == ' ' || c == '\t' || c == '\n' || c == '\u{0B}' || c == '"');
    if !needs_quotes {
        out.push_str(arg);
        return;
    }

    out.push('"');
    let mut backslashes = 0usize;
    for c in arg.chars() {
        match c {
            '\\' => backslashes += 1,
            '"' => {
                // Escape all pending backslashes (doubled) plus the quote.
                for _ in 0..(backslashes * 2 + 1) {
                    out.push('\\');
                }
                out.push('"');
                backslashes = 0;
            }
            other => {
                for _ in 0..backslashes {
                    out.push('\\');
                }
                backslashes = 0;
                out.push(other);
            }
        }
    }
    // Trailing backslashes precede the closing quote, so double them.
    for _ in 0..(backslashes * 2) {
        out.push('\\');
    }
    out.push('"');
}

/// Build a `CREATE_UNICODE_ENVIRONMENT` block: `KEY=VALUE\0` entries, double-null
/// terminated. Returns `None` when `env` is empty so the child inherits the
/// supervisor's environment block (an empty block would give the child an empty
/// environment, breaking even `cmd`).
fn environment_block(env: &BTreeMap<String, String>) -> Option<Vec<u16>> {
    if env.is_empty() {
        return None;
    }
    let mut block: Vec<u16> = Vec::new();
    for (key, value) in env {
        block.extend(key.encode_utf16());
        block.push(u16::from(b'='));
        block.extend(value.encode_utf16());
        block.push(0);
    }
    block.push(0);
    Some(block)
}

const _: () = {
    // The supervisor must be safe to share across threads (handles stored as
    // isize, table behind a Mutex). Assert it at compile time.
    fn assert_send_sync<T: Send + Sync>() {}
    let _ = assert_send_sync::<JobObjectSupervisor>;
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_argument_with_spaces() {
        let mut out = String::new();
        append_quoted(r"C:\Program Files\x.exe", &mut out);
        assert_eq!(out, r#""C:\Program Files\x.exe""#);
    }

    #[test]
    fn quotes_embedded_quote_and_trailing_backslashes() {
        let mut out = String::new();
        append_quoted(r#"a\\b" c\"#, &mut out);
        // The two backslashes before `b` do not precede a quote, so they pass
        // through unchanged; the backslash(es) immediately before the embedded
        // quote and before the closing quote are doubled, and the embedded
        // quote is escaped.
        assert_eq!(out, r#""a\\b\" c\\""#);
    }

    #[test]
    fn bare_argument_is_unquoted() {
        let mut out = String::new();
        append_quoted("--listen", &mut out);
        assert_eq!(out, "--listen");
    }

    #[test]
    fn environment_block_empty_means_inherit() {
        assert!(environment_block(&BTreeMap::new()).is_none());
    }

    #[test]
    fn environment_block_is_double_null_terminated() {
        let env = BTreeMap::from([("A".to_string(), "1".to_string())]);
        let block = environment_block(&env).expect("block");
        // "A=1\0\0"
        assert_eq!(block, vec![65, 61, 49, 0, 0]);
    }
}
