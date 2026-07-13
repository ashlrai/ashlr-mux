//! Red suite for the differential-remediation slice.
//!
//! Contracts of record: docs/parity/differential/REMEDIATION.md and the
//! archived live canonical capture
//! (parity/diff-lane:docs/parity/differential/captures/
//! pane_surface_lifecycle.canonical.e1825d40d.ndjson). Per the root ruling,
//! the CAPTURE is ground truth over any reading of the pinned Swift.

use super::pane_surface_lifecycle::{
    dispatch_lifecycle_request, LifecycleDispatchContext, LifecycleTransition,
};
use super::*;

fn remediation_context() -> LifecycleDispatchContext {
    LifecycleDispatchContext {
        viewport_size: Some((1_000.0, 800.0)),
        browser_enabled: true,
        dock_available: true,
        active_window_id: None,
    }
}

fn transition(snapshot: &AppSessionSnapshot, method: &str, params: Value) -> LifecycleTransition {
    let Value::Object(params) = params else {
        unreachable!("params fixture must be an object");
    };
    dispatch_lifecycle_request(snapshot, method, &params, &remediation_context())
}

fn ok_value(transition: &LifecycleTransition) -> Value {
    match &transition.result {
        ControlCallResult::Ok(payload) => Value::from(payload.clone()),
        ControlCallResult::Err { code, message, .. } => {
            panic!("expected success, got {code}: {message}")
        }
    }
}

fn expect_error(transition: &LifecycleTransition) -> (String, String) {
    match &transition.result {
        ControlCallResult::Err { code, message, .. } => (code.clone(), message.clone()),
        ControlCallResult::Ok(payload) => panic!("expected error, got {payload:?}"),
    }
}

/// Horizontal split at 0.5 (surface-left | surface-right).
fn resizable_snapshot() -> AppSessionSnapshot {
    let mut snapshot = test_snapshot();
    let workspace = &mut snapshot.windows[0].tab_manager.workspaces[0];
    workspace.focused_panel_id = Some("surface-left".into());
    let SessionWorkspaceLayoutSnapshot::Pane(base) = workspace.layout.take().expect("base pane")
    else {
        unreachable!();
    };
    let mut left = base.clone();
    left.pane_id = Some("pane-left".into());
    left.panel_ids = vec!["surface-left".into()];
    left.selected_panel_id = Some("surface-left".into());
    let mut right = base;
    right.pane_id = Some("pane-right".into());
    right.panel_ids = vec!["surface-right".into()];
    right.selected_panel_id = Some("surface-right".into());
    workspace.layout = Some(SessionWorkspaceLayoutSnapshot::Split(
        cmux_core::session::SessionSplitLayoutSnapshot {
            split_id: Some("split-root".into()),
            orientation: SessionSplitOrientation::Horizontal,
            divider_position: 0.5,
            first: Box::new(SessionWorkspaceLayoutSnapshot::Pane(left)),
            second: Box::new(SessionWorkspaceLayoutSnapshot::Pane(right)),
        },
    ));
    snapshot
}

// ---------------------------------------------------------------------------
// D4 — pane.resize canonical step math + direction/amount echo
// ---------------------------------------------------------------------------
//
// Canonical relative resize divides amount by the split's RENDERED axis
// pixels (v2PaneResizeCollectCandidates, TerminalControllerPaneResizeSupport
// .swift:84-92: axisPixels = max(frameUnion, 1); ControlPaneContext.swift:
// 530-532: delta = amount/axisPixels, clamp 0.1...0.9). This port tracks no
// rendered frames — exactly the state the live canonical capture ran in
// (frames zero => axisPixels 1), so delta = amount and every relative resize
// clamps: capture pins 0.5 -> 0.1 (amount 2, left) and 0.9 -> 0.1 (amount 1,
// up). The viewport must NOT be substituted for frame pixels.

#[test]
fn pane_resize_relative_uses_canonical_frame_pixel_math() {
    // Capture oracle: pane_resize.relative_happy — amount 2, direction left,
    // 0.5 -> 0.1, with direction+amount echoed in the response.
    let snapshot = resizable_snapshot();
    let resized = transition(
        &snapshot,
        "pane.resize",
        json!({"pane_id": "pane-right", "direction": "left", "amount": 2}),
    );
    let value = ok_value(&resized);
    assert_eq!(value["old_divider_position"], json!(0.5));
    assert_eq!(value["new_divider_position"], json!(0.1));
    assert_eq!(
        value["direction"],
        json!("left"),
        "canonical echoes direction"
    );
    assert_eq!(value["amount"], json!(2), "canonical echoes amount");
    assert_eq!(value["split_id"], json!("split-root"));
}

#[test]
fn pane_resize_high_divider_clamps_to_floor_like_the_capture() {
    // Capture oracle: pane_create.divider_clamped_high state probe — amount 1
    // from 0.9 lands on the 0.1 clamp floor (delta = amount with axisPixels 1).
    let mut snapshot = resizable_snapshot();
    let SessionWorkspaceLayoutSnapshot::Split(split) = snapshot.windows[0].tab_manager.workspaces
        [0]
    .layout
    .as_mut()
    .expect("split layout") else {
        unreachable!();
    };
    split.divider_position = 0.9;
    let resized = transition(
        &snapshot,
        "pane.resize",
        json!({"pane_id": "pane-right", "direction": "left", "amount": 1}),
    );
    let value = ok_value(&resized);
    assert_eq!(value["old_divider_position"], json!(0.9));
    assert_eq!(value["new_divider_position"], json!(0.1));
    assert_eq!(value["amount"], json!(1));
}

#[test]
fn pane_resize_absolute_echoes_axis_and_target() {
    // Absolute path: targetFraction = target_pixels / axisPixels(=1) clamps to
    // 0.9 for the first child; echoes absolute_axis + target_pixels verbatim
    // (ControlPaneContext.swift:497-508).
    let snapshot = resizable_snapshot();
    let resized = transition(
        &snapshot,
        "pane.resize",
        json!({"pane_id": "pane-left", "absolute_axis": "horizontal", "target_pixels": 600}),
    );
    let value = ok_value(&resized);
    assert_eq!(value["new_divider_position"], json!(0.9));
    assert_eq!(value["absolute_axis"], json!("horizontal"));
    assert_eq!(
        value["target_pixels"],
        json!(600),
        "echo the request value verbatim (no float rewrite)"
    );
    assert!(value.get("direction").is_none());
    assert!(value.get("amount").is_none());
}

#[test]
fn pane_resize_relative_response_has_the_exact_canonical_key_set() {
    // Capture: {amount, direction, new_divider_position, old_divider_position,
    // pane_id, pane_ref, split_id, window_id, window_ref, workspace_id,
    // workspace_ref} — pane/window/workspace refs come from decoration;
    // split_id has NO ref twin.
    let snapshot = resizable_snapshot();
    let mut resized = transition(
        &snapshot,
        "pane.resize",
        json!({"pane_id": "pane-right", "direction": "left", "amount": 2}),
    );
    decorate_lifecycle_result_refs_with("pane.resize", &mut resized.result, &mut |kind, id| {
        format!("{kind}:ref:{id}")
    });
    let value = ok_value(&resized);
    let mut keys: Vec<_> = value.as_object().unwrap().keys().cloned().collect();
    keys.sort();
    assert_eq!(
        keys,
        [
            "amount",
            "direction",
            "new_divider_position",
            "old_divider_position",
            "pane_id",
            "pane_ref",
            "split_id",
            "window_id",
            "window_ref",
            "workspace_id",
            "workspace_ref",
        ]
    );
}

// ---------------------------------------------------------------------------
// D5 — surface.move dual-anchor rejection
// ---------------------------------------------------------------------------

#[test]
fn surface_move_rejects_both_anchor_params() {
    // Capture oracle: surface_move.both_anchors_rejected — invalid_params
    // BEFORE any move occurs; canonical validates the anchor count right
    // after surface_id and before surface lookup (TerminalController.swift:
    // 4726-4736).
    let mut snapshot = test_snapshot();
    {
        let workspace = &mut snapshot.windows[0].tab_manager.workspaces[0];
        let SessionWorkspaceLayoutSnapshot::Pane(pane) =
            workspace.layout.as_mut().expect("pane layout")
        else {
            unreachable!();
        };
        pane.panel_ids = vec!["surface-1".into(), "surface-9".into()];
    }
    let moved = transition(
        &snapshot,
        "surface.move",
        json!({
            "surface_id": "surface-1",
            "before_surface_id": "surface-9",
            "after_surface_id": "surface-9",
        }),
    );
    let (code, message) = expect_error(&moved);
    assert_eq!(code, "invalid_params");
    assert_eq!(
        message,
        "Specify at most one of before_surface_id or after_surface_id"
    );
    assert!(!moved.changed, "the move must not be performed");
    // The anchor-count check precedes surface existence (canonical order).
    let missing = transition(
        &snapshot,
        "surface.move",
        json!({
            "surface_id": "surface-404",
            "before_surface_id": "surface-9",
            "after_surface_id": "surface-9",
        }),
    );
    let (code, message) = expect_error(&missing);
    assert_eq!(code, "invalid_params");
    assert_eq!(
        message,
        "Specify at most one of before_surface_id or after_surface_id"
    );
}

// ---------------------------------------------------------------------------
// D6 — created terminals inherit requested_working_directory from the creator
// ---------------------------------------------------------------------------
//
// Canonical resolvedTerminalStartupWorkingDirectory (Workspace.swift:6805-6824
// at pinned e1825d40d): explicit request wins; otherwise, when no startup
// command is present, inherit the creator's reported pwd, then the creator's
// own requested working directory, then the workspace currentDirectory (first
// trimmed non-empty). Capture: every fixture surface row carries a non-null
// requested_working_directory (path TEXT is approved-differencable; null is
// not).

fn reported_directory_snapshot() -> AppSessionSnapshot {
    let snapshot = test_snapshot();
    let mut encoded = serde_json::to_value(snapshot).expect("encode");
    encoded["windows"][0]["tab_manager"]["workspaces"][0]["surfaces"] = json!([
        {"surface_id": "surface-1", "pane_id": "pane-1", "generation": 1,
         "kind": {"type": "terminal"},
         "metadata": {"reported_directory": "C:/reported"}}
    ]);
    serde_json::from_value(encoded).expect("decode")
}

fn created_surface_working_directory(transition: &LifecycleTransition) -> Option<String> {
    let created = ok_value(transition)["surface_id"]
        .as_str()
        .expect("created surface id")
        .to_owned();
    transition
        .snapshot
        .windows
        .iter()
        .flat_map(|window| &window.tab_manager.workspaces)
        .flat_map(|workspace| workspace.surfaces.as_deref().unwrap_or_default())
        .find(|record| record.surface_id == created)
        .and_then(|record| record.terminal_startup.as_ref())
        .and_then(|startup| startup.working_directory.clone())
}

#[test]
fn surface_create_inherits_creator_reported_directory_first() {
    let snapshot = reported_directory_snapshot();
    let created = transition(&snapshot, "surface.create", json!({"type": "terminal"}));
    assert_eq!(
        created_surface_working_directory(&created).as_deref(),
        Some("C:/reported"),
        "creator reported pwd wins over the workspace directory"
    );
}

#[test]
fn surface_create_falls_back_to_workspace_current_directory() {
    // test_snapshot workspace current_directory = C:/repo, no reported pwd.
    let snapshot = test_snapshot();
    let created = transition(&snapshot, "surface.create", json!({"type": "terminal"}));
    assert_eq!(
        created_surface_working_directory(&created).as_deref(),
        Some("C:/repo")
    );
    // The spawn effect carries the inherited directory too.
    let effect_dir = created.effects.iter().find_map(|effect| match effect {
        super::pane_surface_lifecycle::LifecycleEffect::TerminalCreate {
            working_directory,
            ..
        } => Some(working_directory.clone()),
        _ => None,
    });
    assert_eq!(effect_dir, Some(Some("C:/repo".into())));
}

#[test]
fn surface_create_explicit_directory_and_startup_command_gate_inheritance() {
    let snapshot = test_snapshot();
    let explicit = transition(
        &snapshot,
        "surface.create",
        json!({"type": "terminal", "working_directory": "C:/explicit"}),
    );
    assert_eq!(
        created_surface_working_directory(&explicit).as_deref(),
        Some("C:/explicit")
    );
    // Canonical: inheritance only when startupCommand == nil
    // (Workspace.swift:7515-7521).
    let with_command = transition(
        &snapshot,
        "surface.create",
        json!({"type": "terminal", "initial_command": "cargo test"}),
    );
    assert_eq!(created_surface_working_directory(&with_command), None);
}

#[test]
fn pane_create_inherits_the_creator_directory() {
    let snapshot = test_snapshot();
    let created = transition(&snapshot, "pane.create", json!({"direction": "right"}));
    assert_eq!(
        created_surface_working_directory(&created).as_deref(),
        Some("C:/repo")
    );
}

// ---------------------------------------------------------------------------
// D7 — non-focus surface.create preserves the pane's selection
// ---------------------------------------------------------------------------

#[test]
fn surface_create_without_focus_preserves_pane_selection() {
    // Capture surface_list.rows_shape: the pre-existing surface keeps
    // selected_in_pane=true (and focus) after a non-focus surface.create —
    // canonical bonsplit transiently selects the new tab then RESTORES the
    // previous selection (capture surface_create.terminal_happy event flip).
    let snapshot = test_snapshot();
    let created = transition(&snapshot, "surface.create", json!({"type": "terminal"}));
    let created_id = ok_value(&created)["surface_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let list = transition(&created.snapshot, "surface.list", json!({}));
    let rows = ok_value(&list)["surfaces"].as_array().unwrap().clone();
    let selected = |id: &str| {
        rows.iter()
            .find(|row| row["id"] == json!(id))
            .map(|row| row["selected_in_pane"] == json!(true))
            .unwrap()
    };
    assert!(
        selected("surface-1"),
        "prior surface stays selected in its pane"
    );
    assert!(
        !selected(&created_id),
        "new non-focused tab is not selected"
    );
}

#[test]
fn surface_create_with_focus_selects_the_new_surface() {
    let snapshot = test_snapshot();
    let created = transition(
        &snapshot,
        "surface.create",
        json!({"type": "terminal", "focus": true}),
    );
    let created_id = ok_value(&created)["surface_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let list = transition(&created.snapshot, "surface.list", json!({}));
    let rows = ok_value(&list)["surfaces"].as_array().unwrap().clone();
    let row = rows
        .iter()
        .find(|row| row["id"] == json!(created_id))
        .unwrap();
    assert_eq!(row["selected_in_pane"], json!(true));
    assert_eq!(row["focused"], json!(true));
}

// ---------------------------------------------------------------------------
// D8a — events.stream default subscribes at latest (no replay)
// ---------------------------------------------------------------------------

#[test]
fn events_stream_default_subscribes_at_latest_without_replay() {
    // Capture: canonical ack has resume.after_seq null with
    // requested_after_seq == latest_seq and replay_count 0 when the caller
    // omits after_seq; Windows replayed the whole session from seq 0.
    let retained = vec![
        json!({"seq": 1, "name": "surface.created", "category": "surface"}),
        json!({"seq": 2, "name": "surface.closed", "category": "surface"}),
    ];
    let (ack, events, _) = events_parts_from_retained(
        "boot".into(),
        3,
        retained.clone(),
        None,
        100,
        false,
        Vec::new(),
        Vec::new(),
    );
    assert_eq!(
        ack["resume"]["after_seq"],
        json!(null),
        "echo the raw param"
    );
    assert_eq!(ack["resume"]["requested_after_seq"], json!(2));
    assert_eq!(ack["resume"]["latest_seq"], json!(2));
    assert_eq!(ack["resume"]["gap"], json!(false));
    assert_eq!(ack["replay_count"], json!(0));
    assert!(events.is_empty(), "no default replay");

    // An explicit after_seq still replays from that point.
    let (ack, events, _) = events_parts_from_retained(
        "boot".into(),
        3,
        retained,
        Some(0),
        100,
        false,
        Vec::new(),
        Vec::new(),
    );
    assert_eq!(ack["resume"]["after_seq"], json!(0));
    assert_eq!(ack["resume"]["requested_after_seq"], json!(0));
    assert_eq!(ack["replay_count"], json!(2));
    assert_eq!(events.len(), 2);
}

// ---------------------------------------------------------------------------
// D8b — canonical event emissions (capture frame lists are the oracle)
// ---------------------------------------------------------------------------

fn event_summary(event: &super::pane_surface_lifecycle::LifecycleEvent) -> (String, Value) {
    (event.name.to_string(), event.payload.clone())
}

#[test]
fn surface_create_without_focus_emits_the_canonical_selection_flip() {
    // Capture surface_create.terminal_happy: created -> selected(new) ->
    // focused(new) -> selected(previous) -> focused(previous), origin
    // bonsplit_selection, all workspace.lifecycle with a NULL envelope
    // window_id.
    let snapshot = test_snapshot();
    let created = transition(&snapshot, "surface.create", json!({"type": "terminal"}));
    let new_id = ok_value(&created)["surface_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let events: Vec<_> = created.events.iter().map(event_summary).collect();
    assert_eq!(events.len(), 5, "created + the transient selection flip");
    assert_eq!(events[0].0, "surface.created");
    assert_eq!(events[0].1["focused"], json!(false));
    assert_eq!(events[1].0, "surface.selected");
    assert_eq!(
        events[1].1,
        json!({
            "focused": true,
            "kind": "terminal",
            "origin": "bonsplit_selection",
            "pane_id": "pane-1",
            "previous_surface_id": "surface-1",
            "surface_id": new_id,
        })
    );
    assert_eq!(events[2].0, "surface.focused");
    assert_eq!(
        events[2].1,
        json!({
            "kind": "terminal",
            "origin": "bonsplit_selection",
            "pane_id": "pane-1",
            "surface_id": new_id,
        })
    );
    assert_eq!(events[3].0, "surface.selected");
    assert_eq!(events[3].1["surface_id"], json!("surface-1"));
    assert_eq!(events[3].1["previous_surface_id"], json!(new_id));
    assert_eq!(events[4].0, "surface.focused");
    assert_eq!(events[4].1["surface_id"], json!("surface-1"));
    for event in &created.events {
        assert_eq!(event.source, "workspace.lifecycle");
        assert_eq!(
            event.window_id, None,
            "canonical workspace.lifecycle envelopes carry window_id null"
        );
    }
}

#[test]
fn surface_create_with_focus_emits_selection_once() {
    let snapshot = test_snapshot();
    let created = transition(
        &snapshot,
        "surface.create",
        json!({"type": "terminal", "focus": true}),
    );
    let names: Vec<_> = created.events.iter().map(|event| event.name).collect();
    assert_eq!(
        names,
        ["surface.created", "surface.selected", "surface.focused"],
        "focused create keeps the new selection (no restore flip)"
    );
}

#[test]
fn surface_close_emits_canonical_payload_and_reselection() {
    // Capture surface_close.happy: surface.closed carries
    // {kind, origin: tab_close, pane_id, surface_id}, then the pane's new
    // selection emits selected(previous_surface_id = prior selection) +
    // focused.
    let snapshot = mixed_pane_snapshot();
    let closed = transition(
        &snapshot,
        "surface.close",
        json!({"surface_id": "surface-b"}),
    );
    let _ = ok_value(&closed);
    let events: Vec<_> = closed.events.iter().map(event_summary).collect();
    assert_eq!(events.len(), 3);
    assert_eq!(events[0].0, "surface.closed");
    assert_eq!(
        events[0].1,
        json!({
            "kind": "terminal",
            "origin": "tab_close",
            "pane_id": "pane-1",
            "surface_id": "surface-b",
        })
    );
    assert_eq!(events[1].0, "surface.selected");
    assert_eq!(events[1].1["surface_id"], json!("surface-a"));
    assert_eq!(events[1].1["previous_surface_id"], json!("surface-b"));
    assert_eq!(events[1].1["origin"], json!("bonsplit_selection"));
    assert_eq!(events[2].0, "surface.focused");
    assert_eq!(events[2].1["surface_id"], json!("surface-a"));
}

/// One pane, two terminals, surface-b selected+focused.
fn mixed_pane_snapshot() -> AppSessionSnapshot {
    let mut snapshot = test_snapshot();
    let workspace = &mut snapshot.windows[0].tab_manager.workspaces[0];
    workspace.focused_panel_id = Some("surface-b".into());
    let SessionWorkspaceLayoutSnapshot::Pane(pane) =
        workspace.layout.as_mut().expect("pane layout")
    else {
        unreachable!();
    };
    pane.panel_ids = vec!["surface-a".into(), "surface-b".into()];
    pane.selected_panel_id = Some("surface-b".into());
    let mut encoded = serde_json::to_value(snapshot).expect("encode mixed pane");
    encoded["windows"][0]["tab_manager"]["workspaces"][0]["surfaces"] = json!([
        {"surface_id": "surface-a", "pane_id": "pane-1", "generation": 1,
         "kind": {"type": "terminal"}},
        {"surface_id": "surface-b", "pane_id": "pane-1", "generation": 1,
         "kind": {"type": "terminal"}}
    ]);
    serde_json::from_value(encoded).expect("decode mixed pane")
}

#[test]
fn lifecycle_command_commits_do_not_emit_derived_session_model_events() {
    // Capture (all four events cases): the case action emits ONLY the
    // workspace.lifecycle / socket.v2 frames — no session.changed and no
    // derived pane.focused (source session.model) extras.
    let source = include_str!("../../control_socket.rs");
    let start = source
        .find("fn handle_pane_surface_lifecycle_request(")
        .expect("handler present");
    let body = &source[start..start + 6_000];
    assert!(
        !body.contains("record_session_changed_event_suppressing"),
        "the v2 lifecycle commit path must not emit derived session.model events"
    );
}

// ---------------------------------------------------------------------------
// D3 — handle registry seeded with the bootstrap entities
// ---------------------------------------------------------------------------

#[test]
fn bootstrap_registry_seeds_walk_window_workspace_pane_surface() {
    // Capture: the fixture workspace minted workspace:2/pane:2 and the first
    // split pane:3/surface:4 — the bootstrap window/workspace/pane/surface
    // must already occupy :1 of each kind before the first socket mint.
    let seeds = bootstrap_registry_seeds(&test_snapshot());
    assert_eq!(
        seeds,
        vec![
            ("window", "window-1".to_string()),
            ("workspace", "workspace-1".to_string()),
            ("pane", "pane-1".to_string()),
            ("surface", "surface-1".to_string()),
        ]
    );
    // Feeding the seeds into a registry makes the next mints start at :2.
    let mut registry = ControlHandleRegistry::default();
    for (kind, id) in &seeds {
        registry.mint(kind, id);
    }
    assert_eq!(registry.mint("workspace", "fixture-ws"), "workspace:2");
    assert_eq!(registry.mint("pane", "fixture-pane"), "pane:2");
    assert_eq!(registry.mint("surface", "fixture-surface"), "surface:2");
    assert_eq!(
        registry.mint("surface", "surface-1"),
        "surface:1",
        "re-minting a seeded id is stable"
    );
}
