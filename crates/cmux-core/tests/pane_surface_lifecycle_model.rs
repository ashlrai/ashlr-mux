//! RED contract tests for the authoritative pane/surface lifecycle model.
//!
//! These tests intentionally name the smallest public core API needed by the
//! desktop adapters.  The current pane-shaped snapshot plus parallel metadata
//! arrays cannot satisfy these invariants, so this test-only commit must not
//! compile until the model exists.

use cmux_core::session::SessionTabManagerSnapshot;
use cmux_core::surface_lifecycle::{
    AttachOutcome, ContainerKind, LegacyPaneSnapshot, LifecycleSnapshot, MoveFailure, PaneSeed,
    RuntimeHandle, SurfaceKind, SurfaceLifecycleModel, SurfaceMetadata, SurfaceSeed,
};

fn pane(id: &str, workspace_id: &str) -> PaneSeed {
    PaneSeed {
        pane_id: id.into(),
        window_id: "window-1".into(),
        workspace_id: workspace_id.into(),
        container: ContainerKind::Workspace,
    }
}

fn surface(id: &str, pane_id: &str, kind: SurfaceKind) -> SurfaceSeed {
    SurfaceSeed {
        surface_id: id.into(),
        pane_id: pane_id.into(),
        kind,
        metadata: SurfaceMetadata::default(),
    }
}

#[test]
fn heterogeneous_surfaces_are_authoritative_records_not_pane_wide_state() {
    let mut model = SurfaceLifecycleModel::new();
    model.add_pane(pane("pane-1", "workspace-1")).unwrap();
    model
        .reserve_surface(surface("terminal-1", "pane-1", SurfaceKind::Terminal))
        .unwrap();
    model
        .reserve_surface(surface(
            "browser-1",
            "pane-1",
            SurfaceKind::Browser {
                url: "https://example.test".into(),
            },
        ))
        .unwrap();
    model
        .reserve_surface(surface(
            "markdown-1",
            "pane-1",
            SurfaceKind::Markdown {
                path: "C:/repo/README.md".into(),
            },
        ))
        .unwrap();

    assert_eq!(
        model.pane("pane-1").unwrap().surface_ids,
        ["terminal-1", "browser-1", "markdown-1"]
    );
    assert!(matches!(
        model.surface("terminal-1").unwrap().kind,
        SurfaceKind::Terminal
    ));
    assert!(matches!(
        model.surface("browser-1").unwrap().kind,
        SurfaceKind::Browser { .. }
    ));
    assert!(matches!(
        model.surface("markdown-1").unwrap().kind,
        SurfaceKind::Markdown { .. }
    ));
}

#[test]
fn pane_selection_is_distinct_from_workspace_focus() {
    let mut model = SurfaceLifecycleModel::new();
    model.add_pane(pane("pane-left", "workspace-1")).unwrap();
    model.add_pane(pane("pane-right", "workspace-1")).unwrap();
    for (id, pane_id) in [
        ("left-a", "pane-left"),
        ("left-b", "pane-left"),
        ("right-a", "pane-right"),
        ("right-b", "pane-right"),
    ] {
        model
            .reserve_surface(surface(id, pane_id, SurfaceKind::Terminal))
            .unwrap();
    }

    model.select_in_pane("left-b").unwrap();
    model.select_in_pane("right-b").unwrap();
    model.focus_surface("left-b").unwrap();

    assert_eq!(
        model.pane("pane-left").unwrap().selected_surface_id,
        "left-b"
    );
    assert_eq!(
        model.pane("pane-right").unwrap().selected_surface_id,
        "right-b"
    );
    assert_eq!(model.focused_surface("workspace-1"), Some("left-b"));
    assert!(!model.surface("right-b").unwrap().is_workspace_focused);
}

#[test]
fn stable_identity_and_generation_reject_stale_create_callbacks() {
    let mut model = SurfaceLifecycleModel::new();
    model.add_pane(pane("pane-1", "workspace-1")).unwrap();
    let first = model
        .reserve_surface(surface("surface-1", "pane-1", SurfaceKind::Terminal))
        .unwrap();
    model.close_surface("surface-1").unwrap();
    let second = model
        .reserve_surface(surface("surface-1", "pane-1", SurfaceKind::Terminal))
        .unwrap();

    assert_eq!(first.surface_id, second.surface_id);
    assert!(second.generation > first.generation);
    assert_eq!(
        model.attach_runtime(
            "surface-1",
            first.generation,
            RuntimeHandle::new("stale-runtime")
        ),
        AttachOutcome::StaleCleaned
    );
    assert_eq!(model.surface("surface-1").unwrap().runtime, None);
    assert_eq!(
        model.attach_runtime(
            "surface-1",
            second.generation,
            RuntimeHandle::new("live-runtime")
        ),
        AttachOutcome::Attached
    );
    assert_eq!(
        model.owner_of_runtime("live-runtime").unwrap().surface_id,
        "surface-1"
    );
}

#[test]
fn close_removes_runtime_pending_metadata_and_owner_indexes_once() {
    let mut model = SurfaceLifecycleModel::new();
    model.add_pane(pane("pane-1", "workspace-1")).unwrap();
    let token = model
        .reserve_surface(surface("surface-1", "pane-1", SurfaceKind::Terminal))
        .unwrap();
    model.attach_runtime(
        "surface-1",
        token.generation,
        RuntimeHandle::new("runtime-1"),
    );
    model.queue_pending_pwd("surface-1", "C:/repo").unwrap();
    model
        .surface_mut("surface-1")
        .unwrap()
        .metadata
        .custom_title = Some("api".into());

    let closed = model.close_surface("surface-1").unwrap();
    assert_eq!(closed.surface_id, "surface-1");
    assert_eq!(closed.runtime.unwrap().id(), "runtime-1");
    assert!(model.surface("surface-1").is_none());
    assert!(model.owner_of_surface("surface-1").is_none());
    assert!(model.owner_of_runtime("runtime-1").is_none());
    assert!(!model.has_pending_pwd("surface-1"));
    assert!(model.close_surface("surface-1").is_err());
}

#[test]
fn move_transfers_the_complete_record_and_failed_attach_rolls_back() {
    let mut model = SurfaceLifecycleModel::new();
    model.add_pane(pane("pane-a", "workspace-a")).unwrap();
    model.add_pane(pane("pane-b", "workspace-b")).unwrap();
    let mut seed = surface(
        "browser-1",
        "pane-a",
        SurfaceKind::Browser {
            url: "https://example.test".into(),
        },
    );
    seed.metadata.custom_title = Some("docs".into());
    seed.metadata.pinned = true;
    model.reserve_surface(seed).unwrap();
    model.queue_pending_pwd("browser-1", "C:/docs").unwrap();

    model.move_surface("browser-1", "pane-b", 0).unwrap();
    assert_eq!(
        model.owner_of_surface("browser-1").unwrap().pane_id,
        "pane-b"
    );
    assert_eq!(
        model
            .surface("browser-1")
            .unwrap()
            .metadata
            .custom_title
            .as_deref(),
        Some("docs")
    );
    assert!(model.surface("browser-1").unwrap().metadata.pinned);
    assert!(model.has_pending_pwd("browser-1"));

    let before = model.snapshot();
    assert_eq!(
        model.move_surface_with_failure("browser-1", "pane-a", 0, MoveFailure::Attach),
        Err(MoveFailure::Attach)
    );
    assert_eq!(model.snapshot(), before);
}

#[test]
fn respawn_keeps_public_identity_and_replaces_runtime_generation() {
    let mut model = SurfaceLifecycleModel::new();
    model.add_pane(pane("pane-1", "workspace-1")).unwrap();
    let token = model
        .reserve_surface(surface("surface-1", "pane-1", SurfaceKind::Terminal))
        .unwrap();
    model.attach_runtime(
        "surface-1",
        token.generation,
        RuntimeHandle::new("runtime-old"),
    );

    let replacement = model
        .begin_respawn("surface-1", "pwsh -NoLogo", Some("C:/repo"))
        .unwrap();
    assert_eq!(replacement.surface_id, "surface-1");
    assert!(replacement.generation > token.generation);
    assert!(model.owner_of_runtime("runtime-old").is_none());
    assert_eq!(
        model.attach_runtime(
            "surface-1",
            token.generation,
            RuntimeHandle::new("late-old-runtime")
        ),
        AttachOutcome::StaleCleaned
    );
    model.attach_runtime(
        "surface-1",
        replacement.generation,
        RuntimeHandle::new("runtime-new"),
    );
    let record = model.surface("surface-1").unwrap();
    assert_eq!(record.surface_id, "surface-1");
    assert_eq!(
        record.terminal_startup.command.as_deref(),
        Some("pwsh -NoLogo")
    );
    assert_eq!(
        record.terminal_startup.working_directory.as_deref(),
        Some("C:/repo")
    );
}

#[test]
fn pending_pwd_applies_once_to_the_matching_arrival_generation() {
    let mut model = SurfaceLifecycleModel::new();
    model.add_pane(pane("pane-1", "workspace-1")).unwrap();
    model
        .queue_remote_pwd("workspace-1", Some("remote-42"), "/srv/app")
        .unwrap();
    let token = model
        .reserve_remote_arrival("surface-1", "pane-1", "remote-42")
        .unwrap();

    assert_eq!(
        model.reconcile_remote_arrival("surface-1", token.generation),
        AttachOutcome::Attached
    );
    let record = model.surface("surface-1").unwrap();
    assert_eq!(
        record.metadata.reported_directory.as_deref(),
        Some("/srv/app")
    );
    assert_eq!(record.metadata.directory_apply_count, 1);
    assert!(!model.has_pending_remote_pwd("workspace-1", "remote-42"));

    model.reconcile_remote_arrival("surface-1", token.generation);
    assert_eq!(
        model
            .surface("surface-1")
            .unwrap()
            .metadata
            .directory_apply_count,
        1
    );
}

#[test]
fn authoritative_snapshot_restores_identity_kinds_selection_and_transient_runtime_rules() {
    let snapshot = LifecycleSnapshot::from_json(serde_json::json!({
        "version": 2,
        "panes": [{
            "pane_id": "pane-1",
            "window_id": "window-1",
            "workspace_id": "workspace-1",
            "container": "workspace",
            "surface_ids": ["terminal-1", "browser-1"],
            "selected_surface_id": "browser-1"
        }],
        "focused_surfaces": {"workspace-1": "terminal-1"},
        "surfaces": [{
            "surface_id": "terminal-1",
            "pane_id": "pane-1",
            "generation": 7,
            "kind": {"type": "terminal"},
            "metadata": {"custom_title": "shell", "reported_directory": "C:/repo"}
        }, {
            "surface_id": "browser-1",
            "pane_id": "pane-1",
            "generation": 3,
            "kind": {"type": "browser", "url": "https://example.test"},
            "metadata": {"pinned": true, "unread": true}
        }]
    }))
    .unwrap();

    let model = SurfaceLifecycleModel::restore(snapshot).unwrap();
    assert_eq!(
        model.pane("pane-1").unwrap().surface_ids,
        ["terminal-1", "browser-1"]
    );
    assert_eq!(
        model.pane("pane-1").unwrap().selected_surface_id,
        "browser-1"
    );
    assert_eq!(model.focused_surface("workspace-1"), Some("terminal-1"));
    assert_eq!(model.surface("terminal-1").unwrap().generation, 7);
    assert_eq!(model.surface("terminal-1").unwrap().runtime, None);
    assert!(model.surface("browser-1").unwrap().metadata.pinned);
}

#[test]
fn legacy_parallel_snapshot_migrates_without_kind_or_metadata_leakage() {
    let legacy = LegacyPaneSnapshot {
        pane_id: "pane-1".into(),
        window_id: "window-1".into(),
        workspace_id: "workspace-1".into(),
        panel_ids: vec!["terminal-1".into(), "browser-1".into()],
        selected_panel_id: Some("browser-1".into()),
        pane_surface_kind: Some("terminal".into()),
        per_panel_kinds: vec![("browser-1".into(), "browser".into())],
        panel_titles: vec![("terminal-1".into(), "shell".into())],
        panel_pins: vec![("browser-1".into(), true)],
        browser_urls: vec![("browser-1".into(), "https://example.test".into())],
    };

    let model = SurfaceLifecycleModel::migrate_legacy(vec![legacy]).unwrap();
    assert!(matches!(
        model.surface("terminal-1").unwrap().kind,
        SurfaceKind::Terminal
    ));
    assert!(matches!(
        model.surface("browser-1").unwrap().kind,
        SurfaceKind::Browser { .. }
    ));
    assert_eq!(
        model
            .surface("terminal-1")
            .unwrap()
            .metadata
            .custom_title
            .as_deref(),
        Some("shell")
    );
    assert_eq!(
        model.surface("browser-1").unwrap().metadata.custom_title,
        None
    );
    assert!(!model.surface("terminal-1").unwrap().metadata.pinned);
    assert!(model.surface("browser-1").unwrap().metadata.pinned);
    assert!(model.validate_indexes().is_ok());
}

#[test]
fn real_session_snapshot_migrates_to_and_restores_from_one_surface_record_source() {
    // This is deliberately the real persisted cmux-core type, not a lifecycle
    // DTO. It represents today's pane-wide kind and parallel metadata wire
    // shape and therefore exercises the required compatibility boundary.
    let legacy_json = serde_json::json!({
        "selected_workspace_index": 0,
        "workspaces": [{
            "workspace_id": "workspace-1",
            "process_title": "Terminal",
            "focused_panel_id": "terminal-1",
            "layout": {
                "type": "pane",
                "pane": {
                    "pane_id": "pane-1",
                    "panel_ids": ["terminal-1", "browser-1"],
                    "selected_panel_id": "browser-1",
                    "surface_kind": "terminal",
                    "browser_url": "https://wrong-pane-wide.test"
                }
            },
            "panel_titles": [{"panel_id": "terminal-1", "custom_title": "shell"}],
            "panel_pins": [{"panel_id": "browser-1", "is_pinned": true}],
            "panel_unreads": [{"panel_id": "browser-1", "is_unread": true}],
            "panel_terminal_startups": [{
                "panel_id": "terminal-1",
                "initial_terminal_command": "pwsh -NoLogo"
            }]
        }]
    });
    let legacy: SessionTabManagerSnapshot = serde_json::from_value(legacy_json).unwrap();

    let model = SurfaceLifecycleModel::from_session_snapshot("window-1", &legacy).unwrap();
    assert!(matches!(
        model.surface("terminal-1").unwrap().kind,
        SurfaceKind::Terminal
    ));
    assert!(matches!(
        model.surface("browser-1").unwrap().kind,
        SurfaceKind::Browser { .. }
    ));

    let migrated: SessionTabManagerSnapshot = model.to_session_snapshot(&legacy).unwrap();
    let persisted = serde_json::to_value(&migrated).unwrap();
    let workspace = &persisted["workspaces"][0];
    let records = workspace["surfaces"].as_array().unwrap();
    assert_eq!(
        records
            .iter()
            .map(|record| record["surface_id"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["terminal-1", "browser-1"]
    );
    assert_eq!(records[0]["kind"]["type"], "terminal");
    assert_eq!(records[0]["metadata"]["custom_title"], "shell");
    assert_eq!(records[0]["terminal_startup"]["command"], "pwsh -NoLogo");
    assert_eq!(records[1]["kind"]["type"], "browser");
    assert_eq!(records[1]["metadata"]["pinned"], true);
    assert_eq!(records[1]["metadata"]["unread"], true);

    // Once migrated, no serialized pane-wide kind or parallel metadata may
    // remain authoritative beside the surface records.
    assert!(workspace.get("panel_titles").is_none());
    assert!(workspace.get("panel_pins").is_none());
    assert!(workspace.get("panel_unreads").is_none());
    assert!(workspace.get("panel_terminal_startups").is_none());
    let pane = &workspace["layout"]["pane"];
    assert!(pane.get("surface_kind").is_none());
    assert!(pane.get("browser_url").is_none());

    // Exercise serde on the actual session schema before rebuilding the model;
    // this prevents a separate in-memory lifecycle snapshot from passing.
    let encoded = serde_json::to_string(&migrated).unwrap();
    let restored_session: SessionTabManagerSnapshot = serde_json::from_str(&encoded).unwrap();
    let restored =
        SurfaceLifecycleModel::from_session_snapshot("window-1", &restored_session).unwrap();
    assert_eq!(restored.snapshot(), model.snapshot());
}

#[test]
fn close_move_and_respawn_project_back_into_the_real_session_schema() {
    let base: SessionTabManagerSnapshot = serde_json::from_value(serde_json::json!({
        "selected_workspace_index": 0,
        "workspaces": [{
            "workspace_id": "workspace-1",
            "process_title": "Terminal",
            "focused_panel_id": "terminal-a",
            "layout": {
                "type": "split",
                "split": {
                    "split_id": "split-1",
                    "orientation": "horizontal",
                    "divider_position": 0.5,
                    "first": {"type": "pane", "pane": {
                        "pane_id": "pane-a",
                        "panel_ids": ["terminal-a", "browser-a"],
                        "selected_panel_id": "terminal-a"
                    }},
                    "second": {"type": "pane", "pane": {
                        "pane_id": "pane-b",
                        "panel_ids": ["terminal-b"],
                        "selected_panel_id": "terminal-b"
                    }}
                }
            },
            "surfaces": [{
                "surface_id": "terminal-a",
                "pane_id": "pane-a",
                "generation": 2,
                "kind": {"type": "terminal"},
                "metadata": {"custom_title": "api"},
                "terminal_startup": {"command": "pwsh"}
            }, {
                "surface_id": "browser-a",
                "pane_id": "pane-a",
                "generation": 1,
                "kind": {"type": "browser", "url": "https://example.test"},
                "metadata": {"pinned": false}
            }, {
                "surface_id": "terminal-b",
                "pane_id": "pane-b",
                "generation": 1,
                "kind": {"type": "terminal"},
                "metadata": {}
            }]
        }]
    }))
    .unwrap();
    let mut model = SurfaceLifecycleModel::from_session_snapshot("window-1", &base).unwrap();

    model.close_surface("browser-a").unwrap();
    model.move_surface("terminal-a", "pane-b", 1).unwrap();
    let respawn = model
        .begin_respawn("terminal-a", "pwsh -NoProfile", Some("C:/repo"))
        .unwrap();
    assert_eq!(respawn.surface_id, "terminal-a");

    let projected = model.to_session_snapshot(&base).unwrap();
    let json = serde_json::to_value(projected).unwrap();
    let workspace = &json["workspaces"][0];
    let records = workspace["surfaces"].as_array().unwrap();
    assert_eq!(records.len(), 2);
    assert!(records
        .iter()
        .all(|record| record["surface_id"] != "browser-a"));
    let terminal_a = records
        .iter()
        .find(|record| record["surface_id"] == "terminal-a")
        .unwrap();
    assert_eq!(terminal_a["pane_id"], "pane-b");
    assert_eq!(terminal_a["generation"], respawn.generation);
    assert_eq!(terminal_a["metadata"]["custom_title"], "api");
    assert_eq!(terminal_a["terminal_startup"]["command"], "pwsh -NoProfile");
    assert_eq!(
        terminal_a["terminal_startup"]["working_directory"],
        "C:/repo"
    );
    // Moving the last surface out collapses the empty source pane in the same
    // projection transaction; the surviving pane keeps its stable identity.
    assert_eq!(workspace["layout"]["type"], "pane");
    assert_eq!(workspace["layout"]["pane"]["pane_id"], "pane-b");
    assert_eq!(
        workspace["layout"]["pane"]["panel_ids"],
        serde_json::json!(["terminal-b", "terminal-a"])
    );
    assert!(workspace.get("panel_titles").is_none());
    assert!(workspace.get("panel_pins").is_none());
    assert!(workspace.get("panel_unreads").is_none());
    assert!(workspace.get("panel_terminal_startups").is_none());
}
