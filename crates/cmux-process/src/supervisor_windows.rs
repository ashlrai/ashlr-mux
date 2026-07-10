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
//! The kill-on-close job also sets `JOB_OBJECT_LIMIT_BREAKAWAY_OK` so a child
//! that explicitly requests `CREATE_BREAKAWAY_FROM_JOB` (the long-lived cmuxd
//! daemon, which must outlive the supervisor and re-job itself standalone) can
//! detach. This is opt-in only: any child that does NOT pass that flag stays
//! assigned to the job and is still killed on close, preserving the zero-orphan
//! tree-kill guarantee. We deliberately never set
//! `JOB_OBJECT_LIMIT_SILENT_BREAKAWAY_OK`, which would auto-detach *every*
//! child and silently break confinement.
//!
//! Raw `HANDLE`s are stored as `isize` in the session table so the supervisor is
//! `Send + Sync` without wrapping every handle; they are reconstituted at the
//! Win32 call sites.

use std::{
    collections::{BTreeMap, HashMap},
    ffi::c_void,
    fs::File,
    io::Read,
    os::windows::io::FromRawHandle,
    sync::{mpsc, Mutex},
    thread,
};

use windows::{
    core::{PCWSTR, PWSTR},
    Win32::{
        Foundation::{
            CloseHandle, SetHandleInformation, HANDLE, HANDLE_FLAGS, HANDLE_FLAG_INHERIT,
        },
        Security::SECURITY_ATTRIBUTES,
        System::{
            Console::{GenerateConsoleCtrlEvent, CTRL_BREAK_EVENT},
            JobObjects::{
                AssignProcessToJobObject, CreateJobObjectW, JobObjectBasicAccountingInformation,
                JobObjectExtendedLimitInformation, QueryInformationJobObject,
                SetInformationJobObject, TerminateJobObject,
                JOBOBJECT_BASIC_ACCOUNTING_INFORMATION, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
                JOB_OBJECT_LIMIT, JOB_OBJECT_LIMIT_BREAKAWAY_OK,
                JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            },
            Pipes::CreatePipe,
            Threading::{
                CreateProcessW, ResumeThread, WaitForSingleObject, CREATE_NEW_PROCESS_GROUP,
                CREATE_NO_WINDOW, CREATE_SUSPENDED, CREATE_UNICODE_ENVIRONMENT,
                PROCESS_INFORMATION, STARTF_USESTDHANDLES, STARTUPINFOW,
            },
        },
    },
};

use crate::{
    transport::LineFramer, AgentIo, AgentOutputChunk, AgentStream, ProcessError, ProcessSupervisor,
    SessionHandle, SessionId, SpawnSpec, TerminateMode,
};

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

    /// Reap a finished session: remove it from the table and close its job +
    /// process handles. Idempotent — reaping an unknown/already-reaped session is
    /// a no-op.
    ///
    /// Distinct from [`terminate`](ProcessSupervisor::terminate), which stays
    /// idempotent and leaves the session *tracked* so post-kill queries (e.g.
    /// [`active_process_count`](Self::active_process_count)) still resolve. A
    /// long-lived supervisor (held in an `Arc` for a whole session) would
    /// otherwise never release a terminated/exited session's kernel handles until
    /// `Drop`; call `reap` once the caller knows the session is finished (the
    /// child exited, or `terminate` has run) to release them promptly. Closing
    /// the job handle also fires `KILL_ON_JOB_CLOSE`, tearing down any straggler
    /// tree that outlived the caller's expectation.
    pub fn reap(&self, id: SessionId) {
        let session = {
            let mut sessions = self.sessions.lock().expect("sessions mutex poisoned");
            sessions.remove(&id)
        };
        if let Some(session) = session {
            unsafe {
                close(handle(session.process));
                close(handle(session.job));
            }
        }
    }

    /// Shared spawn core: create the per-session NAMED job (armed with
    /// kill-on-close unless `survive_disconnect`), launch the child SUSPENDED
    /// with the caller's `startup_info`, `AssignProcessToJobObject` BEFORE
    /// resuming (closing the assign-before-spawn race), then resume, record the
    /// session, and return its handle. `inherit_handles` is `true` only for the
    /// captured-stdio path (so the child inherits the pipe ends).
    ///
    /// # Safety
    /// `startup_info` must be a valid `STARTUPINFOW`; any handles it references
    /// (captured path) must outlive this call.
    unsafe fn launch_and_confine(
        &self,
        id: SessionId,
        spec: &SpawnSpec,
        startup_info: STARTUPINFOW,
        inherit_handles: bool,
    ) -> Result<SessionHandle, ProcessError> {
        let job_name = to_wide(&crate::job_object_name(id));
        let job = CreateJobObjectW(None, PCWSTR(job_name.as_ptr()))
            .map_err(|_| os_error("CreateJobObjectW"))?;
        if !spec.survive_disconnect {
            if let Err(error) = arm_kill_on_job_close(job) {
                close(job);
                return Err(error);
            }
        }

        let mut command_line = build_command_line(spec);
        let program_wide = to_wide(&spec.program.to_string_lossy());
        let current_dir_wide = spec
            .current_dir
            .as_ref()
            .map(|dir| to_wide(&dir.to_string_lossy()));
        let mut env_block = environment_block(&spec.env);

        let mut creation_flags = CREATE_SUSPENDED | CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW;
        let environment_ptr = match env_block.as_mut() {
            Some(block) => {
                creation_flags |= CREATE_UNICODE_ENVIRONMENT;
                Some(block.as_ptr() as *const c_void)
            }
            None => None,
        };

        let mut process_info = PROCESS_INFORMATION::default();
        let spawn_result = CreateProcessW(
            PCWSTR(program_wide.as_ptr()),
            Some(PWSTR(command_line.as_mut_ptr())),
            None,
            None,
            inherit_handles,
            creation_flags,
            environment_ptr,
            current_dir_wide
                .as_ref()
                .map_or(PCWSTR::null(), |dir| PCWSTR(dir.as_ptr())),
            &startup_info,
            &mut process_info,
        );
        if spawn_result.is_err() {
            close(job);
            return Err(ProcessError::Spawn {
                program: spec.program.to_string_lossy().into_owned(),
                source: std::io::Error::last_os_error(),
            });
        }

        // Confine BEFORE resuming.
        if AssignProcessToJobObject(job, process_info.hProcess).is_err() {
            let error = os_error("AssignProcessToJobObject");
            let _ = TerminateJobObject(job, 1);
            terminate_loose_process(process_info.hProcess);
            close(process_info.hThread);
            close(process_info.hProcess);
            close(job);
            return Err(error);
        }

        if ResumeThread(process_info.hThread) == u32::MAX {
            let error = os_error("ResumeThread");
            let _ = TerminateJobObject(job, 1);
            close(process_info.hThread);
            close(process_info.hProcess);
            close(job);
            return Err(error);
        }
        close(process_info.hThread);

        let root_pid = process_info.dwProcessId;
        self.sessions
            .lock()
            .expect("sessions mutex poisoned")
            .insert(
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
}

impl ProcessSupervisor for JobObjectSupervisor {
    fn spawn(&self, spec: SpawnSpec) -> Result<SessionHandle, ProcessError> {
        let id = SessionId::new();
        let startup_info = STARTUPINFOW {
            cb: std::mem::size_of::<STARTUPINFOW>() as u32,
            ..Default::default()
        };
        // No stdio redirection → inherit handles need not be inheritable.
        unsafe { self.launch_and_confine(id, &spec, startup_info, false) }
    }

    fn spawn_captured(&self, spec: SpawnSpec) -> Result<(SessionHandle, AgentIo), ProcessError> {
        let id = SessionId::new();
        unsafe {
            // Three pipes; the child gets the inheritable ends, the parent keeps
            // the others (made NON-inheritable so the child can't hold a copy
            // that would prevent EOF).
            let (stdout_read, stdout_write) = create_pipe()?;
            let (stderr_read, stderr_write) = create_pipe()?;
            let (stdin_read, stdin_write) = create_pipe()?;
            let parent_ends = [stdout_read, stderr_read, stdin_write];
            let child_ends = [stdout_write, stderr_write, stdin_read];
            for &parent_end in &parent_ends {
                if let Err(error) = set_no_inherit(parent_end) {
                    for &h in parent_ends.iter().chain(child_ends.iter()) {
                        close(h);
                    }
                    return Err(error);
                }
            }

            let startup_info = STARTUPINFOW {
                cb: std::mem::size_of::<STARTUPINFOW>() as u32,
                dwFlags: STARTF_USESTDHANDLES,
                hStdInput: stdin_read,
                hStdOutput: stdout_write,
                hStdError: stderr_write,
                ..Default::default()
            };

            let handle = match self.launch_and_confine(id, &spec, startup_info, true) {
                Ok(handle) => handle,
                Err(error) => {
                    for &h in parent_ends.iter().chain(child_ends.iter()) {
                        close(h);
                    }
                    return Err(error);
                }
            };

            // The child now owns its inherited copies; close the parent's copies
            // of the child ends so EOF propagates once the child exits.
            for &h in &child_ends {
                close(h);
            }

            let stdout_file = File::from_raw_handle(stdout_read.0 as _);
            let stderr_file = File::from_raw_handle(stderr_read.0 as _);
            let stdin_file = File::from_raw_handle(stdin_write.0 as _);

            let (sender, receiver) = mpsc::channel();
            spawn_reader(stdout_file, AgentStream::Stdout, sender.clone());
            spawn_reader(stderr_file, AgentStream::Stderr, sender);

            Ok((handle, AgentIo::new(receiver, Box::new(stdin_file))))
        }
    }

    fn terminate(&self, id: SessionId, mode: TerminateMode) -> Result<(), ProcessError> {
        let (job, process, root_pid) = {
            let sessions = self.sessions.lock().expect("sessions mutex poisoned");
            match sessions.get(&id) {
                Some(session) => (
                    handle(session.job),
                    handle(session.process),
                    session.root_pid,
                ),
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

/// Limit flags for a normal (non-survive-disconnect) session job:
/// `KILL_ON_JOB_CLOSE` tears down the whole tree on close, while `BREAKAWAY_OK`
/// lets a child that explicitly requests `CREATE_BREAKAWAY_FROM_JOB` (the cmuxd
/// daemon) detach. `BREAKAWAY_OK` affects ONLY opt-in children — every other
/// child stays confined and is still killed on close (NOT `SILENT_BREAKAWAY_OK`,
/// which would auto-detach all children and defeat zero-orphan tree-kill).
fn kill_on_job_close_limit_flags() -> JOB_OBJECT_LIMIT {
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_BREAKAWAY_OK
}

/// Set `KILL_ON_JOB_CLOSE | BREAKAWAY_OK` on the job's extended limits. The
/// `BREAKAWAY_OK` bit is what makes the cmuxd daemon's
/// `CREATE_BREAKAWAY_FROM_JOB` spawn actually take effect (detach from this
/// supervisor's kill-on-close job); non-breakaway children stay confined.
fn arm_kill_on_job_close(job: HANDLE) -> Result<(), ProcessError> {
    let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    info.BasicLimitInformation.LimitFlags = kill_on_job_close_limit_flags();
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

/// Create an anonymous pipe whose handles are inheritable; returns
/// `(read_end, write_end)`. The caller marks the parent-retained end
/// non-inheritable via [`set_no_inherit`].
unsafe fn create_pipe() -> Result<(HANDLE, HANDLE), ProcessError> {
    let attributes = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        bInheritHandle: true.into(),
        ..Default::default()
    };
    let mut read = HANDLE::default();
    let mut write = HANDLE::default();
    CreatePipe(&mut read, &mut write, Some(&attributes), 0).map_err(|_| os_error("CreatePipe"))?;
    Ok((read, write))
}

/// Clear the inherit flag on `handle` so a spawned child does not receive a copy
/// (required for the parent-retained pipe ends, else EOF never arrives).
unsafe fn set_no_inherit(handle: HANDLE) -> Result<(), ProcessError> {
    SetHandleInformation(handle, HANDLE_FLAG_INHERIT.0, HANDLE_FLAGS(0))
        .map_err(|_| os_error("SetHandleInformation"))
}

/// Spawn a thread that drains `file` through a [`LineFramer`], sending each
/// completed frame as an [`AgentOutputChunk`] tagged with `stream`. The thread
/// exits at EOF (or read error), after flushing any trailing partial line.
fn spawn_reader(file: File, stream: AgentStream, sender: mpsc::Sender<AgentOutputChunk>) {
    thread::spawn(move || {
        let mut file = file;
        let mut framer = LineFramer::new();
        let mut buffer = [0u8; 8192];
        loop {
            match file.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(read) => {
                    for frame in framer.push(&buffer[..read]) {
                        if sender.send(AgentOutputChunk { stream, frame }).is_err() {
                            return;
                        }
                    }
                }
            }
        }
        if let Some(frame) = framer.flush() {
            let _ = sender.send(AgentOutputChunk { stream, frame });
        }
    });
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
    fn kill_on_job_close_flags_permit_optin_breakaway_only() {
        use windows::Win32::System::JobObjects::JOB_OBJECT_LIMIT_SILENT_BREAKAWAY_OK;
        let flags = kill_on_job_close_limit_flags();
        // Tree-kill guarantee preserved.
        assert_ne!(flags.0 & JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE.0, 0);
        // Opt-in breakaway available for CREATE_BREAKAWAY_FROM_JOB children.
        assert_ne!(flags.0 & JOB_OBJECT_LIMIT_BREAKAWAY_OK.0, 0);
        // Never silent breakaway (would auto-detach every child, breaking zero-orphan).
        assert_eq!(flags.0 & JOB_OBJECT_LIMIT_SILENT_BREAKAWAY_OK.0, 0);
    }

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

    /// End-to-end: spawn a child whose stdout emits two lines and assert both
    /// arrive as tagged stdout frames, with the channel closing at EOF. Proves
    /// the pipe redirection + reader-thread + LineFramer wiring. Lives in the lib
    /// binary (Application Control reliably allows it; standalone test exes get
    /// os error 4551 — see ledger.rs sweep test).
    #[test]
    fn spawn_captured_streams_tagged_stdout_frames() {
        use crate::{AgentStream, ProcessSupervisor, SpawnSpec};
        use std::time::Duration;

        let comspec =
            std::env::var("ComSpec").unwrap_or_else(|_| r"C:\Windows\System32\cmd.exe".into());
        let supervisor = JobObjectSupervisor::new();
        let (handle, io) = supervisor
            .spawn_captured(SpawnSpec::new(comspec).args(["/c", "echo line-1&echo line-2"]))
            .expect("spawn_captured");
        assert_ne!(handle.root_pid, 0);

        let mut stdout_lines = Vec::new();
        // Drain until the channel disconnects (both reader threads exit at EOF).
        while let Ok(chunk) = io.chunks().recv_timeout(Duration::from_secs(8)) {
            if chunk.stream == AgentStream::Stdout {
                if let Ok(line) = chunk.frame {
                    let line = line.trim().to_string();
                    if !line.is_empty() {
                        stdout_lines.push(line);
                    }
                }
            }
        }

        assert!(
            stdout_lines.contains(&"line-1".to_string()),
            "got {stdout_lines:?}"
        );
        assert!(
            stdout_lines.contains(&"line-2".to_string()),
            "got {stdout_lines:?}"
        );
    }

    /// After a session's child exits and is drained, `reap` removes it from the
    /// session table (releasing its kernel handles), so subsequent queries by id
    /// report `UnknownSession`, and a second `reap` is a harmless no-op. This is
    /// the natural-exit cleanup path a long-lived supervisor needs so terminated
    /// sessions don't accumulate handles until `Drop`.
    #[test]
    fn reap_removes_session_and_is_idempotent() {
        use crate::{ProcessError, ProcessSupervisor, SpawnSpec};
        use std::time::Duration;

        let comspec =
            std::env::var("ComSpec").unwrap_or_else(|_| r"C:\Windows\System32\cmd.exe".into());
        let supervisor = JobObjectSupervisor::new();
        let (handle, io) = supervisor
            .spawn_captured(SpawnSpec::new(comspec).args(["/c", "echo done"]))
            .expect("spawn_captured");

        // Drain to EOF so the child has exited before we reap.
        while io.chunks().recv_timeout(Duration::from_secs(8)).is_ok() {}

        // The session is still tracked immediately after exit (terminate stays
        // idempotent by design), then reap releases it.
        supervisor.reap(handle.id);
        assert!(matches!(
            supervisor.active_process_count(handle.id),
            Err(ProcessError::UnknownSession(_))
        ));
        // Idempotent: reaping again does nothing (and does not panic).
        supervisor.reap(handle.id);
    }
}
