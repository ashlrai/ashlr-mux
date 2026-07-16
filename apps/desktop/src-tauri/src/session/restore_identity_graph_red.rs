//! Frozen-canonical restore identity and graph-coherence contract.
//!
//! Canonical e1825d40 does not apply one blanket identity rule. A restored
//! window adopts its persisted id when it is available, every `Workspace` and
//! bonsplit pane/split is rebuilt with a fresh id, and a persisted surface id
//! is reused only when the live surface registry has no collision. Groups keep
//! their ids while their workspace anchors follow the old-to-new workspace
//! map. These tests pin that domain-specific policy and the validation which
//! must precede publication.

use super::*;

const WINDOW_A: u128 = 0x101;
const WINDOW_B: u128 = 0x102;
const WORKSPACE_A: u128 = 0x201;
const WORKSPACE_B: u128 = 0x202;
const GROUP_A: u128 = 0x301;
const GROUP_B: u128 = 0x302;
const SPLIT_A: u128 = 0x401;
const SPLIT_B: u128 = 0x402;
const PANE_A1: u128 = 0x501;
const PANE_A2: u128 = 0x502;
const PANE_B1: u128 = 0x503;
const PANE_B2: u128 = 0x504;
const SURFACE_A1: u128 = 0x601;
const SURFACE_A2: u128 = 0x602;
const SURFACE_B1: u128 = 0x603;
const SURFACE_B2: u128 = 0x604;
const DOCK_PANE: u128 = 0x701;
const DOCK_SURFACE: u128 = 0x702;

fn id(value: u128) -> String {
    Uuid::from_u128(value).to_string()
}

fn pane(pane_id: u128, surface_id: u128) -> SessionWorkspaceLayoutSnapshot {
    SessionWorkspaceLayoutSnapshot::Pane(cmux_core::session::SessionPaneLayoutSnapshot {
        pane_id: Some(id(pane_id)),
        panel_ids: vec![id(surface_id)],
        selected_panel_id: Some(id(surface_id)),
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

fn split(
    split_id: u128,
    first_pane: u128,
    first_surface: u128,
    second_pane: u128,
    second_surface: u128,
) -> SessionWorkspaceLayoutSnapshot {
    SessionWorkspaceLayoutSnapshot::Split(cmux_core::session::SessionSplitLayoutSnapshot {
        split_id: Some(id(split_id)),
        orientation: SessionSplitOrientation::Horizontal,
        divider_position: 0.5,
        first: Box::new(pane(first_pane, first_surface)),
        second: Box::new(pane(second_pane, second_surface)),
    })
}

fn terminal_surface(surface_id: u128, pane_id: u128) -> cmux_core::session::SessionSurfaceSnapshot {
    cmux_core::session::SessionSurfaceSnapshot {
        surface_id: id(surface_id),
        pane_id: id(pane_id),
        generation: 1,
        kind: cmux_core::session::SessionSurfaceKindSnapshot::Terminal,
        metadata: Default::default(),
        terminal_startup: None,
        scrollback: None,
    }
}

fn basic_workspace(
    workspace_id: u128,
    group_id: u128,
    split_id: u128,
    first_pane: u128,
    first_surface: u128,
    second_pane: u128,
    second_surface: u128,
) -> SessionWorkspaceSnapshot {
    SessionWorkspaceSnapshot {
        workspace_id: Some(id(workspace_id)),
        process_title: "terminal".into(),
        layout: Some(split(
            split_id,
            first_pane,
            first_surface,
            second_pane,
            second_surface,
        )),
        zoomed_panel_id: Some(id(first_surface)),
        focused_panel_id: Some(id(first_surface)),
        focused_pane_id: Some(id(first_pane)),
        surfaces: Some(vec![
            terminal_surface(first_surface, first_pane),
            terminal_surface(second_surface, second_pane),
        ]),
        group_id: Some(id(group_id)),
        ..Default::default()
    }
}

fn rich_workspace_a() -> SessionWorkspaceSnapshot {
    let opaque = id(SURFACE_A1);
    let environment = BTreeMap::from([(opaque.clone(), opaque.clone())]);
    let mut workspace = basic_workspace(
        WORKSPACE_A,
        GROUP_A,
        SPLIT_A,
        PANE_A1,
        SURFACE_A1,
        PANE_A2,
        SURFACE_A2,
    );
    workspace.process_title = opaque.clone();
    workspace.custom_title = Some(opaque.clone());
    workspace.custom_title_source = Some(opaque.clone());
    workspace.custom_description = Some(opaque.clone());
    workspace.custom_color = Some(opaque.clone());
    workspace.current_directory = Some(opaque.clone());
    workspace.initial_terminal_command = Some(opaque.clone());
    workspace.initial_terminal_input = Some(opaque.clone());
    workspace.initial_terminal_environment = Some(environment.clone());
    workspace.workspace_environment = Some(environment.clone());
    workspace.pending_remote_pwds =
        Some(vec![cmux_core::session::SessionPendingRemotePwdSnapshot {
            remote_session_id: opaque.clone(),
            path: opaque.clone(),
        }]);
    workspace.pending_surface_pwds =
        Some(vec![cmux_core::session::SessionPendingSurfacePwdSnapshot {
            surface_id: id(SURFACE_A1),
            generation: 1,
            path: opaque.clone(),
        }]);
    workspace.panel_titles = Some(vec![cmux_core::session::SessionPanelTitleSnapshot {
        panel_id: id(SURFACE_A1),
        custom_title: Some(opaque.clone()),
    }]);
    workspace.panel_pins = Some(vec![cmux_core::session::SessionPanelPinSnapshot {
        panel_id: id(SURFACE_A1),
        is_pinned: true,
    }]);
    workspace.panel_unreads = Some(vec![cmux_core::session::SessionPanelUnreadSnapshot {
        panel_id: id(SURFACE_A1),
        is_unread: true,
        unread_at: Some(1),
    }]);
    workspace.restorable_agent_snapshots = Some(vec![SessionPanelRestorableAgentSnapshot {
        panel_id: id(SURFACE_A1),
        snapshot: SessionRestorableAgentSnapshot {
            kind: opaque.clone(),
            session_id: opaque.clone(),
            working_directory: Some(opaque.clone()),
            launch_command: Some(AgentLaunchCommandSnapshot {
                launcher: Some(opaque.clone()),
                executable_path: Some(opaque.clone()),
                arguments: vec![opaque.clone()],
                working_directory: Some(opaque.clone()),
                environment: Some(environment.clone()),
                source: Some(opaque.clone()),
            }),
            resume_command: Some(opaque.clone()),
            fork_command: Some(opaque.clone()),
        },
    }]);
    workspace.surface_resume_bindings = Some(vec![
        cmux_core::session::SessionSurfaceResumeBindingRecordSnapshot {
            surface_id: id(SURFACE_A1),
            binding: cmux_core::session::SessionSurfaceResumeBindingSnapshot {
                name: Some(opaque.clone()),
                kind: Some(opaque.clone()),
                command: opaque.clone(),
                cwd: Some(opaque.clone()),
                checkpoint_id: Some(opaque.clone()),
                source: Some(opaque.clone()),
                environment: Some(environment.clone()),
                auto_resume: true,
                approval_policy: Some(opaque.clone()),
                approval_record_id: Some(opaque.clone()),
                updated_at: 1.0,
            },
        },
    ]);
    workspace.published_pane_selections = Some(vec![
        cmux_core::session::SessionPanePublishedSelectionSnapshot {
            pane_id: id(PANE_A1),
            panel_id: id(SURFACE_A1),
        },
    ]);
    workspace.git_branch = Some(SessionGitBranchSnapshot {
        branch: opaque.clone(),
        is_dirty: true,
    });
    workspace.panel_git_branches = Some(vec![SessionPanelGitBranchSnapshot {
        panel_id: id(SURFACE_A1),
        branch: opaque.clone(),
        is_dirty: true,
    }]);
    workspace.panel_pull_requests = Some(vec![SessionPanelPullRequestSnapshot {
        panel_id: id(SURFACE_A1),
        number: 1,
        label: opaque.clone(),
        url: opaque.clone(),
        status: SessionPullRequestStatusSnapshot::Open,
        branch: Some(opaque.clone()),
        is_stale: false,
    }]);
    workspace.panel_listening_ports = Some(vec![SessionPanelListeningPortsSnapshot {
        panel_id: id(SURFACE_A1),
        ports: vec![1234],
    }]);
    workspace.panel_ttys = Some(vec![SessionPanelTtySnapshot {
        panel_id: id(SURFACE_A1),
        tty: opaque.clone(),
        updated_at: 1,
    }]);
    workspace.panel_shell_activity = Some(vec![SessionPanelShellActivitySnapshot {
        panel_id: id(SURFACE_A1),
        state: SessionPanelShellActivityStateSnapshot::PromptIdle,
        updated_at: 1,
    }]);
    workspace.panel_terminal_startups = Some(vec![SessionPanelTerminalStartupSnapshot {
        panel_id: id(SURFACE_A1),
        initial_terminal_command: Some(opaque.clone()),
        initial_terminal_input: Some(opaque.clone()),
        initial_terminal_environment: Some(environment.clone()),
    }]);
    workspace.canvas_panes = Some(vec![cmux_core::session::SessionCanvasPaneSnapshot {
        panel_id: id(SURFACE_A1),
        x: 1,
        y: 2,
        width: 3,
        height: 4,
        panel_ids: Some(vec![id(SURFACE_A1)]),
        selected_panel_id: Some(id(SURFACE_A1)),
    }]);
    workspace.remote = Some(SessionWorkspaceRemoteSnapshot {
        enabled: true,
        state: opaque.clone(),
        connected: true,
        transport: Some(opaque.clone()),
        destination: Some(opaque.clone()),
        port: Some(22),
        local_proxy_port: Some(8080),
        persistent_daemon_slot: Some(opaque.clone()),
        has_ssh_options: true,
        detail: Some(opaque.clone()),
        daemon: Some(SessionWorkspaceRemoteDaemonSnapshot {
            state: opaque.clone(),
            capabilities: vec![opaque.clone()],
        }),
        proxy: Some(SessionWorkspaceRemoteProxySnapshot {
            state: opaque.clone(),
            host: Some(opaque.clone()),
            port: Some(8080),
            schemes: vec![opaque.clone()],
            url: Some(opaque.clone()),
            error_code: Some(opaque.clone()),
        }),
        detected_ports: vec![1],
        forwarded_ports: vec![2],
        conflicted_ports: vec![3],
        active_terminal_sessions: Some(1),
    });
    workspace.sidebar_progress = Some(SessionWorkspaceSidebarProgressSnapshot {
        value: 0.5,
        label: Some(opaque.clone()),
    });
    workspace.sidebar_status_entries = Some(vec![SessionWorkspaceSidebarStatusSnapshot {
        key: opaque.clone(),
        value: opaque.clone(),
        priority: Some(1),
        updated_at: 1,
    }]);
    workspace.sidebar_metadata_entries = Some(vec![SessionWorkspaceSidebarMetadataSnapshot {
        key: opaque.clone(),
        value: opaque.clone(),
        icon: Some(opaque.clone()),
        color: Some(opaque.clone()),
        url: Some(opaque.clone()),
        priority: Some(1),
        format: Some(opaque.clone()),
        updated_at: 1,
    }]);
    workspace.sidebar_metadata_blocks = Some(vec![SessionWorkspaceSidebarMetadataBlockSnapshot {
        key: opaque.clone(),
        markdown: opaque.clone(),
        priority: Some(1),
        updated_at: 1,
    }]);
    workspace.sidebar_log_entries = Some(vec![SessionWorkspaceSidebarLogEntrySnapshot {
        level: opaque.clone(),
        message: opaque.clone(),
        created_at: 1,
    }]);

    let SessionWorkspaceLayoutSnapshot::Split(layout) = workspace.layout.as_mut().unwrap() else {
        unreachable!()
    };
    let SessionWorkspaceLayoutSnapshot::Pane(first) = layout.first.as_mut() else {
        unreachable!()
    };
    first.browser_url = Some(opaque.clone());
    first.browser_proxy_url = Some(opaque.clone());
    first.browser_back_history = Some(vec![opaque.clone()]);
    first.browser_forward_history = Some(vec![opaque.clone()]);
    first.browser_developer_tools_panel = Some(opaque.clone());

    let surface = &mut workspace.surfaces.as_mut().unwrap()[0];
    surface.kind = cmux_core::session::SessionSurfaceKindSnapshot::RemoteTerminal {
        remote_session_id: Some(opaque.clone()),
        remote_context: Some(serde_json::json!({
            "surface_id": opaque,
            "nested": {"panel_id": id(SURFACE_A1)}
        })),
        arrival_generation: Some(1),
    };
    surface.metadata = cmux_core::session::SessionSurfaceMetadataSnapshot {
        custom_title: Some(id(SURFACE_A1)),
        runtime_title: None,
        pinned: true,
        unread: true,
        unread_at: Some(1),
        reported_directory: Some(id(SURFACE_A1)),
        directory_provenance: Some(id(SURFACE_A1)),
    };
    surface.terminal_startup = Some(cmux_core::session::SessionSurfaceTerminalStartupSnapshot {
        command: Some(id(SURFACE_A1)),
        working_directory: Some(id(SURFACE_A1)),
        initial_input: Some(id(SURFACE_A1)),
        environment: Some(environment),
        tmux_start_command: Some(id(SURFACE_A1)),
        remote_pty_session_id: Some(id(SURFACE_A1)),
        resume_binding: Some(Box::new(SessionRestorableAgentSnapshot {
            kind: id(SURFACE_A1),
            session_id: id(SURFACE_A1),
            working_directory: Some(id(SURFACE_A1)),
            launch_command: None,
            resume_command: Some(id(SURFACE_A1)),
            fork_command: Some(id(SURFACE_A1)),
        })),
        hibernation: None,
    });
    workspace
}

fn group(group_id: u128, workspace_id: u128) -> cmux_core::session::SessionWorkspaceGroupSnapshot {
    cmux_core::session::SessionWorkspaceGroupSnapshot {
        id: id(group_id),
        name: "group".into(),
        is_collapsed: false,
        anchor_workspace_id: Some(id(workspace_id)),
        anchor_member_index: Some(0),
        is_pinned: None,
        custom_color: None,
        icon_symbol: None,
    }
}

fn graph_fixture() -> AppSessionSnapshot {
    let window_a_id = id(WINDOW_A);
    AppSessionSnapshot {
        version: SESSION_SNAPSHOT_SCHEMA_VERSION,
        created_at: 1,
        windows: vec![
            SessionWindowSnapshot {
                window_id: Some(window_a_id.clone()),
                selected_workspace_id: Some(id(WORKSPACE_A)),
                dock: Some(cmux_core::session::SessionDockSnapshot {
                    workspace_id: format!("dock:{window_a_id}"),
                    layout: Some(pane(DOCK_PANE, DOCK_SURFACE)),
                    surfaces: vec![terminal_surface(DOCK_SURFACE, DOCK_PANE)],
                    focused_surface_id: Some(id(DOCK_SURFACE)),
                }),
                tab_manager: SessionTabManagerSnapshot {
                    selected_workspace_index: Some(0),
                    workspaces: vec![rich_workspace_a()],
                    workspace_groups: Some(vec![group(GROUP_A, WORKSPACE_A)]),
                },
            },
            SessionWindowSnapshot {
                window_id: Some(id(WINDOW_B)),
                selected_workspace_id: Some(id(WORKSPACE_B)),
                dock: None,
                tab_manager: SessionTabManagerSnapshot {
                    selected_workspace_index: Some(0),
                    workspaces: vec![basic_workspace(
                        WORKSPACE_B,
                        GROUP_B,
                        SPLIT_B,
                        PANE_B1,
                        SURFACE_B1,
                        PANE_B2,
                        SURFACE_B2,
                    )],
                    workspace_groups: Some(vec![group(GROUP_B, WORKSPACE_B)]),
                },
            },
        ],
    }
}

fn workspace_mut(
    snapshot: &mut AppSessionSnapshot,
    window: usize,
) -> &mut SessionWorkspaceSnapshot {
    &mut snapshot.windows[window].tab_manager.workspaces[0]
}

fn split_mut(
    snapshot: &mut AppSessionSnapshot,
    window: usize,
) -> &mut cmux_core::session::SessionSplitLayoutSnapshot {
    let SessionWorkspaceLayoutSnapshot::Split(split) =
        workspace_mut(snapshot, window).layout.as_mut().unwrap()
    else {
        panic!("split fixture")
    };
    split
}

fn pane_mut(
    snapshot: &mut AppSessionSnapshot,
    window: usize,
    pane_index: usize,
) -> &mut cmux_core::session::SessionPaneLayoutSnapshot {
    let split = split_mut(snapshot, window);
    let node = if pane_index == 0 {
        split.first.as_mut()
    } else {
        split.second.as_mut()
    };
    let SessionWorkspaceLayoutSnapshot::Pane(pane) = node else {
        panic!("pane fixture")
    };
    pane
}

fn opaque_witness(snapshot: &AppSessionSnapshot) -> serde_json::Value {
    let workspace = &snapshot.windows[0].tab_manager.workspaces[0];
    let SessionWorkspaceLayoutSnapshot::Split(split) = workspace.layout.as_ref().unwrap() else {
        panic!("split fixture")
    };
    let SessionWorkspaceLayoutSnapshot::Pane(pane) = split.first.as_ref() else {
        panic!("pane fixture")
    };
    let surface = &workspace.surfaces.as_deref().unwrap()[0];
    serde_json::json!({
        "workspace": {
            "process_title": workspace.process_title,
            "custom_title": workspace.custom_title,
            "custom_title_source": workspace.custom_title_source,
            "custom_description": workspace.custom_description,
            "custom_color": workspace.custom_color,
            "current_directory": workspace.current_directory,
            "initial_terminal_command": workspace.initial_terminal_command,
            "initial_terminal_input": workspace.initial_terminal_input,
            "initial_terminal_environment": workspace.initial_terminal_environment,
            "workspace_environment": workspace.workspace_environment,
        },
        "browser": {
            "url": pane.browser_url,
            "proxy": pane.browser_proxy_url,
            "back": pane.browser_back_history,
            "forward": pane.browser_forward_history,
            "devtools": pane.browser_developer_tools_panel,
        },
        "surface_kind": surface.kind,
        "surface_metadata": surface.metadata,
        "terminal_startup": surface.terminal_startup,
        "pending_remote": workspace.pending_remote_pwds,
        "agent": workspace.restorable_agent_snapshots.as_ref().unwrap()[0].snapshot,
        "resume": workspace.surface_resume_bindings.as_ref().unwrap()[0].binding,
        "remote": workspace.remote,
        "sidebar": {
            "progress": workspace.sidebar_progress,
            "status": workspace.sidebar_status_entries,
            "metadata": workspace.sidebar_metadata_entries,
            "blocks": workspace.sidebar_metadata_blocks,
            "logs": workspace.sidebar_log_entries,
        },
        "row_text": {
            "title": workspace.panel_titles.as_ref().unwrap()[0].custom_title,
            "git": workspace.panel_git_branches.as_ref().unwrap()[0].branch,
            "pull": workspace.panel_pull_requests.as_ref().unwrap()[0],
            "tty": workspace.panel_ttys.as_ref().unwrap()[0].tty,
            "startup": workspace.panel_terminal_startups.as_ref().unwrap()[0],
        }
    })
}

#[derive(Default)]
struct RecordingPublication {
    calls: Vec<&'static str>,
}

impl SnapshotPublicationOperations for RecordingPublication {
    fn persist(&mut self, _candidate: &AppSessionSnapshot) -> Result<(), String> {
        self.calls.push("persist");
        Ok(())
    }

    fn update_event_baseline(&mut self, _candidate: &AppSessionSnapshot) {
        self.calls.push("baseline");
    }

    fn emit(&mut self, _candidate: &AppSessionSnapshot) -> Result<(), String> {
        self.calls.push("emit");
        Ok(())
    }
}

struct RestoreAttempt {
    result: Result<AppSessionSnapshot, String>,
    calls: Vec<&'static str>,
    authority: AppSessionSnapshot,
    current: AppSessionSnapshot,
    next_panel: u64,
}

fn restore_attempt(restored: AppSessionSnapshot) -> RestoreAttempt {
    let current = initial_snapshot("surface-90");
    let authority = GatedSnapshot::new(current.clone());
    let next_panel = AtomicU64::new(91);
    let mut publication = RecordingPublication::default();
    let result =
        restore_previous_launch_transaction(&authority, &next_panel, &mut publication, || {
            Some(restored)
        })
        .map(|outcome| outcome.snapshot);
    let authoritative_snapshot = authority.lock().unwrap().clone();
    RestoreAttempt {
        result,
        calls: publication.calls,
        authority: authoritative_snapshot,
        current,
        next_panel: next_panel.load(Ordering::Relaxed),
    }
}

fn restore_commits_atomically(restored: AppSessionSnapshot) -> Result<AppSessionSnapshot, String> {
    let attempt = restore_attempt(restored);
    let mut committed = attempt.result?;
    if attempt.calls != ["baseline", "emit"] {
        return Err(format!("publication calls were {:?}", attempt.calls));
    }
    if attempt.authority != committed {
        return Err("published snapshot did not become authoritative".into());
    }
    let expected_next_panel = next_panel_counter(&committed);
    if attempt.next_panel != expected_next_panel {
        return Err(format!(
            "next-panel reseed was {}, expected {expected_next_panel}",
            attempt.next_panel
        ));
    }
    let live_window_count = attempt.current.windows.len();
    if committed.windows.get(..live_window_count) != Some(attempt.current.windows.as_slice()) {
        return Err("additive restore changed the live window prefix".into());
    }
    committed.windows.drain(..live_window_count);
    Ok(committed)
}

fn restore_rejects_without_publication(restored: AppSessionSnapshot) -> bool {
    let attempt = restore_attempt(restored);
    attempt.result.is_err()
        && attempt.calls.is_empty()
        && attempt.authority == attempt.current
        && attempt.next_panel == 91
}

type Mutation = fn(&mut AppSessionSnapshot);
type Validation = fn(&AppSessionSnapshot) -> bool;

fn workspace(snapshot: &AppSessionSnapshot, window: usize) -> &SessionWorkspaceSnapshot {
    &snapshot.windows[window].tab_manager.workspaces[0]
}

fn pane_at(
    snapshot: &AppSessionSnapshot,
    window: usize,
    pane_index: usize,
) -> &cmux_core::session::SessionPaneLayoutSnapshot {
    let SessionWorkspaceLayoutSnapshot::Split(split) =
        workspace(snapshot, window).layout.as_ref().unwrap()
    else {
        panic!("split fixture")
    };
    let node = if pane_index == 0 {
        &split.first
    } else {
        &split.second
    };
    let SessionWorkspaceLayoutSnapshot::Pane(pane) = node.as_ref() else {
        panic!("pane fixture")
    };
    pane
}

fn first_pane_selects_first_panel(snapshot: &AppSessionSnapshot) -> bool {
    let pane = pane_at(snapshot, 0, 0);
    pane.selected_panel_id.as_ref() == pane.panel_ids.first()
}

fn focus_is_local_and_coherent(snapshot: &AppSessionSnapshot) -> bool {
    let workspace = workspace(snapshot, 0);
    let Some(panel) = workspace.focused_panel_id.as_deref() else {
        return false;
    };
    let Some(pane) = workspace.focused_pane_id.as_deref() else {
        return false;
    };
    [pane_at(snapshot, 0, 0), pane_at(snapshot, 0, 1)]
        .into_iter()
        .any(|candidate| {
            candidate.pane_id.as_deref() == Some(pane)
                && candidate
                    .panel_ids
                    .iter()
                    .any(|candidate| candidate == panel)
        })
}

#[test]
fn uuid_only_stale_reference_is_normalized_before_atomic_publication() {
    let mut restored = graph_fixture();
    // Remove the Windows-only `dock:<owner>` derived id so every remaining
    // structural definition is UUID-shaped. This specifically guards against
    // an early return based on "no legacy replacements needed".
    restored.windows[0].dock = None;
    workspace_mut(&mut restored, 0).focused_panel_id = Some(id(0xffff));
    let committed = restore_commits_atomically(restored)
        .expect("UUID-shaped definitions still require canonical normalization");
    assert!(focus_is_local_and_coherent(&committed));
    assert_ne!(
        workspace(&committed, 0).focused_panel_id.as_deref(),
        Some(id(0xffff).as_str())
    );
}

#[test]
fn every_typed_reference_uses_its_canonical_fallback_or_pruning_rule() {
    let cases: &[(&str, Mutation, Validation)] = &[
        (
            "window.selected_workspace_id",
            |snapshot| {
                snapshot.windows[0].selected_workspace_id = Some(id(WORKSPACE_B));
            },
            |snapshot| {
                snapshot.windows[0].selected_workspace_id == workspace(snapshot, 0).workspace_id
            },
        ),
        (
            "group.anchor_workspace_id",
            |snapshot| {
                snapshot.windows[0]
                    .tab_manager
                    .workspace_groups
                    .as_mut()
                    .unwrap()[0]
                    .anchor_workspace_id = Some(id(WORKSPACE_B));
            },
            |snapshot| {
                snapshot.windows[0]
                    .tab_manager
                    .workspace_groups
                    .as_ref()
                    .unwrap()[0]
                    .anchor_workspace_id
                    == workspace(snapshot, 0).workspace_id
            },
        ),
        (
            "workspace.group_id",
            |snapshot| {
                workspace_mut(snapshot, 0).group_id = Some(id(GROUP_B));
            },
            |snapshot| workspace(snapshot, 0).group_id.is_none(),
        ),
        (
            "pane.selected_panel_id",
            |snapshot| {
                pane_mut(snapshot, 0, 0).selected_panel_id = Some(id(SURFACE_B1));
            },
            first_pane_selects_first_panel,
        ),
        (
            "pane.selected_panel_id wrong sibling",
            |snapshot| {
                pane_mut(snapshot, 0, 0).selected_panel_id = Some(id(SURFACE_A2));
            },
            first_pane_selects_first_panel,
        ),
        (
            "workspace.zoomed_panel_id",
            |snapshot| {
                workspace_mut(snapshot, 0).zoomed_panel_id = Some(id(SURFACE_B1));
            },
            |snapshot| workspace(snapshot, 0).zoomed_panel_id.is_none(),
        ),
        (
            "workspace.focused_panel_id",
            |snapshot| {
                workspace_mut(snapshot, 0).focused_panel_id = Some(id(SURFACE_B1));
            },
            focus_is_local_and_coherent,
        ),
        (
            "workspace.focused_pane_id",
            |snapshot| {
                workspace_mut(snapshot, 0).focused_pane_id = Some(id(PANE_B1));
            },
            focus_is_local_and_coherent,
        ),
        (
            "surface.pane_id",
            |snapshot| {
                workspace_mut(snapshot, 0).surfaces.as_mut().unwrap()[0].pane_id = id(PANE_B1);
            },
            |snapshot| {
                workspace(snapshot, 0).surfaces.as_ref().unwrap()[0].pane_id
                    == pane_at(snapshot, 0, 0).pane_id.as_deref().unwrap()
            },
        ),
        (
            "surface.pane_id wrong sibling",
            |snapshot| {
                workspace_mut(snapshot, 0).surfaces.as_mut().unwrap()[0].pane_id = id(PANE_A2);
            },
            |snapshot| {
                workspace(snapshot, 0).surfaces.as_ref().unwrap()[0].pane_id
                    == pane_at(snapshot, 0, 0).pane_id.as_deref().unwrap()
            },
        ),
        (
            "pending_surface_pwds.surface_id",
            |snapshot| {
                workspace_mut(snapshot, 0)
                    .pending_surface_pwds
                    .as_mut()
                    .unwrap()[0]
                    .surface_id = id(SURFACE_B1);
            },
            |snapshot| {
                workspace(snapshot, 0)
                    .pending_surface_pwds
                    .as_ref()
                    .is_none_or(Vec::is_empty)
            },
        ),
        (
            "panel_titles.panel_id",
            |snapshot| {
                workspace_mut(snapshot, 0).panel_titles.as_mut().unwrap()[0].panel_id =
                    id(SURFACE_B1);
            },
            |snapshot| {
                workspace(snapshot, 0)
                    .panel_titles
                    .as_ref()
                    .is_none_or(Vec::is_empty)
            },
        ),
        (
            "panel_pins.panel_id",
            |snapshot| {
                workspace_mut(snapshot, 0).panel_pins.as_mut().unwrap()[0].panel_id =
                    id(SURFACE_B1);
            },
            |snapshot| {
                workspace(snapshot, 0)
                    .panel_pins
                    .as_ref()
                    .is_none_or(Vec::is_empty)
            },
        ),
        (
            "panel_unreads.panel_id",
            |snapshot| {
                workspace_mut(snapshot, 0).panel_unreads.as_mut().unwrap()[0].panel_id =
                    id(SURFACE_B1);
            },
            |snapshot| {
                workspace(snapshot, 0)
                    .panel_unreads
                    .as_ref()
                    .is_none_or(Vec::is_empty)
            },
        ),
        (
            "restorable_agent_snapshots.panel_id",
            |snapshot| {
                workspace_mut(snapshot, 0)
                    .restorable_agent_snapshots
                    .as_mut()
                    .unwrap()[0]
                    .panel_id = id(SURFACE_B1);
            },
            |snapshot| {
                workspace(snapshot, 0)
                    .restorable_agent_snapshots
                    .as_ref()
                    .is_none_or(Vec::is_empty)
            },
        ),
        (
            "surface_resume_bindings.surface_id",
            |snapshot| {
                workspace_mut(snapshot, 0)
                    .surface_resume_bindings
                    .as_mut()
                    .unwrap()[0]
                    .surface_id = id(SURFACE_B1);
            },
            |snapshot| {
                workspace(snapshot, 0)
                    .surface_resume_bindings
                    .as_ref()
                    .is_none_or(Vec::is_empty)
            },
        ),
        (
            "published_pane_selections.pane_id",
            |snapshot| {
                workspace_mut(snapshot, 0)
                    .published_pane_selections
                    .as_mut()
                    .unwrap()[0]
                    .pane_id = id(PANE_B1);
            },
            |snapshot| {
                workspace(snapshot, 0)
                    .published_pane_selections
                    .as_ref()
                    .is_none_or(Vec::is_empty)
            },
        ),
        (
            "published_pane_selections.panel_id",
            |snapshot| {
                workspace_mut(snapshot, 0)
                    .published_pane_selections
                    .as_mut()
                    .unwrap()[0]
                    .panel_id = id(SURFACE_B1);
            },
            |snapshot| {
                workspace(snapshot, 0)
                    .published_pane_selections
                    .as_ref()
                    .is_none_or(Vec::is_empty)
            },
        ),
        (
            "published selection mismatched pair",
            |snapshot| {
                workspace_mut(snapshot, 0)
                    .published_pane_selections
                    .as_mut()
                    .unwrap()[0]
                    .panel_id = id(SURFACE_A2);
            },
            |snapshot| {
                workspace(snapshot, 0)
                    .published_pane_selections
                    .as_ref()
                    .is_none_or(Vec::is_empty)
            },
        ),
        (
            "panel_git_branches.panel_id",
            |snapshot| {
                workspace_mut(snapshot, 0)
                    .panel_git_branches
                    .as_mut()
                    .unwrap()[0]
                    .panel_id = id(SURFACE_B1);
            },
            |snapshot| {
                workspace(snapshot, 0)
                    .panel_git_branches
                    .as_ref()
                    .is_none_or(Vec::is_empty)
            },
        ),
        (
            "panel_pull_requests.panel_id",
            |snapshot| {
                workspace_mut(snapshot, 0)
                    .panel_pull_requests
                    .as_mut()
                    .unwrap()[0]
                    .panel_id = id(SURFACE_B1);
            },
            |snapshot| {
                workspace(snapshot, 0)
                    .panel_pull_requests
                    .as_ref()
                    .is_none_or(Vec::is_empty)
            },
        ),
        (
            "panel_listening_ports.panel_id",
            |snapshot| {
                workspace_mut(snapshot, 0)
                    .panel_listening_ports
                    .as_mut()
                    .unwrap()[0]
                    .panel_id = id(SURFACE_B1);
            },
            |snapshot| {
                workspace(snapshot, 0)
                    .panel_listening_ports
                    .as_ref()
                    .is_none_or(Vec::is_empty)
            },
        ),
        (
            "panel_ttys.panel_id",
            |snapshot| {
                workspace_mut(snapshot, 0).panel_ttys.as_mut().unwrap()[0].panel_id =
                    id(SURFACE_B1);
            },
            |snapshot| {
                workspace(snapshot, 0)
                    .panel_ttys
                    .as_ref()
                    .is_none_or(Vec::is_empty)
            },
        ),
        (
            "panel_shell_activity.panel_id",
            |snapshot| {
                workspace_mut(snapshot, 0)
                    .panel_shell_activity
                    .as_mut()
                    .unwrap()[0]
                    .panel_id = id(SURFACE_B1);
            },
            |snapshot| {
                workspace(snapshot, 0)
                    .panel_shell_activity
                    .as_ref()
                    .is_none_or(Vec::is_empty)
            },
        ),
        (
            "panel_terminal_startups.panel_id",
            |snapshot| {
                workspace_mut(snapshot, 0)
                    .panel_terminal_startups
                    .as_mut()
                    .unwrap()[0]
                    .panel_id = id(SURFACE_B1);
            },
            |snapshot| {
                workspace(snapshot, 0)
                    .panel_terminal_startups
                    .as_ref()
                    .is_none_or(Vec::is_empty)
            },
        ),
        (
            "canvas.panel_id",
            |snapshot| {
                workspace_mut(snapshot, 0).canvas_panes.as_mut().unwrap()[0].panel_id =
                    id(SURFACE_B1);
            },
            |snapshot| {
                workspace(snapshot, 0).canvas_panes.as_ref().unwrap()[0].panel_id == id(SURFACE_A1)
            },
        ),
        (
            "canvas.panel_ids",
            |snapshot| {
                workspace_mut(snapshot, 0).canvas_panes.as_mut().unwrap()[0].panel_ids =
                    Some(vec![id(SURFACE_B1)]);
            },
            |snapshot| {
                workspace(snapshot, 0)
                    .canvas_panes
                    .as_ref()
                    .is_none_or(Vec::is_empty)
            },
        ),
        (
            "canvas.selected_panel_id",
            |snapshot| {
                workspace_mut(snapshot, 0).canvas_panes.as_mut().unwrap()[0].selected_panel_id =
                    Some(id(SURFACE_B1));
            },
            |snapshot| {
                let canvas = &workspace(snapshot, 0).canvas_panes.as_ref().unwrap()[0];
                canvas.selected_panel_id.as_ref() == canvas.panel_ids.as_ref().unwrap().first()
            },
        ),
        (
            "canvas selected outside panel_ids",
            |snapshot| {
                workspace_mut(snapshot, 0).canvas_panes.as_mut().unwrap()[0].selected_panel_id =
                    Some(id(SURFACE_A2));
            },
            |snapshot| {
                let canvas = &workspace(snapshot, 0).canvas_panes.as_ref().unwrap()[0];
                canvas.selected_panel_id.as_ref() == canvas.panel_ids.as_ref().unwrap().first()
            },
        ),
        (
            "dock.workspace_id owner",
            |snapshot| {
                snapshot.windows[0].dock.as_mut().unwrap().workspace_id =
                    format!("dock:{}", id(WINDOW_B));
            },
            |snapshot| snapshot.windows[0].dock.is_none(),
        ),
        (
            "dock.layout.selected_panel_id",
            |snapshot| {
                let dock = snapshot.windows[0].dock.as_mut().unwrap();
                let SessionWorkspaceLayoutSnapshot::Pane(pane) = dock.layout.as_mut().unwrap()
                else {
                    panic!("dock pane")
                };
                pane.selected_panel_id = Some(id(SURFACE_A1));
            },
            |snapshot| snapshot.windows[0].dock.is_none(),
        ),
        (
            "dock.surface.pane_id",
            |snapshot| {
                snapshot.windows[0].dock.as_mut().unwrap().surfaces[0].pane_id = id(PANE_A1);
            },
            |snapshot| snapshot.windows[0].dock.is_none(),
        ),
        (
            "dock.focused_surface_id",
            |snapshot| {
                snapshot.windows[0]
                    .dock
                    .as_mut()
                    .unwrap()
                    .focused_surface_id = Some(id(SURFACE_A1));
            },
            |snapshot| snapshot.windows[0].dock.is_none(),
        ),
    ];

    let mut wrong = Vec::new();
    for (name, mutation, validation) in cases {
        let mut restored = graph_fixture();
        mutation(&mut restored);
        match restore_commits_atomically(restored) {
            Ok(committed) if validation(&committed) => {}
            Ok(_) => wrong.push(format!("{name}: wrong normalized value")),
            Err(error) => wrong.push(format!("{name}: rejected ({error})")),
        }
    }
    assert!(
        wrong.is_empty(),
        "restore diverged from canonical typed-reference behavior:\n{}",
        wrong.join("\n")
    );
}

fn replace_workspace_surface(
    snapshot: &mut AppSessionSnapshot,
    window: usize,
    old: &str,
    replacement: &str,
) {
    let workspace = workspace_mut(snapshot, window);
    let replace = |value: &mut String| {
        if value == old {
            *value = replacement.to_string();
        }
    };
    fn replace_layout(layout: &mut SessionWorkspaceLayoutSnapshot, old: &str, replacement: &str) {
        match layout {
            SessionWorkspaceLayoutSnapshot::Pane(pane) => {
                for panel_id in &mut pane.panel_ids {
                    if panel_id == old {
                        *panel_id = replacement.to_string();
                    }
                }
                if pane.selected_panel_id.as_deref() == Some(old) {
                    pane.selected_panel_id = Some(replacement.to_string());
                }
            }
            SessionWorkspaceLayoutSnapshot::Split(split) => {
                replace_layout(&mut split.first, old, replacement);
                replace_layout(&mut split.second, old, replacement);
            }
        }
    }
    replace_layout(workspace.layout.as_mut().unwrap(), old, replacement);
    for surface in workspace.surfaces.as_mut().unwrap() {
        replace(&mut surface.surface_id);
    }
    if let Some(value) = workspace.focused_panel_id.as_mut() {
        replace(value);
    }
    if let Some(value) = workspace.zoomed_panel_id.as_mut() {
        replace(value);
    }
}

#[test]
fn duplicate_definitions_follow_their_domain_specific_collision_policy() {
    let cases: &[(&str, Mutation)] = &[
        ("duplicate UUID window", |snapshot| {
            snapshot.windows[1].window_id = snapshot.windows[0].window_id.clone();
        }),
        ("duplicate legacy window", |snapshot| {
            snapshot.windows[0].window_id = Some("legacy-window".into());
            snapshot.windows[1].window_id = Some("legacy-window".into());
        }),
        ("duplicate UUID workspace", |snapshot| {
            let duplicate = snapshot.windows[0].tab_manager.workspaces[0]
                .workspace_id
                .clone();
            snapshot.windows[1].tab_manager.workspaces[0].workspace_id = duplicate.clone();
            snapshot.windows[1].selected_workspace_id = duplicate.clone();
            snapshot.windows[1]
                .tab_manager
                .workspace_groups
                .as_mut()
                .unwrap()[0]
                .anchor_workspace_id = duplicate;
        }),
        ("duplicate legacy workspace", |snapshot| {
            for window in &mut snapshot.windows {
                window.tab_manager.workspaces[0].workspace_id = Some("legacy-workspace".into());
                window.selected_workspace_id = Some("legacy-workspace".into());
                window.tab_manager.workspace_groups.as_mut().unwrap()[0].anchor_workspace_id =
                    Some("legacy-workspace".into());
            }
        }),
        ("duplicate UUID group", |snapshot| {
            snapshot.windows[1]
                .tab_manager
                .workspace_groups
                .as_mut()
                .unwrap()[0]
                .id = id(GROUP_A);
            workspace_mut(snapshot, 1).group_id = Some(id(GROUP_A));
        }),
        ("duplicate legacy group", |snapshot| {
            for window in 0..2 {
                snapshot.windows[window]
                    .tab_manager
                    .workspace_groups
                    .as_mut()
                    .unwrap()[0]
                    .id = "legacy-group".into();
                workspace_mut(snapshot, window).group_id = Some("legacy-group".into());
            }
        }),
        ("duplicate UUID split", |snapshot| {
            split_mut(snapshot, 1).split_id = Some(id(SPLIT_A));
        }),
        ("duplicate legacy split", |snapshot| {
            split_mut(snapshot, 0).split_id = Some("legacy-split".into());
            split_mut(snapshot, 1).split_id = Some("legacy-split".into());
        }),
        ("duplicate UUID pane", |snapshot| {
            pane_mut(snapshot, 1, 0).pane_id = Some(id(PANE_A1));
            workspace_mut(snapshot, 1).surfaces.as_mut().unwrap()[0].pane_id = id(PANE_A1);
            workspace_mut(snapshot, 1).focused_pane_id = Some(id(PANE_A1));
        }),
        ("duplicate legacy pane", |snapshot| {
            pane_mut(snapshot, 0, 0).pane_id = Some("legacy-pane".into());
            workspace_mut(snapshot, 0).surfaces.as_mut().unwrap()[0].pane_id = "legacy-pane".into();
            workspace_mut(snapshot, 0).focused_pane_id = Some("legacy-pane".into());
            pane_mut(snapshot, 1, 0).pane_id = Some("legacy-pane".into());
            workspace_mut(snapshot, 1).surfaces.as_mut().unwrap()[0].pane_id = "legacy-pane".into();
            workspace_mut(snapshot, 1).focused_pane_id = Some("legacy-pane".into());
        }),
        ("duplicate UUID surface", |snapshot| {
            replace_workspace_surface(snapshot, 1, &id(SURFACE_B1), &id(SURFACE_A1));
        }),
        ("duplicate legacy surface", |snapshot| {
            replace_workspace_surface(snapshot, 0, &id(SURFACE_A1), "legacy-surface");
            replace_workspace_surface(snapshot, 1, &id(SURFACE_B1), "legacy-surface");
        }),
    ];

    fn uuid(value: &str) -> bool {
        Uuid::parse_str(value).is_ok()
    }

    fn unique(values: &[&str]) -> bool {
        values.iter().copied().collect::<HashSet<_>>().len() == values.len()
    }

    fn canonical_collision_result(name: &str, snapshot: &AppSessionSnapshot) -> bool {
        let window_ids = snapshot
            .windows
            .iter()
            .map(|window| window.window_id.as_deref().unwrap())
            .collect::<Vec<_>>();
        let workspace_ids = snapshot
            .windows
            .iter()
            .map(|window| {
                window.tab_manager.workspaces[0]
                    .workspace_id
                    .as_deref()
                    .unwrap()
            })
            .collect::<Vec<_>>();
        let split_ids = snapshot
            .windows
            .iter()
            .map(|window| {
                let SessionWorkspaceLayoutSnapshot::Split(split) =
                    window.tab_manager.workspaces[0].layout.as_ref().unwrap()
                else {
                    panic!("split fixture")
                };
                split.split_id.as_deref().unwrap()
            })
            .collect::<Vec<_>>();
        let pane_ids = snapshot
            .windows
            .iter()
            .flat_map(|window| {
                let SessionWorkspaceLayoutSnapshot::Split(split) =
                    window.tab_manager.workspaces[0].layout.as_ref().unwrap()
                else {
                    panic!("split fixture")
                };
                [&split.first, &split.second].map(|node| {
                    let SessionWorkspaceLayoutSnapshot::Pane(pane) = node.as_ref() else {
                        panic!("pane fixture")
                    };
                    pane.pane_id.as_deref().unwrap()
                })
            })
            .collect::<Vec<_>>();
        let surface_ids = snapshot
            .windows
            .iter()
            .flat_map(|window| {
                window.tab_manager.workspaces[0]
                    .surfaces
                    .as_deref()
                    .unwrap()
                    .iter()
                    .map(|surface| surface.surface_id.as_str())
            })
            .collect::<Vec<_>>();

        match name {
            "duplicate UUID window" => {
                window_ids[0] == id(WINDOW_A)
                    && window_ids[1] != window_ids[0]
                    && uuid(window_ids[1])
            }
            "duplicate legacy window" => {
                unique(&window_ids) && window_ids.iter().all(|id| uuid(id))
            }
            "duplicate UUID workspace" | "duplicate legacy workspace" => {
                unique(&workspace_ids)
                    && workspace_ids.iter().all(|id| uuid(id))
                    && snapshot.windows.iter().enumerate().all(|(index, window)| {
                        window.selected_workspace_id.as_deref() == Some(workspace_ids[index])
                    })
            }
            "duplicate UUID group" => snapshot.windows.iter().all(|window| {
                let group = &window.tab_manager.workspace_groups.as_ref().unwrap()[0];
                group.id == id(GROUP_A)
                    && window.tab_manager.workspaces[0].group_id.as_deref()
                        == Some(id(GROUP_A).as_str())
            }),
            "duplicate legacy group" => {
                let groups = snapshot
                    .windows
                    .iter()
                    .map(|window| {
                        window.tab_manager.workspace_groups.as_ref().unwrap()[0]
                            .id
                            .as_str()
                    })
                    .collect::<Vec<_>>();
                unique(&groups)
                    && groups.iter().all(|id| uuid(id))
                    && snapshot.windows.iter().enumerate().all(|(index, window)| {
                        window.tab_manager.workspaces[0].group_id.as_deref() == Some(groups[index])
                    })
            }
            "duplicate UUID split" | "duplicate legacy split" => {
                unique(&split_ids) && split_ids.iter().all(|id| uuid(id))
            }
            "duplicate UUID pane" | "duplicate legacy pane" => {
                unique(&pane_ids) && pane_ids.iter().all(|id| uuid(id))
            }
            "duplicate UUID surface" => {
                unique(&surface_ids)
                    && surface_ids[0] == id(SURFACE_A1)
                    && surface_ids.iter().all(|id| uuid(id))
            }
            "duplicate legacy surface" => {
                unique(&surface_ids) && surface_ids.iter().all(|id| uuid(id))
            }
            _ => false,
        }
    }

    let mut wrong = Vec::new();
    for (name, mutation) in cases {
        let mut restored = graph_fixture();
        mutation(&mut restored);
        match restore_commits_atomically(restored) {
            Ok(committed) if canonical_collision_result(name, &committed) => {}
            Ok(_) => wrong.push(format!("{name}: wrong collision result")),
            Err(error) => wrong.push(format!("{name}: rejected ({error})")),
        }
    }
    assert!(
        wrong.is_empty(),
        "restore diverged from canonical collision behavior:\n{}",
        wrong.join("\n")
    );
}

fn assert_fresh(actual: &str, persisted: &str, kind: &str) {
    assert_ne!(actual, persisted, "{kind} must be rebuilt with a fresh id");
    Uuid::parse_str(actual).unwrap_or_else(|_| panic!("fresh {kind} id is not a UUID: {actual}"));
}

#[test]
fn frozen_restore_identity_policy_remaps_typed_references_and_preserves_opaque_text() {
    let mut restored = graph_fixture();
    let opaque_before = opaque_witness(&restored);
    assert!(remint_noncanonical_identities(&mut restored));

    // Frozen canonical: collision-free window and surface public ids survive.
    assert_eq!(
        restored.windows[0].window_id.as_deref(),
        Some(id(WINDOW_A).as_str())
    );
    assert_eq!(
        restored.windows[1].window_id.as_deref(),
        Some(id(WINDOW_B).as_str())
    );
    assert_eq!(
        workspace_mut(&mut restored, 0).surfaces.as_ref().unwrap()[0].surface_id,
        id(SURFACE_A1)
    );
    assert_eq!(
        restored.windows[0].dock.as_ref().unwrap().surfaces[0].surface_id,
        id(DOCK_SURFACE)
    );
    // Window-scoped Dock identity is derived from its owning window.
    assert_eq!(
        restored.windows[0].dock.as_ref().unwrap().workspace_id,
        format!("dock:{}", id(WINDOW_A))
    );
    let dock = restored.windows[0].dock.as_ref().unwrap();
    let SessionWorkspaceLayoutSnapshot::Pane(dock_pane) = dock.layout.as_ref().unwrap() else {
        panic!("dock pane fixture")
    };
    assert_fresh(
        dock_pane.pane_id.as_deref().unwrap(),
        &id(DOCK_PANE),
        "Dock pane",
    );
    assert_eq!(dock_pane.panel_ids, [id(DOCK_SURFACE)]);
    assert_eq!(
        dock.focused_surface_id.as_deref(),
        Some(id(DOCK_SURFACE).as_str())
    );

    // Frozen canonical: workspaces and bonsplit nodes are live objects rebuilt
    // from the persisted graph, so their runtime ids are always fresh.
    let workspace = &restored.windows[0].tab_manager.workspaces[0];
    let new_workspace_id = workspace.workspace_id.as_deref().unwrap();
    assert_fresh(new_workspace_id, &id(WORKSPACE_A), "workspace");
    assert_eq!(
        restored.windows[0].selected_workspace_id.as_deref(),
        Some(new_workspace_id)
    );
    assert_eq!(
        restored.windows[0]
            .tab_manager
            .workspace_groups
            .as_ref()
            .unwrap()[0]
            .anchor_workspace_id
            .as_deref(),
        Some(new_workspace_id)
    );
    assert_eq!(workspace.group_id.as_deref(), Some(id(GROUP_A).as_str()));
    assert_eq!(
        restored.windows[0]
            .tab_manager
            .workspace_groups
            .as_ref()
            .unwrap()[0]
            .id,
        id(GROUP_A),
        "group identity persists"
    );

    let SessionWorkspaceLayoutSnapshot::Split(layout) = workspace.layout.as_ref().unwrap() else {
        panic!("split fixture")
    };
    let new_split_id = layout.split_id.as_deref().unwrap();
    assert_fresh(new_split_id, &id(SPLIT_A), "split");
    let SessionWorkspaceLayoutSnapshot::Pane(first) = layout.first.as_ref() else {
        panic!("pane fixture")
    };
    let SessionWorkspaceLayoutSnapshot::Pane(second) = layout.second.as_ref() else {
        panic!("pane fixture")
    };
    let new_first_pane = first.pane_id.as_deref().unwrap();
    let new_second_pane = second.pane_id.as_deref().unwrap();
    assert_fresh(new_first_pane, &id(PANE_A1), "pane");
    assert_fresh(new_second_pane, &id(PANE_A2), "pane");
    assert_eq!(first.panel_ids, [id(SURFACE_A1)]);
    assert_eq!(
        first.selected_panel_id.as_deref(),
        Some(id(SURFACE_A1).as_str())
    );
    assert_eq!(workspace.focused_pane_id.as_deref(), Some(new_first_pane));
    assert_eq!(
        workspace.focused_panel_id.as_deref(),
        Some(id(SURFACE_A1).as_str())
    );
    assert_eq!(
        workspace.zoomed_panel_id.as_deref(),
        Some(id(SURFACE_A1).as_str())
    );
    assert_eq!(
        workspace.surfaces.as_ref().unwrap()[0].pane_id,
        new_first_pane
    );
    assert_eq!(
        workspace.surfaces.as_ref().unwrap()[1].pane_id,
        new_second_pane
    );
    assert_eq!(
        workspace.published_pane_selections.as_ref().unwrap()[0].pane_id,
        new_first_pane
    );
    assert_eq!(
        workspace.published_pane_selections.as_ref().unwrap()[0].panel_id,
        id(SURFACE_A1)
    );

    for panel_id in [
        &workspace.pending_surface_pwds.as_ref().unwrap()[0].surface_id,
        &workspace.panel_titles.as_ref().unwrap()[0].panel_id,
        &workspace.panel_pins.as_ref().unwrap()[0].panel_id,
        &workspace.panel_unreads.as_ref().unwrap()[0].panel_id,
        &workspace.restorable_agent_snapshots.as_ref().unwrap()[0].panel_id,
        &workspace.surface_resume_bindings.as_ref().unwrap()[0].surface_id,
        &workspace.panel_git_branches.as_ref().unwrap()[0].panel_id,
        &workspace.panel_pull_requests.as_ref().unwrap()[0].panel_id,
        &workspace.panel_listening_ports.as_ref().unwrap()[0].panel_id,
        &workspace.panel_ttys.as_ref().unwrap()[0].panel_id,
        &workspace.panel_shell_activity.as_ref().unwrap()[0].panel_id,
        &workspace.panel_terminal_startups.as_ref().unwrap()[0].panel_id,
        &workspace.canvas_panes.as_ref().unwrap()[0].panel_id,
        &workspace.canvas_panes.as_ref().unwrap()[0]
            .panel_ids
            .as_ref()
            .unwrap()[0],
        workspace.canvas_panes.as_ref().unwrap()[0]
            .selected_panel_id
            .as_ref()
            .unwrap(),
    ] {
        assert_eq!(panel_id, &id(SURFACE_A1));
    }
    assert_eq!(opaque_witness(&restored), opaque_before);

    let second_workspace = &restored.windows[1].tab_manager.workspaces[0];
    assert_fresh(
        second_workspace.workspace_id.as_deref().unwrap(),
        &id(WORKSPACE_B),
        "workspace",
    );
    let SessionWorkspaceLayoutSnapshot::Split(second_split) =
        second_workspace.layout.as_ref().unwrap()
    else {
        panic!("second split fixture")
    };
    assert_fresh(
        second_split.split_id.as_deref().unwrap(),
        &id(SPLIT_B),
        "split",
    );
}

#[test]
fn equal_legacy_literals_in_different_identity_domains_remain_unambiguous() {
    let legacy = "same-legacy-literal";
    let mut snapshot = AppSessionSnapshot {
        version: SESSION_SNAPSHOT_SCHEMA_VERSION,
        created_at: 1,
        windows: vec![SessionWindowSnapshot {
            window_id: Some(legacy.into()),
            selected_workspace_id: Some(legacy.into()),
            dock: None,
            tab_manager: SessionTabManagerSnapshot {
                selected_workspace_index: Some(0),
                workspaces: vec![SessionWorkspaceSnapshot {
                    workspace_id: Some(legacy.into()),
                    process_title: legacy.into(),
                    layout: Some(SessionWorkspaceLayoutSnapshot::Pane(
                        cmux_core::session::SessionPaneLayoutSnapshot {
                            pane_id: Some(legacy.into()),
                            panel_ids: vec![legacy.into()],
                            selected_panel_id: Some(legacy.into()),
                            surface_kind: None,
                            markdown_file_path: None,
                            file_path: None,
                            diff_viewer_token: None,
                            diff_viewer_request_path: None,
                            browser_url: Some(legacy.into()),
                            browser_proxy_url: None,
                            browser_back_history: None,
                            browser_forward_history: None,
                            browser_omnibar_visible: None,
                            browser_focus_mode_active: None,
                            browser_developer_tools_visible: None,
                            browser_developer_tools_panel: None,
                            browser_page_zoom: None,
                        },
                    )),
                    focused_panel_id: Some(legacy.into()),
                    focused_pane_id: Some(legacy.into()),
                    surfaces: Some(vec![cmux_core::session::SessionSurfaceSnapshot {
                        surface_id: legacy.into(),
                        pane_id: legacy.into(),
                        generation: 1,
                        kind: cmux_core::session::SessionSurfaceKindSnapshot::RemoteTerminal {
                            remote_session_id: Some(legacy.into()),
                            remote_context: Some(serde_json::json!({"surface_id": legacy})),
                            arrival_generation: Some(1),
                        },
                        metadata: Default::default(),
                        terminal_startup: None,
                        scrollback: None,
                    }]),
                    ..Default::default()
                }],
                workspace_groups: None,
            },
        }],
    };
    assert!(remint_noncanonical_identities(&mut snapshot));
    let window = &snapshot.windows[0];
    let workspace = &window.tab_manager.workspaces[0];
    let SessionWorkspaceLayoutSnapshot::Pane(pane) = workspace.layout.as_ref().unwrap() else {
        panic!("pane fixture")
    };
    let identities = [
        window.window_id.as_deref().unwrap(),
        workspace.workspace_id.as_deref().unwrap(),
        pane.pane_id.as_deref().unwrap(),
        pane.panel_ids[0].as_str(),
    ];
    assert_eq!(identities.iter().collect::<HashSet<_>>().len(), 4);
    assert_eq!(workspace.process_title, legacy);
    assert_eq!(pane.browser_url.as_deref(), Some(legacy));
    let cmux_core::session::SessionSurfaceKindSnapshot::RemoteTerminal {
        remote_session_id,
        remote_context,
        ..
    } = &workspace.surfaces.as_ref().unwrap()[0].kind
    else {
        panic!("remote fixture")
    };
    assert_eq!(remote_session_id.as_deref(), Some(legacy));
    assert_eq!(remote_context.as_ref().unwrap()["surface_id"], legacy);
}

#[test]
fn layout_surface_aliases_follow_canonical_filter_prune_and_collision_rules() {
    let cases: &[(&str, Mutation)] = &[
        ("same panel appears in two panes", |snapshot| {
            pane_mut(snapshot, 0, 1).panel_ids = vec![id(SURFACE_A1)];
            pane_mut(snapshot, 0, 1).selected_panel_id = Some(id(SURFACE_A1));
        }),
        ("duplicate authoritative surface row", |snapshot| {
            let duplicate = workspace_mut(snapshot, 0).surfaces.as_ref().unwrap()[0].clone();
            workspace_mut(snapshot, 0)
                .surfaces
                .as_mut()
                .unwrap()
                .push(duplicate);
        }),
        ("authoritative surface has wrong pane owner", |snapshot| {
            workspace_mut(snapshot, 0).surfaces.as_mut().unwrap()[0].pane_id = id(PANE_A2);
        }),
        ("authoritative surface absent from layout", |snapshot| {
            workspace_mut(snapshot, 0)
                .surfaces
                .as_mut()
                .unwrap()
                .push(terminal_surface(0x7fff, PANE_A1));
        }),
        (
            "layout surface absent from authoritative rows",
            |snapshot| {
                workspace_mut(snapshot, 0)
                    .surfaces
                    .as_mut()
                    .unwrap()
                    .remove(0);
            },
        ),
        (
            "missing pane id has conflicting surface owners",
            |snapshot| {
                let workspace = workspace_mut(snapshot, 0);
                workspace.layout = Some(SessionWorkspaceLayoutSnapshot::Pane(
                    cmux_core::session::SessionPaneLayoutSnapshot {
                        pane_id: None,
                        panel_ids: vec![id(SURFACE_A1), id(SURFACE_A2)],
                        selected_panel_id: Some(id(SURFACE_A1)),
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
                    },
                ));
                workspace.focused_pane_id = None;
            },
        ),
        ("legacy layout-only alias is duplicated", |snapshot| {
            let workspace = workspace_mut(snapshot, 0);
            workspace.surfaces = None;
            workspace.pending_surface_pwds = None;
            workspace.panel_titles = None;
            workspace.panel_pins = None;
            workspace.panel_unreads = None;
            workspace.restorable_agent_snapshots = None;
            workspace.surface_resume_bindings = None;
            workspace.published_pane_selections = None;
            workspace.panel_git_branches = None;
            workspace.panel_pull_requests = None;
            workspace.panel_listening_ports = None;
            workspace.panel_ttys = None;
            workspace.panel_shell_activity = None;
            workspace.panel_terminal_startups = None;
            workspace.canvas_panes = None;
            workspace.focused_panel_id = Some("legacy-duplicate".into());
            workspace.zoomed_panel_id = Some("legacy-duplicate".into());
            let SessionWorkspaceLayoutSnapshot::Split(split) = workspace.layout.as_mut().unwrap()
            else {
                panic!("split fixture")
            };
            for node in [&mut split.first, &mut split.second] {
                let SessionWorkspaceLayoutSnapshot::Pane(pane) = node.as_mut() else {
                    panic!("pane fixture")
                };
                pane.panel_ids = vec!["legacy-duplicate".into()];
                pane.selected_panel_id = Some("legacy-duplicate".into());
            }
        }),
    ];

    fn collect_layout(
        layout: &SessionWorkspaceLayoutSnapshot,
        panes: &mut Vec<(String, Vec<String>, Option<String>)>,
    ) {
        match layout {
            SessionWorkspaceLayoutSnapshot::Pane(pane) => panes.push((
                pane.pane_id.clone().unwrap(),
                pane.panel_ids.clone(),
                pane.selected_panel_id.clone(),
            )),
            SessionWorkspaceLayoutSnapshot::Split(split) => {
                collect_layout(&split.first, panes);
                collect_layout(&split.second, panes);
            }
        }
    }

    fn canonical_alias_result(name: &str, snapshot: &AppSessionSnapshot) -> bool {
        let workspace = workspace(snapshot, 0);
        let mut panes = Vec::new();
        collect_layout(workspace.layout.as_ref().unwrap(), &mut panes);
        let layout_panels = panes
            .iter()
            .flat_map(|(_, panels, _)| panels.iter().map(String::as_str))
            .collect::<Vec<_>>();
        let surfaces = workspace
            .surfaces
            .as_deref()
            .unwrap_or_default()
            .iter()
            .map(|surface| surface.surface_id.as_str())
            .collect::<Vec<_>>();
        let selections_are_local = panes.iter().all(|(_, panels, selected)| {
            selected
                .as_ref()
                .is_none_or(|selected| panels.contains(selected))
        });
        let owners_follow_layout = workspace
            .surfaces
            .as_deref()
            .unwrap_or_default()
            .iter()
            .all(|surface| {
                panes.iter().any(|(pane, panels, _)| {
                    pane == &surface.pane_id && panels.contains(&surface.surface_id)
                })
            });

        match name {
            "same panel appears in two panes" => {
                layout_panels.len() == 2
                    && layout_panels.iter().copied().collect::<HashSet<_>>().len() == 2
                    && surfaces.iter().copied().collect::<HashSet<_>>().len() == 2
                    && !layout_panels.contains(&id(SURFACE_A2).as_str())
                    && selections_are_local
                    && owners_follow_layout
            }
            "authoritative surface has wrong pane owner" => owners_follow_layout,
            "authoritative surface absent from layout" => {
                !surfaces.contains(&id(0x7fff).as_str()) && owners_follow_layout
            }
            "layout surface absent from authoritative rows" => {
                !layout_panels.contains(&id(SURFACE_A1).as_str())
                    && !surfaces.contains(&id(SURFACE_A1).as_str())
                    && panes.iter().all(|(_, panels, _)| !panels.is_empty())
                    && selections_are_local
                    && owners_follow_layout
            }
            "missing pane id has conflicting surface owners" => {
                panes.len() == 1
                    && panes[0].1.len() == 2
                    && Uuid::parse_str(&panes[0].0).is_ok()
                    && panes[0].0 != id(PANE_A1)
                    && panes[0].0 != id(PANE_A2)
                    && selections_are_local
                    && owners_follow_layout
            }
            "legacy layout-only alias is duplicated" => {
                panes.len() == 2
                    && layout_panels.len() == 2
                    && layout_panels.iter().all(|id| Uuid::parse_str(id).is_ok())
                    && layout_panels.iter().all(|id| *id != "legacy-duplicate")
                    && layout_panels.iter().copied().collect::<HashSet<_>>().len() == 2
                    && selections_are_local
                    && owners_follow_layout
            }
            _ => false,
        }
    }

    let mut wrong = Vec::new();
    for (name, mutation) in cases {
        let mut restored = graph_fixture();
        mutation(&mut restored);
        if *name == "duplicate authoritative surface row" {
            if !restore_rejects_without_publication(restored) {
                wrong.push(format!("{name}: not rejected atomically"));
            }
            continue;
        }
        match restore_commits_atomically(restored) {
            Ok(committed) if canonical_alias_result(name, &committed) => {}
            Ok(_) => wrong.push(format!("{name}: wrong normalized graph")),
            Err(error) => wrong.push(format!("{name}: rejected ({error})")),
        }
    }
    assert!(
        wrong.is_empty(),
        "restore diverged from canonical layout/surface behavior:\n{}",
        wrong.join("\n")
    );
}

fn layout_leaves(
    layout: &SessionWorkspaceLayoutSnapshot,
) -> Vec<&cmux_core::session::SessionPaneLayoutSnapshot> {
    fn collect<'a>(
        layout: &'a SessionWorkspaceLayoutSnapshot,
        leaves: &mut Vec<&'a cmux_core::session::SessionPaneLayoutSnapshot>,
    ) {
        match layout {
            SessionWorkspaceLayoutSnapshot::Pane(pane) => leaves.push(pane),
            SessionWorkspaceLayoutSnapshot::Split(split) => {
                collect(&split.first, leaves);
                collect(&split.second, leaves);
            }
        }
    }

    let mut leaves = Vec::new();
    collect(layout, &mut leaves);
    leaves
}

#[test]
fn authoritative_hole_keeps_its_scaffold_leaf_and_saved_divider() {
    let mut restored = graph_fixture();
    let workspace_before = workspace_mut(&mut restored, 0);
    workspace_before
        .surfaces
        .as_mut()
        .unwrap()
        .retain(|surface| surface.surface_id != id(SURFACE_A1));
    let SessionWorkspaceLayoutSnapshot::Split(split) = workspace_before.layout.as_mut().unwrap()
    else {
        panic!("split fixture")
    };
    split.orientation = SessionSplitOrientation::Vertical;
    split.divider_position = 0.375;

    let committed = restore_commits_atomically(restored).expect("restore must publish atomically");
    let workspace = workspace(&committed, 0);
    let SessionWorkspaceLayoutSnapshot::Split(split) = workspace.layout.as_ref().unwrap() else {
        panic!("a missing authoritative row must not collapse the saved split")
    };
    assert_eq!(split.orientation, SessionSplitOrientation::Vertical);
    assert_eq!(split.divider_position, 0.375);

    let leaves = layout_leaves(workspace.layout.as_ref().unwrap());
    assert_eq!(leaves.len(), 2, "every saved leaf keeps a live pane");
    assert!(leaves.iter().all(|pane| pane.panel_ids.len() == 1));
    let scaffold_id = &leaves[0].panel_ids[0];
    assert!(Uuid::parse_str(scaffold_id).is_ok());
    assert_ne!(scaffold_id, &id(SURFACE_A1));
    assert_eq!(leaves[1].panel_ids, [id(SURFACE_A2)]);

    let surfaces = workspace.surfaces.as_deref().unwrap();
    assert_eq!(
        surfaces.len(),
        2,
        "the scaffold is authoritative live state"
    );
    for (pane, panel_id) in leaves.iter().zip([
        leaves[0].panel_ids[0].as_str(),
        leaves[1].panel_ids[0].as_str(),
    ]) {
        assert!(surfaces.iter().any(|surface| {
            surface.surface_id == panel_id
                && Some(surface.pane_id.as_str()) == pane.pane_id.as_deref()
        }));
    }
}

#[test]
fn colliding_legacy_empty_leaves_get_distinct_scaffolds_without_collapse() {
    let mut restored = graph_fixture();
    let workspace_before = workspace_mut(&mut restored, 0);
    workspace_before.surfaces = Some(Vec::new());
    let SessionWorkspaceLayoutSnapshot::Split(split) = workspace_before.layout.as_mut().unwrap()
    else {
        panic!("split fixture")
    };
    split.divider_position = 0.625;
    for node in [&mut split.first, &mut split.second] {
        let SessionWorkspaceLayoutSnapshot::Pane(pane) = node.as_mut() else {
            panic!("pane fixture")
        };
        pane.panel_ids = vec!["legacy-collision".into()];
        pane.selected_panel_id = Some("legacy-collision".into());
    }

    let committed = restore_commits_atomically(restored).expect("restore must publish atomically");
    let workspace = workspace(&committed, 0);
    let Some(layout) = workspace.layout.as_ref() else {
        panic!("empty legacy leaves must retain a live saved layout")
    };
    let SessionWorkspaceLayoutSnapshot::Split(split) = layout else {
        panic!("empty legacy leaves must retain the saved split")
    };
    assert_eq!(split.divider_position, 0.625);
    let leaves = layout_leaves(layout);
    assert_eq!(leaves.len(), 2);
    let scaffold_ids = leaves
        .iter()
        .map(|pane| pane.panel_ids.as_slice())
        .collect::<Vec<_>>();
    assert!(scaffold_ids.iter().all(|ids| ids.len() == 1));
    assert!(scaffold_ids
        .iter()
        .all(|ids| Uuid::parse_str(&ids[0]).is_ok() && ids[0] != "legacy-collision"));
    assert_ne!(scaffold_ids[0][0], scaffold_ids[1][0]);
}

#[test]
fn groups_without_restored_local_members_are_dropped_after_dedupe() {
    let mut restored = graph_fixture();
    let groups = restored.windows[0]
        .tab_manager
        .workspace_groups
        .as_mut()
        .unwrap();
    groups.push(group(GROUP_B, WORKSPACE_B));
    let mut legacy_orphan = group(GROUP_B + 1, WORKSPACE_B);
    legacy_orphan.id = "legacy-orphan-group".into();
    legacy_orphan.anchor_workspace_id = Some("legacy-orphan-workspace".into());
    groups.push(legacy_orphan);

    let committed = restore_commits_atomically(restored).expect("restore must publish atomically");
    let groups = committed.windows[0]
        .tab_manager
        .workspace_groups
        .as_deref()
        .unwrap();
    assert_eq!(groups.len(), 1, "only groups with local members survive");
    assert_eq!(groups[0].id, id(GROUP_A));
    assert!(groups[0].anchor_workspace_id.is_some());
}

#[test]
fn focus_fallback_tracks_the_last_restored_selected_leaf_including_collisions() {
    let mut wrong = Vec::new();
    for (name, focused_panel, focused_pane) in [
        ("absent", None, None),
        ("stale UUID", Some(id(0xfff0)), Some(id(0xfff1))),
        (
            "stale legacy",
            Some("legacy-focused-panel".into()),
            Some("legacy-focused-pane".into()),
        ),
    ] {
        let mut restored = graph_fixture();
        let workspace_before = workspace_mut(&mut restored, 0);
        workspace_before.focused_panel_id = focused_panel;
        workspace_before.focused_pane_id = focused_pane;
        let committed =
            restore_commits_atomically(restored).expect("restore must publish atomically");
        let workspace = workspace(&committed, 0);
        let expected = pane_at(&committed, 0, 1);
        if workspace.focused_panel_id.as_ref() != expected.selected_panel_id.as_ref()
            || workspace.focused_pane_id.as_ref() != expected.pane_id.as_ref()
        {
            wrong.push(name);
        }
    }

    let mut collision = graph_fixture();
    pane_mut(&mut collision, 0, 1).panel_ids = vec![id(SURFACE_A1)];
    pane_mut(&mut collision, 0, 1).selected_panel_id = Some(id(SURFACE_A1));
    let workspace_before = workspace_mut(&mut collision, 0);
    workspace_before.focused_panel_id = Some(id(0xfff2));
    workspace_before.focused_pane_id = Some(id(0xfff3));
    let committed = restore_commits_atomically(collision).expect("restore must publish atomically");
    let workspace = workspace(&committed, 0);
    let expected = pane_at(&committed, 0, 1);
    if workspace.focused_panel_id.as_ref() != expected.selected_panel_id.as_ref()
        || workspace.focused_pane_id.as_ref() != expected.pane_id.as_ref()
        || workspace.focused_panel_id.as_deref() == Some(id(SURFACE_A1).as_str())
    {
        wrong.push("surface collision");
    }

    assert!(
        wrong.is_empty(),
        "focus did not follow canonical live-focus fallback for {wrong:?}"
    );
}

#[test]
fn canvas_restore_preserves_compact_map_order_duplicates_and_legacy_selection_fallback() {
    let mut restored = graph_fixture();
    workspace_mut(&mut restored, 0).canvas_panes =
        Some(vec![cmux_core::session::SessionCanvasPaneSnapshot {
            panel_id: id(SURFACE_A2),
            x: 1,
            y: 2,
            width: 3,
            height: 4,
            panel_ids: Some(vec![id(SURFACE_A1), id(SURFACE_A1), id(SURFACE_A2)]),
            selected_panel_id: None,
        }]);

    let committed = restore_commits_atomically(restored).expect("restore must publish atomically");
    let canvas = &workspace(&committed, 0).canvas_panes.as_ref().unwrap()[0];
    assert_eq!(
        canvas.panel_ids.as_ref().unwrap(),
        &[id(SURFACE_A1), id(SURFACE_A1), id(SURFACE_A2)],
        "canonical compactMap does not deduplicate mapped panel ids"
    );
    assert_eq!(canvas.panel_id, id(SURFACE_A1));
    assert_eq!(
        canvas.selected_panel_id.as_deref(),
        Some(id(SURFACE_A2).as_str()),
        "selectedPanelId ?? panelId precedes first-panel fallback"
    );
}

fn focus_matches_pane(snapshot: &AppSessionSnapshot, pane_index: usize) -> bool {
    let workspace = workspace(snapshot, 0);
    let pane = pane_at(snapshot, 0, pane_index);
    workspace.focused_panel_id.as_ref() == pane.selected_panel_id.as_ref()
        && workspace.focused_pane_id.as_ref() == pane.pane_id.as_ref()
}

#[test]
fn stale_later_selection_does_not_replace_the_prior_restored_live_focus() {
    let canonical = include_str!("../../../../../Sources/Workspace.swift");
    assert!(canonical.contains("if let selectedOldId = snapshot.selectedPanelId"));
    assert!(canonical.contains("return oldToNewPanelIds[selectedOldId]"));
    assert!(canonical.contains("return createdPanelIds.first"));

    let mut wrong = Vec::new();
    for (name, stale_panel, stale_pane) in [
        ("stale UUID", id(0xffe0), id(0xffe1)),
        (
            "stale legacy",
            "legacy-stale-selection".into(),
            "legacy-stale-pane".into(),
        ),
    ] {
        let mut restored = graph_fixture();
        pane_mut(&mut restored, 0, 1).selected_panel_id = Some(stale_panel.clone());
        let workspace = workspace_mut(&mut restored, 0);
        workspace.focused_panel_id = Some(stale_panel);
        workspace.focused_pane_id = Some(stale_pane);

        let committed =
            restore_commits_atomically(restored).expect("restore must publish atomically");
        if !focus_matches_pane(&committed, 0) {
            wrong.push(name);
        }
    }

    assert!(
        wrong.is_empty(),
        "a stale later selection incorrectly replaced prior live focus for {wrong:?}"
    );
}

#[test]
fn trailing_scaffold_only_leaf_never_steals_prior_restored_focus() {
    let canonical = include_str!("../../../../../Sources/Workspace.swift");
    assert!(canonical.contains("guard !createdPanelIds.isEmpty else { return }"));
    assert!(canonical.contains("preserveFocusAfterNonFocusSplit("));

    let mut wrong = Vec::new();
    let cases: &[(&str, Mutation)] = &[
        ("missing UUID", |snapshot: &mut AppSessionSnapshot| {
            workspace_mut(snapshot, 0)
                .surfaces
                .as_mut()
                .unwrap()
                .retain(|surface| surface.surface_id != id(SURFACE_A2));
        }),
        ("colliding legacy", |snapshot: &mut AppSessionSnapshot| {
            workspace_mut(snapshot, 0)
                .surfaces
                .as_mut()
                .unwrap()
                .retain(|surface| surface.surface_id != id(SURFACE_A2));
            let pane = pane_mut(snapshot, 0, 1);
            pane.panel_ids = vec!["legacy-hole".into(), "legacy-hole".into()];
            pane.selected_panel_id = Some("legacy-hole".into());
        }),
    ];
    for (name, mutate) in cases {
        let mut restored = graph_fixture();
        mutate(&mut restored);
        let workspace_before = workspace_mut(&mut restored, 0);
        workspace_before.focused_panel_id = Some(id(0xffe2));
        workspace_before.focused_pane_id = Some(id(0xffe3));

        let committed =
            restore_commits_atomically(restored).expect("restore must publish atomically");
        let leaves = layout_leaves(workspace(&committed, 0).layout.as_ref().unwrap());
        let topology_is_scaffolded = leaves.len() == 2
            && leaves.iter().all(|pane| pane.panel_ids.len() == 1)
            && Uuid::parse_str(&leaves[1].panel_ids[0]).is_ok()
            && leaves[0].panel_ids[0] != leaves[1].panel_ids[0];
        if !topology_is_scaffolded || !focus_matches_pane(&committed, 0) {
            wrong.push(name);
        }
    }

    assert!(
        wrong.is_empty(),
        "a trailing scaffold changed topology or stole live focus for {wrong:?}"
    );
}

#[test]
fn absent_later_selection_still_focuses_its_first_created_panel() {
    let mut wrong = Vec::new();
    for (name, collide) in [("UUID", false), ("surface collision", true)] {
        let mut restored = graph_fixture();
        if collide {
            pane_mut(&mut restored, 0, 1).panel_ids = vec![id(SURFACE_A1)];
        }
        pane_mut(&mut restored, 0, 1).selected_panel_id = None;
        let workspace = workspace_mut(&mut restored, 0);
        workspace.focused_panel_id = Some(id(0xffe4));
        workspace.focused_pane_id = Some(id(0xffe5));

        let committed =
            restore_commits_atomically(restored).expect("restore must publish atomically");
        let second = pane_at(&committed, 0, 1);
        if !focus_matches_pane(&committed, 1)
            || second.selected_panel_id.as_ref() != second.panel_ids.first()
            || (collide && second.panel_ids[0] == id(SURFACE_A1))
        {
            wrong.push(name);
        }
    }

    assert!(
        wrong.is_empty(),
        "an absent selection did not choose the first created panel for {wrong:?}"
    );
}

#[test]
fn legacy_empty_leaf_without_surface_rows_keeps_unique_scaffold_and_prior_focus() {
    let mut restored = graph_fixture();
    let workspace_before = workspace_mut(&mut restored, 0);
    workspace_before.surfaces = None;
    workspace_before.focused_panel_id = Some("legacy-stale-focus".into());
    workspace_before.focused_pane_id = Some("legacy-stale-pane".into());
    let SessionWorkspaceLayoutSnapshot::Split(split) = workspace_before.layout.as_mut().unwrap()
    else {
        panic!("split fixture")
    };
    split.orientation = SessionSplitOrientation::Vertical;
    split.divider_position = 0.7;
    let SessionWorkspaceLayoutSnapshot::Pane(second) = split.second.as_mut() else {
        panic!("pane fixture")
    };
    second.panel_ids.clear();
    second.selected_panel_id = None;

    let committed = restore_commits_atomically(restored).expect("restore must publish atomically");
    let workspace = workspace(&committed, 0);
    let SessionWorkspaceLayoutSnapshot::Split(split) = workspace.layout.as_ref().unwrap() else {
        panic!("legacy empty leaf must not collapse its saved split")
    };
    assert_eq!(split.orientation, SessionSplitOrientation::Vertical);
    assert_eq!(split.divider_position, 0.7);
    let leaves = layout_leaves(workspace.layout.as_ref().unwrap());
    assert_eq!(leaves.len(), 2);
    assert!(leaves.iter().all(|pane| pane.panel_ids.len() == 1));
    assert!(Uuid::parse_str(&leaves[1].panel_ids[0]).is_ok());
    assert_ne!(leaves[0].panel_ids[0], leaves[1].panel_ids[0]);
    assert!(focus_matches_pane(&committed, 0));
}

#[test]
fn local_contract_qualifies_reused_uuid_ids_and_member_scoped_groups() {
    let contract = include_str!("../../../../../docs/parity/contracts/pane_surface_lifecycle.json");
    assert!(contract.contains(
        "Reuse collision-free persisted UUID window and surface ids; rebuild workspace, pane, and split ids; preserve valid group ids only when they have restored local members, with remapped anchors; derive Dock ids from their owning windows and rebuild Dock pane ids."
    ));
    assert!(
        !contract.contains("Reuse collision-free persisted window and surface ids;"),
        "legacy string identities are reminted, not reused"
    );
}

#[test]
fn local_contract_must_match_the_frozen_domain_specific_restore_policy() {
    let contract = include_str!("../../../../../docs/parity/contracts/pane_surface_lifecycle.json");
    let app_delegate = include_str!("../../../../../Sources/AppDelegate.swift");
    assert!(app_delegate
        .contains("let requestedWindowId = preferredWindowId ?? sessionWindowSnapshot?.windowId"));
    assert!(app_delegate.contains(
        "availableWindowIdForNewMainWindow(preferredWindowId: requestedWindowId) ?? UUID()"
    ));
    let tab_manager = include_str!("../../../../../Sources/TabManager.swift");
    assert!(tab_manager.contains("let workspace = Workspace("));
    assert!(tab_manager.contains("seen.insert(groupSnapshot.id).inserted else { return nil }"));
    assert!(tab_manager.contains("if let index = groupSnapshot.anchorMemberIndex,"));
    assert!(tab_manager
        .contains("if let stored = groupSnapshot.anchorWorkspaceId, members.contains(stored)"));
    assert!(tab_manager.contains("return members[0]"));
    assert!(tab_manager.contains("let knownGroupIds = Set(restoredGroups.map(\\.id))"));
    assert!(tab_manager.contains("workspace.groupId = nil"));
    let workspace = include_str!("../../../../../Sources/Workspace.swift");
    assert!(workspace
        .contains("let panelSnapshotsById = Dictionary(uniqueKeysWithValues: snapshot.panels.map"));
    assert!(workspace.contains("restoreSessionLayoutNode(layout, inPane: rootPaneId"));
    assert!(workspace.contains(
        "let desiredOldPanelIds = snapshot.panelIds.filter { panelSnapshotsById[$0] != nil }"
    ));
    assert!(workspace.contains("guard !createdPanelIds.isEmpty else { return }"));
    assert!(workspace.contains("return oldToNewPanelIds[selectedOldId]"));
    assert!(workspace.contains("return createdPanelIds.first"));
    assert!(workspace.contains("pruneSurfaceMetadata(validSurfaceIds: Set(panels.keys))"));
    assert!(workspace.contains(
        "else if let fallbackFocusedPanelId = focusedPanelId, panels[fallbackFocusedPanelId] != nil"
    ));
    assert!(workspace.contains(
        "GhosttyApp.terminalSurfaceRegistry.surface(id: snapshot.id) == nil ? snapshot.id : nil"
    ));
    assert!(workspace.contains("oldToNewPanelIds[oldPanelId] = createdPanelId"));
    let canvas = include_str!("../../../../../Sources/Canvas/Workspace+CanvasLayout.swift");
    assert!(canvas.contains("let oldPanelIds = pane.panelIds ?? [pane.panelId]"));
    assert!(canvas.contains("let newPanelIds = oldPanelIds.compactMap"));
    assert!(canvas.contains("guard !newPanelIds.isEmpty else { return nil }"));
    assert!(canvas.contains("selectedPanelId: newSelected ?? newPanelIds[0]"));
    assert!(
        !contract.contains("do not mint replacement public ids on restore"),
        "the blanket stable-id sentence contradicts frozen e1825d40: Workspace and bonsplit ids are rebuilt"
    );
}
