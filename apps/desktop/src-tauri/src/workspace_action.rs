//! Pure planning and model mutation for the canonical `workspace.action` v2 method.

use cmux_core::session::{AppSessionSnapshot, SessionWindowSnapshot, SessionWorkspaceSnapshot};
use cmux_core::session_ops;
use cmux_ipc::{ControlCallResult, JsonValue};
use cmux_workspaces::{palette, resolved_color_hex, PaletteStoreSnapshot};
use serde_json::{json, Map, Value};
use uuid::Uuid;

pub(crate) const SUPPORTED_WORKSPACE_ACTIONS: [&str; 16] = [
    "pin",
    "unpin",
    "rename",
    "clear_name",
    "set_description",
    "clear_description",
    "move_up",
    "move_down",
    "move_top",
    "close_others",
    "close_above",
    "close_below",
    "mark_read",
    "mark_unread",
    "set_color",
    "clear_color",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum WorkspaceActionMutation {
    None,
    SetPinned {
        window_index: usize,
        workspace_index: usize,
        pinned: bool,
    },
    Rename {
        window_index: usize,
        workspace_index: usize,
        title: String,
    },
    SetDescription {
        window_index: usize,
        workspace_index: usize,
        description: String,
    },
    Reorder {
        window_index: usize,
        workspace_index: usize,
        destination_index: usize,
    },
    MoveTop {
        window_index: usize,
        workspace_index: usize,
    },
    Close {
        window_index: usize,
        workspace_indices: Vec<usize>,
    },
    MarkUnread {
        workspace_id: String,
        unread: bool,
    },
    SetColor {
        window_index: usize,
        workspace_index: usize,
        color: Option<String>,
    },
}

#[derive(Debug, Clone)]
pub(crate) struct WorkspaceActionPlan {
    pub result: ControlCallResult,
    pub mutation: WorkspaceActionMutation,
    pub window_index: Option<usize>,
    pub workspace_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct WorkspaceActionCompletion {
    pub window_id: Option<String>,
    pub workspace_id: Option<String>,
    pub payload: Value,
}

pub(crate) fn workspace_action_completion(
    params: &Map<String, Value>,
    result: &Value,
) -> WorkspaceActionCompletion {
    WorkspaceActionCompletion {
        window_id: result
            .get("window_id")
            .and_then(Value::as_str)
            .map(str::to_owned),
        workspace_id: result
            .get("workspace_id")
            .and_then(Value::as_str)
            .map(str::to_owned),
        payload: json!({"method":"workspace.action", "params":params, "result":result}),
    }
}

fn ok(value: Value) -> ControlCallResult {
    ControlCallResult::Ok(JsonValue::try_from(value).expect("workspace action payload is JSON"))
}

fn error(code: &str, message: &str, data: Option<Value>) -> WorkspaceActionPlan {
    WorkspaceActionPlan {
        result: ControlCallResult::Err {
            code: code.to_owned(),
            message: message.to_owned(),
            data: data.and_then(|value| JsonValue::try_from(value).ok()),
        },
        mutation: WorkspaceActionMutation::None,
        window_index: None,
        workspace_id: None,
    }
}

fn normalized_action(params: &Map<String, Value>) -> Option<String> {
    params
        .get("action")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|action| !action.is_empty())
        .map(|action| action.to_lowercase().replace('-', "_"))
}

fn valid_uuid_param(params: &Map<String, Value>, key: &str) -> Option<String> {
    params
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| Uuid::parse_str(value).is_ok())
        .map(str::to_owned)
}

fn routed_window_index(
    snapshot: &AppSessionSnapshot,
    params: &Map<String, Value>,
    active_window_id: Option<&str>,
) -> Option<usize> {
    if params
        .get("window_id")
        .is_some_and(|value| !value.is_null())
    {
        let window_id = valid_uuid_param(params, "window_id")?;
        return snapshot
            .windows
            .iter()
            .position(|window| window.window_id.as_deref() == Some(window_id.as_str()));
    }
    if let Some(workspace_id) = valid_uuid_param(params, "workspace_id") {
        if let Some(index) =
            snapshot.windows.iter().position(|window| {
                window.tab_manager.workspaces.iter().any(|workspace| {
                    workspace.workspace_id.as_deref() == Some(workspace_id.as_str())
                })
            })
        {
            return Some(index);
        }
    }
    active_window_id
        .and_then(|active| {
            snapshot
                .windows
                .iter()
                .position(|window| window.window_id.as_deref() == Some(active))
        })
        .or_else(|| (!snapshot.windows.is_empty()).then_some(0))
}

fn target_workspace_index(
    window: &SessionWindowSnapshot,
    params: &Map<String, Value>,
) -> Option<usize> {
    let requested = valid_uuid_param(params, "workspace_id").or_else(|| {
        window
            .tab_manager
            .selected_workspace_index
            .and_then(|index| usize::try_from(index).ok())
            .and_then(|index| window.tab_manager.workspaces.get(index))
            .and_then(|workspace| workspace.workspace_id.clone())
            .or_else(|| window.selected_workspace_id.clone())
    })?;
    window
        .tab_manager
        .workspaces
        .iter()
        .position(|workspace| workspace.workspace_id.as_deref() == Some(requested.as_str()))
}

fn required_string(params: &Map<String, Value>, key: &str) -> Option<String> {
    params
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn required_preserved_string(params: &Map<String, Value>, key: &str) -> Option<String> {
    params
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_owned)
}

fn workspace_display_name(workspace: &SessionWorkspaceSnapshot) -> String {
    workspace
        .custom_title
        .as_deref()
        .filter(|title| !title.trim().is_empty())
        .or_else(|| {
            (!workspace.process_title.trim().is_empty()).then_some(workspace.process_title.as_str())
        })
        .unwrap_or("Workspace")
        .to_owned()
}

fn sync_selected_workspace_id(window: &mut SessionWindowSnapshot) {
    window.selected_workspace_id = window
        .tab_manager
        .selected_workspace_index
        .and_then(|index| usize::try_from(index).ok())
        .and_then(|index| window.tab_manager.workspaces.get(index))
        .and_then(|workspace| workspace.workspace_id.clone());
}

pub(crate) fn apply_workspace_action_mutation(
    snapshot: &mut AppSessionSnapshot,
    mutation: &WorkspaceActionMutation,
) -> bool {
    let changed = match mutation {
        WorkspaceActionMutation::None | WorkspaceActionMutation::MarkUnread { .. } => false,
        WorkspaceActionMutation::SetPinned {
            window_index,
            workspace_index,
            pinned,
        } => snapshot
            .windows
            .get_mut(*window_index)
            .is_some_and(|window| {
                session_ops::set_workspace_pinned(
                    &mut window.tab_manager,
                    *workspace_index as i64,
                    *pinned,
                )
            }),
        WorkspaceActionMutation::Rename {
            window_index,
            workspace_index,
            title,
        } => snapshot
            .windows
            .get_mut(*window_index)
            .is_some_and(|window| {
                session_ops::rename_workspace(
                    &mut window.tab_manager,
                    *workspace_index as i64,
                    title,
                )
            }),
        WorkspaceActionMutation::SetDescription {
            window_index,
            workspace_index,
            description,
        } => snapshot
            .windows
            .get_mut(*window_index)
            .is_some_and(|window| {
                session_ops::set_workspace_description(
                    &mut window.tab_manager,
                    *workspace_index as i64,
                    description,
                )
            }),
        WorkspaceActionMutation::Reorder {
            window_index,
            workspace_index,
            destination_index,
        } => snapshot
            .windows
            .get_mut(*window_index)
            .is_some_and(|window| {
                session_ops::reorder_workspaces_with_mode(
                    &mut window.tab_manager,
                    *workspace_index as i64,
                    *destination_index as i64,
                    false,
                )
            }),
        WorkspaceActionMutation::MoveTop {
            window_index,
            workspace_index,
        } => snapshot
            .windows
            .get_mut(*window_index)
            .is_some_and(|window| {
                session_ops::move_workspace_to_top(&mut window.tab_manager, *workspace_index as i64)
            }),
        WorkspaceActionMutation::Close {
            window_index,
            workspace_indices,
        } => snapshot
            .windows
            .get_mut(*window_index)
            .is_some_and(|window| {
                session_ops::close_workspaces(
                    &mut window.tab_manager,
                    &workspace_indices
                        .iter()
                        .map(|index| *index as i64)
                        .collect::<Vec<_>>(),
                )
            }),
        WorkspaceActionMutation::SetColor {
            window_index,
            workspace_index,
            color,
        } => snapshot
            .windows
            .get_mut(*window_index)
            .is_some_and(|window| {
                session_ops::set_workspace_color(
                    &mut window.tab_manager,
                    *workspace_index as i64,
                    color.as_deref(),
                )
            }),
    };
    if changed {
        let window_index = match mutation {
            WorkspaceActionMutation::SetPinned { window_index, .. }
            | WorkspaceActionMutation::Rename { window_index, .. }
            | WorkspaceActionMutation::SetDescription { window_index, .. }
            | WorkspaceActionMutation::Reorder { window_index, .. }
            | WorkspaceActionMutation::MoveTop { window_index, .. }
            | WorkspaceActionMutation::Close { window_index, .. }
            | WorkspaceActionMutation::SetColor { window_index, .. } => Some(*window_index),
            WorkspaceActionMutation::None | WorkspaceActionMutation::MarkUnread { .. } => None,
        };
        if let Some(window) = window_index.and_then(|index| snapshot.windows.get_mut(index)) {
            sync_selected_workspace_id(window);
        }
    }
    changed
}

#[cfg(test)]
pub(crate) fn plan_workspace_action(
    snapshot: &AppSessionSnapshot,
    params: &Map<String, Value>,
    palette_snapshot: &PaletteStoreSnapshot,
) -> WorkspaceActionPlan {
    plan_workspace_action_with_active_window(snapshot, params, palette_snapshot, None)
}

pub(crate) fn plan_workspace_action_with_active_window(
    snapshot: &AppSessionSnapshot,
    params: &Map<String, Value>,
    palette_snapshot: &PaletteStoreSnapshot,
    active_window_id: Option<&str>,
) -> WorkspaceActionPlan {
    let Some(window_index) = routed_window_index(snapshot, params, active_window_id) else {
        return error("unavailable", "TabManager not available", None);
    };
    let Some(action) = normalized_action(params) else {
        return error("invalid_params", "Missing action", None);
    };
    let Some(window) = snapshot.windows.get(window_index) else {
        return error("unavailable", "TabManager not available", None);
    };
    let Some(workspace_index) = target_workspace_index(window, params) else {
        return error("not_found", "Workspace not found", None);
    };
    let Some(workspace_id) = window.tab_manager.workspaces[workspace_index]
        .workspace_id
        .clone()
    else {
        return error("not_found", "Workspace not found", None);
    };

    let mutation = match action.as_str() {
        "pin" => WorkspaceActionMutation::SetPinned {
            window_index,
            workspace_index,
            pinned: true,
        },
        "unpin" => WorkspaceActionMutation::SetPinned {
            window_index,
            workspace_index,
            pinned: false,
        },
        "rename" => WorkspaceActionMutation::Rename {
            window_index,
            workspace_index,
            title: match required_string(params, "title") {
                Some(title) => title,
                None => return error("invalid_params", "Missing or invalid title", None),
            },
        },
        "clear_name" => WorkspaceActionMutation::Rename {
            window_index,
            workspace_index,
            title: String::new(),
        },
        "set_description" => WorkspaceActionMutation::SetDescription {
            window_index,
            workspace_index,
            description: match required_preserved_string(params, "description") {
                Some(description) => description,
                None => return error("invalid_params", "Missing or invalid description", None),
            },
        },
        "clear_description" => WorkspaceActionMutation::SetDescription {
            window_index,
            workspace_index,
            description: String::new(),
        },
        "move_up" => WorkspaceActionMutation::Reorder {
            window_index,
            workspace_index,
            destination_index: workspace_index.saturating_sub(1),
        },
        "move_down" => WorkspaceActionMutation::Reorder {
            window_index,
            workspace_index,
            destination_index: (workspace_index + 1)
                .min(window.tab_manager.workspaces.len().saturating_sub(1)),
        },
        "move_top" => WorkspaceActionMutation::MoveTop {
            window_index,
            workspace_index,
        },
        "close_others" | "close_above" | "close_below" => {
            let indices = window
                .tab_manager
                .workspaces
                .iter()
                .enumerate()
                .filter(|(index, workspace)| {
                    *index != workspace_index
                        && workspace.is_pinned != Some(true)
                        && match action.as_str() {
                            "close_above" => *index < workspace_index,
                            "close_below" => *index > workspace_index,
                            _ => true,
                        }
                })
                .map(|(index, _)| index)
                .collect();
            WorkspaceActionMutation::Close {
                window_index,
                workspace_indices: indices,
            }
        }
        "mark_read" => WorkspaceActionMutation::MarkUnread {
            workspace_id: workspace_id.clone(),
            unread: false,
        },
        "mark_unread" => WorkspaceActionMutation::MarkUnread {
            workspace_id: workspace_id.clone(),
            unread: true,
        },
        "set_color" => {
            let raw = match required_string(params, "color") {
                Some(color) => color,
                None => return error("invalid_params", "Missing or invalid color", None),
            };
            let Some(color) = resolved_color_hex(&raw, palette_snapshot) else {
                return error(
                    "invalid_params",
                    "Invalid color. Use a hex value (#RRGGBB) or a named color.",
                    Some(json!({
                        "named_colors": palette(palette_snapshot)
                            .into_iter()
                            .map(|entry| entry.name)
                            .collect::<Vec<_>>()
                    })),
                );
            };
            WorkspaceActionMutation::SetColor {
                window_index,
                workspace_index,
                color: Some(color),
            }
        }
        "clear_color" => WorkspaceActionMutation::SetColor {
            window_index,
            workspace_index,
            color: None,
        },
        _ => {
            return error(
                "invalid_params",
                "Unknown workspace action",
                Some(json!({
                    "action": action,
                    "supported_actions": SUPPORTED_WORKSPACE_ACTIONS,
                })),
            )
        }
    };

    let mut candidate = snapshot.clone();
    let before_count = candidate.windows[window_index].tab_manager.workspaces.len();
    apply_workspace_action_mutation(&mut candidate, &mutation);
    let after_count = candidate.windows[window_index].tab_manager.workspaces.len();
    let target = candidate.windows[window_index]
        .tab_manager
        .workspaces
        .iter()
        .find(|workspace| workspace.workspace_id.as_deref() == Some(workspace_id.as_str()))
        .expect("workspace action never removes its target");
    let target_index = candidate.windows[window_index]
        .tab_manager
        .workspaces
        .iter()
        .position(|workspace| workspace.workspace_id.as_deref() == Some(workspace_id.as_str()))
        .expect("target index exists");
    let mut payload = json!({
        "action": action,
        "workspace_id": workspace_id,
        "workspace_ref": format!("workspace:{}", target_index + 1),
        "window_id": candidate.windows[window_index].window_id,
        "window_ref": candidate.windows[window_index]
            .window_id
            .as_ref()
            .map(|_| format!("window:{}", window_index + 1)),
    });
    match (&mutation, action.as_str()) {
        (WorkspaceActionMutation::SetPinned { pinned, .. }, _) => payload["pinned"] = json!(pinned),
        (WorkspaceActionMutation::Rename { title, .. }, "rename") => {
            payload["title"] = json!(title)
        }
        (WorkspaceActionMutation::Rename { .. }, "clear_name") => {
            payload["title"] = json!(workspace_display_name(target))
        }
        (WorkspaceActionMutation::SetDescription { .. }, _) => {
            payload["description"] = json!(target.custom_description)
        }
        (WorkspaceActionMutation::Reorder { .. } | WorkspaceActionMutation::MoveTop { .. }, _) => {
            payload["index"] = json!(target_index)
        }
        (WorkspaceActionMutation::Close { .. }, _) => {
            payload["closed"] = json!(before_count.saturating_sub(after_count))
        }
        (WorkspaceActionMutation::SetColor { .. }, _) => {
            payload["color"] = json!(target.custom_color)
        }
        _ => {}
    }
    WorkspaceActionPlan {
        result: ok(payload),
        mutation,
        window_index: Some(window_index),
        workspace_id: Some(workspace_id),
    }
}
