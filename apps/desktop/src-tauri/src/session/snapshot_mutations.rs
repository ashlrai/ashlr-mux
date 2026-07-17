use super::*;

/// Close `panel_id`, collapsing an emptied split into its sibling. Pure.
pub(super) fn apply_close(snapshot: &mut AppSessionSnapshot, panel_id: &str) -> CloseOutcome {
    let outcome = {
        let Some(slot) = active_layout_slot(snapshot) else {
            return CloseOutcome::NotFound;
        };
        session_ops::close_panel(slot, panel_id)
    };
    if matches!(outcome, CloseOutcome::Removed) {
        for window in &mut snapshot.windows {
            for workspace in &mut window.tab_manager.workspaces {
                prune_workspace_panel_listening_ports(workspace, panel_id);
                prune_workspace_panel_tty(workspace, panel_id);
                prune_workspace_panel_shell_activity(workspace, panel_id);
            }
        }
    };
    outcome
}

pub(super) fn restorable_browser_url(url: Option<&str>) -> Option<String> {
    let trimmed = url?.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case(DEFAULT_BROWSER_URL) {
        None
    } else {
        Some(trimmed.to_string())
    }
}

pub(super) fn closed_browser_tab(url: Option<&str>) -> Option<ClosedBrowserTabSnapshot> {
    restorable_browser_url(url).map(|url| ClosedBrowserTabSnapshot { url })
}

pub(super) fn closed_browser_tab_for_panel(
    layout: &SessionWorkspaceLayoutSnapshot,
    panel_id: &str,
) -> Option<ClosedBrowserTabSnapshot> {
    match layout {
        SessionWorkspaceLayoutSnapshot::Pane(pane) => {
            if pane.panel_ids.len() == 1
                && pane.panel_ids.iter().any(|id| id == panel_id)
                && pane.surface_kind.as_deref() == Some("browser")
            {
                closed_browser_tab(pane.browser_url.as_deref())
            } else {
                None
            }
        }
        SessionWorkspaceLayoutSnapshot::Split(split) => {
            closed_browser_tab_for_panel(&split.first, panel_id)
                .or_else(|| closed_browser_tab_for_panel(&split.second, panel_id))
        }
    }
}

pub(super) fn closed_browser_tabs_in_layout(
    layout: &SessionWorkspaceLayoutSnapshot,
    tabs: &mut Vec<ClosedBrowserTabSnapshot>,
) {
    match layout {
        SessionWorkspaceLayoutSnapshot::Pane(pane) => {
            if pane.surface_kind.as_deref() == Some("browser") {
                if let Some(tab) = closed_browser_tab(pane.browser_url.as_deref()) {
                    tabs.push(tab);
                }
            }
        }
        SessionWorkspaceLayoutSnapshot::Split(split) => {
            closed_browser_tabs_in_layout(&split.first, tabs);
            closed_browser_tabs_in_layout(&split.second, tabs);
        }
    }
}

pub(super) fn closed_browser_tab_for_active_panel(
    snapshot: &AppSessionSnapshot,
    panel_id: &str,
) -> Option<ClosedBrowserTabSnapshot> {
    let layout = snapshot
        .windows
        .first()?
        .tab_manager
        .workspaces
        .get(
            snapshot
                .windows
                .first()?
                .tab_manager
                .selected_workspace_index
                .unwrap_or(0)
                .max(0) as usize,
        )?
        .layout
        .as_ref()?;
    closed_browser_tab_for_panel(layout, panel_id)
}

pub(super) fn closed_browser_tabs_for_workspace(
    workspace: &SessionWorkspaceSnapshot,
) -> Vec<ClosedBrowserTabSnapshot> {
    let mut tabs = Vec::new();
    if let Some(layout) = workspace.layout.as_ref() {
        closed_browser_tabs_in_layout(layout, &mut tabs);
    }
    tabs
}

pub(super) fn closed_browser_tabs_for_workspace_index(
    snapshot: &AppSessionSnapshot,
    index: i64,
) -> Vec<ClosedBrowserTabSnapshot> {
    if index < 0 {
        return Vec::new();
    }
    snapshot
        .windows
        .first()
        .and_then(|window| window.tab_manager.workspaces.get(index as usize))
        .map(closed_browser_tabs_for_workspace)
        .unwrap_or_default()
}

pub(super) fn push_closed_browser_tabs(
    history: &mut Vec<ClosedBrowserTabSnapshot>,
    tabs: impl IntoIterator<Item = ClosedBrowserTabSnapshot>,
) {
    for tab in tabs {
        if history.last() == Some(&tab) {
            continue;
        }
        history.push(tab);
    }
    let overflow = history.len().saturating_sub(CLOSED_BROWSER_HISTORY_LIMIT);
    if overflow > 0 {
        history.drain(0..overflow);
    }
}

pub(super) fn apply_reopen_closed_browser_tab(
    snapshot: &mut AppSessionSnapshot,
    tab: &ClosedBrowserTabSnapshot,
    new_panel_id: &str,
) -> bool {
    apply_new_workspace(snapshot, new_panel_id, None, None, None, None);
    apply_open_browser_url(snapshot, new_panel_id, Some(&tab.url))
}

/// Move the divider of the split at `path`. Returns whether the path resolved to
/// a split. Pure.
pub(super) fn apply_set_divider(
    snapshot: &mut AppSessionSnapshot,
    path: &[SplitChild],
    position: f64,
) -> bool {
    let Some(slot) = active_layout_slot(snapshot) else {
        return false;
    };
    let Some(root) = slot.as_mut() else {
        return false;
    };
    session_ops::set_divider_at_path(root, path, position)
}

/// Equalize every split divider in the active workspace layout to its
/// orientation-aware span ratio. Returns whether any split was found (`false`
/// for absent/empty/single-pane layouts). Pure — mirrors `apply_set_divider`.
pub(super) fn apply_equalize_dividers(snapshot: &mut AppSessionSnapshot) -> bool {
    let Some(slot) = active_layout_slot(snapshot) else {
        return false;
    };
    let Some(root) = slot.as_mut() else {
        return false;
    };
    session_ops::equalize_dividers(root)
}

/// Toggle split zoom for the pane holding `panel_id` in the active workspace.
pub(super) fn apply_toggle_split_zoom(snapshot: &mut AppSessionSnapshot, panel_id: &str) -> bool {
    let Some(workspace) = active_workspace_slot(snapshot) else {
        return false;
    };
    session_ops::toggle_split_zoom(workspace, panel_id)
}

/// Set the active workspace's layout mode (`"canvas"` or split/default).
pub(super) fn apply_set_layout_mode(snapshot: &mut AppSessionSnapshot, mode: Option<&str>) -> bool {
    let Some(workspace) = active_workspace_slot(snapshot) else {
        return false;
    };
    session_ops::set_layout_mode(workspace, mode)
}

pub(super) fn apply_set_canvas_pane_frame(
    snapshot: &mut AppSessionSnapshot,
    panel_id: &str,
    x: i64,
    y: i64,
    width: i64,
    height: i64,
) -> bool {
    let Some(workspace) = active_workspace_slot(snapshot) else {
        return false;
    };
    session_ops::set_canvas_pane_frame(workspace, panel_id, x, y, width, height)
}

pub(super) fn apply_canvas_action(
    snapshot: &mut AppSessionSnapshot,
    action: &str,
    pane_gap: Option<i64>,
) -> bool {
    let Some(workspace) = active_workspace_slot(snapshot) else {
        return false;
    };
    session_ops::apply_canvas_action_with_gap(workspace, action, pane_gap)
}

/// Set the surface kind of the pane holding `panel_id` (`None` = terminal).
/// Returns whether a matching pane was found. Pure.
pub(super) fn apply_set_surface_kind(
    snapshot: &mut AppSessionSnapshot,
    panel_id: &str,
    kind: Option<String>,
) -> bool {
    let Some(slot) = active_layout_slot(snapshot) else {
        return false;
    };
    let Some(root) = slot.as_mut() else {
        return false;
    };
    session_ops::set_surface_kind(root, panel_id, kind)
}

/// Select the next/previous panel hosted by the pane that holds `panel_id`.
pub(super) fn apply_select_adjacent_panel(
    snapshot: &mut AppSessionSnapshot,
    panel_id: &str,
    next: bool,
) -> bool {
    let Some(slot) = active_layout_slot(snapshot) else {
        return false;
    };
    let Some(root) = slot.as_mut() else {
        return false;
    };
    session_ops::select_adjacent_panel(root, panel_id, next)
}

/// Select a workspace by id and select `panel_id` inside that workspace's pane.
pub(super) fn apply_select_workspace_surface(
    snapshot: &mut AppSessionSnapshot,
    workspace_id: &str,
    panel_id: &str,
) -> bool {
    let Some(window) = snapshot.windows.first_mut() else {
        return false;
    };
    let tab_manager = &mut window.tab_manager;
    let Some(index) = tab_manager
        .workspaces
        .iter()
        .position(|workspace| workspace.workspace_id.as_deref() == Some(workspace_id))
    else {
        return false;
    };
    let Some(layout) = tab_manager.workspaces[index].layout.as_mut() else {
        return false;
    };
    if !session_ops::contains_panel(layout, panel_id) {
        return false;
    }
    let selected_changed = session_ops::select_panel(layout, panel_id);
    let focus_changed = tab_manager.workspaces[index].focused_panel_id.as_deref() != Some(panel_id);
    tab_manager.workspaces[index].focused_panel_id = Some(panel_id.to_string());
    let index_i64 = index as i64;
    let workspace_changed = tab_manager.selected_workspace_index != Some(index_i64);
    tab_manager.selected_workspace_index = Some(index_i64);
    sync_window_selected_workspace_id(window);
    selected_changed || workspace_changed || focus_changed
}

pub(super) fn apply_focus_panel(snapshot: &mut AppSessionSnapshot, panel_id: &str) -> bool {
    for window in &mut snapshot.windows {
        for workspace in &mut window.tab_manager.workspaces {
            if workspace
                .layout
                .as_ref()
                .is_some_and(|layout| session_ops::contains_panel(layout, panel_id))
            {
                let changed = workspace.focused_panel_id.as_deref() != Some(panel_id);
                workspace.focused_panel_id = Some(panel_id.to_string());
                return changed;
            }
        }
    }
    false
}

/// Open `file_path` in the markdown surface of the pane holding `panel_id`.
/// This binds the pane-local markdown file path and flips the pane into the
/// `"markdown"` surface in one mutation.
pub(super) fn apply_open_markdown_file(
    snapshot: &mut AppSessionSnapshot,
    panel_id: &str,
    file_path: &str,
) -> bool {
    let trimmed = file_path.trim();
    if trimmed.is_empty() {
        return false;
    }
    let Some(slot) = active_layout_slot(snapshot) else {
        return false;
    };
    let Some(root) = slot.as_mut() else {
        return false;
    };
    let changed_kind = session_ops::set_surface_kind(root, panel_id, Some("markdown".to_string()));
    let changed_file =
        session_ops::set_markdown_file_path(root, panel_id, Some(trimmed.to_string()));
    changed_kind || changed_file
}

/// Open `file_path` in the plain-text file surface of the pane holding
/// `panel_id`. This binds the pane-local file path and flips the pane into the
/// `"file"` surface in one mutation.
pub(super) fn apply_open_file(
    snapshot: &mut AppSessionSnapshot,
    panel_id: &str,
    file_path: &str,
) -> bool {
    let trimmed = file_path.trim();
    if trimmed.is_empty() {
        return false;
    }
    let Some(slot) = active_layout_slot(snapshot) else {
        return false;
    };
    let Some(root) = slot.as_mut() else {
        return false;
    };
    let changed_kind = session_ops::set_surface_kind(root, panel_id, Some("file".to_string()));
    let changed_file = session_ops::set_file_path(root, panel_id, Some(trimmed.to_string()));
    changed_kind || changed_file
}

/// Open a custom sidebar source in the pane holding `panel_id`. This stores the
/// sidebar source path in the existing pane-local `file_path` slot and flips the
/// pane into the Windows/Tauri custom-sidebar renderer surface.
pub(super) fn apply_open_custom_sidebar(
    snapshot: &mut AppSessionSnapshot,
    panel_id: &str,
    file_path: &str,
) -> bool {
    let trimmed = file_path.trim();
    if trimmed.is_empty() {
        return false;
    }
    let Some(slot) = active_layout_slot(snapshot) else {
        return false;
    };
    let Some(root) = slot.as_mut() else {
        return false;
    };
    let changed_kind =
        session_ops::set_surface_kind(root, panel_id, Some("custom-sidebar".to_string()));
    let changed_file = session_ops::set_file_path(root, panel_id, Some(trimmed.to_string()));
    changed_kind || changed_file
}

pub(super) fn normalize_diff_request_path(request_path: &str) -> Option<String> {
    let trimmed = request_path.trim();
    let normalized = if trimmed.is_empty() {
        "/index.html".to_string()
    } else if trimmed.starts_with('/') {
        trimmed.to_string()
    } else {
        format!("/{trimmed}")
    };
    cmux_diff::DiffSessionRegistry::is_valid_request_path(&normalized).then_some(normalized)
}

/// Open a registered diff-viewer session in the pane holding `panel_id`.
/// Binds the pane-local token + request path and flips the pane into the
/// `"diff"` surface in one mutation.
pub(super) fn apply_open_diff_viewer(
    snapshot: &mut AppSessionSnapshot,
    panel_id: &str,
    token: &str,
    request_path: &str,
) -> bool {
    let trimmed_token = token.trim();
    let Some(normalized_request_path) = normalize_diff_request_path(request_path) else {
        return false;
    };
    if !cmux_diff::DiffSessionRegistry::is_valid_token(trimmed_token) {
        return false;
    }
    let Some(slot) = active_layout_slot(snapshot) else {
        return false;
    };
    let Some(root) = slot.as_mut() else {
        return false;
    };
    let changed_kind = session_ops::set_surface_kind(root, panel_id, Some("diff".to_string()));
    let changed_diff = session_ops::set_diff_viewer_session(
        root,
        panel_id,
        Some(trimmed_token.to_string()),
        Some(normalized_request_path),
    );
    changed_kind || changed_diff
}

pub(super) fn normalize_browser_url(url: Option<&str>) -> String {
    let trimmed = url.unwrap_or(DEFAULT_BROWSER_URL).trim();
    if trimmed.is_empty() {
        DEFAULT_BROWSER_URL.to_string()
    } else {
        trimmed.to_string()
    }
}

pub(super) fn normalize_browser_zoom(zoom: f64) -> f64 {
    if zoom.is_finite() {
        zoom.clamp(0.25, 3.0)
    } else {
        1.0
    }
}

pub(super) fn browser_proxy_url_for_local_port(port: u16) -> String {
    crate::remote_proxy::loopback_socks5_proxy_url(port)
}

pub(super) fn remote_browser_proxy_url(
    remote: Option<&SessionWorkspaceRemoteSnapshot>,
) -> Option<String> {
    let remote = remote?;
    remote
        .proxy
        .as_ref()
        .and_then(|proxy| proxy.url.clone())
        .or_else(|| {
            remote
                .local_proxy_port
                .map(browser_proxy_url_for_local_port)
        })
}

pub(super) fn set_layout_browser_proxy_url(
    layout: &mut SessionWorkspaceLayoutSnapshot,
    proxy_url: Option<&str>,
) -> bool {
    match layout {
        SessionWorkspaceLayoutSnapshot::Pane(pane) => {
            let next = proxy_url.map(str::to_string);
            if pane.browser_proxy_url == next {
                return false;
            }
            pane.browser_proxy_url = next;
            true
        }
        SessionWorkspaceLayoutSnapshot::Split(split) => {
            let first_changed = set_layout_browser_proxy_url(&mut split.first, proxy_url);
            let second_changed = set_layout_browser_proxy_url(&mut split.second, proxy_url);
            first_changed || second_changed
        }
    }
}

pub(super) fn set_layout_browser_proxy_url_for_panel(
    layout: &mut SessionWorkspaceLayoutSnapshot,
    panel_id: &str,
    proxy_url: Option<&str>,
) -> bool {
    match layout {
        SessionWorkspaceLayoutSnapshot::Pane(pane) => {
            if !pane.panel_ids.iter().any(|candidate| candidate == panel_id) {
                return false;
            }
            let next = proxy_url.map(str::to_string);
            if pane.browser_proxy_url == next {
                return false;
            }
            pane.browser_proxy_url = next;
            true
        }
        SessionWorkspaceLayoutSnapshot::Split(split) => {
            let first_changed =
                set_layout_browser_proxy_url_for_panel(&mut split.first, panel_id, proxy_url);
            let second_changed =
                set_layout_browser_proxy_url_for_panel(&mut split.second, panel_id, proxy_url);
            first_changed || second_changed
        }
    }
}

pub(super) fn set_browser_layout_proxy_url_for_browser_panes(
    layout: &mut SessionWorkspaceLayoutSnapshot,
    proxy_url: &str,
) -> bool {
    match layout {
        SessionWorkspaceLayoutSnapshot::Pane(pane) => {
            if pane.surface_kind.as_deref() != Some("browser") {
                return false;
            }
            let next = Some(proxy_url.to_string());
            if pane.browser_proxy_url == next {
                return false;
            }
            pane.browser_proxy_url = next;
            true
        }
        SessionWorkspaceLayoutSnapshot::Split(split) => {
            let first_changed =
                set_browser_layout_proxy_url_for_browser_panes(&mut split.first, proxy_url);
            let second_changed =
                set_browser_layout_proxy_url_for_browser_panes(&mut split.second, proxy_url);
            first_changed || second_changed
        }
    }
}

pub(super) fn apply_workspace_browser_proxy_url(
    workspace: &mut SessionWorkspaceSnapshot,
    proxy_url: Option<&str>,
) -> bool {
    let Some(layout) = workspace.layout.as_mut() else {
        return false;
    };
    match proxy_url {
        Some(proxy_url) => set_browser_layout_proxy_url_for_browser_panes(layout, proxy_url),
        None => set_layout_browser_proxy_url(layout, None),
    }
}

pub(super) fn configured_remote_snapshot(
    config: WorkspaceRemoteControlConfig,
) -> SessionWorkspaceRemoteSnapshot {
    let proxy_url = config
        .local_proxy_port
        .map(browser_proxy_url_for_local_port);
    let proxy_configured = proxy_url.is_some();
    let state = if config.auto_connect {
        "connecting"
    } else {
        "disconnected"
    };
    SessionWorkspaceRemoteSnapshot {
        enabled: true,
        state: state.to_string(),
        connected: state == "connected",
        transport: Some(config.transport),
        destination: Some(config.destination),
        port: config.port,
        local_proxy_port: config.local_proxy_port,
        persistent_daemon_slot: config.persistent_daemon_slot,
        has_ssh_options: !config.ssh_options.is_empty() || config.identity_file.is_some(),
        detail: None,
        daemon: Some(SessionWorkspaceRemoteDaemonSnapshot {
            state: if config.auto_connect {
                "bootstrapping"
            } else {
                "configured"
            }
            .to_string(),
            capabilities: vec!["proxy.stream.push".to_string()],
        }),
        proxy: Some(SessionWorkspaceRemoteProxySnapshot {
            state: if config.auto_connect {
                "connecting"
            } else if proxy_configured {
                "ready"
            } else {
                "unavailable"
            }
            .to_string(),
            host: proxy_configured.then(|| "127.0.0.1".to_string()),
            port: config.local_proxy_port,
            schemes: vec!["socks5".to_string(), "http_connect".to_string()],
            url: proxy_url,
            error_code: (!proxy_configured).then(|| "proxy_unavailable".to_string()),
        }),
        detected_ports: Vec::new(),
        forwarded_ports: Vec::new(),
        conflicted_ports: Vec::new(),
        active_terminal_sessions: Some(1),
    }
}

/// Open `url` in the browser surface of the pane holding `panel_id`, switching
/// that pane to the browser surface and binding the pane-local browser state.
pub(super) fn apply_open_browser_url(
    snapshot: &mut AppSessionSnapshot,
    panel_id: &str,
    url: Option<&str>,
) -> bool {
    let normalized = normalize_browser_url(url);
    let proxy_url = active_workspace(snapshot).and_then(|workspace| {
        remote_browser_proxy_url(workspace.remote.as_ref()).filter(|url| !url.is_empty())
    });
    let Some(slot) = active_layout_slot(snapshot) else {
        return false;
    };
    let Some(root) = slot.as_mut() else {
        return false;
    };
    let changed_kind = session_ops::set_surface_kind(root, panel_id, Some("browser".to_string()));
    let changed_url = session_ops::navigate_browser(root, panel_id, normalized);
    let changed_zoom = session_ops::ensure_browser_page_zoom(root, panel_id, 1.0);
    let changed_proxy = proxy_url
        .as_deref()
        .is_some_and(|url| set_layout_browser_proxy_url_for_panel(root, panel_id, Some(url)));
    changed_kind || changed_url || changed_zoom || changed_proxy
}

pub(super) fn apply_browser_go_back(snapshot: &mut AppSessionSnapshot, panel_id: &str) -> bool {
    let Some(slot) = active_layout_slot(snapshot) else {
        return false;
    };
    let Some(root) = slot.as_mut() else {
        return false;
    };
    session_ops::browser_go_back(root, panel_id)
}

pub(super) fn apply_browser_go_forward(snapshot: &mut AppSessionSnapshot, panel_id: &str) -> bool {
    let Some(slot) = active_layout_slot(snapshot) else {
        return false;
    };
    let Some(root) = slot.as_mut() else {
        return false;
    };
    session_ops::browser_go_forward(root, panel_id)
}

pub(super) fn apply_clear_browser_history(
    snapshot: &mut AppSessionSnapshot,
    panel_id: &str,
) -> bool {
    let Some(slot) = active_layout_slot(snapshot) else {
        return false;
    };
    let Some(root) = slot.as_mut() else {
        return false;
    };
    session_ops::clear_browser_history(root, panel_id)
}

pub(super) fn apply_toggle_browser_omnibar(
    snapshot: &mut AppSessionSnapshot,
    panel_id: &str,
) -> bool {
    let Some(slot) = active_layout_slot(snapshot) else {
        return false;
    };
    let Some(root) = slot.as_mut() else {
        return false;
    };
    session_ops::toggle_browser_omnibar_visible(root, panel_id)
}

pub(super) fn apply_toggle_browser_focus_mode(
    snapshot: &mut AppSessionSnapshot,
    panel_id: &str,
) -> bool {
    let Some(slot) = active_layout_slot(snapshot) else {
        return false;
    };
    let Some(root) = slot.as_mut() else {
        return false;
    };
    session_ops::toggle_browser_focus_mode(root, panel_id)
}

pub(super) fn apply_toggle_browser_developer_tools(
    snapshot: &mut AppSessionSnapshot,
    panel_id: &str,
) -> bool {
    let Some(slot) = active_layout_slot(snapshot) else {
        return false;
    };
    let Some(root) = slot.as_mut() else {
        return false;
    };
    session_ops::toggle_browser_developer_tools(root, panel_id)
}

pub(super) fn apply_show_browser_developer_tools(
    snapshot: &mut AppSessionSnapshot,
    panel_id: &str,
    panel: &str,
) -> bool {
    let Some(slot) = active_layout_slot(snapshot) else {
        return false;
    };
    let Some(root) = slot.as_mut() else {
        return false;
    };
    session_ops::show_browser_developer_tools(root, panel_id, panel.to_string())
}

pub(super) fn apply_set_browser_zoom(
    snapshot: &mut AppSessionSnapshot,
    panel_id: &str,
    zoom: f64,
) -> bool {
    let Some(slot) = active_layout_slot(snapshot) else {
        return false;
    };
    let Some(root) = slot.as_mut() else {
        return false;
    };
    session_ops::set_browser_page_zoom(root, panel_id, Some(normalize_browser_zoom(zoom)))
}

/// Append a fresh single-pane workspace to the first window and select it.
/// Delegates the tab-manager mutation to [`session_ops::new_workspace`].
pub(super) fn apply_new_workspace(
    snapshot: &mut AppSessionSnapshot,
    new_panel_id: &str,
    current_directory: Option<&str>,
    initial_terminal_command: Option<&str>,
    initial_terminal_input: Option<&str>,
    initial_terminal_environment: Option<BTreeMap<String, String>>,
) {
    if let Some(window) = snapshot.windows.first_mut() {
        session_ops::new_workspace(&mut window.tab_manager, new_panel_id);
        if let Some(selected_index) = window.tab_manager.selected_workspace_index {
            if let Some(workspace) = usize::try_from(selected_index)
                .ok()
                .and_then(|index| window.tab_manager.workspaces.get_mut(index))
            {
                if let Some(current_directory) = current_directory.filter(|path| !path.is_empty()) {
                    workspace.current_directory = Some(current_directory.to_owned());
                }
                workspace.initial_terminal_command = initial_terminal_command
                    .filter(|command| !command.trim().is_empty())
                    .map(str::to_owned);
                workspace.initial_terminal_input = initial_terminal_input
                    .filter(|input| !input.is_empty())
                    .map(str::to_owned);
                workspace.initial_terminal_environment =
                    initial_terminal_environment.filter(|environment| !environment.is_empty());
            }
        }
    }
    // The fresh workspace comes out of the pure builder id-less; identity is
    // minted here, in the stateful layer (see `ensure_workspace_ids`).
    ensure_workspace_ids(snapshot);
    ensure_pane_ids(snapshot);
    if let Some(workspace) = snapshot.windows.first_mut().and_then(|window| {
        let index = usize::try_from(window.tab_manager.selected_workspace_index?).ok()?;
        window.tab_manager.workspaces.get_mut(index)
    }) {
        seed_initial_surface_record(workspace);
    }
}

/// Move an existing panel/surface into a newly-created workspace and select it.
/// The pure session op preserves pane-local surface state and panel metadata;
/// this stateful layer only mints the destination workspace/pane identity.
pub(super) fn apply_move_panel_to_new_workspace(
    snapshot: &mut AppSessionSnapshot,
    panel_id: &str,
) -> bool {
    let changed = match snapshot.windows.first_mut() {
        Some(window) => session_ops::move_panel_to_new_workspace(&mut window.tab_manager, panel_id),
        None => false,
    };
    if changed {
        ensure_workspace_ids(snapshot);
        ensure_pane_ids(snapshot);
    }
    changed
}

/// Select the workspace at `index` in the first window (out-of-range is a
/// no-op). Delegates to [`session_ops::select_workspace`].
pub(super) fn apply_select_workspace(snapshot: &mut AppSessionSnapshot, index: i64) -> bool {
    match snapshot.windows.first_mut() {
        Some(window) => {
            let changed = session_ops::select_workspace(&mut window.tab_manager, index);
            if changed {
                sync_window_selected_workspace_id(window);
            }
            changed
        }
        None => false,
    }
}

pub(super) fn apply_select_workspace_by_id(
    snapshot: &mut AppSessionSnapshot,
    workspace_id: &str,
) -> bool {
    let Some(window) = snapshot.windows.first_mut() else {
        return false;
    };
    let Some(index) = window
        .tab_manager
        .workspaces
        .iter()
        .position(|workspace| workspace.workspace_id.as_deref() == Some(workspace_id))
    else {
        return false;
    };
    let index = index as i64;
    let changed = window.tab_manager.selected_workspace_index != Some(index);
    window.tab_manager.selected_workspace_index = Some(index);
    sync_window_selected_workspace_id(window);
    changed
}

/// Close the workspace at `index` in the first window. Mirrors canonical
/// `TabManager.closeWorkspace`: closing the only workspace is a no-op.
/// Delegates to [`session_ops::close_workspace`].
pub(super) fn apply_close_workspace(snapshot: &mut AppSessionSnapshot, index: i64) -> bool {
    match snapshot.windows.first_mut() {
        Some(window) => {
            let changed = session_ops::close_workspace(&mut window.tab_manager, index);
            if changed {
                sync_window_selected_workspace_id(window);
            }
            changed
        }
        None => false,
    }
}

/// Close multiple workspaces identified by their ORIGINAL indices in the first
/// window. Delegates to [`session_ops::close_workspaces`], which canonicalizes
/// the request into tab order, preserves the final surviving workspace, and
/// applies canonical group-anchor dissolution semantics on each close.
pub(super) fn apply_close_workspaces(snapshot: &mut AppSessionSnapshot, indices: &[i64]) -> bool {
    match snapshot.windows.first_mut() {
        Some(window) => session_ops::close_workspaces(&mut window.tab_manager, indices),
        None => false,
    }
}

/// Rename the workspace at `index` in the first window — canonical
/// `Workspace.setCustomTitle` trim/clear semantics (the title is trimmed;
/// empty/whitespace-only clears `custom_title`, restoring the process-title
/// fallback in the display chain). Pure — delegates to
/// [`session_ops::rename_workspace`]. Returns whether the title actually
/// changed.
pub(super) fn apply_rename_workspace(
    snapshot: &mut AppSessionSnapshot,
    index: i64,
    title: &str,
) -> bool {
    match snapshot.windows.first_mut() {
        Some(window) => session_ops::rename_workspace(&mut window.tab_manager, index, title),
        None => false,
    }
}

/// Set or clear the workspace description at `index` in the first window —
/// canonical `Workspace.setCustomDescription` semantics: blank/whitespace-only
/// clears, and line endings normalize to `\n`. Pure — delegates to
/// [`session_ops::set_workspace_description`]. Returns whether the description
/// actually changed.
pub(super) fn apply_set_workspace_description(
    snapshot: &mut AppSessionSnapshot,
    index: i64,
    description: &str,
) -> bool {
    match snapshot.windows.first_mut() {
        Some(window) => {
            session_ops::set_workspace_description(&mut window.tab_manager, index, description)
        }
        None => false,
    }
}

/// Clear the custom workspace tab color at `index` in the first window. Pure —
/// delegates to [`session_ops::reset_workspace_color`]. Returns whether a
/// color was actually cleared.
pub(super) fn apply_reset_workspace_color(snapshot: &mut AppSessionSnapshot, index: i64) -> bool {
    match snapshot.windows.first_mut() {
        Some(window) => session_ops::reset_workspace_color(&mut window.tab_manager, index),
        None => false,
    }
}

pub(super) fn apply_set_workspace_sidebar_progress(
    snapshot: &mut AppSessionSnapshot,
    index: i64,
    value: f64,
    label: Option<&str>,
) -> bool {
    let Some(workspace) = snapshot.windows.first_mut().and_then(|window| {
        window
            .tab_manager
            .workspaces
            .get_mut(usize::try_from(index).ok()?)
    }) else {
        return false;
    };
    let next = Some(SessionWorkspaceSidebarProgressSnapshot {
        value: value.clamp(0.0, 1.0),
        label: label
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned),
    });
    if workspace.sidebar_progress == next {
        return false;
    }
    workspace.sidebar_progress = next;
    true
}

pub(super) fn apply_clear_workspace_sidebar_progress(
    snapshot: &mut AppSessionSnapshot,
    index: i64,
) -> bool {
    let Some(workspace) = snapshot.windows.first_mut().and_then(|window| {
        window
            .tab_manager
            .workspaces
            .get_mut(usize::try_from(index).ok()?)
    }) else {
        return false;
    };
    if workspace.sidebar_progress.is_none() {
        return false;
    }
    workspace.sidebar_progress = None;
    true
}

pub(super) fn workspace_mut_by_index(
    snapshot: &mut AppSessionSnapshot,
    index: i64,
) -> Option<&mut SessionWorkspaceSnapshot> {
    snapshot.windows.first_mut().and_then(|window| {
        window
            .tab_manager
            .workspaces
            .get_mut(usize::try_from(index).ok()?)
    })
}

pub(super) fn sorted_sidebar_status_entries(entries: &mut [SessionWorkspaceSidebarStatusSnapshot]) {
    entries.sort_by(|left, right| {
        right
            .priority
            .unwrap_or(0)
            .cmp(&left.priority.unwrap_or(0))
            .then_with(|| left.key.cmp(&right.key))
    });
}

pub(super) fn sorted_sidebar_metadata_entries(
    entries: &mut [SessionWorkspaceSidebarMetadataSnapshot],
) {
    entries.sort_by(|left, right| {
        right
            .priority
            .unwrap_or(0)
            .cmp(&left.priority.unwrap_or(0))
            .then_with(|| left.key.cmp(&right.key))
    });
}

pub(super) fn sorted_sidebar_metadata_blocks(
    entries: &mut [SessionWorkspaceSidebarMetadataBlockSnapshot],
) {
    entries.sort_by(|left, right| {
        right
            .priority
            .unwrap_or(0)
            .cmp(&left.priority.unwrap_or(0))
            .then_with(|| left.key.cmp(&right.key))
    });
}

pub(super) fn apply_set_workspace_sidebar_status(
    snapshot: &mut AppSessionSnapshot,
    index: i64,
    key: &str,
    value: &str,
    priority: Option<i64>,
    updated_at: i64,
) -> bool {
    let Some(workspace) = workspace_mut_by_index(snapshot, index) else {
        return false;
    };
    let key = key.trim();
    if key.is_empty() {
        return false;
    }
    let value = value.trim();
    let mut entries = workspace.sidebar_status_entries.clone().unwrap_or_default();
    if value.is_empty() {
        let original_len = entries.len();
        entries.retain(|entry| entry.key != key);
        if entries.len() == original_len {
            return false;
        }
    } else if let Some(entry) = entries.iter_mut().find(|entry| entry.key == key) {
        let next_value = value.to_owned();
        if entry.value == next_value && entry.priority == priority {
            return false;
        }
        entry.value = next_value;
        entry.priority = priority;
        entry.updated_at = updated_at;
    } else {
        entries.push(SessionWorkspaceSidebarStatusSnapshot {
            key: key.to_owned(),
            value: value.to_owned(),
            priority,
            updated_at,
        });
    }
    sorted_sidebar_status_entries(&mut entries);
    workspace.sidebar_status_entries = (!entries.is_empty()).then_some(entries);
    true
}

pub(super) fn apply_set_workspace_sidebar_metadata(
    snapshot: &mut AppSessionSnapshot,
    index: i64,
    key: &str,
    value: &str,
    icon: Option<&str>,
    color: Option<&str>,
    url: Option<&str>,
    priority: Option<i64>,
    format: Option<&str>,
    updated_at: i64,
) -> bool {
    let Some(workspace) = workspace_mut_by_index(snapshot, index) else {
        return false;
    };
    let key = key.trim();
    if key.is_empty() {
        return false;
    }
    let value = value.trim();
    let normalized_icon = icon.and_then(normalize_nonempty).map(str::to_string);
    let normalized_color = color.and_then(normalize_nonempty).map(str::to_string);
    let normalized_url = url.and_then(normalize_nonempty).map(str::to_string);
    let normalized_format = format
        .and_then(normalize_nonempty)
        .map(|value| value.to_ascii_lowercase());
    let mut entries = workspace
        .sidebar_metadata_entries
        .clone()
        .unwrap_or_default();
    if value.is_empty() {
        let original_len = entries.len();
        entries.retain(|entry| entry.key != key);
        if entries.len() == original_len {
            return false;
        }
    } else if let Some(entry) = entries.iter_mut().find(|entry| entry.key == key) {
        let next_value = value.to_owned();
        if entry.value == next_value
            && entry.icon == normalized_icon
            && entry.color == normalized_color
            && entry.url == normalized_url
            && entry.priority == priority
            && entry.format == normalized_format
        {
            return false;
        }
        entry.value = next_value;
        entry.icon = normalized_icon;
        entry.color = normalized_color;
        entry.url = normalized_url;
        entry.priority = priority;
        entry.format = normalized_format;
        entry.updated_at = updated_at;
    } else {
        entries.push(SessionWorkspaceSidebarMetadataSnapshot {
            key: key.to_owned(),
            value: value.to_owned(),
            icon: normalized_icon,
            color: normalized_color,
            url: normalized_url,
            priority,
            format: normalized_format,
            updated_at,
        });
    }
    sorted_sidebar_metadata_entries(&mut entries);
    workspace.sidebar_metadata_entries = (!entries.is_empty()).then_some(entries);
    true
}

pub(super) fn apply_clear_workspace_sidebar_metadata(
    snapshot: &mut AppSessionSnapshot,
    index: i64,
    key: &str,
) -> bool {
    let Some(workspace) = workspace_mut_by_index(snapshot, index) else {
        return false;
    };
    let key = key.trim();
    if key.is_empty() {
        return false;
    }
    let mut entries = workspace
        .sidebar_metadata_entries
        .clone()
        .unwrap_or_default();
    let original_len = entries.len();
    entries.retain(|entry| entry.key != key);
    if entries.len() == original_len {
        return false;
    }
    workspace.sidebar_metadata_entries = (!entries.is_empty()).then_some(entries);
    true
}

pub(super) fn apply_set_workspace_sidebar_metadata_block(
    snapshot: &mut AppSessionSnapshot,
    index: i64,
    key: &str,
    markdown: &str,
    priority: Option<i64>,
    updated_at: i64,
) -> bool {
    let Some(workspace) = workspace_mut_by_index(snapshot, index) else {
        return false;
    };
    let key = key.trim();
    if key.is_empty() {
        return false;
    }
    let markdown = markdown.trim();
    let mut entries = workspace
        .sidebar_metadata_blocks
        .clone()
        .unwrap_or_default();
    if markdown.is_empty() {
        let original_len = entries.len();
        entries.retain(|entry| entry.key != key);
        if entries.len() == original_len {
            return false;
        }
    } else if let Some(entry) = entries.iter_mut().find(|entry| entry.key == key) {
        let next_markdown = markdown.to_owned();
        if entry.markdown == next_markdown && entry.priority == priority {
            return false;
        }
        entry.markdown = next_markdown;
        entry.priority = priority;
        entry.updated_at = updated_at;
    } else {
        entries.push(SessionWorkspaceSidebarMetadataBlockSnapshot {
            key: key.to_owned(),
            markdown: markdown.to_owned(),
            priority,
            updated_at,
        });
    }
    sorted_sidebar_metadata_blocks(&mut entries);
    workspace.sidebar_metadata_blocks = (!entries.is_empty()).then_some(entries);
    true
}

pub(super) fn apply_clear_workspace_sidebar_metadata_block(
    snapshot: &mut AppSessionSnapshot,
    index: i64,
    key: &str,
) -> bool {
    let Some(workspace) = workspace_mut_by_index(snapshot, index) else {
        return false;
    };
    let key = key.trim();
    if key.is_empty() {
        return false;
    }
    let mut entries = workspace
        .sidebar_metadata_blocks
        .clone()
        .unwrap_or_default();
    let original_len = entries.len();
    entries.retain(|entry| entry.key != key);
    if entries.len() == original_len {
        return false;
    }
    workspace.sidebar_metadata_blocks = (!entries.is_empty()).then_some(entries);
    true
}

pub(super) fn apply_reset_workspace_sidebar_metadata(
    snapshot: &mut AppSessionSnapshot,
    index: i64,
) -> bool {
    let Some(workspace) = workspace_mut_by_index(snapshot, index) else {
        return false;
    };
    let changed = workspace.sidebar_progress.is_some()
        || workspace.sidebar_status_entries.is_some()
        || workspace.sidebar_metadata_entries.is_some()
        || workspace.sidebar_metadata_blocks.is_some()
        || workspace.sidebar_log_entries.is_some();
    workspace.sidebar_progress = None;
    workspace.sidebar_status_entries = None;
    workspace.sidebar_metadata_entries = None;
    workspace.sidebar_metadata_blocks = None;
    workspace.sidebar_log_entries = None;
    changed
}

pub(super) fn apply_clear_workspace_sidebar_status(
    snapshot: &mut AppSessionSnapshot,
    index: i64,
    key: &str,
) -> bool {
    let Some(workspace) = workspace_mut_by_index(snapshot, index) else {
        return false;
    };
    let key = key.trim();
    if key.is_empty() {
        return false;
    }
    let mut entries = workspace.sidebar_status_entries.clone().unwrap_or_default();
    let original_len = entries.len();
    entries.retain(|entry| entry.key != key);
    if entries.len() == original_len {
        return false;
    }
    workspace.sidebar_status_entries = (!entries.is_empty()).then_some(entries);
    true
}

pub(super) fn apply_append_workspace_sidebar_log(
    snapshot: &mut AppSessionSnapshot,
    index: i64,
    message: &str,
    level: &str,
    created_at: i64,
) -> bool {
    let Some(workspace) = workspace_mut_by_index(snapshot, index) else {
        return false;
    };
    let message = message.trim();
    if message.is_empty() {
        return false;
    }
    let level = level.trim();
    let level = if level.is_empty() { "info" } else { level };
    let mut entries = workspace.sidebar_log_entries.clone().unwrap_or_default();
    entries.push(SessionWorkspaceSidebarLogEntrySnapshot {
        level: level.to_owned(),
        message: message.to_owned(),
        created_at,
    });
    let overflow = entries.len().saturating_sub(SIDEBAR_LOG_ENTRY_LIMIT);
    if overflow > 0 {
        entries.drain(0..overflow);
    }
    workspace.sidebar_log_entries = Some(entries);
    true
}

pub(super) fn apply_clear_workspace_sidebar_log(
    snapshot: &mut AppSessionSnapshot,
    index: i64,
) -> bool {
    let Some(workspace) = workspace_mut_by_index(snapshot, index) else {
        return false;
    };
    if workspace.sidebar_log_entries.is_none() {
        return false;
    }
    workspace.sidebar_log_entries = None;
    true
}
