#![cfg(windows)]

use std::process::Command;

#[test]
fn agent_hibernation_help_is_the_canonical_public_contract() {
    let output = Command::new(env!("CARGO_BIN_EXE_cmux"))
        .args(["agent-hibernation", "--help"])
        .env_remove("CMUX_SOCKET")
        .env_remove("CMUX_SOCKET_PASSWORD")
        .env_remove("CMUX_SOCKET_PATH")
        .output()
        .expect("cmux binary must execute");

    assert_eq!(
        output.status.code(),
        Some(0),
        "unexpected result: {output:?}"
    );
    assert!(output.stderr.is_empty(), "unexpected stderr: {output:?}");
    assert_eq!(
        String::from_utf8(output.stdout).expect("help output must be UTF-8"),
        "cmux agent-hibernation\n\nUsage: cmux agent-hibernation <on|off> [--json]\n\nEnable or disable Agent Hibernation.\nConfigure idle and live-terminal limits from Settings or cmux settings JSON.\n"
    );
}
