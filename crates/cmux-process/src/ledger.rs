//! Session→process ledger for orphan recovery after a core crash (M3 WS4).
//!
//! Windows has no parent-death propagation: if the cmux core crashes (rather
//! than exiting cleanly, which would close the job handles and trigger
//! `KILL_ON_JOB_CLOSE`), supervised agent trees can survive. On the next launch
//! the core must reconcile and kill those escapees. To do that safely it
//! persists a ledger of `session → root-pid + process-creation-time` and, on
//! startup, terminates any entry whose process is *still alive with the same
//! identity*.
//!
//! ## PID reuse is the hazard (M3 risk table)
//!
//! A bare PID is not a safe kill target: Windows recycles PIDs, so by the time
//! the core relaunches, the recorded PID may belong to an innocent unrelated
//! process. The defense is to pair the PID with the process **creation time**
//! (`GetProcessTimes`) — a (pid, created_at) tuple uniquely identifies a process
//! for practical purposes. The sweep only acts when both match.
//!
//! This module owns the data model + JSON persistence (pure, tested on every
//! OS) and the creation-time identity probe (`#[cfg(windows)]`, with a stub that
//! reports "gone" elsewhere so the workspace builds and the sweep is a no-op).

use std::{
    io,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::SessionHandle;

/// One supervised root process, identified resistant to PID reuse.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LedgerEntry {
    /// The owning session.
    pub session_id: Uuid,
    /// Root child PID the supervisor spawned.
    pub root_pid: u32,
    /// Process creation time as a Windows `FILETIME` (100 ns ticks since 1601),
    /// captured at spawn. `0` means "unknown" (non-Windows / capture failed) and
    /// never matches a live process, so such entries are never swept.
    pub created_at: u64,
}

impl LedgerEntry {
    /// Build an entry for `handle`, capturing the root process's creation time
    /// now. Returns an entry with `created_at == 0` if the time can't be read
    /// (process already gone, access denied, or non-Windows).
    pub fn capture(handle: &SessionHandle) -> Self {
        Self {
            session_id: handle.id.0,
            root_pid: handle.root_pid,
            created_at: process_creation_time(handle.root_pid).unwrap_or(0),
        }
    }

    /// Whether the recorded process is still alive with the *same* identity
    /// (pid AND creation time). An entry with `created_at == 0` is never alive
    /// (we refuse to act on an unverifiable identity).
    pub fn is_alive(&self) -> bool {
        self.created_at != 0 && process_creation_time(self.root_pid) == Some(self.created_at)
    }
}

/// The persisted set of supervised sessions.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionLedger {
    /// Live entries, keyed logically by `session_id`.
    pub entries: Vec<LedgerEntry>,
}

impl SessionLedger {
    /// An empty ledger.
    pub fn new() -> Self {
        Self::default()
    }

    /// The default on-disk location: `%LOCALAPPDATA%\cmux\state\process-ledger.json`.
    /// `None` if `LOCALAPPDATA` is unset (non-Windows / unusual environments).
    pub fn default_path() -> Option<PathBuf> {
        let local_app_data = std::env::var_os("LOCALAPPDATA")?;
        Some(
            PathBuf::from(local_app_data)
                .join("cmux")
                .join("state")
                .join("process-ledger.json"),
        )
    }

    /// Load the ledger from `path`. A missing file yields an empty ledger (the
    /// common first-run case); malformed JSON is a hard error.
    pub fn load(path: impl AsRef<Path>) -> io::Result<Self> {
        match std::fs::read(path.as_ref()) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Self::new()),
            Err(error) => Err(error),
        }
    }

    /// Persist the ledger to `path`, creating parent directories as needed.
    /// Written pretty for human inspection during incident response.
    pub fn save(&self, path: impl AsRef<Path>) -> io::Result<()> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_vec_pretty(self)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        std::fs::write(path, json)
    }

    /// Insert or replace the entry for its session.
    pub fn upsert(&mut self, entry: LedgerEntry) {
        match self
            .entries
            .iter_mut()
            .find(|existing| existing.session_id == entry.session_id)
        {
            Some(existing) => *existing = entry,
            None => self.entries.push(entry),
        }
    }

    /// Remove the entry for `session_id`, if present. Returns whether anything
    /// was removed.
    pub fn remove(&mut self, session_id: Uuid) -> bool {
        let before = self.entries.len();
        self.entries.retain(|entry| entry.session_id != session_id);
        self.entries.len() != before
    }

    /// The entries whose recorded process is still alive with a matching
    /// identity — i.e. orphan trees that escaped a crash and must be swept.
    pub fn survivors(&self) -> Vec<&LedgerEntry> {
        self.entries
            .iter()
            .filter(|entry| entry.is_alive())
            .collect()
    }

    /// Reconcile after a (possibly crashed) prior run: terminate the *entire*
    /// job tree of every still-alive entry by reopening its named Job Object,
    /// then clear the ledger. Entries already gone are reported, not acted on.
    /// This is the startup orphan-sweep behind the M3 exit criterion "a
    /// simulated core crash leaves no surviving agent trees".
    ///
    /// Orphan-free because it terminates the whole *descendant tree* of the
    /// recorded root, not just the root PID (which would leave grandchildren
    /// behind). A named Job Object cannot be reopened here: a kernel object's
    /// name is released when the last handle closes, so once the (crashed) core
    /// exits the job name is gone — reopening only works while a handle-holder
    /// such as the M4 daemon lives. After a full crash the reliable recovery is
    /// to walk the live process tree from the identity-verified root.
    pub fn sweep(&mut self) -> SweepReport {
        let mut report = SweepReport::default();
        for entry in &self.entries {
            if !entry.is_alive() {
                report.already_gone.push(entry.session_id);
                continue;
            }
            match terminate_orphan_tree(entry.root_pid) {
                Ok(()) => report.terminated.push(entry.session_id),
                Err(error) => report.failed.push((entry.session_id, error.to_string())),
            }
        }
        self.entries.clear();
        report
    }
}

/// Outcome of a [`SessionLedger::sweep`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SweepReport {
    /// Sessions whose surviving tree was terminated.
    pub terminated: Vec<Uuid>,
    /// Sessions already gone before the sweep (nothing to do).
    pub already_gone: Vec<Uuid>,
    /// Sessions whose termination failed, with the error string.
    pub failed: Vec<(Uuid, String)>,
}

/// Terminate the entire descendant tree rooted at `root_pid`.
///
/// Snapshots all processes (ToolHelp), builds the parent→child tree from
/// `th32ParentProcessID`, and `TerminateProcess`es every descendant
/// leaves-first, then the root. Best-effort: a single stubborn PID does not fail
/// the whole sweep, since recovery should make maximum progress.
///
/// The caller has already verified the root's (pid, creation-time) identity, so
/// the root is genuinely ours; descendants are taken from the live snapshot's
/// parent linkage (PID-reuse mis-linking a descendant is possible but unlikely
/// for a quiescent post-crash orphan, the standard `taskkill /T` tradeoff).
#[cfg(windows)]
fn terminate_orphan_tree(root_pid: u32) -> io::Result<()> {
    use windows::Win32::{
        Foundation::CloseHandle,
        System::{
            Diagnostics::ToolHelp::{
                CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
                TH32CS_SNAPPROCESS,
            },
            Threading::{OpenProcess, TerminateProcess, PROCESS_TERMINATE},
        },
    };

    // 1. Snapshot (pid, parent-pid) for every process.
    let mut pairs: Vec<(u32, u32)> = Vec::new();
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0)
            .map_err(|error| io::Error::other(format!("CreateToolhelp32Snapshot: {error}")))?;
        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        if Process32FirstW(snapshot, &mut entry).is_ok() {
            loop {
                pairs.push((entry.th32ProcessID, entry.th32ParentProcessID));
                if Process32NextW(snapshot, &mut entry).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snapshot);
    }

    // 2. Breadth-first collect the root and all descendants.
    let mut tree = vec![root_pid];
    let mut cursor = 0;
    while cursor < tree.len() {
        let parent = tree[cursor];
        for &(pid, parent_pid) in &pairs {
            if parent_pid == parent && pid != 0 && pid != parent && !tree.contains(&pid) {
                tree.push(pid);
            }
        }
        cursor += 1;
    }

    // 3. Terminate leaves-first (reverse discovery order), then the root.
    for &pid in tree.iter().rev() {
        unsafe {
            if let Ok(process) = OpenProcess(PROCESS_TERMINATE, false, pid) {
                let _ = TerminateProcess(process, 1);
                let _ = CloseHandle(process);
            }
        }
    }
    Ok(())
}

/// Non-Windows: nothing to sweep (identity probing reports "gone", so this is
/// never reached for a live entry); keeps the build green.
#[cfg(not(windows))]
fn terminate_orphan_tree(_root_pid: u32) -> io::Result<()> {
    Ok(())
}

/// Read a process's creation time as a `FILETIME`-derived `u64` (100 ns ticks
/// since 1601-01-01 UTC). `None` if the process does not exist, access is
/// denied, or the call fails.
#[cfg(windows)]
pub fn process_creation_time(pid: u32) -> Option<u64> {
    use windows::Win32::{
        Foundation::{CloseHandle, FILETIME},
        System::Threading::{GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION},
    };

    if pid == 0 {
        return None;
    }
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut creation = FILETIME::default();
        let mut exit = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        let result = GetProcessTimes(handle, &mut creation, &mut exit, &mut kernel, &mut user);
        let _ = CloseHandle(handle);
        result.ok()?;
        Some(((creation.dwHighDateTime as u64) << 32) | u64::from(creation.dwLowDateTime))
    }
}

/// Non-Windows stub: process identity probing is Windows-only, so callers treat
/// every entry as "gone" and the sweep is a no-op.
#[cfg(not(windows))]
pub fn process_creation_time(_pid: u32) -> Option<u64> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(session: Uuid, pid: u32, created_at: u64) -> LedgerEntry {
        LedgerEntry {
            session_id: session,
            root_pid: pid,
            created_at,
        }
    }

    #[test]
    fn upsert_inserts_then_replaces() {
        let session = Uuid::new_v4();
        let mut ledger = SessionLedger::new();
        ledger.upsert(entry(session, 100, 1));
        ledger.upsert(entry(session, 200, 2));
        assert_eq!(ledger.entries.len(), 1);
        assert_eq!(ledger.entries[0].root_pid, 200);
        assert_eq!(ledger.entries[0].created_at, 2);
    }

    #[test]
    fn remove_reports_whether_present() {
        let session = Uuid::new_v4();
        let mut ledger = SessionLedger::new();
        ledger.upsert(entry(session, 100, 1));
        assert!(ledger.remove(session));
        assert!(!ledger.remove(session));
        assert!(ledger.entries.is_empty());
    }

    #[test]
    fn json_round_trips_through_disk() {
        let dir = std::env::temp_dir().join(format!("cmux-ledger-{}", Uuid::new_v4()));
        let path = dir.join("nested").join("process-ledger.json");

        let mut ledger = SessionLedger::new();
        ledger.upsert(entry(Uuid::new_v4(), 4321, 0x01D9_5C00_0000_0000));
        ledger.upsert(entry(Uuid::new_v4(), 8765, 0x01D9_5C00_DEAD_BEEF));
        ledger.save(&path).expect("save creates parents + writes");

        let loaded = SessionLedger::load(&path).expect("load");
        assert_eq!(loaded, ledger);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn load_missing_file_is_empty() {
        let path = std::env::temp_dir()
            .join(format!("cmux-ledger-missing-{}", Uuid::new_v4()))
            .join("process-ledger.json");
        let loaded = SessionLedger::load(&path).expect("missing file → empty");
        assert!(loaded.entries.is_empty());
    }

    #[test]
    fn entry_with_unknown_creation_time_is_never_alive() {
        // created_at == 0 must never be considered a live identity, even if the
        // recorded pid happens to belong to a running process.
        let live_pid = std::process::id();
        assert!(!entry(Uuid::new_v4(), live_pid, 0).is_alive());
    }

    #[test]
    fn default_path_is_under_localappdata_when_set() {
        // Don't assume the host has LOCALAPPDATA; only assert the shape when it does.
        if std::env::var_os("LOCALAPPDATA").is_some() {
            let path = SessionLedger::default_path().expect("path");
            assert!(
                path.ends_with("cmux/state/process-ledger.json")
                    || path.ends_with(r"cmux\state\process-ledger.json")
            );
        }
    }

    #[cfg(windows)]
    #[test]
    fn current_process_identity_is_live_and_stable() {
        let pid = std::process::id();
        let created_at = process_creation_time(pid).expect("own creation time");
        assert_ne!(created_at, 0);
        // Stable across calls.
        assert_eq!(process_creation_time(pid), Some(created_at));
        // An entry capturing our own identity reports alive.
        let live = entry(Uuid::new_v4(), pid, created_at);
        assert!(live.is_alive());
        // A mismatched creation time (PID-reuse simulation) is NOT alive.
        let reused = entry(Uuid::new_v4(), pid, created_at.wrapping_add(1));
        assert!(!reused.is_alive());
    }

    #[cfg(windows)]
    #[test]
    fn nonexistent_pid_has_no_creation_time() {
        // PID 0 is reserved (System Idle); our probe rejects it outright, and a
        // very high unlikely PID should not resolve.
        assert_eq!(process_creation_time(0), None);
        assert_eq!(process_creation_time(0x7FFF_FFFE), None);
    }

    /// M3 WS4 acceptance, end-to-end: a simulated core crash leaves no surviving
    /// agent tree after the next startup sweep.
    ///
    /// Spawn a `survive_disconnect` tree (no `KILL_ON_JOB_CLOSE`, so it outlives
    /// the supervisor), record a ledger entry, then DROP the supervisor —
    /// modelling a crash that loses the in-memory job handle. A fresh
    /// `SessionLedger::sweep()` must reopen the job by name and terminate the
    /// whole tree. Killing by reopened *job* (not the recorded root PID) is what
    /// keeps it orphan-free. The child is `ping -n 60`, which self-terminates
    /// within ~60 s, so a pre-sweep assertion failure cannot leak indefinitely.
    ///
    /// Lives in the lib test binary (not a standalone integration test) because
    /// freshly-built standalone test `.exe`s are nondeterministically blocked by
    /// this box's Windows Application Control policy (os error 4551); the lib
    /// binary runs reliably. CI has no such policy.
    #[cfg(windows)]
    #[test]
    fn sweep_kills_orphan_tree_after_simulated_crash() {
        use crate::{JobObjectSupervisor, ProcessSupervisor, SpawnSpec};
        use std::{
            thread::sleep,
            time::{Duration, Instant},
        };

        fn wait_until(timeout: Duration, mut predicate: impl FnMut() -> bool) -> bool {
            let deadline = Instant::now() + timeout;
            loop {
                if predicate() {
                    return true;
                }
                if Instant::now() >= deadline {
                    return false;
                }
                sleep(Duration::from_millis(25));
            }
        }

        let comspec =
            std::env::var("ComSpec").unwrap_or_else(|_| r"C:\Windows\System32\cmd.exe".into());
        let mut spec = SpawnSpec::new(comspec).args(["/c", "ping -n 60 127.0.0.1"]);
        spec.survive_disconnect = true;

        let captured;
        {
            let supervisor = JobObjectSupervisor::new();
            let handle = supervisor.spawn(spec).expect("spawn");
            assert!(
                wait_until(Duration::from_secs(8), || supervisor
                    .active_process_count(handle.id)
                    .unwrap_or(0)
                    >= 2),
                "child tree should be confined to the job",
            );
            captured = LedgerEntry::capture(&handle);
            assert_ne!(captured.created_at, 0, "creation time captured");
            assert!(captured.is_alive(), "tree alive before the crash");
            // Drop the supervisor without terminating → simulated crash.
        }

        assert!(
            captured.is_alive(),
            "survive_disconnect tree must outlive the dropped supervisor",
        );

        let mut ledger = SessionLedger::new();
        ledger.upsert(captured.clone());
        let report = ledger.sweep();
        assert_eq!(
            report.terminated,
            vec![captured.session_id],
            "sweep terminates the orphan tree (failed: {:?})",
            report.failed,
        );
        assert!(ledger.entries.is_empty(), "sweep clears the ledger");
        assert!(
            wait_until(Duration::from_secs(8), || !captured.is_alive()),
            "orphan tree survived the sweep",
        );
    }
}
