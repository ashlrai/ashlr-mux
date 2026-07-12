//! RED contract tests for the authoritative pane/surface lifecycle model.
//!
//! These tests intentionally name the smallest public core API needed by the
//! desktop adapters.  The current pane-shaped snapshot plus parallel metadata
//! arrays cannot satisfy these invariants, so this test-only commit must not
//! compile until the model exists.

use cmux_core::session::{AppSessionSnapshot, SessionTabManagerSnapshot};
use cmux_core::surface_lifecycle::{
    AttachOutcome, CloseIntent, ContainerKind, LegacyPaneSnapshot, LifecycleSnapshot,
    MoveTransactionError, PaneSeed, RuntimeHandle, SurfaceKind, SurfaceLifecycleModel,
    SurfaceMetadata, SurfaceSeed,
};

fn pane(id: &str, workspace_id: &str) -> PaneSeed {
    PaneSeed {
        pane_id: id.into(),
        window_id: "window-1".into(),
        workspace_id: workspace_id.into(),
        container: ContainerKind::Workspace,
    }
}

#[test]
fn legacy_browser_url_survives_missing_selection_without_fabricating_multiple_urls() {
    for panel_ids in [
        serde_json::json!(["browser-1"]),
        serde_json::json!(["browser-1", "other"]),
    ] {
        let tabs: SessionTabManagerSnapshot = serde_json::from_value(serde_json::json!({
            "workspaces":[{"process_title":"browser","layout":{"type":"pane","pane":{"pane_id":"pane-1","panel_ids":panel_ids,"surface_kind":"browser","browser_url":"https://example.test"}}}]
        })).unwrap();
        let model = SurfaceLifecycleModel::from_session_snapshot("window-1", &tabs).unwrap();
        let projected = serde_json::to_value(model.to_session_snapshot(&tabs).unwrap()).unwrap();
        let urls = projected["workspaces"][0]["surfaces"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|surface| surface["kind"].get("url").and_then(|url| url.as_str()))
            .collect::<Vec<_>>();
        assert_eq!(urls, ["https://example.test"]);
    }
}

#[test]
fn generated_surface_kind_options_are_optional_not_required_nullable() {
    let generated = include_str!(
        "../../../apps/desktop/packages/core-types/src/generated/SessionSurfaceKindSnapshot.ts"
    );
    for property in [
        "url",
        "proxy_url",
        "provider",
        "renderer",
        "path",
        "token",
        "remote_session_id",
    ] {
        assert!(
            generated.contains(&format!("{property}?:")),
            "{property} must be optional: {generated}"
        );
        assert!(
            !generated.contains(&format!("{property}: string | null")),
            "{property} must not be required nullable"
        );
    }
}

#[test]
fn guarded_close_rejects_last_surface_and_range_close_skips_pinned() {
    let mut model = SurfaceLifecycleModel::new();
    model.add_pane(pane("pane-1", "workspace-1")).unwrap();
    model
        .reserve_surface(surface("pinned", "pane-1", SurfaceKind::Terminal))
        .unwrap();
    model
        .update_metadata("pinned", |metadata| metadata.pinned = true)
        .unwrap();
    assert!(model.close_surface("pinned", CloseIntent::Range).is_err());
    assert!(model
        .close_surface("pinned", CloseIntent::Explicit)
        .is_err());
    model
        .reserve_surface(surface("other", "pane-1", SurfaceKind::Terminal))
        .unwrap();
    assert!(model.close_surface("pinned", CloseIntent::Range).is_err());
    assert_eq!(
        model
            .close_surface("pinned", CloseIntent::Explicit)
            .unwrap()
            .surface_id,
        "pinned"
    );
}

#[test]
fn pending_surface_pwd_restarts_applies_once_and_rejects_ghosts() {
    let base: AppSessionSnapshot = serde_json::from_value(serde_json::json!({"version":1,"created_at":0,"windows":[{"window_id":"window-1","tab_manager":{"workspaces":[{"workspace_id":"workspace-1","process_title":"shell","layout":{"type":"pane","pane":{"pane_id":"pane-1","panel_ids":["surface-1"]}},"surfaces":[{"surface_id":"surface-1","pane_id":"pane-1","generation":1,"kind":{"type":"terminal"},"metadata":{}}]}]}}]})).unwrap();
    let mut model = SurfaceLifecycleModel::from_app_session(&base).unwrap();
    assert!(model.queue_pending_pwd("ghost", "C:/bad").is_err());
    model.queue_pending_pwd("surface-1", "C:/repo").unwrap();
    let persisted = model.to_app_session(&base).unwrap();
    let mut restored = SurfaceLifecycleModel::from_app_session(&persisted).unwrap();
    assert!(restored.has_pending_pwd("surface-1"));
    assert_eq!(
        restored.attach_runtime("surface-1", 1, RuntimeHandle::new("runtime")),
        AttachOutcome::Attached
    );
    let applied = restored.surface("surface-1").unwrap().metadata.clone();
    assert_eq!(
        restored.attach_runtime("surface-1", 1, RuntimeHandle::new("runtime")),
        AttachOutcome::Attached
    );
    assert_eq!(restored.surface("surface-1").unwrap().metadata, applied);
}

#[test]
fn pending_surface_pwd_serialization_is_stable_across_insertion_order() {
    fn projected(order: [&str; 2]) -> serde_json::Value {
        let base: SessionTabManagerSnapshot = serde_json::from_value(serde_json::json!({
            "workspaces":[{"workspace_id":"workspace-1","process_title":"shell","layout":{"type":"pane","pane":{"pane_id":"pane-1","panel_ids":["a","b"]}},"surfaces":[
                {"surface_id":"a","pane_id":"pane-1","generation":1,"kind":{"type":"terminal"},"metadata":{}},
                {"surface_id":"b","pane_id":"pane-1","generation":1,"kind":{"type":"terminal"},"metadata":{}}
            ]}]
        })).unwrap();
        let mut model = SurfaceLifecycleModel::from_session_snapshot("window-1", &base).unwrap();
        for id in order {
            model.queue_pending_pwd(id, &format!("C:/{id}")).unwrap();
        }
        serde_json::to_value(model.to_session_snapshot(&base).unwrap()).unwrap()
    }
    assert_eq!(projected(["a", "b"]), projected(["b", "a"]));
}

#[test]
fn missing_workspace_ids_materialize_globally_unique_stable_ids() {
    let legacy: AppSessionSnapshot = serde_json::from_value(serde_json::json!({"version":1,"created_at":0,"windows":[
        {"window_id":"window-a","tab_manager":{"workspaces":[{"process_title":"a","layout":null}]}},
        {"window_id":"window-b","tab_manager":{"workspaces":[{"process_title":"b","layout":null}]}}
    ]})).unwrap();
    let first = SurfaceLifecycleModel::from_app_session(&legacy)
        .unwrap()
        .to_app_session(&legacy)
        .unwrap();
    let ids = first
        .windows
        .iter()
        .map(|window| {
            window.tab_manager.workspaces[0]
                .workspace_id
                .clone()
                .unwrap()
        })
        .collect::<Vec<_>>();
    assert_ne!(ids[0], ids[1]);
    let second = SurfaceLifecycleModel::from_app_session(&first)
        .unwrap()
        .to_app_session(&first)
        .unwrap();
    assert_eq!(
        second
            .windows
            .iter()
            .map(|window| window.tab_manager.workspaces[0]
                .workspace_id
                .clone()
                .unwrap())
            .collect::<Vec<_>>(),
        ids
    );
}

fn surface(id: &str, pane_id: &str, kind: SurfaceKind) -> SurfaceSeed {
    SurfaceSeed {
        surface_id: id.into(),
        pane_id: pane_id.into(),
        kind,
        metadata: SurfaceMetadata::default(),
    }
}

fn browser(url: &str) -> SurfaceKind {
    SurfaceKind::Browser {
        url: Some(url.into()),
        proxy_url: None,
        back_history: None,
        forward_history: None,
        omnibar_visible: None,
        focus_mode_active: None,
        developer_tools_visible: None,
        developer_tools_panel: None,
        page_zoom: None,
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
            browser("https://example.test"),
        ))
        .unwrap();
    model
        .reserve_surface(surface(
            "markdown-1",
            "pane-1",
            SurfaceKind::Markdown {
                path: Some("C:/repo/README.md".into()),
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
        .set_custom_title("surface-1", Some("api".into()))
        .unwrap();

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
    let mut seed = surface("browser-1", "pane-a", browser("https://example.test"));
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
        model.move_surface_transactionally("browser-1", "pane-a", 0, |_, _| Err("attach")),
        Err(MoveTransactionError::Effect("attach"))
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
fn surface_pwd_queued_before_runtime_arrival_applies_once_on_attach() {
    let mut model = SurfaceLifecycleModel::new();
    model.add_pane(pane("pane-1", "workspace-1")).unwrap();
    model.queue_pending_pwd("surface-1", "C:/early").unwrap();
    let token = model
        .reserve_surface(surface("surface-1", "pane-1", SurfaceKind::Terminal))
        .unwrap();
    assert_eq!(
        model
            .surface("surface-1")
            .unwrap()
            .metadata
            .reported_directory,
        None
    );
    assert_eq!(
        model.attach_runtime(
            "surface-1",
            token.generation,
            RuntimeHandle::new("runtime-1")
        ),
        AttachOutcome::Attached
    );
    assert_eq!(
        model
            .surface("surface-1")
            .unwrap()
            .metadata
            .reported_directory
            .as_deref(),
        Some("C:/early")
    );
    assert_eq!(
        model
            .surface("surface-1")
            .unwrap()
            .metadata
            .directory_apply_count,
        1
    );
    assert!(!model.has_pending_pwd("surface-1"));
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

#[test]
fn browser_and_nonterminal_kind_state_round_trips_losslessly() {
    let input = serde_json::json!({
        "selected_workspace_index": 0,
        "workspaces": [{
            "workspace_id": "workspace-1", "process_title": "mixed",
            "layout": {"type":"pane","pane":{"pane_id":"pane-1","panel_ids":["browser","markdown","file","diff","remote","agent"],"selected_panel_id":"browser"}},
            "surfaces": [
                {"surface_id":"browser","pane_id":"pane-1","generation":1,"kind":{"type":"browser","url":"https://now.test","proxy_url":"socks5://127.0.0.1:9","back_history":["https://back.test"],"forward_history":["https://forward.test"],"omnibar_visible":false,"focus_mode_active":true,"developer_tools_visible":true,"developer_tools_panel":"console","page_zoom":1.23456789},"metadata":{}},
                {"surface_id":"markdown","pane_id":"pane-1","generation":1,"kind":{"type":"markdown","path":"README.md"},"metadata":{}},
                {"surface_id":"file","pane_id":"pane-1","generation":1,"kind":{"type":"file","path":"src/main.rs"},"metadata":{}},
                {"surface_id":"diff","pane_id":"pane-1","generation":1,"kind":{"type":"diff","token":"diff-7","request_path":"/changes"},"metadata":{}},
                {"surface_id":"remote","pane_id":"pane-1","generation":1,"kind":{"type":"remote_terminal","remote_session_id":"pty-9","remote_context":{"transport":"ssh","host":"box"},"arrival_generation":4},"metadata":{}},
                {"surface_id":"agent","pane_id":"pane-1","generation":1,"kind":{"type":"agent_session","provider":"codex","renderer":"react","working_directory":"C:/repo","session_id":"agent-42","lifecycle":"running"},"metadata":{}}
            ]
        }]
    });
    let tabs: SessionTabManagerSnapshot = serde_json::from_value(input.clone()).unwrap();
    let model = SurfaceLifecycleModel::from_session_snapshot("window-1", &tabs).unwrap();
    let output = serde_json::to_value(model.to_session_snapshot(&tabs).unwrap()).unwrap();
    assert_eq!(
        output["workspaces"][0]["surfaces"],
        input["workspaces"][0]["surfaces"]
    );
}

#[test]
fn legacy_optional_kind_identities_remain_absent_instead_of_becoming_empty_defaults() {
    let tabs: SessionTabManagerSnapshot = serde_json::from_value(serde_json::json!({
        "workspaces":[
            {"workspace_id":"browser-ws","process_title":"browser","layout":{"type":"pane","pane":{"pane_id":"browser-pane","panel_ids":["browser"],"surface_kind":"browser"}}},
            {"workspace_id":"markdown-ws","process_title":"markdown","layout":{"type":"pane","pane":{"pane_id":"markdown-pane","panel_ids":["markdown"],"surface_kind":"markdown"}}},
            {"workspace_id":"file-ws","process_title":"file","layout":{"type":"pane","pane":{"pane_id":"file-pane","panel_ids":["file"],"surface_kind":"file"}}},
            {"workspace_id":"diff-ws","process_title":"diff","layout":{"type":"pane","pane":{"pane_id":"diff-pane","panel_ids":["diff"],"surface_kind":"diff"}}},
            {"workspace_id":"remote-ws","process_title":"remote","layout":{"type":"pane","pane":{"pane_id":"remote-pane","panel_ids":["remote"],"surface_kind":"remote_terminal"}}},
            {"workspace_id":"agent-ws","process_title":"agent","layout":{"type":"pane","pane":{"pane_id":"agent-pane","panel_ids":["agent"],"surface_kind":"agent"}}}
        ]
    })).unwrap();
    let model = SurfaceLifecycleModel::from_session_snapshot("window-1", &tabs).unwrap();
    let json = serde_json::to_value(model.to_session_snapshot(&tabs).unwrap()).unwrap();
    for (index, forbidden) in [
        (0, "url"),
        (1, "path"),
        (2, "path"),
        (3, "token"),
        (4, "remote_session_id"),
        (5, "provider"),
    ] {
        assert!(json["workspaces"][index]["surfaces"][0]["kind"]
            .get(forbidden)
            .is_none());
    }
    assert!(json["workspaces"][5]["surfaces"][0]["kind"]
        .get("renderer")
        .is_none());
}

#[test]
fn legacy_pane_identity_is_materialized_before_projection() {
    let legacy: SessionTabManagerSnapshot = serde_json::from_value(serde_json::json!({
        "workspaces":[{"workspace_id":"workspace-1","process_title":"shell","layout":{"type":"pane","pane":{"panel_ids":["surface-1"],"selected_panel_id":"surface-1"}}}]
    })).unwrap();
    let model = SurfaceLifecycleModel::from_session_snapshot("window-1", &legacy).unwrap();
    let projected = serde_json::to_value(model.to_session_snapshot(&legacy).unwrap()).unwrap();
    assert_eq!(projected["workspaces"][0]["layout"]["type"], "pane");
    assert_eq!(
        projected["workspaces"][0]["layout"]["pane"]["pane_id"],
        "surface-1"
    );
    assert_eq!(
        projected["workspaces"][0]["layout"]["pane"]["panel_ids"],
        serde_json::json!(["surface-1"])
    );
}

#[test]
fn malformed_authoritative_snapshots_are_rejected_not_silently_reconciled() {
    for surfaces in [
        serde_json::json!([
            {"surface_id":"dup","pane_id":"pane-1","generation":1,"kind":{"type":"terminal"},"metadata":{}},
            {"surface_id":"dup","pane_id":"pane-1","generation":2,"kind":{"type":"terminal"},"metadata":{}}
        ]),
        serde_json::json!([
            {"surface_id":"surface-1","pane_id":"missing-pane","generation":1,"kind":{"type":"terminal"},"metadata":{}}
        ]),
    ] {
        let tabs: SessionTabManagerSnapshot = serde_json::from_value(serde_json::json!({
            "workspaces":[{"workspace_id":"workspace-1","process_title":"bad","layout":{"type":"pane","pane":{"pane_id":"pane-1","panel_ids":["surface-1"],"selected_panel_id":"surface-1"}},"surfaces":surfaces}]
        })).unwrap();
        assert!(SurfaceLifecycleModel::from_session_snapshot("window-1", &tabs).is_err());
    }

    let stale: SessionTabManagerSnapshot = serde_json::from_value(serde_json::json!({
        "workspaces":[{"workspace_id":"workspace-1","process_title":"bad","focused_panel_id":"ghost","layout":{"type":"pane","pane":{"pane_id":"pane-1","panel_ids":["surface-1"],"selected_panel_id":"ghost"}},"surfaces":[{"surface_id":"surface-1","pane_id":"pane-1","generation":1,"kind":{"type":"terminal"},"metadata":{}}]}]
    })).unwrap();
    assert!(SurfaceLifecycleModel::from_session_snapshot("window-1", &stale).is_err());
}

#[test]
fn one_runtime_handle_cannot_attach_to_two_live_surfaces() {
    let mut model = SurfaceLifecycleModel::new();
    model.add_pane(pane("pane-1", "workspace-1")).unwrap();
    let a = model
        .reserve_surface(surface("a", "pane-1", SurfaceKind::Terminal))
        .unwrap();
    let b = model
        .reserve_surface(surface("b", "pane-1", SurfaceKind::Terminal))
        .unwrap();
    assert_eq!(
        model.attach_runtime("a", a.generation, RuntimeHandle::new("runtime")),
        AttachOutcome::Attached
    );
    assert_eq!(
        model.attach_runtime("b", b.generation, RuntimeHandle::new("runtime")),
        AttachOutcome::StaleCleaned
    );
    assert_eq!(model.owner_of_runtime("runtime").unwrap().surface_id, "a");
    assert_eq!(model.surface("b").unwrap().runtime, None);
    assert!(model.validate_indexes().is_ok());
}

#[test]
fn moves_preserve_same_pane_selection_and_rehome_source_workspace_focus() {
    let mut model = SurfaceLifecycleModel::new();
    model.add_pane(pane("pane-a", "workspace-a")).unwrap();
    model.add_pane(pane("pane-b", "workspace-b")).unwrap();
    for (id, pane_id) in [("a1", "pane-a"), ("a2", "pane-a"), ("b1", "pane-b")] {
        model
            .reserve_surface(surface(id, pane_id, SurfaceKind::Terminal))
            .unwrap();
    }
    model.select_in_pane("a2").unwrap();
    model.focus_surface("a2").unwrap();
    model.move_surface("a2", "pane-a", 0).unwrap();
    assert_eq!(model.pane("pane-a").unwrap().selected_surface_id, "a2");
    model.move_surface("a2", "pane-b", 1).unwrap();
    assert_eq!(model.focused_surface("workspace-a"), Some("a1"));
    assert!(model.surface("a1").unwrap().is_workspace_focused);
    assert!(!model.surface("a2").unwrap().is_workspace_focused);
}

#[test]
fn pending_remote_pwd_is_persisted_moved_applied_once_and_cleaned_on_close_or_respawn() {
    let mut model = SurfaceLifecycleModel::new();
    model.add_pane(pane("pane-a", "workspace-a")).unwrap();
    model.add_pane(pane("pane-b", "workspace-b")).unwrap();
    model
        .queue_remote_pwd("workspace-a", Some("remote-1"), "/srv/a")
        .unwrap();
    let base: SessionTabManagerSnapshot = serde_json::from_value(serde_json::json!({
        "workspaces":[
            {"workspace_id":"workspace-a","process_title":"a","layout":{"type":"pane","pane":{"pane_id":"pane-a","panel_ids":[]}}},
            {"workspace_id":"workspace-b","process_title":"b","layout":{"type":"pane","pane":{"pane_id":"pane-b","panel_ids":[]}}}
        ]
    })).unwrap();
    let persisted = model.to_session_snapshot(&base).unwrap();
    let mut restored =
        SurfaceLifecycleModel::from_session_snapshot("window-1", &persisted).unwrap();
    assert!(restored.has_pending_remote_pwd("workspace-a", "remote-1"));
    let token = restored
        .reserve_remote_arrival("surface-1", "pane-a", "remote-1")
        .unwrap();
    restored.move_surface("surface-1", "pane-b", 0).unwrap();
    assert!(restored.has_pending_remote_pwd("workspace-b", "remote-1"));
    restored.reconcile_remote_arrival("surface-1", token.generation);
    assert_eq!(
        restored
            .surface("surface-1")
            .unwrap()
            .metadata
            .directory_apply_count,
        1
    );
    restored.begin_respawn("surface-1", "pwsh", None).unwrap();
    assert!(!restored.has_pending_remote_pwd("workspace-b", "remote-1"));
    restored
        .queue_remote_pwd("workspace-b", Some("remote-1"), "/srv/b")
        .unwrap();
    restored.close_surface("surface-1").unwrap();
    assert!(!restored.has_pending_remote_pwd("workspace-b", "remote-1"));
}

#[test]
fn constrained_metadata_mutation_preserves_indexes_and_rejects_missing_ids() {
    let mut model = SurfaceLifecycleModel::new();
    model.add_pane(pane("pane-1", "workspace-1")).unwrap();
    model
        .reserve_surface(surface("surface-1", "pane-1", SurfaceKind::Terminal))
        .unwrap();
    model
        .set_custom_title("surface-1", Some("api".into()))
        .unwrap();
    assert_eq!(
        model
            .surface("surface-1")
            .unwrap()
            .metadata
            .custom_title
            .as_deref(),
        Some("api")
    );
    assert!(model.set_custom_title("ghost", Some("bad".into())).is_err());
    assert!(model.validate_indexes().is_ok());
}

#[test]
fn app_wide_authority_moves_across_windows_atomically_and_projects_both() {
    let base: AppSessionSnapshot = serde_json::from_value(serde_json::json!({
        "version":1,"created_at":0,"windows":[
            {"window_id":"window-a","tab_manager":{"workspaces":[{"workspace_id":"workspace-a","process_title":"a","focused_panel_id":"surface-a","layout":{"type":"pane","pane":{"pane_id":"pane-a","panel_ids":["surface-a"],"selected_panel_id":"surface-a"}},"surfaces":[{"surface_id":"surface-a","pane_id":"pane-a","generation":1,"kind":{"type":"terminal"},"metadata":{"custom_title":"api"}}]}]}},
            {"window_id":"window-b","tab_manager":{"workspaces":[{"workspace_id":"workspace-b","process_title":"b","focused_panel_id":"surface-b","layout":{"type":"pane","pane":{"pane_id":"pane-b","panel_ids":["surface-b"],"selected_panel_id":"surface-b"}},"surfaces":[{"surface_id":"surface-b","pane_id":"pane-b","generation":1,"kind":{"type":"terminal"},"metadata":{}}]}]}}
        ]
    })).unwrap();
    let mut model = SurfaceLifecycleModel::from_app_session(&base).unwrap();
    let before = model.snapshot();
    assert_eq!(
        model.move_surface_transactionally("surface-a", "pane-b", 1, |_, _| Err("attach")),
        Err(MoveTransactionError::Effect("attach"))
    );
    assert_eq!(model.snapshot(), before);
    model.move_surface("surface-a", "pane-b", 1).unwrap();
    assert_eq!(
        model.owner_of_surface("surface-a").unwrap().window_id,
        "window-b"
    );
    let projected = serde_json::to_value(model.to_app_session(&base).unwrap()).unwrap();
    assert!(projected["windows"][0]["tab_manager"]["workspaces"][0]["layout"].is_null());
    assert_eq!(
        projected["windows"][1]["tab_manager"]["workspaces"][0]["layout"]["pane"]["panel_ids"],
        serde_json::json!(["surface-b", "surface-a"])
    );
    assert_eq!(
        projected["windows"][1]["tab_manager"]["workspaces"][0]["surfaces"][1]["metadata"]
            ["custom_title"],
        "api"
    );
}
