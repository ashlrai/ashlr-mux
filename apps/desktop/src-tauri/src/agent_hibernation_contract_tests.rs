use std::collections::BTreeSet;

use cmux_core::session::{
    AppSessionSnapshot, SessionAgentHibernationSnapshot, SessionCanvasPaneSnapshot,
    SessionPaneLayoutSnapshot, SessionPanelRestorableAgentSnapshot, SessionRestorableAgentSnapshot,
    SessionSplitLayoutSnapshot, SessionSplitOrientation, SessionSurfaceKindSnapshot,
    SessionSurfaceMetadataSnapshot, SessionSurfaceResumeBindingRecordSnapshot,
    SessionSurfaceResumeBindingSnapshot, SessionSurfaceSnapshot,
    SessionSurfaceTerminalStartupSnapshot, SessionTabManagerSnapshot, SessionWindowSnapshot,
    SessionWorkspaceLayoutSnapshot, SessionWorkspaceSnapshot,
};

use crate::agent_hibernation::{
    aggregate_lifecycle, is_allowed_lifecycle_key, is_manual_lifecycle_key,
    process_fallback_fingerprint, protected_panel_ids, sanitize_invalid_hibernation,
    sanitized_confirmation_seconds, sanitized_idle_seconds, sanitized_max_live_terminals,
    scrollback_fingerprint, selected_panel_keys, tail_fingerprint_stable_since,
    AgentHibernationCandidate, AgentHibernationPolicy, AgentHibernationSettingsValues,
    AgentLifecycleState, ConfirmationDecision, PanelKey, EVALUATION_INTERVAL_SECONDS,
    INITIAL_EVALUATION_DELAY_SECONDS,
};

fn key(workspace: &str, panel: &str) -> PanelKey {
    PanelKey::new(workspace, panel)
}

fn settings(
    enabled: bool,
    idle_seconds: f64,
    max_live_terminals: usize,
) -> AgentHibernationSettingsValues {
    AgentHibernationSettingsValues {
        enabled,
        idle_seconds,
        max_live_terminals,
        confirmation_seconds: 5.0,
    }
}

fn candidate(panel: &str, last_activity_at: f64) -> AgentHibernationCandidate {
    AgentHibernationCandidate {
        key: key("workspace", panel),
        has_restorable_agent: true,
        is_live: true,
        is_protected: false,
        lifecycle: AgentLifecycleState::Idle,
        has_unconfirmed_terminal_input: false,
        last_activity_at,
    }
}

#[test]
fn agent_hibernation_contract_settings_match_canonical_defaults_bounds_and_cadence() {
    assert_eq!(
        AgentHibernationSettingsValues::default(),
        AgentHibernationSettingsValues {
            enabled: false,
            idle_seconds: 5.0,
            max_live_terminals: 12,
            confirmation_seconds: 60.0,
        }
    );
    assert_eq!(INITIAL_EVALUATION_DELAY_SECONDS, 5);
    assert_eq!(EVALUATION_INTERVAL_SECONDS, 30);

    assert_eq!(sanitized_idle_seconds(f64::NAN), 5.0);
    assert_eq!(sanitized_idle_seconds(f64::INFINITY), 5.0);
    assert_eq!(sanitized_idle_seconds(-20.0), 5.0);
    assert_eq!(sanitized_idle_seconds(8.6), 9.0);
    assert_eq!(sanitized_idle_seconds(999_999.0), 604_800.0);
    assert_eq!(sanitized_max_live_terminals(-4), 1);
    assert_eq!(sanitized_max_live_terminals(40), 40);
    assert_eq!(sanitized_max_live_terminals(999), 256);
    assert_eq!(sanitized_confirmation_seconds(f64::NEG_INFINITY), 60.0);
    assert_eq!(sanitized_confirmation_seconds(4.9), 5.0);
    assert_eq!(sanitized_confirmation_seconds(42.4), 42.0);
    assert_eq!(sanitized_confirmation_seconds(8_000.0), 3_600.0);
}

#[test]
fn agent_hibernation_contract_lifecycle_parsing_keys_and_idle_authority_are_exact() {
    for raw in ["idle", " IDLE\n"] {
        assert_eq!(
            AgentLifecycleState::parse_cli(raw),
            Some(AgentLifecycleState::Idle)
        );
    }
    for raw in ["needsInput", "needs-input", "needs_input"] {
        assert_eq!(
            AgentLifecycleState::parse_cli(raw),
            Some(AgentLifecycleState::NeedsInput)
        );
    }
    assert_eq!(AgentLifecycleState::parse_cli("paused"), None);
    assert!(AgentLifecycleState::Idle.allows_hibernation());
    assert!(!AgentLifecycleState::Unknown.allows_hibernation());
    assert!(!AgentLifecycleState::Running.allows_hibernation());
    assert!(!AgentLifecycleState::NeedsInput.allows_hibernation());

    for allowed in [
        "amp",
        "claude_code",
        "codex",
        "hermes-agent",
        "opencode",
        "rovodev",
    ] {
        assert!(is_allowed_lifecycle_key(allowed), "{allowed}");
    }
    assert!(!is_allowed_lifecycle_key("manual"));
    assert!(!is_allowed_lifecycle_key("manual:build"));
    assert!(!is_allowed_lifecycle_key("unknown-agent"));
    assert!(is_manual_lifecycle_key("manual"));
    assert!(is_manual_lifecycle_key("manual:build"));
    assert!(!is_manual_lifecycle_key("manualish"));
}

#[test]
fn agent_hibernation_contract_lifecycle_aggregation_uses_canonical_priority_and_fallback() {
    use AgentLifecycleState::{Idle, NeedsInput, Running, Unknown};

    assert_eq!(aggregate_lifecycle([], NeedsInput), NeedsInput);
    assert_eq!(aggregate_lifecycle([Idle, Idle], Unknown), Idle);
    assert_eq!(aggregate_lifecycle([Idle, Unknown], Idle), Unknown);
    assert_eq!(aggregate_lifecycle([Unknown, NeedsInput], Idle), NeedsInput);
    assert_eq!(
        aggregate_lifecycle([NeedsInput, Running, Idle], Unknown),
        Running
    );
}

#[test]
fn agent_hibernation_contract_live_policy_tracks_activity_input_and_lifecycle_acknowledgement() {
    let panel = key("workspace", "panel");
    let mut policy = AgentHibernationPolicy::default();

    policy.record_focus(panel.clone(), 10.0);
    assert_eq!(policy.effective_last_activity_at(&panel, 8.0, 9.0), 10.0);

    policy.record_terminal_input(panel.clone(), 20.0);
    assert!(policy.has_unconfirmed_terminal_input(&panel));
    assert_eq!(policy.effective_last_activity_at(&panel, 30.0, 25.0), 30.0);

    policy.record_lifecycle(panel.clone(), "codex", AgentLifecycleState::Running, 19.0);
    assert!(policy.has_unconfirmed_terminal_input(&panel));
    policy.record_lifecycle(panel.clone(), "codex", AgentLifecycleState::Idle, 20.0);
    assert!(!policy.has_unconfirmed_terminal_input(&panel));
    assert_eq!(
        policy.lifecycle(&panel, AgentLifecycleState::Unknown),
        AgentLifecycleState::Idle
    );

    policy.record_lifecycle(
        panel.clone(),
        "claude_code",
        AgentLifecycleState::NeedsInput,
        21.0,
    );
    assert_eq!(policy.latest_lifecycle_change_at(&panel), Some(21.0));
    assert_eq!(
        policy.lifecycle(&panel, AgentLifecycleState::Idle),
        AgentLifecycleState::NeedsInput
    );
    policy.clear_panel(&panel);
    assert_eq!(
        policy.lifecycle(&panel, AgentLifecycleState::Unknown),
        AgentLifecycleState::Unknown
    );
}

#[test]
fn agent_hibernation_contract_planner_selects_only_oldest_eligible_excess_live_agents() {
    let now = 1_000.0;
    let mut old = candidate("old", now - 300.0);
    let newer = candidate("new", now - 10.0);
    let mut protected = candidate("protected", now - 400.0);
    protected.is_protected = true;
    let mut running = candidate("running", now - 500.0);
    running.lifecycle = AgentLifecycleState::Running;
    let mut unconfirmed = candidate("unconfirmed", now - 500.0);
    unconfirmed.has_unconfirmed_terminal_input = true;
    let mut not_restorable = candidate("not-restorable", now - 500.0);
    not_restorable.has_restorable_agent = false;
    let mut not_live = candidate("not-live", now - 500.0);
    not_live.is_live = false;
    let mut too_recent = candidate("recent", now - 59.0);
    too_recent.lifecycle = AgentLifecycleState::Idle;
    old.lifecycle = AgentLifecycleState::Idle;

    let selected = selected_panel_keys(
        &[
            old.clone(),
            newer,
            protected,
            running,
            unconfirmed,
            not_restorable,
            not_live,
            too_recent,
        ],
        settings(true, 60.0, 1),
        now,
    );
    assert_eq!(selected, BTreeSet::from([old.key]));
}

#[test]
fn agent_hibernation_contract_planner_cap_disable_and_panel_id_tie_break_are_deterministic() {
    let inputs = [
        candidate("z-panel", 0.0),
        candidate("a-panel", 0.0),
        candidate("m-panel", 0.0),
    ];
    assert!(selected_panel_keys(&inputs, settings(false, 5.0, 1), 100.0).is_empty());
    assert!(selected_panel_keys(&inputs, settings(true, 5.0, 3), 100.0).is_empty());
    assert_eq!(
        selected_panel_keys(&inputs, settings(true, 5.0, 2), 100.0),
        BTreeSet::from([key("workspace", "a-panel")])
    );
}

#[test]
fn agent_hibernation_contract_fingerprints_include_sorted_process_identity_and_tail_stability() {
    let first = process_fallback_fingerprint("opencode", "same-session", [7, 3]);
    assert_eq!(
        first,
        process_fallback_fingerprint("opencode", "same-session", [3, 7])
    );
    assert_ne!(
        first,
        process_fallback_fingerprint("opencode", "same-session", [8])
    );
    assert_eq!(
        scrollback_fingerprint("stable tail", [7, 3]),
        scrollback_fingerprint("stable tail", [3, 7])
    );
    assert_ne!(
        scrollback_fingerprint("stable tail", [7, 3]),
        scrollback_fingerprint("stable tail", [8])
    );

    assert_eq!(
        tail_fingerprint_stable_since(None, None, "tail-a", 10.0, 30.0),
        30.0
    );
    assert_eq!(
        tail_fingerprint_stable_since(Some("tail-a"), Some(30.0), "tail-a", 10.0, 40.0),
        30.0
    );
    assert_eq!(
        tail_fingerprint_stable_since(Some("tail-a"), Some(30.0), "tail-b", 10.0, 40.0),
        40.0
    );
    assert_eq!(
        tail_fingerprint_stable_since(Some("tail-a"), None, "tail-a", 10.0, 40.0),
        10.0
    );

    let panel = key("workspace", "tail-panel");
    let mut policy = AgentHibernationPolicy::default();
    assert_eq!(
        policy.observe_tail_fingerprint(&panel, Some("tail-a"), true, 10.0, 30.0),
        Some(30.0)
    );
    assert_eq!(
        policy.observe_tail_fingerprint(&panel, Some("tail-a"), true, 10.0, 40.0),
        Some(30.0)
    );
    assert_eq!(
        policy.observe_tail_fingerprint(&panel, Some("tail-b"), true, 10.0, 50.0),
        Some(50.0)
    );
    assert_eq!(
        policy.observe_tail_fingerprint(&panel, Some("tail-b"), false, 10.0, 60.0),
        None
    );
}

#[test]
fn agent_hibernation_contract_confirmation_requires_stable_fingerprint_and_no_new_activity() {
    let panel = key("workspace", "panel");
    let mut policy = AgentHibernationPolicy::default();

    assert_eq!(
        policy.confirm(&panel, Some("fp-a"), 10.0, 5.0, 20.0),
        ConfirmationDecision::Waiting
    );
    assert_eq!(
        policy.confirm(&panel, Some("fp-b"), 10.0, 5.0, 24.0),
        ConfirmationDecision::Waiting
    );
    assert_eq!(
        policy.confirm(&panel, Some("fp-b"), 10.0, 5.0, 25.0),
        ConfirmationDecision::Reset
    );

    assert_eq!(
        policy.confirm(&panel, Some("fp-a"), 10.0, 5.0, 30.0),
        ConfirmationDecision::Waiting
    );
    assert_eq!(
        policy.confirm(&panel, Some("fp-a"), 31.0, 5.0, 35.0),
        ConfirmationDecision::Reset
    );

    assert_eq!(
        policy.confirm(&panel, Some("fp-a"), 31.0, 5.0, 40.0),
        ConfirmationDecision::Waiting
    );
    assert_eq!(
        policy.confirm(&panel, Some("fp-a"), 31.0, 5.0, 45.0),
        ConfirmationDecision::Ready
    );
    assert!(!policy.has_pending_confirmation(&panel));
}

#[test]
fn agent_hibernation_contract_activity_cancels_confirmation_and_pruning_drops_stale_tracking() {
    let selected = key("workspace", "selected");
    let current = key("workspace", "current");
    let stale = key("workspace", "stale");
    let mut policy = AgentHibernationPolicy::default();

    policy.record_focus(selected.clone(), 1.0);
    policy.record_focus(current.clone(), 1.0);
    policy.record_focus(stale.clone(), 1.0);
    policy.confirm(&selected, Some("fp"), 1.0, 5.0, 10.0);
    policy.confirm(&current, Some("fp"), 1.0, 5.0, 10.0);
    policy.record_terminal_input(selected.clone(), 11.0);
    assert!(!policy.has_pending_confirmation(&selected));

    policy.confirm(&selected, Some("fp"), 11.0, 5.0, 12.0);
    policy.prune(
        &BTreeSet::from([selected.clone(), current.clone()]),
        &BTreeSet::from([selected.clone()]),
    );
    assert!(policy.has_panel(&selected));
    assert!(policy.has_panel(&current));
    assert!(!policy.has_panel(&stale));
    assert!(policy.has_pending_confirmation(&selected));
    assert!(!policy.has_pending_confirmation(&current));
}

fn pane(selected: Option<&str>, panels: &[&str]) -> SessionWorkspaceLayoutSnapshot {
    SessionWorkspaceLayoutSnapshot::Pane(SessionPaneLayoutSnapshot {
        pane_id: None,
        panel_ids: panels.iter().map(|panel| (*panel).to_owned()).collect(),
        selected_panel_id: selected.map(str::to_owned),
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
    })
}

fn window(window_id: &str, workspace: SessionWorkspaceSnapshot) -> SessionWindowSnapshot {
    SessionWindowSnapshot {
        window_id: Some(window_id.to_owned()),
        selected_workspace_id: workspace.workspace_id.clone(),
        dock: None,
        tab_manager: SessionTabManagerSnapshot {
            selected_workspace_index: Some(0),
            workspaces: vec![workspace],
            workspace_groups: None,
        },
    }
}

#[test]
fn agent_hibernation_contract_visible_split_canvas_and_zoom_protection_is_layout_exact() {
    let split = SessionWorkspaceLayoutSnapshot::Split(SessionSplitLayoutSnapshot {
        split_id: None,
        orientation: SessionSplitOrientation::Horizontal,
        divider_position: 0.5,
        first: Box::new(pane(Some("split-a2"), &["split-a1", "split-a2"])),
        second: Box::new(pane(None, &["split-b1", "split-b2"])),
    });
    let split_workspace = SessionWorkspaceSnapshot {
        workspace_id: Some("split-workspace".into()),
        process_title: "split".into(),
        layout: Some(split),
        ..Default::default()
    };
    let hidden_workspace = SessionWorkspaceSnapshot {
        workspace_id: Some("hidden-workspace".into()),
        process_title: "hidden".into(),
        layout: Some(pane(Some("hidden-panel"), &["hidden-panel"])),
        ..Default::default()
    };
    let mut canvas_workspace = SessionWorkspaceSnapshot {
        workspace_id: Some("canvas-workspace".into()),
        process_title: "canvas".into(),
        layout: Some(pane(Some("ignored-layout"), &["ignored-layout"])),
        layout_mode: Some("canvas".into()),
        canvas_panes: Some(vec![
            SessionCanvasPaneSnapshot {
                panel_id: "canvas-a".into(),
                x: 0,
                y: 0,
                width: 10,
                height: 10,
                panel_ids: Some(vec!["canvas-a".into(), "canvas-a2".into()]),
                selected_panel_id: Some("canvas-a2".into()),
            },
            SessionCanvasPaneSnapshot {
                panel_id: "canvas-b".into(),
                x: 10,
                y: 0,
                width: 10,
                height: 10,
                panel_ids: None,
                selected_panel_id: None,
            },
        ]),
        ..Default::default()
    };
    let snapshot = AppSessionSnapshot {
        version: 1,
        created_at: 0,
        windows: vec![
            window("split-window", split_workspace),
            window("hidden-window", hidden_workspace),
            window("canvas-window", canvas_workspace.clone()),
        ],
    };
    let visible = BTreeSet::from(["split-window".to_owned(), "canvas-window".to_owned()]);
    assert_eq!(
        protected_panel_ids(&snapshot, &visible),
        BTreeSet::from([
            "canvas-a2".to_owned(),
            "canvas-b".to_owned(),
            "split-a2".to_owned(),
            "split-b1".to_owned(),
        ])
    );

    canvas_workspace.zoomed_panel_id = Some("zoom-only".into());
    let zoomed = AppSessionSnapshot {
        version: 1,
        created_at: 0,
        windows: vec![window("canvas-window", canvas_workspace)],
    };
    assert_eq!(
        protected_panel_ids(&zoomed, &BTreeSet::from(["canvas-window".to_owned()])),
        BTreeSet::from(["zoom-only".to_owned()])
    );
}

fn agent(
    kind: &str,
    session_id: &str,
    resume_command: Option<&str>,
) -> SessionRestorableAgentSnapshot {
    SessionRestorableAgentSnapshot {
        kind: kind.into(),
        session_id: session_id.into(),
        working_directory: None,
        launch_command: None,
        resume_command: resume_command.map(str::to_owned),
        fork_command: None,
    }
}

fn dormant_surface(
    panel_id: &str,
    binding: SessionRestorableAgentSnapshot,
) -> SessionSurfaceSnapshot {
    SessionSurfaceSnapshot {
        surface_id: panel_id.into(),
        pane_id: format!("pane-{panel_id}"),
        generation: 1,
        kind: SessionSurfaceKindSnapshot::Terminal,
        metadata: SessionSurfaceMetadataSnapshot::default(),
        terminal_startup: Some(SessionSurfaceTerminalStartupSnapshot {
            command: Some("stale command".into()),
            working_directory: Some("C:/repo".into()),
            initial_input: Some("stale input".into()),
            environment: None,
            tmux_start_command: Some("stale tmux".into()),
            remote_pty_session_id: None,
            resume_binding: Some(Box::new(binding)),
            hibernation: Some(SessionAgentHibernationSnapshot {
                hibernated_at: 100.0,
                last_activity_at: 90.0,
            }),
        }),
        scrollback: Some("preserved scrollback".into()),
    }
}

fn resume_binding(
    panel_id: &str,
    source: &str,
    kind: &str,
    checkpoint: &str,
) -> SessionSurfaceResumeBindingRecordSnapshot {
    SessionSurfaceResumeBindingRecordSnapshot {
        surface_id: panel_id.into(),
        binding: SessionSurfaceResumeBindingSnapshot {
            name: None,
            kind: Some(kind.into()),
            command: "resume".into(),
            cwd: None,
            checkpoint_id: Some(checkpoint.into()),
            source: Some(source.into()),
            environment: None,
            auto_resume: false,
            approval_policy: None,
            approval_record_id: None,
            updated_at: 1.0,
        },
    }
}

#[test]
fn agent_hibernation_contract_invalid_dormancy_sanitizes_agent_authority_but_preserves_manual_bindings(
) {
    let invalid_binding = agent("codex", "session-a", Some("resume codex"));
    let valid_binding = agent("claude_code", "session-b", Some("resume claude"));
    let mut snapshot = AppSessionSnapshot {
        version: 1,
        created_at: 0,
        windows: vec![window(
            "window",
            SessionWorkspaceSnapshot {
                workspace_id: Some("workspace".into()),
                process_title: "shell".into(),
                surfaces: Some(vec![
                    dormant_surface("invalid", invalid_binding.clone()),
                    dormant_surface("valid", valid_binding.clone()),
                ]),
                restorable_agent_snapshots: Some(vec![
                    SessionPanelRestorableAgentSnapshot {
                        panel_id: "invalid".into(),
                        snapshot: agent("codex", "different-session", Some("resume codex")),
                    },
                    SessionPanelRestorableAgentSnapshot {
                        panel_id: "valid".into(),
                        snapshot: valid_binding.clone(),
                    },
                ]),
                surface_resume_bindings: Some(vec![
                    resume_binding("invalid", "agent-hook", "codex", "session-a"),
                    resume_binding("invalid", "manual", "codex", "session-a"),
                    resume_binding("valid", "agent-hook", "claude_code", "session-b"),
                ]),
                ..Default::default()
            },
        )],
    };

    assert_eq!(sanitize_invalid_hibernation(&mut snapshot), vec!["invalid"]);
    let workspace = &snapshot.windows[0].tab_manager.workspaces[0];
    let surfaces = workspace.surfaces.as_ref().unwrap();
    let invalid = surfaces
        .iter()
        .find(|surface| surface.surface_id == "invalid")
        .unwrap();
    let invalid_startup = invalid.terminal_startup.as_ref().unwrap();
    assert!(invalid_startup.hibernation.is_none());
    assert!(invalid_startup.resume_binding.is_none());
    assert!(invalid_startup.command.is_none());
    assert!(invalid_startup.initial_input.is_none());
    assert!(invalid_startup.tmux_start_command.is_none());
    assert_eq!(invalid.scrollback.as_deref(), Some("preserved scrollback"));

    let valid = surfaces
        .iter()
        .find(|surface| surface.surface_id == "valid")
        .unwrap();
    assert!(valid
        .terminal_startup
        .as_ref()
        .unwrap()
        .hibernation
        .is_some());
    assert_eq!(
        workspace
            .restorable_agent_snapshots
            .as_ref()
            .unwrap()
            .iter()
            .map(|row| row.panel_id.as_str())
            .collect::<Vec<_>>(),
        vec!["valid"]
    );
    let bindings = workspace.surface_resume_bindings.as_ref().unwrap();
    assert!(bindings
        .iter()
        .any(|row| row.surface_id == "invalid" && row.binding.source.as_deref() == Some("manual")));
    assert!(!bindings
        .iter()
        .any(|row| row.surface_id == "invalid"
            && row.binding.source.as_deref() == Some("agent-hook")));
    assert!(bindings.iter().any(
        |row| row.surface_id == "valid" && row.binding.source.as_deref() == Some("agent-hook")
    ));
}

#[test]
fn agent_hibernation_contract_blank_resume_authority_is_never_treated_as_valid_dormancy() {
    let mut snapshot = AppSessionSnapshot {
        version: 1,
        created_at: 0,
        windows: vec![window(
            "window",
            SessionWorkspaceSnapshot {
                workspace_id: Some("workspace".into()),
                process_title: "shell".into(),
                surfaces: Some(vec![dormant_surface(
                    "blank",
                    agent("codex", "session", Some("  ")),
                )]),
                ..Default::default()
            },
        )],
    };
    assert_eq!(sanitize_invalid_hibernation(&mut snapshot), vec!["blank"]);
    assert!(snapshot.windows[0].tab_manager.workspaces[0]
        .surfaces
        .as_ref()
        .unwrap()[0]
        .terminal_startup
        .as_ref()
        .unwrap()
        .hibernation
        .is_none());
}
