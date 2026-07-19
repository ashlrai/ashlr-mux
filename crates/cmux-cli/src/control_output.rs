//! Plain-text and JSON presentation for control-socket results.

use std::path::{Path, PathBuf};

pub(super) fn format_control_result(method: &str, result: &serde_json::Value) -> String {
    match method {
        "workspace.set_progress"
        | "workspace.clear_progress"
        | "workspace.set_status"
        | "workspace.clear_status"
        | "workspace.set_agent_pid"
        | "workspace.clear_agent_pid"
        | "workspace.set_agent_lifecycle"
        | "agent_hibernation"
        | "workspace.report_pr"
        | "workspace.report_review"
        | "workspace.clear_pr"
        | "workspace.report_meta"
        | "workspace.clear_meta"
        | "workspace.report_meta_block"
        | "workspace.clear_meta_block"
        | "workspace.reset_sidebar"
        | "workspace.log"
        | "workspace.clear_log" => "OK".to_string(),
        "surface.report_tty" | "surface.report_shell_state" => "OK".to_string(),
        "surface.read_text" => result
            .get("text")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_string(),
        "browser.url.get" => result
            .get("url")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_string(),
        "browser.is_webview_focused" => result
            .get("focused")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false)
            .to_string(),
        "browser.focus_webview"
        | "browser.reload"
        | "config.reload"
        | "notification.dismiss"
        | "notification.mark_read"
        | "notification.clear"
        | "notification.create"
        | "session.restore_previous"
        | "surface.clear_history"
        | "surface.refresh_all"
        | "surface.trigger_flash"
        | "pane.swap"
        | "pane.break"
        | "pane.join" => "OK".to_string(),
        "window.list" => format_window_entries(result),
        "workspace.list" => format_workspace_entries(result),
        "workspace.current" => control_handle(result, "workspace").to_string(),
        "workspace.create" | "workspace.close" | "workspace.select" | "workspace.rename" => {
            format!("OK {}", control_handle(result, "workspace"))
        }
        "pane.list" => format_pane_entries(result),
        "pane.surfaces" => format_pane_surface_entries(result),
        "window.displays" => format_display_entries(result),
        "window.display" => format_window_display_result(result),
        "window.current" => result
            .get("window_id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_string(),
        "notification.list" => format_notification_entries(result),
        "notification.open" | "notification.jump_to_unread" => {
            format_notification_navigation(result)
        }
        "right_sidebar" => {
            if result.get("mode").is_some() {
                serde_json::to_string(result).unwrap_or_default()
            } else {
                "OK".to_string()
            }
        }
        "workspace.list_status" => format_status_entries(result),
        "workspace.list_meta" => format_metadata_entries(result),
        "workspace.list_meta_blocks" => format_metadata_blocks(result),
        "workspace.list_log" => format_log_entries(result),
        "workspace.sidebar_state" => format_sidebar_state(result),
        "workspace.reorder" => format_workspace_reorder(result),
        "workspace.reorder_many" => format_workspace_reorder_items(result),
        "workspace.move_to_window" => format!(
            "OK {} {}",
            control_handle(result, "workspace"),
            control_handle(result, "window"),
        ),
        "surface.reorder" => format!(
            "OK surface={} pane={} workspace={}",
            control_handle(result, "surface"),
            control_handle(result, "pane"),
            control_handle(result, "workspace"),
        ),
        "surface.move" => format!(
            "OK surface={} pane={} workspace={} window={}",
            control_handle(result, "surface"),
            control_handle(result, "pane"),
            control_handle(result, "workspace"),
            control_handle(result, "window"),
        ),
        "surface.split_off" | "surface.drag_to_split" => format!(
            "OK surface={} pane={} workspace={} window={}",
            control_handle(result, "surface"),
            control_handle(result, "pane"),
            control_handle(result, "workspace"),
            control_handle(result, "window"),
        ),
        "pane.last" => format!("OK {}", control_handle(result, "pane")),
        "pane.focus" => format!("OK {}", control_handle(result, "pane")),
        "workspace.last" | "workspace.next" | "workspace.previous" => {
            format!("OK {}", control_handle(result, "workspace"))
        }
        "pane.resize" => format!("OK {}", control_handle(result, "pane")),
        _ => serde_json::to_string(result).unwrap_or_default(),
    }
}

pub(super) fn format_lifecycle_text(
    method: &str,
    result: &serde_json::Value,
    id_format: &str,
    requested_action: Option<&str>,
) -> String {
    if method == "surface.respawn" {
        return "OK".to_string();
    }
    let mut fields = vec![format!(
        "action={}",
        requested_action
            .or_else(|| result.get("action").and_then(serde_json::Value::as_str))
            .unwrap_or_default()
    )];
    push_lifecycle_id_alias_field(
        &mut fields,
        result,
        "tab",
        ["tab_id", "surface_id"],
        ["tab_ref", "surface_ref"],
        id_format,
    );
    push_lifecycle_id_field(
        &mut fields,
        result,
        "workspace",
        "workspace_id",
        "workspace_ref",
        id_format,
    );
    for key in ["closed", "full_width_tab_mode"] {
        if let Some(value) = result.get(key) {
            fields.push(format!("{key}={value}"));
        }
    }
    push_lifecycle_id_alias_field(
        &mut fields,
        result,
        "created",
        ["created_tab_id", "created_surface_id"],
        ["created_tab_ref", "created_surface_ref"],
        id_format,
    );
    push_lifecycle_id_field(
        &mut fields,
        result,
        "created_workspace",
        "created_workspace_id",
        "created_workspace_ref",
        id_format,
    );
    format!("OK {}", fields.join(" "))
}

pub(super) fn format_workspace_action_text(
    result: &serde_json::Value,
    id_format: &str,
    requested_action: Option<&str>,
) -> String {
    let mut fields = vec![format!(
        "action={}",
        requested_action
            .or_else(|| result.get("action").and_then(serde_json::Value::as_str))
            .unwrap_or_default()
    )];
    push_lifecycle_id_field(
        &mut fields,
        result,
        "workspace",
        "workspace_id",
        "workspace_ref",
        id_format,
    );
    push_lifecycle_id_field(
        &mut fields,
        result,
        "window",
        "window_id",
        "window_ref",
        id_format,
    );
    for key in ["closed", "index"] {
        if let Some(value) = result.get(key) {
            let rendered = value
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| value.to_string());
            fields.push(format!("{key}={rendered}"));
        }
    }
    if let Some(color) = result.get("color").and_then(serde_json::Value::as_str) {
        fields.push(format!("color={color}"));
    }
    format!("OK {}", fields.join(" "))
}

fn push_lifecycle_id_alias_field(
    fields: &mut Vec<String>,
    result: &serde_json::Value,
    label: &str,
    id_keys: [&str; 2],
    ref_keys: [&str; 2],
    id_format: &str,
) {
    let id = id_keys
        .iter()
        .find_map(|key| result.get(*key).and_then(serde_json::Value::as_str));
    let reference = ref_keys
        .iter()
        .find_map(|key| result.get(*key).and_then(serde_json::Value::as_str));
    let Some(mut handle) = format_id_pair(id, reference, id_format) else {
        return;
    };
    if matches!(label, "tab" | "created")
        && handle
            .get(.."surface:".len())
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("surface:"))
    {
        handle = format!("tab:{}", &handle["surface:".len()..]);
    }
    fields.push(format!("{label}={handle}"));
}

fn push_lifecycle_id_field(
    fields: &mut Vec<String>,
    result: &serde_json::Value,
    label: &str,
    id_key: &str,
    ref_key: &str,
    id_format: &str,
) {
    push_lifecycle_id_alias_field(
        fields,
        result,
        label,
        [id_key, id_key],
        [ref_key, ref_key],
        id_format,
    );
}

pub(super) fn format_workspace_entries(result: &serde_json::Value) -> String {
    format_workspace_entries_with_mode(result, "refs")
}

pub(super) fn format_workspace_group_text(
    method: &str,
    result: &serde_json::Value,
    id_format: &str,
) -> String {
    match method {
        "workspace.group.list" => {
            let groups = result
                .get("groups")
                .and_then(serde_json::Value::as_array)
                .map(Vec::as_slice)
                .unwrap_or_default();
            if groups.is_empty() {
                return "No groups".to_string();
            }
            groups
                .iter()
                .map(|group| {
                    let handle = workspace_group_text_handle(
                        group.get("id").and_then(serde_json::Value::as_str),
                        group.get("ref").and_then(serde_json::Value::as_str),
                        id_format,
                    )
                    .unwrap_or_else(|| "unknown".to_string());
                    let name = group
                        .get("name")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or_default();
                    let count = group
                        .get("member_count")
                        .and_then(serde_json::Value::as_i64)
                        .unwrap_or_default();
                    let pinned = if group.get("is_pinned").and_then(serde_json::Value::as_bool)
                        == Some(true)
                    {
                        " [pinned]"
                    } else {
                        ""
                    };
                    let collapsed = if group
                        .get("is_collapsed")
                        .and_then(serde_json::Value::as_bool)
                        == Some(true)
                    {
                        " [collapsed]"
                    } else {
                        ""
                    };
                    format!("{handle}  {name}  ({count} members){pinned}{collapsed}")
                })
                .collect::<Vec<_>>()
                .join("\n")
        }
        "workspace.group.create" => result
            .get("group")
            .and_then(|group| {
                workspace_group_text_handle(
                    group.get("id").and_then(serde_json::Value::as_str),
                    group.get("ref").and_then(serde_json::Value::as_str),
                    id_format,
                )
            })
            .map(|handle| format!("OK {handle}"))
            .unwrap_or_else(|| "OK".to_string()),
        "workspace.group.new_workspace" => result
            .get("workspace_ref")
            .and_then(serde_json::Value::as_str)
            .map(|reference| format!("OK {reference}"))
            .unwrap_or_else(|| "OK".to_string()),
        _ => "OK".to_string(),
    }
}

fn workspace_group_text_handle(
    id: Option<&str>,
    reference: Option<&str>,
    id_format: &str,
) -> Option<String> {
    match id_format {
        "uuids" => id.or(reference).map(str::to_owned),
        "both" => match (reference, id) {
            (Some(reference), Some(id)) => Some(format!("{reference} {id}")),
            _ => reference.or(id).map(str::to_owned),
        },
        _ => reference.or(id).map(str::to_owned),
    }
}

fn format_workspace_entries_with_mode(result: &serde_json::Value, id_format: &str) -> String {
    let Some(workspaces) = result
        .get("workspaces")
        .and_then(serde_json::Value::as_array)
    else {
        return "No workspaces".to_string();
    };
    if workspaces.is_empty() {
        return "No workspaces".to_string();
    }
    workspaces
        .iter()
        .map(|workspace| {
            let selected = workspace
                .get("selected")
                .and_then(serde_json::Value::as_bool)
                == Some(true);
            let prefix = if selected { "* " } else { "  " };
            let mut line = format!("{prefix}{}", workspace_row_handle(workspace, id_format));
            if let Some(title) = workspace
                .get("title")
                .and_then(serde_json::Value::as_str)
                .filter(|title| !title.is_empty())
            {
                line.push_str("  ");
                line.push_str(title);
            }
            if let Some(remote) = workspace
                .get("remote")
                .and_then(serde_json::Value::as_object)
                .filter(|remote| {
                    remote.get("enabled").and_then(serde_json::Value::as_bool) == Some(true)
                })
            {
                let transport = remote
                    .get("transport")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("remote");
                let state = remote
                    .get("state")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("unknown");
                line.push_str(&format!("  [{transport}:{state}]"));
            }
            if selected {
                line.push_str("  [selected]");
            }
            line
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn workspace_row_handle(workspace: &serde_json::Value, id_format: &str) -> String {
    format_id_pair(
        workspace.get("id").and_then(serde_json::Value::as_str),
        workspace.get("ref").and_then(serde_json::Value::as_str),
        id_format,
    )
    .unwrap_or_else(|| "unknown".to_string())
}

pub(super) fn workspace_handle(result: &serde_json::Value, id_format: &str) -> String {
    format_id_pair(
        result
            .get("workspace_id")
            .and_then(serde_json::Value::as_str),
        result
            .get("workspace_ref")
            .and_then(serde_json::Value::as_str),
        id_format,
    )
    .unwrap_or_else(|| "unknown".to_string())
}

fn format_id_pair(id: Option<&str>, reference: Option<&str>, id_format: &str) -> Option<String> {
    match id_format {
        "uuids" => id.or(reference).map(str::to_owned),
        "both" => match (reference, id) {
            (Some(reference), Some(id)) => Some(format!("{reference} ({id})")),
            (Some(reference), None) => Some(reference.to_string()),
            (None, Some(id)) => Some(id.to_string()),
            (None, None) => None,
        },
        _ => reference.or(id).map(str::to_owned),
    }
}

pub(super) fn format_legacy_workspace_text(
    method: &str,
    result: &serde_json::Value,
    id_format: &str,
) -> String {
    match method {
        "workspace.list" => format_workspace_entries_with_mode(result, id_format),
        "workspace.current" => workspace_handle(result, id_format),
        "workspace.create" | "workspace.close" | "workspace.select" | "workspace.rename" => {
            format!("OK {}", workspace_handle(result, id_format))
        }
        _ => format_control_result(method, result),
    }
}

pub(super) fn prune_tmux_compat_workspace_value(
    store: &mut serde_json::Value,
    workspace_id: &str,
) -> bool {
    let mut changed = false;
    for key in ["mainVerticalLayouts", "lastSplitSurface"] {
        if let Some(map) = store
            .get_mut(key)
            .and_then(serde_json::Value::as_object_mut)
        {
            changed |= map.remove(workspace_id).is_some();
        }
    }
    changed
}

pub(super) fn prune_tmux_compat_workspace_state(workspace_id: &str) -> Result<(), std::io::Error> {
    let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) else {
        return Ok(());
    };
    let path = PathBuf::from(home)
        .join(".cmuxterm")
        .join("tmux-compat-store.json");
    prune_tmux_compat_workspace_state_at(&path, workspace_id)
}

pub(super) fn prune_tmux_compat_workspace_state_at(
    path: &Path,
    workspace_id: &str,
) -> Result<(), std::io::Error> {
    let Ok(contents) = std::fs::read_to_string(path) else {
        return Ok(());
    };
    let Ok(mut store) = serde_json::from_str::<serde_json::Value>(&contents) else {
        return Ok(());
    };
    if prune_tmux_compat_workspace_value(&mut store, workspace_id) {
        std::fs::write(
            path,
            serde_json::to_vec(&store).map_err(std::io::Error::other)?,
        )?;
    }
    Ok(())
}

fn format_workspace_reorder(result: &serde_json::Value) -> String {
    if result.get("dry_run").and_then(serde_json::Value::as_bool) == Some(true) {
        return format_workspace_reorder_items(result);
    }
    format!(
        "OK workspace={} window={} index={}",
        control_handle(result, "workspace"),
        control_handle(result, "window"),
        control_index(result),
    )
}

fn format_workspace_reorder_items(result: &serde_json::Value) -> String {
    let prefix = if result
        .get("dry_run")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
    {
        "OK plan"
    } else {
        "OK"
    };
    let plan = result.get("plan").and_then(serde_json::Value::as_array);
    let items: Vec<&serde_json::Value> = plan
        .map(|items| items.iter().collect())
        .unwrap_or_else(|| vec![result]);
    items
        .into_iter()
        .map(|item| {
            format!(
                "{prefix} workspace={} window={} index={}",
                control_handle(item, "workspace"),
                control_handle(item, "window"),
                control_index(item),
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn control_handle<'a>(result: &'a serde_json::Value, kind: &str) -> &'a str {
    result
        .get(format!("{kind}_ref"))
        .and_then(serde_json::Value::as_str)
        .or_else(|| {
            result
                .get(format!("{kind}_id"))
                .and_then(serde_json::Value::as_str)
        })
        .unwrap_or("unknown")
}

fn control_index(result: &serde_json::Value) -> String {
    result
        .get("to_index")
        .or_else(|| result.get("index"))
        .map(|value| match value {
            serde_json::Value::String(value) => value.clone(),
            value => value.to_string(),
        })
        .unwrap_or_else(|| "?".to_string())
}

fn entry_handle(entry: &serde_json::Value) -> &str {
    entry
        .get("ref")
        .and_then(serde_json::Value::as_str)
        .or_else(|| entry.get("id").and_then(serde_json::Value::as_str))
        .unwrap_or("unknown")
}

fn format_pane_entries(result: &serde_json::Value) -> String {
    let Some(panes) = result
        .get("panes")
        .and_then(serde_json::Value::as_array)
        .filter(|panes| !panes.is_empty())
    else {
        return "No panes".to_string();
    };
    panes
        .iter()
        .map(|pane| {
            let focused = pane.get("focused").and_then(serde_json::Value::as_bool) == Some(true);
            let count = pane
                .get("surface_count")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0);
            format!(
                "{}{}  [{} surface{}]{}",
                if focused { "* " } else { "  " },
                entry_handle(pane),
                count,
                if count == 1 { "" } else { "s" },
                if focused { "  [focused]" } else { "" },
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn format_pane_surface_entries(result: &serde_json::Value) -> String {
    let Some(surfaces) = result
        .get("surfaces")
        .and_then(serde_json::Value::as_array)
        .filter(|surfaces| !surfaces.is_empty())
    else {
        return "No surfaces in pane".to_string();
    };
    surfaces
        .iter()
        .map(|surface| {
            let selected =
                surface.get("selected").and_then(serde_json::Value::as_bool) == Some(true);
            let title = surface
                .get("title")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            format!(
                "{}{}  {}{}",
                if selected { "* " } else { "  " },
                entry_handle(surface),
                title,
                if selected { "  [selected]" } else { "" },
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn format_display_entries(result: &serde_json::Value) -> String {
    let displays = result.get("displays").and_then(serde_json::Value::as_array);
    let Some(displays) = displays.filter(|displays| !displays.is_empty()) else {
        return "No displays found.".into();
    };
    displays
        .iter()
        .map(|display| {
            let index = display
                .get("index")
                .and_then(serde_json::Value::as_i64)
                .unwrap_or(-1);
            let name = display
                .get("name")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("(unknown)");
            let main = if display.get("main").and_then(serde_json::Value::as_bool) == Some(true) {
                "  (main)"
            } else {
                ""
            };
            format!("{index}: {name}{main}")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub(super) fn format_window_display_result(result: &serde_json::Value) -> String {
    let display = result
        .get("display")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    let moved = result
        .get("moved")
        .and_then(serde_json::Value::as_array)
        .map_or(0, Vec::len);
    let suffix = if moved == 1 { "" } else { "s" };
    format!("Moved {moved} window{suffix} to {display}.")
}

fn format_window_entries(result: &serde_json::Value) -> String {
    let windows = result
        .as_array()
        .or_else(|| result.get("windows").and_then(serde_json::Value::as_array));
    let Some(windows) = windows else {
        return "No windows".to_string();
    };
    if windows.is_empty() {
        return "No windows".to_string();
    }
    windows
        .iter()
        .map(|window| {
            let selected = if window
                .get("key")
                .or_else(|| window.get("selected"))
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false)
            {
                "*"
            } else {
                " "
            };
            let index = window
                .get("index")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or_default();
            let id = string_field(window, "id");
            let selected_workspace = window
                .get("selected_workspace_id")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("none");
            let workspace_count = window
                .get("workspace_count")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or_default();
            format!(
                "{selected} {index}: {id} selected_workspace={selected_workspace} workspaces={workspace_count}"
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn format_notification_entries(result: &serde_json::Value) -> String {
    let Some(rows) = result.as_array() else {
        return "No notifications".to_string();
    };
    if rows.is_empty() {
        return "No notifications".to_string();
    }
    rows.iter()
        .enumerate()
        .map(|(index, row)| {
            let surface = row
                .get("surface_id")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("none");
            let read = if row
                .get("is_read")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false)
            {
                "read"
            } else {
                "unread"
            };
            let created_at = row
                .get("created_at")
                .map(|value| {
                    value
                        .as_str()
                        .map(str::to_owned)
                        .unwrap_or_else(|| value.to_string())
                })
                .unwrap_or_default();
            format!(
                "{index}:{}|{}|{surface}|{read}|{}|{}|{}|{created_at}|{}",
                string_field(row, "id"),
                string_field(row, "workspace_id"),
                string_field(row, "title"),
                string_field(row, "subtitle"),
                string_field(row, "body"),
                string_field(row, "tab_title")
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn format_notification_navigation(result: &serde_json::Value) -> String {
    if result.get("opened").and_then(serde_json::Value::as_bool) != Some(true) {
        return "OK".to_string();
    }
    let mut parts = vec!["OK".to_string()];
    for (reference, id) in [
        ("workspace_ref", "workspace_id"),
        ("surface_ref", "surface_id"),
    ] {
        if let Some(handle) = result
            .get(reference)
            .or_else(|| result.get(id))
            .and_then(serde_json::Value::as_str)
        {
            parts.push(handle.to_string());
        }
    }
    parts.join(" ")
}

fn format_status_entries(result: &serde_json::Value) -> String {
    result
        .get("status_entries")
        .and_then(serde_json::Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .map(|entry| {
                    let mut line = format!(
                        "{}={}",
                        string_field(entry, "key"),
                        string_field(entry, "value")
                    );
                    append_i64_field(&mut line, entry, "priority");
                    line
                })
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}

fn format_metadata_entries(result: &serde_json::Value) -> String {
    result
        .get("metadata_entries")
        .and_then(serde_json::Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .map(|entry| {
                    let mut line = format!(
                        "{}={}",
                        string_field(entry, "key"),
                        string_field(entry, "value")
                    );
                    append_string_field(&mut line, entry, "icon");
                    append_string_field(&mut line, entry, "color");
                    append_string_field(&mut line, entry, "url");
                    append_i64_field(&mut line, entry, "priority");
                    append_string_field(&mut line, entry, "format");
                    line
                })
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}

fn format_metadata_blocks(result: &serde_json::Value) -> String {
    result
        .get("metadata_blocks")
        .and_then(serde_json::Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .map(|entry| {
                    let mut line = format!(
                        "{}={}",
                        string_field(entry, "key"),
                        string_field(entry, "markdown")
                    );
                    append_i64_field(&mut line, entry, "priority");
                    line
                })
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}

fn format_log_entries(result: &serde_json::Value) -> String {
    result
        .get("log_entries")
        .and_then(serde_json::Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .map(|entry| {
                    format!(
                        "[{}] {}",
                        string_field(entry, "level"),
                        string_field(entry, "message")
                    )
                })
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}

fn format_sidebar_state(result: &serde_json::Value) -> String {
    let mut lines = Vec::new();
    if let Some(workspace_id) = result
        .get("workspace_id")
        .and_then(serde_json::Value::as_str)
    {
        lines.push(format!("workspace_id={workspace_id}"));
    }
    if let Some(workspace_ref) = result
        .get("workspace_ref")
        .and_then(serde_json::Value::as_str)
    {
        lines.push(format!("workspace_ref={workspace_ref}"));
    }
    lines.push(format!("ports={}", format_ports(result.get("ports"))));
    lines.push(format!(
        "agent_pid_count={}",
        usize_field(result, "agent_pid_count")
    ));
    lines.push(format!(
        "status_count={}",
        usize_field(result, "status_count")
    ));
    lines.push(format!(
        "metadata_count={}",
        usize_field(result, "metadata_count")
    ));
    lines.push(format!(
        "metadata_block_count={}",
        usize_field(result, "metadata_block_count")
    ));
    lines.push(format!("log_count={}", usize_field(result, "log_count")));
    lines.push(format!(
        "progress={}",
        format_progress(result.get("progress"))
    ));

    let status_lines = format_status_entries(result);
    if !status_lines.is_empty() {
        lines.push(status_lines);
    }
    let metadata_lines = format_metadata_entries(result);
    if !metadata_lines.is_empty() {
        lines.push(metadata_lines);
    }
    let block_lines = format_metadata_blocks(result);
    if !block_lines.is_empty() {
        lines.push(block_lines);
    }
    let log_lines = format_log_entries(result);
    if !log_lines.is_empty() {
        lines.push(log_lines);
    }
    lines.join("\n")
}

fn format_ports(ports: Option<&serde_json::Value>) -> String {
    let Some(values) = ports.and_then(serde_json::Value::as_array) else {
        return "none".to_string();
    };
    let ports: Vec<String> = values
        .iter()
        .filter_map(serde_json::Value::as_u64)
        .map(|value| value.to_string())
        .collect();
    if ports.is_empty() {
        "none".to_string()
    } else {
        ports.join(",")
    }
}

fn format_progress(progress: Option<&serde_json::Value>) -> String {
    let Some(progress) = progress else {
        return "none".to_string();
    };
    if progress.is_null() {
        return "none".to_string();
    }
    let value = progress
        .get("value")
        .and_then(serde_json::Value::as_f64)
        .unwrap_or(0.0)
        .clamp(0.0, 1.0);
    let label = progress
        .get("label")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    match label {
        Some(label) => format!("{value:.2} {label}"),
        None => format!("{value:.2}"),
    }
}

fn string_field<'a>(entry: &'a serde_json::Value, key: &str) -> &'a str {
    entry
        .get(key)
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
}

fn usize_field(entry: &serde_json::Value, key: &str) -> usize {
    entry
        .get(key)
        .and_then(serde_json::Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .unwrap_or(0)
}

fn append_string_field(line: &mut String, entry: &serde_json::Value, key: &str) {
    if let Some(value) = entry
        .get(key)
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        line.push_str(&format!(" {key}={value}"));
    }
}

fn append_i64_field(line: &mut String, entry: &serde_json::Value, key: &str) {
    if let Some(value) = entry.get(key).and_then(serde_json::Value::as_i64) {
        line.push_str(&format!(" {key}={value}"));
    }
}
