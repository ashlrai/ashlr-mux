//! Native window-title resolution for the desktop shell.
//!
//! This is the Windows/Tauri counterpart of `Sources/TabManager+WindowTitle.swift`:
//! resolve the selected workspace's display title, optionally apply the
//! configured `app.windowTitleTemplate`, and push the result into the native
//! window title bar.

use cmux_core::session::{
    AppSessionSnapshot, SessionTabManagerSnapshot, SessionWorkspaceGroupSnapshot,
    SessionWorkspaceSnapshot,
};
use cmux_window_title::{WindowTitleTemplate, WindowTitleTemplateContext};
use tauri::{AppHandle, Manager};
use uuid::Uuid;

/// Stable fallback UUID for `{windowId}` / `{windowToken}` placeholders until
/// the desktop session model carries real window UUIDs. Secondary windows
/// currently reuse the same token because the session snapshot still models a
/// single shared tab-manager tree.
const MAIN_WINDOW_UUID: &str = "00000000-0000-4000-8000-000000000001";

fn selected_workspace(tabs: &SessionTabManagerSnapshot) -> Option<&SessionWorkspaceSnapshot> {
    let workspaces = &tabs.workspaces;
    let raw_index = tabs.selected_workspace_index.unwrap_or(0);
    let index = if raw_index < 0 {
        0usize
    } else {
        raw_index as usize
    };
    if index < workspaces.len() {
        workspaces.get(index)
    } else {
        workspaces.first()
    }
}

fn resolved_workspace_display_title(
    workspace: &SessionWorkspaceSnapshot,
    groups: &[SessionWorkspaceGroupSnapshot],
) -> String {
    if let Some(group) = groups
        .iter()
        .find(|group| group.anchor_workspace_id.as_deref() == workspace.workspace_id.as_deref())
    {
        return group.name.clone();
    }
    workspace
        .custom_title
        .clone()
        .unwrap_or_else(|| workspace.process_title.clone())
}

fn default_window_title(
    workspace: Option<&SessionWorkspaceSnapshot>,
    groups: &[SessionWorkspaceGroupSnapshot],
) -> String {
    let Some(workspace) = workspace else {
        return "cmux".to_owned();
    };
    let trimmed_title = resolved_workspace_display_title(workspace, groups)
        .trim()
        .to_owned();
    if !trimmed_title.is_empty() {
        return trimmed_title;
    }
    let trimmed_directory = workspace.current_directory.as_deref().unwrap_or("").trim();
    if trimmed_directory.is_empty() {
        "cmux".to_owned()
    } else {
        trimmed_directory.to_owned()
    }
}

fn configured_window_title_template_raw() -> Option<String> {
    let path = cmux_config::config_path()?;
    let json = std::fs::read_to_string(path).ok()?;
    let config = cmux_config::decode_config(&json).ok()?;
    config
        .app
        .map(|app| app.window_title_template)
        .filter(|raw| !raw.trim().is_empty())
}

fn main_window_uuid() -> Uuid {
    Uuid::parse_str(MAIN_WINDOW_UUID).expect("static main-window uuid is valid")
}

fn current_tabs(snapshot: &AppSessionSnapshot) -> Option<&SessionTabManagerSnapshot> {
    snapshot.windows.first().map(|window| &window.tab_manager)
}

/// Resolve the native title for a desktop window from the current snapshot.
pub fn resolve_window_title(snapshot: &AppSessionSnapshot, template_raw: Option<&str>) -> String {
    let tabs = match current_tabs(snapshot) {
        Some(tabs) => tabs,
        None => return "cmux".to_owned(),
    };
    let groups = tabs.workspace_groups.as_deref().unwrap_or(&[]);
    let workspace = selected_workspace(tabs);
    let default_title = default_window_title(workspace, groups);
    let Some(raw) = template_raw else {
        return default_title;
    };
    let Some(template) = WindowTitleTemplate::configured_from_raw(raw) else {
        return default_title;
    };
    let workspace_title = workspace
        .map(|workspace| resolved_workspace_display_title(workspace, groups))
        .unwrap_or_default();
    let active_directory = workspace
        .and_then(|workspace| workspace.current_directory.clone())
        .unwrap_or_default();
    let resolved = template.resolved(&WindowTitleTemplateContext {
        default_title: default_title.clone(),
        active_workspace: if workspace_title.trim().is_empty() {
            default_title.clone()
        } else {
            workspace_title.trim().to_owned()
        },
        active_directory: active_directory.trim().to_owned(),
        window_id: main_window_uuid(),
        app_name: "cmux".to_owned(),
    });
    let trimmed = resolved.trim();
    if trimmed.is_empty() {
        default_title
    } else {
        trimmed.to_owned()
    }
}

/// Push the resolved title into every live desktop window.
pub fn refresh_window_titles(app: &AppHandle, snapshot: &AppSessionSnapshot) {
    let template = configured_window_title_template_raw();
    for (label, window) in app.webview_windows() {
        let projected = crate::session::snapshot_for_window(snapshot, &label);
        let title = resolve_window_title(&projected, template.as_deref());
        let _ = window.set_title(&title);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cmux_core::session::{
        SessionWindowSnapshot, SessionWorkspaceLayoutSnapshot, SESSION_SNAPSHOT_SCHEMA_VERSION,
    };

    fn workspace(
        id: &str,
        process_title: &str,
        custom_title: Option<&str>,
        current_directory: Option<&str>,
        group_id: Option<&str>,
    ) -> SessionWorkspaceSnapshot {
        SessionWorkspaceSnapshot {
            surfaces: None,
            pending_remote_pwds: None,
            workspace_id: Some(id.to_owned()),
            process_title: process_title.to_owned(),
            custom_title: custom_title.map(str::to_owned),
            custom_title_source: custom_title.map(|_| "user".to_owned()),
            custom_description: None,
            custom_color: None,
            current_directory: current_directory.map(str::to_owned),
            initial_terminal_command: None,
            initial_terminal_input: None,
            initial_terminal_environment: None,
            workspace_environment: None,
            layout: Some(SessionWorkspaceLayoutSnapshot::Pane(
                cmux_core::session::SessionPaneLayoutSnapshot {
                    pane_id: None,
                    panel_ids: vec!["surface-1".to_owned()],
                    selected_panel_id: Some("surface-1".to_owned()),
                    surface_kind: None,
                    markdown_file_path: None,
                    file_path: None,
                    diff_viewer_token: None,
                    diff_viewer_request_path: None,
                    browser_url: None,
                    browser_proxy_url: None,
                    browser_back_history: None,
                    browser_forward_history: None,
                    browser_omnibar_visible: None,
                    browser_focus_mode_active: None,
                    browser_developer_tools_visible: None,
                    browser_developer_tools_panel: None,
                    browser_page_zoom: None,
                },
            )),
            layout_mode: None,
            zoomed_panel_id: None,
            focused_panel_id: Some("surface-1".to_owned()),
            panel_titles: None,
            panel_pins: None,
            panel_unreads: None,
            restorable_agent_snapshots: None,
            git_branch: None,
            panel_git_branches: None,
            panel_pull_requests: None,
            remote: None,
            sidebar_progress: None,
            sidebar_status_entries: None,
            sidebar_metadata_entries: None,
            sidebar_metadata_blocks: None,
            sidebar_log_entries: None,
            listening_ports: None,
            agent_listening_ports: None,
            agent_pids: None,
            panel_listening_ports: None,
            panel_ttys: None,
            panel_shell_activity: None,
            panel_terminal_startups: None,
            canvas_panes: None,
            group_id: group_id.map(str::to_owned),
            is_pinned: None,
        }
    }

    fn snapshot(
        workspaces: Vec<SessionWorkspaceSnapshot>,
        selected_workspace_index: Option<i64>,
        groups: Option<Vec<SessionWorkspaceGroupSnapshot>>,
    ) -> AppSessionSnapshot {
        AppSessionSnapshot {
            version: SESSION_SNAPSHOT_SCHEMA_VERSION,
            created_at: 0,
            windows: vec![SessionWindowSnapshot {
                window_id: Some("window-1".to_owned()),
                selected_workspace_id: None,
                tab_manager: SessionTabManagerSnapshot {
                    selected_workspace_index,
                    workspaces,
                    workspace_groups: groups,
                },
            }],
        }
    }

    #[test]
    fn default_title_prefers_group_name_for_an_anchor_workspace() {
        let ws = workspace(
            "11111111-1111-1111-1111-111111111111",
            "shell",
            Some("Old anchor title"),
            Some("C:/repo"),
            Some("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa"),
        );
        let groups = vec![SessionWorkspaceGroupSnapshot {
            id: "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa".to_owned(),
            name: "Backend".to_owned(),
            anchor_workspace_id: ws.workspace_id.clone(),
            ..Default::default()
        }];
        let snapshot = snapshot(vec![ws], Some(0), Some(groups));
        assert_eq!(resolve_window_title(&snapshot, None), "Backend");
    }

    #[test]
    fn default_title_falls_back_to_current_directory_then_cmux() {
        let with_directory = snapshot(
            vec![workspace(
                "11111111-1111-1111-1111-111111111111",
                "   ",
                None,
                Some("C:/repo"),
                None,
            )],
            Some(0),
            None,
        );
        assert_eq!(resolve_window_title(&with_directory, None), "C:/repo");

        let empty = snapshot(
            vec![workspace(
                "11111111-1111-1111-1111-111111111111",
                "   ",
                None,
                Some("   "),
                None,
            )],
            Some(0),
            None,
        );
        assert_eq!(resolve_window_title(&empty, None), "cmux");
    }

    #[test]
    fn configured_template_resolves_workspace_directory_and_window_placeholders() {
        let snapshot = snapshot(
            vec![workspace(
                "11111111-1111-1111-1111-111111111111",
                "zsh",
                Some("Docs"),
                Some("C:/repo"),
                None,
            )],
            Some(0),
            None,
        );
        let title = resolve_window_title(
            &snapshot,
            Some("[cmux:{windowToken}] {activeWorkspace} {activeDirectory}"),
        );
        assert_eq!(title, "[cmux:00000000] Docs C:/repo");
    }

    #[test]
    fn blank_or_whitespace_resolved_template_falls_back_to_default_title() {
        let snapshot = snapshot(
            vec![workspace(
                "11111111-1111-1111-1111-111111111111",
                "zsh",
                Some("Workspace"),
                Some("C:/repo"),
                None,
            )],
            Some(0),
            None,
        );
        assert_eq!(resolve_window_title(&snapshot, Some("   ")), "Workspace");
        assert_eq!(
            resolve_window_title(&snapshot, Some("{unknown}")),
            "{unknown}"
        );
    }
}
