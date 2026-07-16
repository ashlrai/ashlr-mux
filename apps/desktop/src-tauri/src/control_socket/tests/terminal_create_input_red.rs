use super::*;

const FROZEN_METHODS: [&str; 4] = [
    "mobile.terminal.create",
    "mobile.terminal.input",
    "terminal.create",
    "terminal.input",
];

#[test]
fn terminal_create_input_methods_are_public_exactly_once() {
    for method in FROZEN_METHODS {
        assert_eq!(
            CONTROL_SOCKET_METHODS
                .iter()
                .filter(|candidate| **candidate == method)
                .count(),
            1,
            "frozen terminal route must be advertised exactly once: {method}"
        );
    }
}

#[test]
fn terminal_create_input_dispatch_has_one_shared_alias_path() {
    let source = include_str!("../../control_socket.rs");
    assert!(source.contains("mod terminal_runtime_v2;"));
    assert!(source.contains(
        "\"terminal.create\" | \"mobile.terminal.create\" | \"terminal.input\" | \"mobile.terminal.input\""
    ));
    assert!(source.contains("terminal_create_input_control(app, &request.method, &request.params)"));
}

#[test]
fn terminal_create_has_a_scoped_deferred_runtime_policy() {
    let source = include_str!("../../control_socket.rs");
    assert!(source.contains("enum TerminalCreateRuntimePolicy"));
    assert!(source.contains("TerminalCreateRuntimePolicy::Deferred"));
    assert!(source.contains("TerminalCreateRuntimePolicy::Eager"));
    assert!(source.contains("terminal_create_runtime_policy:"));
    assert!(source.contains("if self.terminal_create_runtime_policy == TerminalCreateRuntimePolicy::Deferred"));
}

#[test]
fn terminal_input_contract_freezes_exact_public_errors() {
    let source = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src/control_socket/terminal_runtime_v2.rs"),
    )
    .unwrap_or_default();
    for exact in [
        "Missing text",
        "Missing or invalid workspace_id",
        "Missing or invalid terminal_id",
        "Conflicting terminal identifiers",
        "Terminal surface not found",
        "The terminal can't accept more input right now. Wait a moment and retry, or reopen the terminal if it stays unavailable.",
        "The terminal surface is no longer available; reopen it or create a new terminal session.",
        "The terminal session has ended; reopen it or create a new terminal session.",
    ] {
        assert!(source.contains(exact), "missing frozen terminal contract text: {exact}");
    }
}

#[test]
fn terminal_input_contract_freezes_ordered_key_and_parser_events() {
    let source = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src/control_socket/terminal_runtime_v2.rs"),
    )
    .unwrap_or_default();
    for contract in [
        "TerminalMaterializationEvent::Input",
        "TerminalMaterializationEvent::ProcessOutput",
        "parse_terminal_input",
        "terminal_control_sequence_length",
        "navigation_input",
        "0x08 | 0x7f",
        "b'\\r'",
        "b'\\n'",
    ] {
        assert!(source.contains(contract), "missing frozen grammar contract: {contract}");
    }
}

#[test]
fn terminal_input_live_and_materialized_paths_are_mode_aware() {
    let source = include_str!("../../terminal.rs");
    assert!(source.contains("fn terminal_input_bytes_for_grid"));
    assert!(source.contains("application_cursor_keys_enabled"));
    assert!(source.contains("terminal_input_bytes_for_grid(&grid, &bytes)"));
    assert!(source.contains("terminal_input_bytes_for_grid(&grid, data)"));
}

#[test]
fn process_proof_is_owned_isolated_and_bounded_to_the_frozen_routes() {
    let source = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/terminal_create_input_process.rs"),
    )
    .unwrap_or_default();
    for contract in [
        "CARGO_BIN_EXE_cmux-desktop",
        "CMUX_CONTROL_PIPE_NAME",
        "CMUX_TEST_DISABLE_SINGLE_INSTANCE",
        "runtime_surface_ready",
        "input_queue_full",
        "created_terminal_id",
    ] {
        assert!(source.contains(contract), "missing process-proof contract: {contract}");
    }
    assert!(!source.contains("terminal.replay"));
    assert!(!source.contains("terminal.paste"));
    assert!(!source.contains("terminal.viewport"));
}
