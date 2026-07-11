use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use serde_json::Value;

fn cli() -> &'static str {
    env!("CARGO_BIN_EXE_cmux")
}

fn run(args: &[&str]) -> Output {
    Command::new(cli()).args(args).output().unwrap()
}

fn wait_with_timeout(child: &mut std::process::Child) -> std::process::ExitStatus {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            return status;
        }
        assert!(Instant::now() < deadline, "child timed out");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn probe_modes_and_closed_stdin_match_the_canonical_contract() {
    for mode in ["spawn", "spawn-stderr", "exec"] {
        let output = run(&["__sigpipe-probe", mode]);
        assert!(output.status.success(), "{mode}: {output:?}");
        assert!(output.stderr.is_empty(), "{mode}: {output:?}");
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["signal"], "default", "{mode}");
        assert_eq!(value["stdout_nosigpipe"], 0, "{mode}");
        assert_eq!(value["stderr_nosigpipe"], 0, "{mode}");
    }

    let output = run(&["__sigpipe-stdin-pipe-probe"]);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(output.stdout, b"ok\n");
    assert!(output.stderr.is_empty(), "{output:?}");
}

#[test]
fn closed_output_pipes_do_not_change_command_exit_codes() {
    let mut version = Command::new(cli())
        .arg("version")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    drop(version.stdout.take());
    assert!(wait_with_timeout(&mut version).success());

    let mut failure = Command::new(cli())
        .args(["__sigpipe-inspect", "bad"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    drop(failure.stderr.take());
    assert_eq!(wait_with_timeout(&mut failure).code(), Some(1));
}
