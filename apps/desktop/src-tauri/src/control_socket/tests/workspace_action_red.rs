//! Frozen-e1825d40 RED oracle for `workspace.action`.

use super::*;
use crate::workspace_action::{
    apply_workspace_action_mutation, plan_workspace_action, WorkspaceActionMutation,
    SUPPORTED_WORKSPACE_ACTIONS,
};
use cmux_workspaces::PaletteStoreSnapshot;

const WINDOW: &str = "10000000-0000-4000-8000-000000000001";
const WORKSPACES: [&str; 5] = [
    "20000000-0000-4000-8000-000000000001",
    "20000000-0000-4000-8000-000000000002",
    "20000000-0000-4000-8000-000000000003",
    "20000000-0000-4000-8000-000000000004",
    "20000000-0000-4000-8000-000000000005",
];

fn action_snapshot() -> AppSessionSnapshot {
    let mut snapshot = test_snapshot();
    let window = &mut snapshot.windows[0];
    window.window_id = Some(WINDOW.into());
    let template = window.tab_manager.workspaces[0].clone();
    window.tab_manager.workspaces = WORKSPACES
        .iter()
        .enumerate()
        .map(|(index, id)| {
            let mut workspace = template.clone();
            workspace.workspace_id = Some((*id).into());
            workspace.process_title = format!("shell-{index}");
            workspace.custom_title = Some(format!("Workspace {index}"));
            workspace.custom_description = None;
            workspace.custom_color = None;
            workspace.is_pinned = Some(index == 1 || index == 4);
            workspace
        })
        .collect();
    window.tab_manager.selected_workspace_index = Some(2);
    window.selected_workspace_id = Some(WORKSPACES[2].into());
    snapshot
}

fn params(value: Value) -> serde_json::Map<String, Value> {
    value.as_object().unwrap().clone()
}

fn plan(
    snapshot: &AppSessionSnapshot,
    value: Value,
) -> crate::workspace_action::WorkspaceActionPlan {
    plan_workspace_action(snapshot, &params(value), &PaletteStoreSnapshot::default())
}

fn ok_value(plan: &crate::workspace_action::WorkspaceActionPlan) -> Value {
    let ControlCallResult::Ok(value) = &plan.result else {
        panic!("expected success, got {:?}", plan.result)
    };
    value.clone().into()
}

fn error_parts(plan: &crate::workspace_action::WorkspaceActionPlan) -> (&str, &str, Value) {
    let ControlCallResult::Err {
        code,
        message,
        data,
    } = &plan.result
    else {
        panic!("expected error, got {:?}", plan.result)
    };
    (
        code,
        message,
        data.clone().map(Value::from).unwrap_or(Value::Null),
    )
}

#[test]
fn workspace_action_is_a_first_class_advertised_route() {
    assert!(CONTROL_SOCKET_METHODS.contains(&"workspace.action"));
    assert_eq!(
        control_request_route_for_method("workspace.action"),
        ControlRequestRoute::WorkspaceAction
    );
}

#[test]
fn validation_and_unknown_action_contract_are_exact() {
    let snapshot = action_snapshot();
    let missing = plan(&snapshot, json!({}));
    assert_eq!(
        (error_parts(&missing).0, error_parts(&missing).1),
        ("invalid_params", "Missing action")
    );

    let unknown = plan(&snapshot, json!({"action":"  Future-Action  "}));
    let (code, message, data) = error_parts(&unknown);
    assert_eq!(
        (code, message),
        ("invalid_params", "Unknown workspace action")
    );
    assert_eq!(data["action"], "future_action");
    assert_eq!(
        data["supported_actions"],
        json!(SUPPORTED_WORKSPACE_ACTIONS)
    );

    let unavailable = plan(&AppSessionSnapshot::default(), json!({"action":"pin"}));
    assert_eq!(
        (error_parts(&unavailable).0, error_parts(&unavailable).1),
        ("unavailable", "TabManager not available")
    );

    let missing_target = plan(
        &snapshot,
        json!({"workspace_id":"20000000-0000-4000-8000-000000000099", "action":"pin"}),
    );
    assert_eq!(
        (
            error_parts(&missing_target).0,
            error_parts(&missing_target).1
        ),
        ("not_found", "Workspace not found")
    );
}

#[test]
fn all_actions_plan_exact_mutations_and_payload_extras_without_changing_focus() {
    let snapshot = action_snapshot();
    let before_selected = snapshot.windows[0].tab_manager.selected_workspace_index;
    let before_selected_id = snapshot.windows[0].selected_workspace_id.clone();
    let cases = [
        ("pin", json!({"pinned":true})),
        ("unpin", json!({"pinned":false})),
        ("rename", json!({"title":"Renamed"})),
        ("clear_name", json!({"title":"shell-2"})),
        (
            "set_description",
            json!({"description":"Line one\nLine two"}),
        ),
        ("clear_description", json!({"description":null})),
        ("move_up", json!({"index":1})),
        ("move_down", json!({"index":3})),
        ("move_top", json!({"index":0})),
        ("close_others", json!({"closed":2})),
        ("close_above", json!({"closed":1})),
        ("close_below", json!({"closed":1})),
        ("mark_read", json!({})),
        ("mark_unread", json!({})),
        ("set_color", json!({"color":"#7D6608"})),
        ("clear_color", json!({"color":null})),
    ];

    for (action, extras) in cases {
        let mut request = json!({"action": action, "workspace_id": WORKSPACES[2]});
        if action == "rename" {
            request["title"] = json!("  Renamed  ");
        }
        if action == "set_description" {
            request["description"] = json!("Line one\r\nLine two");
        }
        if action == "set_color" {
            request["color"] = json!("amber");
        }
        let planned = plan(&snapshot, request);
        let payload = ok_value(&planned);
        assert_eq!(payload["action"], action);
        assert_eq!(payload["workspace_id"], WORKSPACES[2]);
        assert_eq!(payload["window_id"], WINDOW);
        for (key, value) in extras.as_object().unwrap() {
            assert_eq!(&payload[key], value, "{action} field {key}");
        }
        assert_eq!(
            snapshot.windows[0].tab_manager.selected_workspace_index, before_selected,
            "planner changed selection for {action}"
        );
        assert_eq!(
            snapshot.windows[0].selected_workspace_id, before_selected_id,
            "planner changed selected identity for {action}"
        );
        assert_ne!(planned.mutation, WorkspaceActionMutation::None, "{action}");
    }
}

#[test]
fn close_plans_filter_pinned_and_keep_original_indices() {
    let snapshot = action_snapshot();
    let others = plan(
        &snapshot,
        json!({"workspace_id":WORKSPACES[2], "action":"close-others"}),
    );
    assert_eq!(
        others.mutation,
        WorkspaceActionMutation::Close {
            window_index: 0,
            workspace_indices: vec![0, 3],
        }
    );
    assert_eq!(ok_value(&others)["closed"], 2);

    let above = plan(
        &snapshot,
        json!({"workspace_id":WORKSPACES[2], "action":"close-above"}),
    );
    assert_eq!(
        above.mutation,
        WorkspaceActionMutation::Close {
            window_index: 0,
            workspace_indices: vec![0],
        }
    );

    let below = plan(
        &snapshot,
        json!({"workspace_id":WORKSPACES[2], "action":"close-below"}),
    );
    assert_eq!(
        below.mutation,
        WorkspaceActionMutation::Close {
            window_index: 0,
            workspace_indices: vec![3],
        }
    );
}

#[test]
fn pure_mutations_change_only_the_target_window_and_preserve_selection_identity() {
    let mut snapshot = action_snapshot();
    let mut second = snapshot.windows[0].clone();
    second.window_id = Some("10000000-0000-4000-8000-000000000002".into());
    for (index, workspace) in second.tab_manager.workspaces.iter_mut().enumerate() {
        workspace.workspace_id = Some(format!("30000000-0000-4000-8000-{index:012}"));
    }
    snapshot.windows.push(second.clone());
    let selected = snapshot.windows[0].selected_workspace_id.clone();

    let rename = plan(
        &snapshot,
        json!({"workspace_id":WORKSPACES[2], "action":"rename", "title":"  New name  "}),
    );
    assert!(apply_workspace_action_mutation(
        &mut snapshot,
        &rename.mutation
    ));
    assert_eq!(
        snapshot.windows[0].tab_manager.workspaces[2]
            .custom_title
            .as_deref(),
        Some("New name")
    );
    assert_eq!(snapshot.windows[1], second);
    assert_eq!(snapshot.windows[0].selected_workspace_id, selected);
}

#[test]
fn color_resolution_uses_effective_palette_and_exact_errors() {
    let snapshot = action_snapshot();
    let palette = PaletteStoreSnapshot {
        stored: Some(vec![
            ("Amber".into(), "#010203".into()),
            ("Team Blue".into(), "#abcdef".into()),
        ]),
        ..Default::default()
    };
    let named = plan_workspace_action(
        &snapshot,
        &params(json!({"action":"set-color", "color":"team blue"})),
        &palette,
    );
    assert_eq!(ok_value(&named)["color"], "#ABCDEF");

    let hex = plan_workspace_action(
        &snapshot,
        &params(json!({"action":"set_color", "color":" c0392b "})),
        &palette,
    );
    assert_eq!(ok_value(&hex)["color"], "#C0392B");

    let invalid = plan_workspace_action(
        &snapshot,
        &params(json!({"action":"set_color", "color":"ultraviolet"})),
        &palette,
    );
    let (code, message, data) = error_parts(&invalid);
    assert_eq!(code, "invalid_params");
    assert_eq!(
        message,
        "Invalid color. Use a hex value (#RRGGBB) or a named color."
    );
    assert_eq!(data["named_colors"], json!(["Amber", "Team Blue"]));
}

#[test]
fn action_specific_required_values_have_canonical_errors() {
    let snapshot = action_snapshot();
    for (action, key, message) in [
        ("rename", "title", "Missing or invalid title"),
        (
            "set_description",
            "description",
            "Missing or invalid description",
        ),
        ("set_color", "color", "Missing or invalid color"),
    ] {
        let mut blank = json!({"action":action});
        blank[key] = json!("   \r\n  ");
        for request in [json!({"action":action}), blank] {
            let planned = plan(&snapshot, request);
            assert_eq!(
                (error_parts(&planned).0, error_parts(&planned).1),
                ("invalid_params", message),
                "{action}"
            );
        }
    }
}
