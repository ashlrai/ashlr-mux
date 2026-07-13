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

fn restore_rejects_without_publication(restored: AppSessionSnapshot) -> bool {
    let current = initial_snapshot("surface-90");
    let authority = GatedSnapshot::new(current.clone());
    let next_panel = AtomicU64::new(91);
    let mut publication = RecordingPublication::default();
    let result =
        restore_previous_launch_transaction(&authority, &next_panel, &mut publication, || {
            Some(restored)
        });
    result.is_err()
        && publication.calls.is_empty()
        && *authority.lock().unwrap() == current
        && next_panel.load(Ordering::Relaxed) == 91
}

type Mutation = fn(&mut AppSessionSnapshot);

#[test]
fn uuid_only_stale_reference_is_validated_and_rejected_atomically() {
    let mut restored = graph_fixture();
    // Remove the Windows-only `dock:<owner>` derived id so every remaining
    // structural definition is UUID-shaped. This specifically guards against
    // an early return based on "no legacy replacements needed".
    restored.windows[0].dock = None;
    workspace_mut(&mut restored, 0).focused_panel_id = Some(id(0xffff));
    assert!(
        restore_rejects_without_publication(restored),
        "UUID-shaped definitions must not bypass stale-reference validation"
    );
}

#[test]
fn every_typed_reference_is_scoped_to_its_exact_owner() {
    let cases: &[(&str, Mutation)] = &[
        ("window.selected_workspace_id", |snapshot| {
            snapshot.windows[0].selected_workspace_id = Some(id(WORKSPACE_B));
        }),
        ("group.anchor_workspace_id", |snapshot| {
            snapshot.windows[0]
                .tab_manager
                .workspace_groups
                .as_mut()
                .unwrap()[0]
                .anchor_workspace_id = Some(id(WORKSPACE_B));
        }),
        ("workspace.group_id", |snapshot| {
            workspace_mut(snapshot, 0).group_id = Some(id(GROUP_B));
        }),
        ("pane.selected_panel_id", |snapshot| {
            pane_mut(snapshot, 0, 0).selected_panel_id = Some(id(SURFACE_B1));
        }),
        ("pane.selected_panel_id wrong sibling", |snapshot| {
            pane_mut(snapshot, 0, 0).selected_panel_id = Some(id(SURFACE_A2));
        }),
        ("workspace.zoomed_panel_id", |snapshot| {
            workspace_mut(snapshot, 0).zoomed_panel_id = Some(id(SURFACE_B1));
        }),
        ("workspace.focused_panel_id", |snapshot| {
            workspace_mut(snapshot, 0).focused_panel_id = Some(id(SURFACE_B1));
        }),
        ("workspace.focused_pane_id", |snapshot| {
            workspace_mut(snapshot, 0).focused_pane_id = Some(id(PANE_B1));
        }),
        ("surface.pane_id", |snapshot| {
            workspace_mut(snapshot, 0).surfaces.as_mut().unwrap()[0].pane_id = id(PANE_B1);
        }),
        ("surface.pane_id wrong sibling", |snapshot| {
            workspace_mut(snapshot, 0).surfaces.as_mut().unwrap()[0].pane_id = id(PANE_A2);
        }),
        ("pending_surface_pwds.surface_id", |snapshot| {
            workspace_mut(snapshot, 0)
                .pending_surface_pwds
                .as_mut()
                .unwrap()[0]
                .surface_id = id(SURFACE_B1);
        }),
        ("panel_titles.panel_id", |snapshot| {
            workspace_mut(snapshot, 0).panel_titles.as_mut().unwrap()[0].panel_id = id(SURFACE_B1);
        }),
        ("panel_pins.panel_id", |snapshot| {
            workspace_mut(snapshot, 0).panel_pins.as_mut().unwrap()[0].panel_id = id(SURFACE_B1);
        }),
        ("panel_unreads.panel_id", |snapshot| {
            workspace_mut(snapshot, 0).panel_unreads.as_mut().unwrap()[0].panel_id = id(SURFACE_B1);
        }),
        ("restorable_agent_snapshots.panel_id", |snapshot| {
            workspace_mut(snapshot, 0)
                .restorable_agent_snapshots
                .as_mut()
                .unwrap()[0]
                .panel_id = id(SURFACE_B1);
        }),
        ("surface_resume_bindings.surface_id", |snapshot| {
            workspace_mut(snapshot, 0)
                .surface_resume_bindings
                .as_mut()
                .unwrap()[0]
                .surface_id = id(SURFACE_B1);
        }),
        ("published_pane_selections.pane_id", |snapshot| {
            workspace_mut(snapshot, 0)
                .published_pane_selections
                .as_mut()
                .unwrap()[0]
                .pane_id = id(PANE_B1);
        }),
        ("published_pane_selections.panel_id", |snapshot| {
            workspace_mut(snapshot, 0)
                .published_pane_selections
                .as_mut()
                .unwrap()[0]
                .panel_id = id(SURFACE_B1);
        }),
        ("published selection mismatched pair", |snapshot| {
            workspace_mut(snapshot, 0)
                .published_pane_selections
                .as_mut()
                .unwrap()[0]
                .panel_id = id(SURFACE_A2);
        }),
        ("panel_git_branches.panel_id", |snapshot| {
            workspace_mut(snapshot, 0)
                .panel_git_branches
                .as_mut()
                .unwrap()[0]
                .panel_id = id(SURFACE_B1);
        }),
        ("panel_pull_requests.panel_id", |snapshot| {
            workspace_mut(snapshot, 0)
                .panel_pull_requests
                .as_mut()
                .unwrap()[0]
                .panel_id = id(SURFACE_B1);
        }),
        ("panel_listening_ports.panel_id", |snapshot| {
            workspace_mut(snapshot, 0)
                .panel_listening_ports
                .as_mut()
                .unwrap()[0]
                .panel_id = id(SURFACE_B1);
        }),
        ("panel_ttys.panel_id", |snapshot| {
            workspace_mut(snapshot, 0).panel_ttys.as_mut().unwrap()[0].panel_id = id(SURFACE_B1);
        }),
        ("panel_shell_activity.panel_id", |snapshot| {
            workspace_mut(snapshot, 0)
                .panel_shell_activity
                .as_mut()
                .unwrap()[0]
                .panel_id = id(SURFACE_B1);
        }),
        ("panel_terminal_startups.panel_id", |snapshot| {
            workspace_mut(snapshot, 0)
                .panel_terminal_startups
                .as_mut()
                .unwrap()[0]
                .panel_id = id(SURFACE_B1);
        }),
        ("canvas.panel_id", |snapshot| {
            workspace_mut(snapshot, 0).canvas_panes.as_mut().unwrap()[0].panel_id = id(SURFACE_B1);
        }),
        ("canvas.panel_ids", |snapshot| {
            workspace_mut(snapshot, 0).canvas_panes.as_mut().unwrap()[0].panel_ids =
                Some(vec![id(SURFACE_B1)]);
        }),
        ("canvas.selected_panel_id", |snapshot| {
            workspace_mut(snapshot, 0).canvas_panes.as_mut().unwrap()[0].selected_panel_id =
                Some(id(SURFACE_B1));
        }),
        ("canvas selected outside panel_ids", |snapshot| {
            workspace_mut(snapshot, 0).canvas_panes.as_mut().unwrap()[0].selected_panel_id =
                Some(id(SURFACE_A2));
        }),
        ("dock.workspace_id owner", |snapshot| {
            snapshot.windows[0].dock.as_mut().unwrap().workspace_id =
                format!("dock:{}", id(WINDOW_B));
        }),
        ("dock.layout.selected_panel_id", |snapshot| {
            let dock = snapshot.windows[0].dock.as_mut().unwrap();
            let SessionWorkspaceLayoutSnapshot::Pane(pane) = dock.layout.as_mut().unwrap() else {
                panic!("dock pane")
            };
            pane.selected_panel_id = Some(id(SURFACE_A1));
        }),
        ("dock.surface.pane_id", |snapshot| {
            snapshot.windows[0].dock.as_mut().unwrap().surfaces[0].pane_id = id(PANE_A1);
        }),
        ("dock.focused_surface_id", |snapshot| {
            snapshot.windows[0]
                .dock
                .as_mut()
                .unwrap()
                .focused_surface_id = Some(id(SURFACE_A1));
        }),
    ];

    let mut accepted = Vec::new();
    for (name, mutation) in cases {
        let mut restored = graph_fixture();
        mutation(&mut restored);
        if !restore_rejects_without_publication(restored) {
            accepted.push(*name);
        }
    }
    assert!(
        accepted.is_empty(),
        "restore accepted cross-owner typed references: {}",
        accepted.join(", ")
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
fn duplicate_uuid_and_legacy_definitions_reject_without_alias_collapse() {
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

    let mut accepted = Vec::new();
    for (name, mutation) in cases {
        let mut restored = graph_fixture();
        mutation(&mut restored);
        if !restore_rejects_without_publication(restored) {
            accepted.push(*name);
        }
    }
    assert!(
        accepted.is_empty(),
        "restore accepted duplicate definitions: {}",
        accepted.join(", ")
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
fn ambiguous_layout_surface_aliases_reject_before_any_publication() {
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

    let mut accepted = Vec::new();
    for (name, mutation) in cases {
        let mut restored = graph_fixture();
        mutation(&mut restored);
        if !restore_rejects_without_publication(restored) {
            accepted.push(*name);
        }
    }
    assert!(
        accepted.is_empty(),
        "restore accepted ambiguous layout/surface representations: {}",
        accepted.join(", ")
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
    let workspace = include_str!("../../../../../Sources/Workspace.swift");
    assert!(workspace.contains("restoreSessionLayoutNode(layout, inPane: rootPaneId"));
    assert!(workspace.contains(
        "GhosttyApp.terminalSurfaceRegistry.surface(id: snapshot.id) == nil ? snapshot.id : nil"
    ));
    assert!(workspace.contains("oldToNewPanelIds[oldPanelId] = createdPanelId"));
    assert!(
        !contract.contains("do not mint replacement public ids on restore"),
        "the blanket stable-id sentence contradicts frozen e1825d40: Workspace and bonsplit ids are rebuilt"
    );
}
