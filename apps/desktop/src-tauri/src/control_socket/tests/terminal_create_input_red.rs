use super::*;
use cmux_core::session::{
    SessionSurfaceSnapshot, SessionTabManagerSnapshot, SessionWorkspaceGroupSnapshot,
};

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
    for method in FROZEN_METHODS {
        assert!(source.contains(&format!("\"{method}\"")));
    }
    assert!(source.contains("terminal_create_control(app, &request.method, &request.params)"));
    assert!(
        source.contains("prepare_terminal_input_control(app, &request.method, &request.params)")
    );
    assert!(source.contains("finish_terminal_input_control(app, prepared)"));
}

#[test]
fn terminal_create_has_a_scoped_deferred_runtime_policy() {
    let source = include_str!("../../control_socket.rs");
    assert!(source.contains("enum TerminalCreateRuntimePolicy"));
    assert!(source.contains("TerminalCreateRuntimePolicy::Deferred"));
    assert!(source.contains("TerminalCreateRuntimePolicy::Eager"));
    assert!(source.contains("terminal_create_runtime_policy:"));
    assert!(source.contains(
        "if self.terminal_create_runtime_policy == TerminalCreateRuntimePolicy::Deferred"
    ));
}

#[test]
fn terminal_input_contract_freezes_exact_public_errors() {
    let mut source = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src/control_socket/terminal_runtime_v2.rs"),
    )
    .unwrap_or_default();
    source.push_str(include_str!("../../control_socket.rs"));
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
        assert!(
            source.contains(contract),
            "missing frozen grammar contract: {contract}"
        );
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
    let mut source = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/terminal_create_input_process.rs"),
    )
    .unwrap_or_default();
    source.push_str(
        &std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/support/mod.rs"),
        )
        .unwrap_or_default(),
    );
    for contract in [
        "CARGO_BIN_EXE_cmux-desktop",
        "CMUX_CONTROL_PIPE_NAME",
        "CMUX_TEST_DISABLE_SINGLE_INSTANCE",
        "runtime_surface_ready",
        "input_queue_full",
        "created_terminal_id",
    ] {
        assert!(
            source.contains(contract),
            "missing process-proof contract: {contract}"
        );
    }
    assert!(!source.contains("terminal.replay"));
    assert!(!source.contains("terminal.paste"));
    assert!(!source.contains("terminal.viewport"));
}

#[test]
fn mobile_preview_is_bounded_plain_text() {
    assert_eq!(
        mobile_workspace_preview("\u{1b}[38:2::255:0:0m red\n\u{1b}[0m  alert"),
        Some("red alert".into())
    );
    assert_eq!(
        mobile_workspace_preview("\u{1b}]0;private title\u{7}visible"),
        Some("visible".into())
    );
    assert_eq!(mobile_workspace_preview("\u{1b}[31"), None);

    let preview = mobile_workspace_preview(&"x".repeat(140 * 16 + 1)).unwrap();
    assert_eq!(preview.chars().count(), 140);
    assert!(preview.ends_with('…'));
}

#[test]
fn terminal_input_releases_the_request_mutation_gate_before_io() {
    let state = std::sync::Arc::new(SessionState::default());
    let guard = state.lock_control_mutation().unwrap();
    let other = state.clone();
    let (acquired_tx, acquired_rx) = std::sync::mpsc::channel();
    let mut waiter = None;

    let acquired = run_after_control_mutation_gate(guard, || {
        waiter = Some(std::thread::spawn(move || {
            let _guard = other.lock_control_mutation().unwrap();
            acquired_tx.send(()).unwrap();
        }));
        acquired_rx
            .recv_timeout(std::time::Duration::from_secs(1))
            .is_ok()
    });

    assert!(
        acquired,
        "terminal I/O closure retained the session mutation gate"
    );
    waiter.unwrap().join().unwrap();
}

#[test]
fn remote_terminal_input_is_live_only_and_never_materializes_a_local_shell() {
    let source = include_str!("../../control_socket.rs");
    assert!(source.contains("request_live_terminal_input"));
    assert!(source.contains("TerminalInputTarget::Remote"));
    assert!(!source.contains(
        "SessionSurfaceKindSnapshot::Terminal | SessionSurfaceKindSnapshot::RemoteTerminal"
    ));
}

#[test]
fn terminal_create_mobile_projection_preserves_scope_groups_and_runtime_titles() {
    const WORKSPACE: &str = "22222222-2222-4222-8222-222222222222";
    const PANE: &str = "33333333-3333-4333-8333-333333333333";
    const FIRST: &str = "44444444-4444-4444-8444-444444444444";
    const SECOND: &str = "55555555-5555-4555-8555-555555555555";

    let pane = SessionPaneLayoutSnapshot {
        pane_id: Some(PANE.into()),
        panel_ids: vec![FIRST.into(), SECOND.into()],
        selected_panel_id: Some(FIRST.into()),
        surface_kind: None,
        markdown_file_path: None,
        file_path: None,
        diff_viewer_token: None,
        diff_viewer_request_path: None,
        browser_url: None,
        browser_proxy_url: None,
        browser_back_history: None,
        browser_forward_history: None,
        browser_omnibar_visible: None,
        browser_focus_mode_active: None,
        browser_developer_tools_visible: None,
        browser_developer_tools_panel: None,
        browser_page_zoom: None,
    };
    let mut runtime_metadata = cmux_core::session::SessionSurfaceMetadataSnapshot::default();
    runtime_metadata.runtime_title = Some("runtime shell".into());
    let workspace = SessionWorkspaceSnapshot {
        workspace_id: Some(WORKSPACE.into()),
        process_title: "Workspace".into(),
        group_id: Some("66666666-6666-4666-8666-666666666666".into()),
        layout: Some(SessionWorkspaceLayoutSnapshot::Pane(pane)),
        focused_panel_id: Some(FIRST.into()),
        surfaces: Some(vec![
            SessionSurfaceSnapshot {
                surface_id: FIRST.into(),
                pane_id: PANE.into(),
                generation: 1,
                kind: SessionSurfaceKindSnapshot::Terminal,
                metadata: runtime_metadata,
                terminal_startup: None,
                scrollback: None,
            },
            SessionSurfaceSnapshot {
                surface_id: SECOND.into(),
                pane_id: PANE.into(),
                generation: 1,
                kind: SessionSurfaceKindSnapshot::Terminal,
                metadata: Default::default(),
                terminal_startup: None,
                scrollback: None,
            },
        ]),
        ..Default::default()
    };
    let window = SessionWindowSnapshot {
        window_id: Some("11111111-1111-4111-8111-111111111111".into()),
        selected_workspace_id: Some(WORKSPACE.into()),
        dock: None,
        tab_manager: SessionTabManagerSnapshot {
            selected_workspace_index: Some(0),
            workspaces: vec![workspace.clone()],
            workspace_groups: Some(vec![SessionWorkspaceGroupSnapshot {
                id: "66666666-6666-4666-8666-666666666666".into(),
                name: "Group".into(),
                is_collapsed: true,
                anchor_workspace_id: Some(WORKSPACE.into()),
                anchor_member_index: Some(0),
                is_pinned: Some(true),
                custom_color: None,
                icon_symbol: None,
            }]),
        },
    };

    let all_groups = terminal_groups_for_mobile(&window, false);
    assert_eq!(all_groups.len(), 1);
    assert_eq!(all_groups[0]["member_workspace_ids"], json!([WORKSPACE]));
    assert!(terminal_groups_for_mobile(&window, true).is_empty());

    let mut never_ready = |_surface_id: &str| false;
    let filtered = terminal_rows_for_mobile(&workspace, Some(SECOND), &mut never_ready);
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0]["id"], json!(SECOND));

    let titled = terminal_rows_for_mobile(&workspace, Some(FIRST), &mut never_ready);
    assert_eq!(titled[0]["title"], json!("runtime shell"));
}
