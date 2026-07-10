//! M3 WS1 acceptance: killing a session terminates the entire descendant tree
//! with zero orphans (M3 exit criteria). Windows-only — the supervisor is a
//! no-op stub elsewhere.
//!
//! The Job Object's own `ActiveProcesses` accounting is the oracle: a confined
//! `cmd` that spawns a child `ping` shows ≥2 active processes; after a forced
//! terminate the count must fall to exactly 0 (no survivors). Because child
//! processes automatically inherit the parent's job, this also covers the
//! self-detaching case the milestone calls out.

#![cfg(windows)]

use std::{
    thread::sleep,
    time::{Duration, Instant},
};

use cmux_process::{
    JobObjectSupervisor, ProcessError, ProcessSupervisor, SessionId, SpawnSpec, TerminateMode,
};

/// A child tree that lives long enough to observe: `cmd` waits on a `ping` that
/// sends ~60 packets one second apart. Both processes are confined to the job.
fn long_running_tree() -> SpawnSpec {
    let comspec =
        std::env::var("ComSpec").unwrap_or_else(|_| r"C:\Windows\System32\cmd.exe".into());
    // The whole "ping ..." string is one argument; cmd /c strips the outer
    // quotes our quoter adds and runs it. No env → child inherits ours (so
    // cmd/ping can find System32).
    SpawnSpec::new(comspec).args(["/c", "ping -n 60 127.0.0.1"])
}

/// Poll `predicate(count)` on the live process count until it holds or the
/// deadline elapses. Returns the last observed count.
fn wait_for_count(
    supervisor: &JobObjectSupervisor,
    id: SessionId,
    timeout: Duration,
    predicate: impl Fn(u32) -> bool,
) -> u32 {
    let deadline = Instant::now() + timeout;
    loop {
        let count = supervisor.active_process_count(id).expect("active count");
        if predicate(count) || Instant::now() >= deadline {
            return count;
        }
        sleep(Duration::from_millis(25));
    }
}

#[test]
fn force_terminate_kills_whole_tree_with_zero_orphans() {
    let supervisor = JobObjectSupervisor::new();
    let handle = supervisor.spawn(long_running_tree()).expect("spawn");
    assert_ne!(handle.root_pid, 0, "root pid should be assigned");

    // The cmd→ping tree should grow to at least two confined processes.
    let peak = wait_for_count(&supervisor, handle.id, Duration::from_secs(8), |n| n >= 2);
    assert!(
        peak >= 2,
        "expected the child tree to be confined, saw {peak}"
    );

    supervisor
        .terminate(handle.id, TerminateMode::Force)
        .expect("force terminate");

    // After the forced kill the job must drain to zero — no orphans.
    let survivors = wait_for_count(&supervisor, handle.id, Duration::from_secs(8), |n| n == 0);
    assert_eq!(survivors, 0, "tree-kill left {survivors} orphan(s)");
}

#[test]
fn graceful_terminate_also_drains_the_tree() {
    let supervisor = JobObjectSupervisor::new();
    let handle = supervisor.spawn(long_running_tree()).expect("spawn");
    wait_for_count(&supervisor, handle.id, Duration::from_secs(8), |n| n >= 1);

    supervisor
        .terminate(handle.id, TerminateMode::Graceful)
        .expect("graceful terminate");

    let survivors = wait_for_count(&supervisor, handle.id, Duration::from_secs(8), |n| n == 0);
    assert_eq!(
        survivors, 0,
        "graceful terminate left {survivors} orphan(s)"
    );
}

#[test]
fn terminate_is_idempotent_after_tree_exits() {
    let supervisor = JobObjectSupervisor::new();
    let handle = supervisor.spawn(long_running_tree()).expect("spawn");
    supervisor
        .terminate(handle.id, TerminateMode::Force)
        .expect("first terminate");
    wait_for_count(&supervisor, handle.id, Duration::from_secs(8), |n| n == 0);
    // A second terminate on the drained (but still-tracked) job is still Ok.
    supervisor
        .terminate(handle.id, TerminateMode::Force)
        .expect("second terminate is idempotent");
}

#[test]
fn terminate_unknown_session_errors() {
    let supervisor = JobObjectSupervisor::new();
    let result = supervisor.terminate(SessionId::new(), TerminateMode::Force);
    assert!(matches!(result, Err(ProcessError::UnknownSession(_))));
}

#[test]
fn spawn_failure_for_missing_program() {
    let supervisor = JobObjectSupervisor::new();
    let result = supervisor.spawn(SpawnSpec::new(r"C:\does\not\exist\nope.exe"));
    assert!(matches!(result, Err(ProcessError::Spawn { .. })));
}
