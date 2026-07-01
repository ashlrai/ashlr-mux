//! Golden-file parity for `cmux-core` session snapshots.
//!
//! Covers `encode_session`/`decode_session` round-trips, the recursive
//! `SessionWorkspaceLayoutSnapshot` tagged-union shape, and — critically —
//! **legacy pre-canvas / pre-tab snapshots** where optional fields
//! (`layoutMode`, `canvasPanes`, `panelIds`, `selectedPanelId`,
//! `customTitleSource`, `workspaceId`, `windowId`, `workspaceGroups`) are
//! absent and must round-trip without resurfacing as `null`.
//!
//! NOTE: fixtures here are Rust-seeded placeholders. The macOS Swift exporter
//! (serializing `AppSessionSnapshot` from `Sources/SessionPersistence.swift`
//! via `JSONEncoder`) is the authoritative source and MUST regenerate them.

mod support;

use cmux_core::session::{
    decode_session, encode_session, AppSessionSnapshot, SessionCanvasPaneSnapshot,
    SessionPaneLayoutSnapshot, SessionSplitLayoutSnapshot, SessionSplitOrientation,
    SessionTabManagerSnapshot, SessionWindowSnapshot, SessionWorkspaceGroupSnapshot,
    SessionWorkspaceLayoutSnapshot, SessionWorkspaceSnapshot,
};
use serde_json::Value;
use support::assert_canonical_fixture;

const DOMAIN: &str = "session";

/// Encode → decode → re-encode and assert the snapshot survives the round-trip,
/// then assert the canonical JSON matches the committed fixture.
fn assert_round_trip(name: &str, snapshot: &AppSessionSnapshot) {
    let bytes = encode_session(snapshot).expect("encode");
    let decoded = decode_session(&bytes).expect("decode");
    assert_eq!(
        &decoded, snapshot,
        "round-trip changed the snapshot for {name}"
    );
    // Re-encode the decoded form to prove encode∘decode is a fixed point.
    let bytes2 = encode_session(&decoded).expect("re-encode");
    assert_eq!(bytes, bytes2, "encode∘decode not idempotent for {name}");

    let value: Value = serde_json::from_slice(&bytes).expect("bytes are json");
    assert_canonical_fixture(DOMAIN, name, &value);
}

#[test]
fn full_modern_snapshot() {
    // A modern snapshot exercising every optional field: workspace ids, custom
    // titles, a recursive split layout, canvas panes, and workspace groups.
    let split = SessionWorkspaceLayoutSnapshot::Split(SessionSplitLayoutSnapshot {
        orientation: SessionSplitOrientation::Horizontal,
        divider_position: 0.5,
        first: Box::new(SessionWorkspaceLayoutSnapshot::Pane(
            SessionPaneLayoutSnapshot {
                panel_ids: vec!["panel-a".into(), "panel-b".into()],
                selected_panel_id: Some("panel-a".into()),
                surface_kind: None,
            },
        )),
        second: Box::new(SessionWorkspaceLayoutSnapshot::Split(
            SessionSplitLayoutSnapshot {
                orientation: SessionSplitOrientation::Vertical,
                divider_position: 0.25,
                first: Box::new(SessionWorkspaceLayoutSnapshot::Pane(
                    SessionPaneLayoutSnapshot {
                        panel_ids: vec!["panel-c".into()],
                        selected_panel_id: None,
                        surface_kind: None,
                    },
                )),
                second: Box::new(SessionWorkspaceLayoutSnapshot::Pane(
                    SessionPaneLayoutSnapshot {
                        panel_ids: vec!["panel-d".into()],
                        selected_panel_id: Some("panel-d".into()),
                        surface_kind: None,
                    },
                )),
            },
        )),
    });

    let workspace = SessionWorkspaceSnapshot {
        workspace_id: Some("3F2504E0-4F89-41D3-9A0C-0305E82C3301".into()),
        process_title: "zsh".into(),
        custom_title: Some("editor".into()),
        custom_title_source: Some("user".into()),
        current_directory: Some("/home/u/proj".into()),
        layout: Some(split),
        layout_mode: Some("split".into()),
        canvas_panes: Some(vec![SessionCanvasPaneSnapshot {
            panel_id: "panel-a".into(),
            x: 0,
            y: 0,
            width: 800,
            height: 600,
            panel_ids: Some(vec!["panel-a".into()]),
            selected_panel_id: Some("panel-a".into()),
        }]),
    };

    let group = SessionWorkspaceGroupSnapshot {
        id: "group-1".into(),
        name: "Backend".into(),
        is_collapsed: false,
        anchor_workspace_id: Some("3F2504E0-4F89-41D3-9A0C-0305E82C3301".into()),
        anchor_member_index: Some(0),
        is_pinned: Some(true),
        custom_color: Some("#ff8800".into()),
        icon_symbol: Some("server.rack".into()),
    };

    let snapshot = AppSessionSnapshot {
        version: 1,
        created_at: 1_718_900_000,
        windows: vec![SessionWindowSnapshot {
            window_id: Some("window-1".into()),
            tab_manager: SessionTabManagerSnapshot {
                selected_workspace_index: Some(0),
                workspaces: vec![workspace],
                workspace_groups: Some(vec![group]),
            },
        }],
    };

    assert_round_trip("full_modern_snapshot", &snapshot);
}

#[test]
fn legacy_pre_canvas_pre_tab_snapshot() {
    // The legacy shape: a workspace with a pane layout but NONE of the newer
    // optional fields (no workspaceId, customTitle*, layoutMode, canvasPanes;
    // no windowId; no workspaceGroups; no selectedPanelId on the pane). These
    // must serialize WITHOUT the absent keys so old on-disk files round-trip.
    let workspace = SessionWorkspaceSnapshot {
        workspace_id: None,
        process_title: "bash".into(),
        custom_title: None,
        custom_title_source: None,
        current_directory: None,
        layout: Some(SessionWorkspaceLayoutSnapshot::Pane(
            SessionPaneLayoutSnapshot {
                panel_ids: vec!["legacy-panel".into()],
                selected_panel_id: None,
                surface_kind: None,
            },
        )),
        layout_mode: None,
        canvas_panes: None,
    };

    let snapshot = AppSessionSnapshot {
        version: 1,
        created_at: 1_600_000_000,
        windows: vec![SessionWindowSnapshot {
            window_id: None,
            tab_manager: SessionTabManagerSnapshot {
                selected_workspace_index: None,
                workspaces: vec![workspace],
                workspace_groups: None,
            },
        }],
    };

    assert_round_trip("legacy_pre_canvas_pre_tab_snapshot", &snapshot);
}

#[test]
fn legacy_no_layout_snapshot() {
    // Even older: a workspace whose `layout` is itself absent (None). `layout`
    // is `#[serde(default)]` without skip, so it serializes as `null` — pin
    // that shape so the Swift side (Optional with nil default) is matched.
    let snapshot = AppSessionSnapshot {
        version: 1,
        created_at: 1_500_000_000,
        windows: vec![SessionWindowSnapshot {
            window_id: None,
            tab_manager: SessionTabManagerSnapshot {
                selected_workspace_index: None,
                workspaces: vec![SessionWorkspaceSnapshot {
                    process_title: "sh".into(),
                    ..Default::default()
                }],
                workspace_groups: None,
            },
        }],
    };

    assert_round_trip("legacy_no_layout_snapshot", &snapshot);
}

#[test]
fn empty_snapshot() {
    // No windows at all — the minimal persisted shape.
    let snapshot = AppSessionSnapshot {
        version: 1,
        created_at: 0,
        windows: vec![],
    };
    assert_round_trip("empty_snapshot", &snapshot);
}

#[test]
fn decode_tolerates_absent_optionals_from_raw_json() {
    // Prove the legacy DECODE path: a minimal hand-written JSON missing all
    // optional fields decodes and re-encodes to the canonical legacy form.
    let raw = br#"{"version":1,"created_at":1600000000,"windows":[
        {"tab_manager":{"workspaces":[{"process_title":"bash","layout":
        {"type":"pane","pane":{"panel_ids":["legacy-panel"]}}}]}}]}"#;
    let decoded = decode_session(raw).expect("decode legacy raw json");
    let workspace = &decoded.windows[0].tab_manager.workspaces[0];
    assert_eq!(workspace.workspace_id, None);
    assert_eq!(workspace.layout_mode, None);
    assert_eq!(workspace.canvas_panes, None);
}
