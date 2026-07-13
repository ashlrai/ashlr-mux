//! Durable publication contract for moving a panel into a new workspace.

use super::*;

struct RecordingPublication {
    calls: Vec<&'static str>,
    persist_error: Option<String>,
    baseline: AppSessionSnapshot,
    events: Vec<AppSessionSnapshot>,
}

impl RecordingPublication {
    fn new(initial: &AppSessionSnapshot) -> Self {
        Self {
            calls: Vec::new(),
            persist_error: None,
            baseline: initial.clone(),
            events: Vec::new(),
        }
    }
}

impl SnapshotPublicationOperations for RecordingPublication {
    fn persist(&mut self, _candidate: &AppSessionSnapshot) -> Result<(), String> {
        self.calls.push("persist");
        match self.persist_error.take() {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    fn update_event_baseline(&mut self, candidate: &AppSessionSnapshot) {
        self.calls.push("baseline");
        self.baseline = candidate.clone();
    }

    fn emit(&mut self, candidate: &AppSessionSnapshot) -> Result<(), String> {
        self.calls.push("emit");
        self.events.push(candidate.clone());
        Ok(())
    }
}

fn rich_snapshot() -> AppSessionSnapshot {
    let mut snapshot = initial_snapshot("surface-1");
    assert!(apply_split(
        &mut snapshot,
        "surface-1",
        SessionSplitOrientation::Horizontal,
        "surface-2",
        false,
    ));
    let workspace = &mut snapshot.windows[0].tab_manager.workspaces[0];
    workspace.current_directory = Some("C:/repo".into());
    workspace.initial_terminal_command = Some("cargo test".into());
    workspace.initial_terminal_input = Some("echo ready".into());
    workspace.initial_terminal_environment =
        Some(BTreeMap::from([("RUST_LOG".into(), "debug".into())]));
    assert!(session_ops::set_panel_title(workspace, "surface-2", "api"));
    assert!(session_ops::set_panel_pinned(workspace, "surface-2", true));
    assert!(session_ops::set_panel_unread(workspace, "surface-2", true));
    workspace.agent_listening_ports = Some(vec![9000]);
    workspace.listening_ports = Some(vec![3000, 5173, 9000]);
    workspace.panel_listening_ports = Some(vec![SessionPanelListeningPortsSnapshot {
        panel_id: "surface-2".into(),
        ports: vec![3000, 5173],
    }]);
    workspace.panel_shell_activity = Some(vec![SessionPanelShellActivitySnapshot {
        panel_id: "surface-2".into(),
        state: SessionPanelShellActivityStateSnapshot::CommandRunning,
        updated_at: 12,
    }]);
    snapshot
}

fn destination(snapshot: &AppSessionSnapshot) -> &SessionWorkspaceSnapshot {
    snapshot.windows[0]
        .tab_manager
        .workspaces
        .iter()
        .find(|workspace| {
            workspace.layout.as_ref().is_some_and(|layout| {
                session_ops::contains_panel(layout, "surface-2")
                    && !session_ops::contains_panel(layout, "surface-1")
            })
        })
        .expect("destination workspace")
}

fn pane_ids(layout: &SessionWorkspaceLayoutSnapshot) -> Vec<&str> {
    match layout {
        SessionWorkspaceLayoutSnapshot::Pane(pane) => pane.pane_id.as_deref().into_iter().collect(),
        SessionWorkspaceLayoutSnapshot::Split(split) => {
            let mut ids = pane_ids(&split.first);
            ids.extend(pane_ids(&split.second));
            ids
        }
    }
}

fn assert_destination_metadata(snapshot: &AppSessionSnapshot) {
    let workspace = destination(snapshot);
    assert!(Uuid::parse_str(workspace.workspace_id.as_deref().unwrap()).is_ok());
    let pane_ids = pane_ids(workspace.layout.as_ref().unwrap());
    assert_eq!(pane_ids.len(), 1);
    assert!(Uuid::parse_str(pane_ids[0]).is_ok());
    assert_eq!(workspace.process_title, "api");
    assert_eq!(workspace.current_directory.as_deref(), Some("C:/repo"));
    assert_eq!(
        workspace.initial_terminal_command.as_deref(),
        Some("cargo test")
    );
    assert_eq!(
        workspace.initial_terminal_input.as_deref(),
        Some("echo ready")
    );
    assert_eq!(
        workspace
            .initial_terminal_environment
            .as_ref()
            .unwrap()
            .get("RUST_LOG")
            .map(String::as_str),
        Some("debug")
    );
    assert_eq!(
        workspace.panel_titles.as_ref().unwrap()[0]
            .custom_title
            .as_deref(),
        Some("api")
    );
    assert!(workspace.panel_pins.as_ref().unwrap()[0].is_pinned);
    assert!(workspace.panel_unreads.as_ref().unwrap()[0].is_unread);
    assert_eq!(workspace.listening_ports, Some(vec![3000, 5173]));
    assert_eq!(
        workspace.panel_listening_ports.as_ref().unwrap()[0].ports,
        vec![3000, 5173]
    );
    assert_eq!(
        workspace.panel_shell_activity.as_ref().unwrap()[0].state,
        SessionPanelShellActivityStateSnapshot::CommandRunning
    );
}

#[test]
fn rich_move_persists_then_updates_baseline_then_emits_exact_committed_snapshot() {
    let before = rich_snapshot();
    let authority = GatedSnapshot::new(before.clone());
    let mut publication = RecordingPublication::new(&before);

    let committed = transact_snapshot_if_changed(&authority, &mut publication, |candidate| {
        apply_move_panel_to_new_workspace(candidate, "surface-2")
    })
    .unwrap();

    assert_eq!(publication.calls, ["persist", "baseline", "emit"]);
    assert_eq!(*authority.lock().unwrap(), committed);
    assert_eq!(publication.baseline, committed);
    assert_eq!(publication.events, [committed.clone()]);
    assert_destination_metadata(&committed);
}

#[test]
fn persistence_failure_exposes_no_destination_workspace_pane_or_uuid() {
    let before = rich_snapshot();
    let authority = GatedSnapshot::new(before.clone());
    let mut publication = RecordingPublication::new(&before);
    publication.persist_error = Some("injected move persistence failure".into());

    assert_eq!(
        transact_snapshot_if_changed(&authority, &mut publication, |candidate| {
            apply_move_panel_to_new_workspace(candidate, "surface-2")
        })
        .unwrap_err(),
        "injected move persistence failure"
    );
    assert_eq!(publication.calls, ["persist"]);
    assert_eq!(*authority.lock().unwrap(), before);
    assert_eq!(publication.baseline, before);
    assert!(publication.events.is_empty());
    assert_eq!(before.windows[0].tab_manager.workspaces.len(), 1);
}

#[test]
fn missing_and_sole_panel_are_exact_successful_no_ops_with_zero_publication() {
    for (before, panel_id) in [
        (rich_snapshot(), "missing"),
        (initial_snapshot("surface-1"), "surface-1"),
    ] {
        let authority = GatedSnapshot::new(before.clone());
        let mut publication = RecordingPublication::new(&before);

        let returned = transact_snapshot_if_changed(&authority, &mut publication, |candidate| {
            apply_move_panel_to_new_workspace(candidate, panel_id)
        })
        .unwrap();

        assert_eq!(returned, before);
        assert_eq!(*authority.lock().unwrap(), before);
        assert_eq!(publication.baseline, before);
        assert!(publication.calls.is_empty());
        assert!(publication.events.is_empty());
    }
}

fn function_source<'a>(source: &'a str, signature: &str) -> &'a str {
    let start = source
        .find(signature)
        .unwrap_or_else(|| panic!("missing {signature}"));
    let tail = &source[start..];
    let body_start = tail.find('{').unwrap();
    let mut depth = 0usize;
    for (offset, character) in tail[body_start..].char_indices() {
        match character {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return &tail[..body_start + offset + 1];
                }
            }
            _ => {}
        }
    }
    panic!("unterminated {signature}")
}

#[test]
fn public_and_control_routes_share_the_fallible_change_gated_helper_without_bypasses() {
    let source = include_str!("../session.rs");
    let public = function_source(source, "pub fn session_move_panel_to_new_workspace(");
    assert!(public.contains(") -> Result<AppSessionSnapshot, String>"));
    assert!(public.contains("move_panel_to_new_workspace_for_control("));
    assert!(!public.contains("snapshot.lock()") && !public.contains("notify_session_changed("));

    let control = function_source(
        source,
        "pub(crate) fn move_panel_to_new_workspace_for_control(",
    );
    assert!(control.contains(") -> Result<AppSessionSnapshot, String>"));
    assert!(control.contains("state.transact_snapshot_if_changed(app,"));
    assert!(!control.contains("snapshot.lock()") && !control.contains("notify_session_changed("));
}

#[test]
fn socket_route_keeps_existing_success_shape_and_maps_publication_failure_internal() {
    let source = include_str!("../control_socket.rs");
    let body = function_source(source, "fn surface_move_to_new_workspace(");
    assert!(body.contains("Ok(snapshot) => workspace_current(&snapshot)"));
    assert!(body.contains("Err(message) => ControlCallResult::Err"));
    assert!(body.contains("code: \"internal\".to_string()"));
}
