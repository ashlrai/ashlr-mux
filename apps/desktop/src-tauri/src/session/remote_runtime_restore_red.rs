//! RED restore contract for non-restorable remote runtime mirrors.

use super::*;
use cmux_core::session::{
    SessionSurfaceKindSnapshot, SessionSurfaceMetadataSnapshot, SessionSurfaceSnapshot,
    SessionWorkspaceLayoutSnapshot,
};

struct RecordingPublication;

impl SnapshotPublicationOperations for RecordingPublication {
    fn persist(&mut self, _candidate: &AppSessionSnapshot) -> Result<(), String> {
        Ok(())
    }

    fn update_event_baseline(&mut self, _candidate: &AppSessionSnapshot) {}

    fn emit(&mut self, _candidate: &AppSessionSnapshot) -> Result<(), String> {
        Ok(())
    }
}

#[test]
fn previous_launch_restore_drops_nonrestorable_remote_mirrors() {
    let current = initial_snapshot("surface-1");
    let mut previous = initial_snapshot("surface-9");
    let workspace = &mut previous.windows[0].tab_manager.workspaces[0];
    let SessionWorkspaceLayoutSnapshot::Pane(pane) = workspace.layout.as_ref().unwrap() else {
        panic!("single-pane fixture")
    };
    workspace.surfaces = Some(vec![SessionSurfaceSnapshot {
        surface_id: "surface-9".into(),
        pane_id: pane.pane_id.clone().unwrap(),
        generation: 9,
        kind: SessionSurfaceKindSnapshot::RemoteTerminal {
            remote_session_id: Some("%42".into()),
            remote_context: None,
            arrival_generation: Some(9),
        },
        metadata: SessionSurfaceMetadataSnapshot::default(),
        terminal_startup: None,
        scrollback: None,
    }]);
    let authority = GatedSnapshot::new(current);
    let next_panel = AtomicU64::new(2);
    let restored = restore_previous_launch_transaction(
        &authority,
        &next_panel,
        &mut RecordingPublication,
        || Some(previous),
    )
    .unwrap()
    .snapshot;

    let remote_mirrors = restored
        .windows
        .iter()
        .flat_map(|window| &window.tab_manager.workspaces)
        .flat_map(|workspace| workspace.surfaces.as_deref().unwrap_or_default())
        .filter(|surface| {
            matches!(
                surface.kind,
                SessionSurfaceKindSnapshot::RemoteTerminal { .. }
            )
        })
        .count();
    assert_eq!(
        remote_mirrors, 0,
        "remote runtime mirrors require fresh authoritative observation after restore"
    );
}

#[test]
fn previous_launch_restore_preserves_nonmirror_remote_terminal_metadata() {
    let current = initial_snapshot("surface-1");
    let mut previous = initial_snapshot("surface-9");
    let workspace = &mut previous.windows[0].tab_manager.workspaces[0];
    let SessionWorkspaceLayoutSnapshot::Pane(pane) = workspace.layout.as_ref().unwrap() else {
        panic!("single-pane fixture")
    };
    workspace.surfaces = Some(vec![SessionSurfaceSnapshot {
        surface_id: "surface-9".into(),
        pane_id: pane.pane_id.clone().unwrap(),
        generation: 9,
        kind: SessionSurfaceKindSnapshot::RemoteTerminal {
            remote_session_id: Some("opaque-remote-context".into()),
            remote_context: Some(serde_json::json!({
                "kind": "cloud",
                "opaque": "surface-9"
            })),
            arrival_generation: Some(9),
        },
        metadata: SessionSurfaceMetadataSnapshot::default(),
        terminal_startup: None,
        scrollback: None,
    }]);
    let authority = GatedSnapshot::new(current);
    let next_panel = AtomicU64::new(2);
    let restored = restore_previous_launch_transaction(
        &authority,
        &next_panel,
        &mut RecordingPublication,
        || Some(previous),
    )
    .unwrap()
    .snapshot;

    let restored_surface = restored
        .windows
        .iter()
        .skip(1)
        .flat_map(|window| &window.tab_manager.workspaces)
        .flat_map(|workspace| workspace.surfaces.as_deref().unwrap_or_default())
        .next()
        .expect("typed remote terminal record survives restore");
    let SessionSurfaceKindSnapshot::RemoteTerminal {
        remote_session_id,
        remote_context,
        arrival_generation,
    } = &restored_surface.kind
    else {
        panic!("non-mirror remote terminal metadata was replaced by a scaffold")
    };
    assert_eq!(remote_session_id.as_deref(), Some("opaque-remote-context"));
    assert_eq!(remote_context.as_ref().unwrap()["kind"], "cloud");
    assert_eq!(remote_context.as_ref().unwrap()["opaque"], "surface-9");
    assert_eq!(*arrival_generation, Some(9));
}
