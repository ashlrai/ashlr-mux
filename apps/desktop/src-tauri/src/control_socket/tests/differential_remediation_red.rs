//! Red suite for the differential-remediation slice.
//!
//! Contracts of record: docs/parity/differential/REMEDIATION.md and the
//! archived live canonical capture
//! (parity/diff-lane:docs/parity/differential/captures/
//! pane_surface_lifecycle.canonical.e1825d40d.ndjson). Per the root ruling,
//! the CAPTURE is ground truth over any reading of the pinned Swift.

use super::pane_surface_lifecycle::{
    commit_lifecycle_transition, dispatch_lifecycle_request, LifecycleDispatchContext,
    LifecycleEffect, LifecycleEffectExecutor, LifecycleTransition,
};
use super::*;
use crate::dock::{DockCreateRequest, DockStore, DockSurfaceKind};

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
    const ANCHOR: &str = "9b1deb4d-3b7d-4bad-9bdd-2b0d7b3dcb6d";
    let moved = transition(
        &snapshot,
        "surface.move",
        json!({
            "surface_id": "surface-1",
            "before_surface_id": ANCHOR,
            "after_surface_id": ANCHOR,
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
            "before_surface_id": ANCHOR,
            "after_surface_id": ANCHOR,
        }),
    );
    let (code, message) = expect_error(&missing);
    assert_eq!(code, "invalid_params");
    assert_eq!(
        message,
        "Specify at most one of before_surface_id or after_surface_id"
    );
    // V3: canonical counts anchors through v2UUID — garbage text does not
    // participate, so garbage + valid is a single-anchor move.
    let single = transition(
        &snapshot,
        "surface.move",
        json!({
            "surface_id": "surface-1",
            "before_surface_id": "garbage",
            "after_surface_id": ANCHOR,
        }),
    );
    match &single.result {
        ControlCallResult::Err { message, .. } => assert_ne!(
            message, "Specify at most one of before_surface_id or after_surface_id",
            "garbage anchor must not count toward the dual-anchor rejection"
        ),
        ControlCallResult::Ok(_) => {}
    }
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
    // Round 7: a born-selected tab never goes through a selection TRANSITION,
    // so nothing is published (publishCmuxFocusedSelection is only reached
    // from applyTabSelectionNow; the close.happy capture's surviving
    // previous_surface_id proves the pointer never advanced).
    assert_eq!(names, ["surface.created"]);
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
    // Round 7: nothing was published before the close in this fixture, so
    // the pointer-backed previous is null (never the dead surface).
    assert_eq!(events[1].1["previous_surface_id"], json!(null));
    assert_eq!(events[1].1["origin"], json!("bonsplit_selection"));
    assert_eq!(events[2].0, "surface.focused");
    assert_eq!(events[2].1["surface_id"], json!("surface-a"));
}

#[test]
fn closing_an_unselected_surface_republishes_a_stale_pointer_selection() {
    // Canonical's close-time guard compares the publisher pointer with the
    // surviving selection, not the pre-close live selection. A born-selected
    // surface can already be the live selection while the publisher pointer
    // still trails it; closing a different tab must publish the surviving
    // selection pair and advance that pointer.
    let snapshot = mixed_pane_snapshot();
    let closed = transition(
        &snapshot,
        "surface.close",
        json!({"surface_id": "surface-a"}),
    );
    let _ = ok_value(&closed);
    let events: Vec<_> = closed.events.iter().map(event_summary).collect();
    assert_eq!(events.len(), 3);
    assert_eq!(events[0].0, "surface.closed");
    assert_eq!(events[0].1["surface_id"], json!("surface-a"));
    assert_eq!(events[1].0, "surface.selected");
    assert_eq!(events[1].1["surface_id"], json!("surface-b"));
    assert_eq!(events[1].1["previous_surface_id"], json!(null));
    assert_eq!(events[2].0, "surface.focused");
    assert_eq!(events[2].1["surface_id"], json!("surface-b"));
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

fn set_published_pointer(
    snapshot: &mut AppSessionSnapshot,
    window_index: usize,
    workspace_index: usize,
    pane_id: &str,
    panel_id: &str,
) {
    snapshot.windows[window_index].tab_manager.workspaces[workspace_index]
        .published_pane_selections = Some(vec![
        cmux_core::session::SessionPanePublishedSelectionSnapshot {
            pane_id: pane_id.into(),
            panel_id: panel_id.into(),
        },
    ]);
}

fn published_pointer<'a>(
    snapshot: &'a AppSessionSnapshot,
    window_index: usize,
    workspace_index: usize,
    pane_id: &str,
) -> Option<&'a str> {
    snapshot.windows[window_index].tab_manager.workspaces[workspace_index]
        .published_pane_selections
        .as_deref()
        .unwrap_or_default()
        .iter()
        .find(|row| row.pane_id == pane_id)
        .map(|row| row.panel_id.as_str())
}

fn split_close_snapshot() -> AppSessionSnapshot {
    let snapshot = resizable_snapshot();
    let mut encoded = serde_json::to_value(snapshot).expect("encode split close fixture");
    encoded["windows"][0]["tab_manager"]["workspaces"][0]["surfaces"] = json!([
        {"surface_id": "surface-left", "pane_id": "pane-left", "generation": 1,
         "kind": {"type": "terminal"}},
        {"surface_id": "surface-right", "pane_id": "pane-right", "generation": 1,
         "kind": {"type": "terminal"}}
    ]);
    serde_json::from_value(encoded).expect("decode split close fixture")
}

fn set_published_rows(
    snapshot: &mut AppSessionSnapshot,
    window_index: usize,
    workspace_index: usize,
    rows: &[(&str, &str)],
) {
    snapshot.windows[window_index].tab_manager.workspaces[workspace_index]
        .published_pane_selections = Some(
        rows.iter()
            .map(
                |(pane_id, panel_id)| cmux_core::session::SessionPanePublishedSelectionSnapshot {
                    pane_id: (*pane_id).into(),
                    panel_id: (*panel_id).into(),
                },
            )
            .collect(),
    );
}

fn published_rows(
    snapshot: &AppSessionSnapshot,
    window_index: usize,
    workspace_index: usize,
) -> Vec<(String, String)> {
    snapshot.windows[window_index].tab_manager.workspaces[workspace_index]
        .published_pane_selections
        .as_deref()
        .unwrap_or_default()
        .iter()
        .map(|row| (row.pane_id.clone(), row.panel_id.clone()))
        .collect()
}

fn published_rows_bytes(
    snapshot: &AppSessionSnapshot,
    window_index: usize,
    workspace_index: usize,
) -> Vec<u8> {
    serde_json::to_vec(
        &snapshot.windows[window_index].tab_manager.workspaces[workspace_index]
            .published_pane_selections,
    )
    .expect("encode exact published rows")
}

fn lifecycle_event_names(transition: &LifecycleTransition) -> Vec<&'static str> {
    transition.events.iter().map(|event| event.name).collect()
}

fn publication_matrix_snapshot(focused_surface_id: &str) -> AppSessionSnapshot {
    let mut snapshot = resizable_snapshot();
    let workspace = &mut snapshot.windows[0].tab_manager.workspaces[0];
    workspace.focused_panel_id = Some(focused_surface_id.into());
    let SessionWorkspaceLayoutSnapshot::Split(split) = workspace.layout.as_mut().unwrap() else {
        unreachable!()
    };
    let SessionWorkspaceLayoutSnapshot::Pane(matrix) = split.first.as_mut() else {
        unreachable!()
    };
    matrix.pane_id = Some("pane-matrix".into());
    matrix.panel_ids = vec![
        "surface-before".into(),
        "surface-selected".into(),
        "surface-after".into(),
    ];
    matrix.selected_panel_id = Some("surface-selected".into());
    let SessionWorkspaceLayoutSnapshot::Pane(other) = split.second.as_mut() else {
        unreachable!()
    };
    other.pane_id = Some("pane-other".into());
    other.panel_ids = vec!["surface-other".into()];
    other.selected_panel_id = Some("surface-other".into());

    let mut encoded = serde_json::to_value(snapshot).expect("encode publication matrix");
    encoded["windows"][0]["tab_manager"]["workspaces"][0]["surfaces"] = json!([
        {"surface_id":"surface-before","pane_id":"pane-matrix","generation":1,"kind":{"type":"terminal"}},
        {"surface_id":"surface-selected","pane_id":"pane-matrix","generation":1,"kind":{"type":"terminal"}},
        {"surface_id":"surface-after","pane_id":"pane-matrix","generation":1,"kind":{"type":"terminal"}},
        {"surface_id":"surface-other","pane_id":"pane-other","generation":1,"kind":{"type":"terminal"}}
    ]);
    serde_json::from_value(encoded).expect("decode publication matrix")
}

fn dock_publication_snapshot() -> (AppSessionSnapshot, String, String, String, String) {
    let mut snapshot = test_snapshot();
    snapshot.windows[0].window_id = Some("main".into());
    let first = DockStore
        .create(
            &mut snapshot,
            "main",
            DockCreateRequest {
                kind: DockSurfaceKind::Terminal,
                focus: true,
                ..DockCreateRequest::default()
            },
        )
        .expect("seed first Dock surface");
    let second = DockStore
        .create(
            &mut snapshot,
            "main",
            DockCreateRequest {
                kind: DockSurfaceKind::Terminal,
                pane_id: Some(first.pane_id),
                focus: false,
                ..DockCreateRequest::default()
            },
        )
        .expect("seed second Dock surface");
    let third = DockStore
        .create(
            &mut snapshot,
            "main",
            DockCreateRequest {
                kind: DockSurfaceKind::Terminal,
                pane_id: Some(first.pane_id),
                focus: false,
                ..DockCreateRequest::default()
            },
        )
        .expect("seed third Dock surface");
    snapshot = transition(
        &snapshot,
        "surface.focus",
        json!({"surface_id": second.surface_id}),
    )
    .snapshot;
    (
        snapshot,
        first.pane_id.to_string(),
        first.surface_id.to_string(),
        second.surface_id.to_string(),
        third.surface_id.to_string(),
    )
}

#[derive(Default)]
struct CloseCommitExecutor {
    fail_commit: bool,
    rollback_count: usize,
}

impl LifecycleEffectExecutor for CloseCommitExecutor {
    type Error = String;

    fn stage(&mut self, _effect: &LifecycleEffect) -> Result<(), Self::Error> {
        Ok(())
    }

    fn commit_staged(&mut self) -> Result<(), Self::Error> {
        if self.fail_commit {
            Err("injected close publication failure".into())
        } else {
            Ok(())
        }
    }

    fn rollback_staged(&mut self) -> Result<(), Self::Error> {
        self.rollback_count += 1;
        Ok(())
    }

    fn rollback_committed(&mut self) -> Result<(), Self::Error> {
        self.rollback_count += 1;
        Ok(())
    }
}

fn commit_close(
    mut snapshot: AppSessionSnapshot,
    transition: LifecycleTransition,
) -> AppSessionSnapshot {
    commit_lifecycle_transition(
        &mut snapshot,
        transition,
        &mut CloseCommitExecutor::default(),
    )
    .expect("close transition commits");
    snapshot
}

#[test]
fn close_publication_position_focus_pointer_matrix_preserves_canonical_state() {
    // Frozen capture evidence proves the unfocused predecessor callback.
    // Bonsplit callback occurrence for the other positional/focus combinations
    // still needs a canonical capture, so those
    // rows deliberately accept either canonical event shape while enforcing
    // the invariants common to both: closed first, dead-pointer pruning,
    // pointer-backed previous ids, and no rewrite when publication suppresses.
    let positions = [
        (
            "before",
            "surface-before",
            "surface-selected",
            "surface-after",
        ),
        (
            "selected",
            "surface-selected",
            "surface-after",
            "surface-before",
        ),
        (
            "after",
            "surface-after",
            "surface-selected",
            "surface-before",
        ),
    ];
    let pointer_states = ["empty", "dead", "stale-surviving", "already-selected"];
    let mut violations = Vec::new();

    for (position, closed_id, post_selected, stale_survivor) in positions {
        for closed_focused in [false, true] {
            for pointer_state in pointer_states {
                let focused_id = if closed_focused {
                    closed_id
                } else if position == "selected" {
                    "surface-before"
                } else {
                    "surface-selected"
                };
                let mut snapshot = publication_matrix_snapshot(focused_id);
                let initial_pointer = match pointer_state {
                    "empty" => None,
                    "dead" => Some(closed_id),
                    "stale-surviving" => Some(stale_survivor),
                    "already-selected" => Some(post_selected),
                    _ => unreachable!(),
                };
                let mut initial_rows = Vec::new();
                if let Some(pointer) = initial_pointer {
                    initial_rows.push(("pane-matrix", pointer));
                }
                initial_rows.push(("pane-other", "surface-other"));
                set_published_rows(&mut snapshot, 0, 0, &initial_rows);

                let closed =
                    transition(&snapshot, "surface.close", json!({"surface_id": closed_id}));
                let names = lifecycle_event_names(&closed);
                let pair_published =
                    names == ["surface.closed", "surface.selected", "surface.focused"];
                let closed_only = names == ["surface.closed"];
                let label = format!("{position}/{closed_focused}/{pointer_state}");
                if !pair_published && !closed_only {
                    violations.push(format!("{label}: invalid event order {names:?}"));
                    continue;
                }

                let surviving_pointer = initial_pointer.filter(|pointer| *pointer != closed_id);
                let callback_is_frozen = position == "before" && !closed_focused;
                if callback_is_frozen {
                    let expected_pair = surviving_pointer != Some(post_selected);
                    if pair_published != expected_pair {
                        violations.push(format!(
                            "{label}: frozen callback expected pair={expected_pair}, got {names:?}"
                        ));
                    }
                }
                if pair_published {
                    let expected_previous =
                        surviving_pointer.map(Value::from).unwrap_or(Value::Null);
                    if closed.events[1].payload["previous_surface_id"] != expected_previous {
                        violations.push(format!(
                            "{label}: previous was {:?}, expected {expected_previous}",
                            closed.events[1].payload["previous_surface_id"]
                        ));
                    }
                    if closed.events[1].payload["surface_id"] != json!(post_selected) {
                        violations.push(format!("{label}: selected wrong survivor"));
                    }
                }

                let mut expected_rows: Vec<(String, String)> = initial_rows
                    .iter()
                    .filter(|(_, panel_id)| *panel_id != closed_id)
                    .map(|(pane_id, panel_id)| ((*pane_id).into(), (*panel_id).into()))
                    .collect();
                if pair_published {
                    if let Some(row) = expected_rows
                        .iter_mut()
                        .find(|(pane_id, _)| pane_id == "pane-matrix")
                    {
                        row.1 = post_selected.into();
                    } else {
                        expected_rows.push(("pane-matrix".into(), post_selected.into()));
                    }
                }
                let actual_rows = published_rows(&closed.snapshot, 0, 0);
                if actual_rows != expected_rows {
                    violations.push(format!(
                        "{label}: pointer rows {actual_rows:?}, expected {expected_rows:?}"
                    ));
                }
            }
        }
    }

    assert!(
        violations.is_empty(),
        "close publication matrix violations:\n{}",
        violations.join("\n")
    );
}

#[test]
fn close_publication_suppressed_trailing_close_keeps_exact_pointer_row_order() {
    let mut snapshot = publication_matrix_snapshot("surface-selected");
    set_published_rows(
        &mut snapshot,
        0,
        0,
        &[
            ("pane-matrix", "surface-selected"),
            ("pane-other", "surface-other"),
        ],
    );
    let before = published_rows(&snapshot, 0, 0);
    let closed = transition(
        &snapshot,
        "surface.close",
        json!({"surface_id":"surface-after"}),
    );
    assert_eq!(lifecycle_event_names(&closed), ["surface.closed"]);
    assert_eq!(published_rows(&closed.snapshot, 0, 0), before);
}

#[test]
fn close_publication_predecessor_callback_suppression_does_not_rewrite_rows() {
    let mut snapshot = publication_matrix_snapshot("surface-selected");
    set_published_rows(
        &mut snapshot,
        0,
        0,
        &[
            ("pane-matrix", "surface-selected"),
            ("pane-other", "surface-other"),
        ],
    );
    let before = published_rows(&snapshot, 0, 0);
    let closed = transition(
        &snapshot,
        "surface.close",
        json!({"surface_id":"surface-before"}),
    );
    assert_eq!(lifecycle_event_names(&closed), ["surface.closed"]);
    assert_eq!(published_rows(&closed.snapshot, 0, 0), before);
}

#[test]
fn close_publication_dock_keeps_backing_workspace_and_public_event_owner_stable() {
    let mut snapshot = test_snapshot();
    snapshot.windows[0].window_id = Some("main".into());
    snapshot.windows[0].selected_workspace_id = Some("workspace-1".into());
    let mut second_workspace = snapshot.windows[0].tab_manager.workspaces[0].clone();
    second_workspace.workspace_id = Some("workspace-2".into());
    second_workspace.focused_panel_id = Some("surface-w2".into());
    let SessionWorkspaceLayoutSnapshot::Pane(pane) = second_workspace.layout.as_mut().unwrap()
    else {
        unreachable!()
    };
    pane.pane_id = Some("pane-w2".into());
    pane.panel_ids = vec!["surface-w2".into()];
    pane.selected_panel_id = Some("surface-w2".into());
    snapshot.windows[0]
        .tab_manager
        .workspaces
        .push(second_workspace);

    let first = DockStore
        .create(
            &mut snapshot,
            "main",
            DockCreateRequest {
                kind: DockSurfaceKind::Terminal,
                focus: true,
                ..DockCreateRequest::default()
            },
        )
        .expect("seed first Dock surface");
    let second = DockStore
        .create(
            &mut snapshot,
            "main",
            DockCreateRequest {
                kind: DockSurfaceKind::Terminal,
                pane_id: Some(first.pane_id),
                focus: false,
                ..DockCreateRequest::default()
            },
        )
        .expect("seed second Dock surface");
    snapshot = transition(
        &snapshot,
        "surface.focus",
        json!({"surface_id": second.surface_id}),
    )
    .snapshot;

    let pane_id = first.pane_id.to_string();
    let first_id = first.surface_id.to_string();
    let second_id = second.surface_id.to_string();
    set_published_rows(&mut snapshot, 0, 0, &[("pane-1", "surface-1")]);
    set_published_rows(
        &mut snapshot,
        0,
        1,
        &[(&pane_id, &first_id), ("pane-w2", "surface-w2")],
    );
    let first_workspace_before = published_rows(&snapshot, 0, 0);
    let closed = transition(&snapshot, "surface.close", json!({"surface_id": first_id}));
    assert_eq!(
        lifecycle_event_names(&closed),
        ["surface.closed", "surface.selected", "surface.focused"]
    );
    assert_eq!(closed.events[1].payload["previous_surface_id"], Value::Null);
    assert_eq!(closed.events[1].payload["surface_id"], json!(second_id));
    for event in &closed.events {
        assert_eq!(event.workspace_id.as_deref(), Some("main"));
        assert_eq!(event.window_id, None);
    }
    assert_eq!(
        published_rows(&closed.snapshot, 0, 0),
        first_workspace_before
    );
    assert_eq!(
        published_rows(&closed.snapshot, 0, 1),
        vec![
            (pane_id, second_id),
            ("pane-w2".into(), "surface-w2".into()),
        ]
    );
}

#[test]
fn close_publication_exact_rows_are_isolated_from_another_window() {
    let mut snapshot = publication_matrix_snapshot("surface-selected");
    set_published_rows(
        &mut snapshot,
        0,
        0,
        &[
            ("pane-matrix", "surface-before"),
            ("pane-other", "surface-other"),
        ],
    );
    let mut other_window = test_snapshot().windows.remove(0);
    other_window.window_id = Some("window-2".into());
    let other_workspace = &mut other_window.tab_manager.workspaces[0];
    other_workspace.workspace_id = Some("workspace-2".into());
    other_workspace.published_pane_selections = Some(vec![
        cmux_core::session::SessionPanePublishedSelectionSnapshot {
            pane_id: "window-2-pane-a".into(),
            panel_id: "window-2-surface-a".into(),
        },
        cmux_core::session::SessionPanePublishedSelectionSnapshot {
            pane_id: "window-2-pane-b".into(),
            panel_id: "window-2-surface-b".into(),
        },
    ]);
    snapshot.windows.push(other_window);
    let other_rows_before = published_rows(&snapshot, 1, 0);
    let closed = transition(
        &snapshot,
        "surface.close",
        json!({"surface_id":"surface-before"}),
    );
    assert_eq!(published_rows(&closed.snapshot, 1, 0), other_rows_before);
    assert!(closed
        .events
        .iter()
        .all(|event| event.workspace_id.as_deref() == Some("workspace-1")));
}

#[test]
fn close_publication_suppressed_workspace_close_only_clears_the_exact_closed_id() {
    for (label, pointer, expected_rows) in [
        (
            "closed",
            "surface-after",
            vec![("pane-other", "surface-other")],
        ),
        (
            "unrelated-unknown",
            "surface-unknown",
            vec![
                ("pane-matrix", "surface-unknown"),
                ("pane-other", "surface-other"),
            ],
        ),
    ] {
        let mut snapshot = publication_matrix_snapshot("surface-selected");
        set_published_rows(
            &mut snapshot,
            0,
            0,
            &[("pane-matrix", pointer), ("pane-other", "surface-other")],
        );
        let before = published_rows_bytes(&snapshot, 0, 0);
        let closed = transition(
            &snapshot,
            "surface.close",
            json!({"surface_id":"surface-after"}),
        );
        assert_eq!(
            lifecycle_event_names(&closed),
            ["surface.closed"],
            "{label}"
        );
        assert_eq!(
            published_rows(&closed.snapshot, 0, 0),
            expected_rows
                .into_iter()
                .map(|(pane, panel)| (pane.into(), panel.into()))
                .collect::<Vec<_>>(),
            "{label}"
        );
        if label == "unrelated-unknown" {
            assert_eq!(
                published_rows_bytes(&closed.snapshot, 0, 0),
                before,
                "suppression cannot rewrite an unrelated publisher row"
            );
        }
    }
}

#[test]
fn close_publication_published_workspace_close_preserves_unknown_previous_and_position() {
    for (label, pointer, previous, expected_rows) in [
        (
            "closed",
            "surface-before",
            Value::Null,
            vec![
                ("pane-other", "surface-other"),
                ("pane-matrix", "surface-selected"),
            ],
        ),
        (
            "unrelated-unknown",
            "surface-unknown",
            json!("surface-unknown"),
            vec![
                ("pane-matrix", "surface-selected"),
                ("pane-other", "surface-other"),
            ],
        ),
    ] {
        let mut snapshot = publication_matrix_snapshot("surface-selected");
        set_published_rows(
            &mut snapshot,
            0,
            0,
            &[("pane-matrix", pointer), ("pane-other", "surface-other")],
        );
        let closed = transition(
            &snapshot,
            "surface.close",
            json!({"surface_id":"surface-before"}),
        );
        assert_eq!(
            lifecycle_event_names(&closed),
            ["surface.closed", "surface.selected", "surface.focused"],
            "{label}"
        );
        assert_eq!(
            closed.events[1].payload["previous_surface_id"], previous,
            "{label}"
        );
        assert_eq!(
            published_rows(&closed.snapshot, 0, 0),
            expected_rows
                .into_iter()
                .map(|(pane, panel)| (pane.into(), panel.into()))
                .collect::<Vec<_>>(),
            "{label}"
        );
    }
}

#[test]
fn close_publication_suppressed_dock_close_only_clears_the_exact_closed_id() {
    for (label, unknown) in [("closed", false), ("unrelated-unknown", true)] {
        let (mut snapshot, pane_id, _, selected_id, trailing_id) = dock_publication_snapshot();
        let pointer = if unknown {
            "surface-unknown".to_owned()
        } else {
            trailing_id.clone()
        };
        set_published_rows(
            &mut snapshot,
            0,
            0,
            &[(&pane_id, &pointer), ("pane-1", "surface-1")],
        );
        let before = published_rows_bytes(&snapshot, 0, 0);
        let closed = transition(
            &snapshot,
            "surface.close",
            json!({"surface_id":trailing_id}),
        );
        assert_eq!(
            lifecycle_event_names(&closed),
            ["surface.closed"],
            "{label}"
        );
        assert_eq!(
            closed.snapshot.windows[0]
                .dock
                .as_ref()
                .and_then(|dock| dock.focused_surface_id.as_deref()),
            Some(selected_id.as_str()),
            "{label}"
        );
        if unknown {
            assert_eq!(published_rows_bytes(&closed.snapshot, 0, 0), before);
        } else {
            assert_eq!(
                published_rows(&closed.snapshot, 0, 0),
                vec![("pane-1".into(), "surface-1".into())]
            );
        }
    }
}

#[test]
fn close_publication_published_dock_close_preserves_unknown_previous_and_backing_row() {
    for (label, unknown) in [("closed", false), ("unrelated-unknown", true)] {
        let (mut snapshot, pane_id, first_id, selected_id, _) = dock_publication_snapshot();
        let pointer = if unknown {
            "surface-unknown".to_owned()
        } else {
            first_id.clone()
        };
        set_published_rows(
            &mut snapshot,
            0,
            0,
            &[(&pane_id, &pointer), ("pane-1", "surface-1")],
        );
        let closed = transition(&snapshot, "surface.close", json!({"surface_id":first_id}));
        assert_eq!(
            lifecycle_event_names(&closed),
            ["surface.closed", "surface.selected", "surface.focused"],
            "{label}"
        );
        assert_eq!(
            closed.events[1].payload["previous_surface_id"],
            if unknown {
                json!("surface-unknown")
            } else {
                Value::Null
            },
            "{label}"
        );
        assert_eq!(
            published_rows(&closed.snapshot, 0, 0),
            vec![
                (pane_id, selected_id),
                ("pane-1".into(), "surface-1".into()),
            ],
            "{label}"
        );
        assert!(closed.events.iter().all(|event| {
            event.workspace_id.as_deref() == Some("main") && event.window_id.is_none()
        }));
    }
}

#[test]
fn close_publication_dock_duplicate_pane_rows_selects_the_exact_closed_backing_owner() {
    let mut snapshot = test_snapshot();
    snapshot.windows[0].window_id = Some("main".into());
    let mut second_workspace = snapshot.windows[0].tab_manager.workspaces[0].clone();
    second_workspace.workspace_id = Some("workspace-2".into());
    second_workspace.focused_panel_id = Some("surface-w2".into());
    let SessionWorkspaceLayoutSnapshot::Pane(pane) = second_workspace.layout.as_mut().unwrap()
    else {
        unreachable!()
    };
    pane.pane_id = Some("pane-w2".into());
    pane.panel_ids = vec!["surface-w2".into()];
    pane.selected_panel_id = Some("surface-w2".into());
    snapshot.windows[0]
        .tab_manager
        .workspaces
        .push(second_workspace);
    let first = DockStore
        .create(
            &mut snapshot,
            "main",
            DockCreateRequest {
                kind: DockSurfaceKind::Terminal,
                focus: true,
                ..DockCreateRequest::default()
            },
        )
        .expect("seed ambiguous Dock owner");
    let second = DockStore
        .create(
            &mut snapshot,
            "main",
            DockCreateRequest {
                kind: DockSurfaceKind::Terminal,
                pane_id: Some(first.pane_id),
                focus: false,
                ..DockCreateRequest::default()
            },
        )
        .expect("seed ambiguous Dock survivor");
    snapshot = transition(
        &snapshot,
        "surface.focus",
        json!({"surface_id":second.surface_id}),
    )
    .snapshot;
    let pane_id = first.pane_id.to_string();
    let first_id = first.surface_id.to_string();
    let selected_id = second.surface_id.to_string();
    set_published_rows(
        &mut snapshot,
        0,
        0,
        &[(&pane_id, "surface-unrelated"), ("pane-1", "surface-1")],
    );
    set_published_rows(
        &mut snapshot,
        0,
        1,
        &[
            (&pane_id, &first_id),
            ("pane-workspace-2", "surface-workspace-2"),
        ],
    );
    let unrelated_backing_before = published_rows_bytes(&snapshot, 0, 0);

    let closed = transition(&snapshot, "surface.close", json!({"surface_id":first_id}));
    assert_eq!(
        lifecycle_event_names(&closed),
        ["surface.closed", "surface.selected", "surface.focused"]
    );
    assert_eq!(closed.events[1].payload["previous_surface_id"], Value::Null);
    assert_eq!(
        published_rows_bytes(&closed.snapshot, 0, 0),
        unrelated_backing_before,
        "the duplicate pane row in another workspace is byte-stable"
    );
    assert_eq!(
        published_rows(&closed.snapshot, 0, 1),
        vec![
            (pane_id, selected_id),
            ("pane-workspace-2".into(), "surface-workspace-2".into()),
        ]
    );
}

#[test]
fn close_publication_selected_sibling_orders_and_persists_the_survivor() {
    let mut snapshot = mixed_pane_snapshot();
    set_published_pointer(&mut snapshot, 0, 0, "pane-1", "surface-b");
    let closed = transition(
        &snapshot,
        "surface.close",
        json!({"surface_id": "surface-b"}),
    );
    let names: Vec<_> = closed.events.iter().map(|event| event.name).collect();
    assert_eq!(
        names,
        ["surface.closed", "surface.selected", "surface.focused"]
    );
    assert_eq!(closed.events[1].payload["surface_id"], json!("surface-a"));
    assert_eq!(
        closed.events[1].payload["previous_surface_id"],
        json!(null),
        "surface.closed clears the dead publisher pointer before reselection"
    );

    let persisted = commit_close(snapshot, closed);
    assert_eq!(
        published_pointer(&persisted, 0, 0, "pane-1"),
        Some("surface-a")
    );
}

#[test]
fn close_publication_unselected_sibling_persists_the_republished_survivor() {
    let mut snapshot = mixed_pane_snapshot();
    set_published_pointer(&mut snapshot, 0, 0, "pane-1", "surface-a");
    let closed = transition(
        &snapshot,
        "surface.close",
        json!({"surface_id": "surface-a"}),
    );
    let persisted = commit_close(snapshot, closed);
    assert_eq!(
        published_pointer(&persisted, 0, 0, "pane-1"),
        Some("surface-b"),
        "the dead pointer is replaced even when live selection did not move"
    );
}

#[test]
fn close_publication_collapsed_final_pane_prunes_its_dead_pointer() {
    let mut snapshot = split_close_snapshot();
    set_published_pointer(&mut snapshot, 0, 0, "pane-left", "surface-left");
    let closed = transition(
        &snapshot,
        "surface.close",
        json!({"surface_id": "surface-left"}),
    );
    let persisted = commit_close(snapshot, closed);
    assert_eq!(published_pointer(&persisted, 0, 0, "pane-left"), None);
    let SessionWorkspaceLayoutSnapshot::Pane(survivor) =
        persisted.windows[0].tab_manager.workspaces[0]
            .layout
            .as_ref()
            .expect("surviving layout")
    else {
        panic!("the empty split branch must collapse")
    };
    assert_eq!(survivor.pane_id.as_deref(), Some("pane-right"));
}

#[test]
fn close_publication_final_dock_surface_prunes_its_dead_pointer() {
    let mut snapshot = test_snapshot();
    snapshot.windows[0].window_id = Some("main".into());
    let created = DockStore
        .create(
            &mut snapshot,
            "main",
            DockCreateRequest {
                kind: DockSurfaceKind::Terminal,
                focus: true,
                ..DockCreateRequest::default()
            },
        )
        .expect("seed final Dock surface");
    let pane_id = created.pane_id.to_string();
    let surface_id = created.surface_id.to_string();
    set_published_pointer(&mut snapshot, 0, 0, &pane_id, &surface_id);
    let closed = transition(
        &snapshot,
        "surface.close",
        json!({"surface_id": surface_id}),
    );
    let persisted = commit_close(snapshot, closed);
    assert!(DockStore.list(&persisted, "main").is_empty());
    assert_eq!(published_pointer(&persisted, 0, 0, &pane_id), None);
}

#[test]
fn close_publication_prunes_only_the_owner_pane_across_windows() {
    let mut snapshot = split_close_snapshot();
    set_published_pointer(&mut snapshot, 0, 0, "pane-left", "surface-left");

    let mut second = test_snapshot().windows.remove(0);
    second.window_id = Some("window-2".into());
    let other = &mut second.tab_manager.workspaces[0];
    other.workspace_id = Some("workspace-2".into());
    other.focused_panel_id = Some("surface-other".into());
    let SessionWorkspaceLayoutSnapshot::Pane(pane) = other.layout.as_mut().unwrap() else {
        unreachable!()
    };
    pane.pane_id = Some("pane-other".into());
    pane.panel_ids = vec!["surface-other".into()];
    pane.selected_panel_id = Some("surface-other".into());
    other.published_pane_selections = Some(vec![
        cmux_core::session::SessionPanePublishedSelectionSnapshot {
            pane_id: "pane-other".into(),
            panel_id: "surface-other".into(),
        },
    ]);
    snapshot.windows.push(second);

    let closed = transition(
        &snapshot,
        "surface.close",
        json!({"surface_id": "surface-left"}),
    );
    let persisted = commit_close(snapshot, closed);
    assert_eq!(published_pointer(&persisted, 0, 0, "pane-left"), None);
    assert_eq!(
        published_pointer(&persisted, 1, 0, "pane-other"),
        Some("surface-other"),
        "another window's publisher state is isolated"
    );
}

#[test]
fn close_publication_last_surface_noop_preserves_the_pointer() {
    let mut snapshot = test_snapshot();
    set_published_pointer(&mut snapshot, 0, 0, "pane-1", "surface-1");
    let closed = transition(
        &snapshot,
        "surface.close",
        json!({"surface_id": "surface-1"}),
    );
    assert_eq!(expect_error(&closed).0, "invalid_state");
    assert!(!closed.changed);
    assert!(closed.events.is_empty());
    assert_eq!(closed.snapshot, snapshot);
}

#[test]
fn close_publication_failed_commit_rolls_back_without_publishing_the_candidate() {
    let mut snapshot = mixed_pane_snapshot();
    set_published_pointer(&mut snapshot, 0, 0, "pane-1", "surface-b");
    let before = snapshot.clone();
    let closed = transition(
        &snapshot,
        "surface.close",
        json!({"surface_id": "surface-b"}),
    );
    let mut executor = CloseCommitExecutor {
        fail_commit: true,
        ..Default::default()
    };
    let result = commit_lifecycle_transition(&mut snapshot, closed, &mut executor);
    assert_eq!(result, Err("injected close publication failure".into()));
    assert_eq!(executor.rollback_count, 1);
    assert_eq!(
        snapshot, before,
        "failed publication cannot expose the candidate"
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

// ---------------------------------------------------------------------------
// Round 2 pins: V2, R2, R3, R4, R6b
// ---------------------------------------------------------------------------

#[test]
fn pane_resize_failures_use_the_canonical_distinct_messages() {
    // V2: ControlCommandCoordinator+Pane.swift:452-476 at pinned e1825d40d.
    let snapshot = resizable_snapshot();
    // Horizontal-only tree: an up/down resize has no vertical ancestor.
    let vertical = transition(
        &snapshot,
        "pane.resize",
        json!({"pane_id": "pane-left", "direction": "up", "amount": 1}),
    );
    let (code, message) = expect_error(&vertical);
    assert_eq!(code, "invalid_state");
    assert_eq!(message, "No vertical split ancestor for pane");
    // The rightmost pane has no border to its right.
    let border = transition(
        &snapshot,
        "pane.resize",
        json!({"pane_id": "pane-right", "direction": "right", "amount": 1}),
    );
    let (code, message) = expect_error(&border);
    assert_eq!(code, "invalid_state");
    assert_eq!(message, "Pane has no adjacent border in direction right");
    // Absolute path keeps its single ancestor error.
    let absolute = transition(
        &snapshot,
        "pane.resize",
        json!({"pane_id": "pane-left", "absolute_axis": "vertical", "target_pixels": 100}),
    );
    let (code, message) = expect_error(&absolute);
    assert_eq!(code, "invalid_state");
    assert_eq!(message, "No split ancestor for absolute pane resize");
}

#[test]
fn surface_create_inserts_next_to_the_selected_tab() {
    // R2: canonical inserts the created tab adjacent to its creator (the
    // pane's selected tab), not at the end (capture rows_shape ordering).
    let snapshot = mixed_pane_snapshot(); // [surface-a, surface-b], b selected
    let created = transition(&snapshot, "surface.create", json!({"type": "terminal"}));
    let created_id = ok_value(&created)["surface_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let SessionWorkspaceLayoutSnapshot::Pane(pane) =
        created.snapshot.windows[0].tab_manager.workspaces[0]
            .layout
            .as_ref()
            .unwrap()
    else {
        unreachable!();
    };
    assert_eq!(
        pane.panel_ids,
        vec!["surface-a".to_string(), "surface-b".to_string(), created_id],
        "inserted after the selected tab (surface-b), before nothing else"
    );
}

#[test]
fn surface_close_of_focused_unselected_surface_suppresses_the_pair() {
    // Round 5 item 4: the Swift guard suppresses the pair when the pane
    // selection did not move (publishCmuxFocusedSelection,
    // CmuxLifecycleEventPublishing.swift:171), even though focus fell back
    // off the closed surface.
    let mut snapshot = mixed_pane_snapshot();
    snapshot.windows[0].tab_manager.workspaces[0].focused_panel_id = Some("surface-a".into());
    let closed = transition(
        &snapshot,
        "surface.close",
        json!({"surface_id": "surface-a"}),
    );
    let _ = ok_value(&closed);
    let names: Vec<_> = closed.events.iter().map(|event| event.name).collect();
    assert_eq!(names, ["surface.closed"], "no selection change, no pair");
}

#[test]
fn pane_created_orientation_is_the_split_axis() {
    // R4: capture pane_create.direction_right_happy — payload orientation is
    // "horizontal" for direction right, not the raw direction token.
    let snapshot = test_snapshot();
    for (direction, axis) in [("right", "horizontal"), ("down", "vertical")] {
        let created = transition(&snapshot, "pane.create", json!({"direction": direction}));
        let event = created
            .events
            .iter()
            .find(|event| event.name == "pane.created")
            .expect("pane.created event");
        assert_eq!(event.payload["orientation"], json!(axis), "{direction}");
    }
}

#[test]
fn surface_focus_selects_the_owner_workspace_globally() {
    // R6b: capture surface_focus.happy selectors probe — focusing a surface
    // moves the window's selected workspace to the owner workspace.
    let mut snapshot = test_snapshot();
    let mut second = snapshot.windows[0].tab_manager.workspaces[0].clone();
    second.workspace_id = Some("workspace-2".into());
    second.focused_panel_id = Some("surface-2".into());
    let SessionWorkspaceLayoutSnapshot::Pane(pane) = second.layout.as_mut().unwrap() else {
        unreachable!();
    };
    pane.pane_id = Some("pane-2".into());
    pane.panel_ids = vec!["surface-2".into()];
    pane.selected_panel_id = Some("surface-2".into());
    snapshot.windows[0].tab_manager.workspaces.push(second);
    snapshot.windows[0].tab_manager.selected_workspace_index = Some(0);

    let focused = transition(
        &snapshot,
        "surface.focus",
        json!({"surface_id": "surface-2"}),
    );
    let payload = ok_value(&focused);
    assert_eq!(payload["workspace_id"], json!("workspace-2"));
    assert_eq!(
        focused.snapshot.windows[0]
            .tab_manager
            .selected_workspace_index,
        Some(1),
        "the owner workspace becomes globally selected"
    );
    assert_eq!(
        focused.snapshot.windows[0].selected_workspace_id.as_deref(),
        Some("workspace-2")
    );
}

#[test]
fn registry_forget_mints_a_fresh_ref_on_rerender() {
    // R5: capture — the close echo renders surface:17 for a surface the
    // registry previously knew as :16; forgetting never rewinds the counter.
    let mut registry = ControlHandleRegistry::default();
    assert_eq!(registry.mint("surface", "s-1"), "surface:1");
    assert_eq!(registry.mint("surface", "s-2"), "surface:2");
    registry.forget("surface", "s-1");
    assert_eq!(
        registry.mint("surface", "s-1"),
        "surface:3",
        "re-render after forget mints fresh"
    );
    assert_eq!(
        registry.mint("surface", "s-2"),
        "surface:2",
        "others stable"
    );
}

#[test]
fn closing_an_unselected_tab_suppresses_the_noop_pair() {
    // Round 5 item 4 (overrides round 3's emit-when-equal): canonical
    // publishCmuxFocusedSelection guards previousSelectedSurfaceId !=
    // surfaceId (CmuxLifecycleEventPublishing.swift:171) — closing a tab that
    // was not selected leaves the selection in place and the no-op pair is
    // suppressed.
    let snapshot = mixed_pane_snapshot(); // surface-a, surface-b (selected+focused)
    let created = transition(&snapshot, "surface.create", json!({"type": "terminal"}));
    let created_id = ok_value(&created)["surface_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let closed = transition(
        &created.snapshot,
        "surface.close",
        json!({"surface_id": created_id}),
    );
    let _ = ok_value(&closed);
    let names: Vec<_> = closed.events.iter().map(|event| event.name).collect();
    assert_eq!(names, ["surface.closed"], "no-op reselection is suppressed");
}

#[test]
fn closing_the_selected_tab_reselects_its_successor() {
    // Round 5 item 3: canonical bonsplit selects the closed tab's SUCCESSOR
    // (the tab that slides into its index), not the first tab in the pane —
    // capture surface_close.happy post-close rows put the new selection
    // (<uuid-25>) at the closed tab's former index_in_pane.
    let mut snapshot = mixed_pane_snapshot();
    {
        let workspace = &mut snapshot.windows[0].tab_manager.workspaces[0];
        let SessionWorkspaceLayoutSnapshot::Pane(pane) =
            workspace.layout.as_mut().expect("pane layout")
        else {
            unreachable!();
        };
        pane.panel_ids = vec!["surface-a".into(), "surface-b".into(), "surface-c".into()];
    }
    let mut encoded = serde_json::to_value(snapshot).expect("encode");
    encoded["windows"][0]["tab_manager"]["workspaces"][0]["surfaces"] = json!([
        {"surface_id": "surface-a", "pane_id": "pane-1", "generation": 1, "kind": {"type": "terminal"}},
        {"surface_id": "surface-b", "pane_id": "pane-1", "generation": 1, "kind": {"type": "terminal"}},
        {"surface_id": "surface-c", "pane_id": "pane-1", "generation": 1, "kind": {"type": "terminal"}}
    ]);
    let snapshot: AppSessionSnapshot = serde_json::from_value(encoded).expect("decode");

    let closed = transition(
        &snapshot,
        "surface.close",
        json!({"surface_id": "surface-b"}),
    );
    let _ = ok_value(&closed);
    let names: Vec<_> = closed.events.iter().map(|event| event.name).collect();
    assert_eq!(
        names,
        ["surface.closed", "surface.selected", "surface.focused"]
    );
    assert_eq!(
        closed.events[1].payload["surface_id"],
        json!("surface-c"),
        "the successor (tab sliding into the closed index) wins"
    );
    // Round 7: no selection was ever PUBLISHED in this synthetic fixture, so
    // the publisher pointer is empty and previous_surface_id is null (the
    // pointer never reports a dead surface).
    assert_eq!(closed.events[1].payload["previous_surface_id"], json!(null));
    // Closing an unselected trailing tab is a no-op for selection.
    let last = transition(
        &snapshot,
        "surface.close",
        json!({"surface_id": "surface-c"}),
    );
    let _ = ok_value(&last);
    assert_eq!(last.events.len(), 1);
}

#[test]
fn surface_focus_with_mismatched_explicit_workspace_fails_closed() {
    // Round 5 item 2: canonical resolveSurfaceWorkspace gives an explicit
    // workspace_id precedence and fails closed on mismatch
    // (TerminalController+ControlSurfaceContext.swift:298-315, dock mismatch
    // :286-292); owner resolution applies only when workspace_id is absent.
    let mut snapshot = test_snapshot();
    let mut second = snapshot.windows[0].tab_manager.workspaces[0].clone();
    second.workspace_id = Some("workspace-2".into());
    second.focused_panel_id = Some("surface-2".into());
    let SessionWorkspaceLayoutSnapshot::Pane(pane) = second.layout.as_mut().unwrap() else {
        unreachable!();
    };
    pane.pane_id = Some("pane-2".into());
    pane.panel_ids = vec!["surface-2".into()];
    pane.selected_panel_id = Some("surface-2".into());
    snapshot.windows[0].tab_manager.workspaces.push(second);

    let mismatched = transition(
        &snapshot,
        "surface.focus",
        json!({"surface_id": "surface-2", "workspace_id": "workspace-1"}),
    );
    let (code, message) = expect_error(&mismatched);
    assert_eq!(code, "not_found");
    assert_eq!(message, "Surface not found");
    // Matching explicit workspace still succeeds.
    let matched = transition(
        &snapshot,
        "surface.focus",
        json!({"surface_id": "surface-2", "workspace_id": "workspace-2"}),
    );
    assert_eq!(ok_value(&matched)["workspace_id"], json!("workspace-2"));
}

#[test]
fn pane_resize_failures_carry_the_canonical_data_blocks() {
    // Round 5 item 5: ControlCommandCoordinator+Pane.swift:450-477 — the
    // orientation/border failures carry {pane_id, direction}; the divider
    // failure carries {split_id}.
    let snapshot = resizable_snapshot();
    let data_of = |transition: &LifecycleTransition| match &transition.result {
        ControlCallResult::Err { data, .. } => data.clone().map(Value::from),
        ControlCallResult::Ok(_) => panic!("expected error"),
    };
    let vertical = transition(
        &snapshot,
        "pane.resize",
        json!({"pane_id": "pane-left", "direction": "up", "amount": 1}),
    );
    assert_eq!(
        data_of(&vertical),
        Some(json!({"pane_id": "pane-left", "direction": "up"}))
    );
    let border = transition(
        &snapshot,
        "pane.resize",
        json!({"pane_id": "pane-right", "direction": "right", "amount": 1}),
    );
    assert_eq!(
        data_of(&border),
        Some(json!({"pane_id": "pane-right", "direction": "right"}))
    );
    let absolute = transition(
        &snapshot,
        "pane.resize",
        json!({"pane_id": "pane-left", "absolute_axis": "vertical", "target_pixels": 100}),
    );
    assert_eq!(
        data_of(&absolute),
        Some(json!({"pane_id": "pane-left", "absolute_axis": "vertical"}))
    );
}

#[test]
fn create_after_explicit_focus_keeps_the_new_tab_selected() {
    // Round 6 item 1: shouldFocusNewTab = focus ?? (focusedPaneId == paneId)
    // (Workspace.swift:7480). After an explicit surface.focus lands in the
    // pane, a create WITHOUT a focus param keeps the new tab selected and
    // focused (canonical close.happy rows: the setup-created tab was
    // selected, so its close reselects the successor). Without prior focus
    // (bonsplit focusedPaneId unset) the create still reverts — the
    // terminal_happy flip both platforms already share.
    let snapshot = mixed_pane_snapshot(); // a, b(selected+focused); no focused_pane_id
                                          // No prior explicit focus: revert (5-event flip) and prior tab stays selected.
    let reverted = transition(&snapshot, "surface.create", json!({"type": "terminal"}));
    assert_eq!(
        reverted.events.len(),
        5,
        "unfocused pane keeps the revert flip"
    );

    // Explicit focus first: the create keeps the new tab selected.
    let focused = transition(
        &snapshot,
        "surface.focus",
        json!({"surface_id": "surface-b"}),
    );
    let after_focus = focused.snapshot.clone();
    assert_eq!(
        after_focus.windows[0].tab_manager.workspaces[0]
            .focused_pane_id
            .as_deref(),
        Some("pane-1"),
        "surface.focus records the bonsplit-focused pane"
    );
    let created = transition(&after_focus, "surface.create", json!({"type": "terminal"}));
    let created_id = ok_value(&created)["surface_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let names: Vec<_> = created.events.iter().map(|event| event.name).collect();
    // Round 7: born-selected keep creates publish no selection transition.
    assert_eq!(names, ["surface.created"]);
    let list = transition(&created.snapshot, "surface.list", json!({}));
    let rows = ok_value(&list)["surfaces"].as_array().unwrap().clone();
    let row = rows
        .iter()
        .find(|row| row["id"] == json!(created_id))
        .unwrap();
    assert_eq!(row["selected_in_pane"], json!(true));
    assert_eq!(row["focused"], json!(true));
    // Closing it in this 2-tab pane reselects the pointer target itself
    // (successor fallback = surface-b == the pointer), so the canonical
    // pointer guard suppresses the pair (round 7; the 3-tab capture shape is
    // pinned by close_pair_previous_is_the_nearest_surviving_published_selection).
    let closed = transition(
        &created.snapshot,
        "surface.close",
        json!({"surface_id": created_id}),
    );
    let names: Vec<_> = closed.events.iter().map(|event| event.name).collect();
    assert_eq!(names, ["surface.closed"]);
}

#[test]
fn unrendered_entities_burn_ref_numbers_via_the_refresh_walk() {
    // Simulate the burned mint: a workspace created between two dispatches
    // occupies the next pane number even though no response ever renders it.
    let mut registry = ControlHandleRegistry::default();
    let base = test_snapshot();
    for (kind, id) in bootstrap_registry_seeds(&base) {
        registry.mint(kind, &id);
    }
    // A second workspace appears (never rendered anywhere)...
    let mut grown = base.clone();
    let mut ghost = grown.windows[0].tab_manager.workspaces[0].clone();
    ghost.workspace_id = Some("ghost-ws".into());
    let SessionWorkspaceLayoutSnapshot::Pane(pane) = ghost.layout.as_mut().unwrap() else {
        unreachable!();
    };
    pane.pane_id = Some("ghost-pane".into());
    pane.panel_ids = vec!["ghost-surface".into()];
    grown.windows[0].tab_manager.workspaces.push(ghost);
    // ...and the next dispatch's refresh mints it.
    for (kind, id) in bootstrap_registry_seeds(&grown) {
        registry.mint(kind, &id);
    }
    assert_eq!(
        registry.mint("pane", "ghost-pane"),
        "pane:2",
        "burned at refresh"
    );
    assert_eq!(
        registry.mint("pane", "later-rendered-pane"),
        "pane:3",
        "later renders skip the burned number"
    );
}

#[test]
fn close_pair_previous_is_the_nearest_surviving_published_selection() {
    // Round 7: the reselection publish reads the publisher pointer AFTER the
    // close mutation (publishCmuxFocusedSelection,
    // CmuxLifecycleEventPublishing.swift:168) — surface.closed's clearSurface
    // (:30-35, invoked at :153) has already purged entries pointing at the
    // dead surface, and born-selected tabs never advanced the pointer. So
    // previous_surface_id is the explicitly-focused SURVIVOR (capture
    // surface_close.happy frame 2: surface:15), never the just-closed id.
    let mut snapshot = mixed_pane_snapshot();
    {
        let workspace = &mut snapshot.windows[0].tab_manager.workspaces[0];
        let SessionWorkspaceLayoutSnapshot::Pane(pane) =
            workspace.layout.as_mut().expect("pane layout")
        else {
            unreachable!();
        };
        pane.panel_ids = vec!["surface-a".into(), "surface-b".into(), "surface-c".into()];
    }
    let mut encoded = serde_json::to_value(snapshot).expect("encode");
    encoded["windows"][0]["tab_manager"]["workspaces"][0]["surfaces"] = json!([
        {"surface_id": "surface-a", "pane_id": "pane-1", "generation": 1, "kind": {"type": "terminal"}},
        {"surface_id": "surface-b", "pane_id": "pane-1", "generation": 1, "kind": {"type": "terminal"}},
        {"surface_id": "surface-c", "pane_id": "pane-1", "generation": 1, "kind": {"type": "terminal"}}
    ]);
    let snapshot: AppSessionSnapshot = serde_json::from_value(encoded).expect("decode");

    // Explicit focus advances the pointer to surface-b...
    let focused = transition(
        &snapshot,
        "surface.focus",
        json!({"surface_id": "surface-b"}),
    );
    // ...the keep-selected create is born selected (no transition publish;
    // pointer stays on surface-b)...
    let created = transition(
        &focused.snapshot,
        "surface.create",
        json!({"type": "terminal"}),
    );
    let created_id = ok_value(&created)["surface_id"]
        .as_str()
        .unwrap()
        .to_owned();
    // ...and closing it reselects the successor (surface-c: the tab sliding
    // into the closed index) with previous = the SURVIVING pointer.
    let closed = transition(
        &created.snapshot,
        "surface.close",
        json!({"surface_id": created_id}),
    );
    let _ = ok_value(&closed);
    let names: Vec<_> = closed.events.iter().map(|event| event.name).collect();
    assert_eq!(
        names,
        ["surface.closed", "surface.selected", "surface.focused"]
    );
    assert_eq!(closed.events[1].payload["surface_id"], json!("surface-c"));
    assert_eq!(
        closed.events[1].payload["previous_surface_id"],
        json!("surface-b"),
        "previous is the surviving published selection, never the dead surface"
    );
}
