use super::*;

/// Restore the previous launch's persisted session snapshot if one exists.
/// Corrupt/missing files are a no-op: the current in-memory snapshot is
/// returned unchanged. A successful restore also reseeds the next `surface-N`
/// counter so future splits/workspaces keep unique panel ids.
#[tauri::command]
pub fn session_restore_previous_launch(
    app: AppHandle,
    state: State<'_, SessionState>,
) -> Result<AppSessionSnapshot, String> {
    restore_previous_launch_for_route(&app, &state, ManualRestoreRoute::Product)
        .map(|outcome| outcome.snapshot)
}

/// Set the OSC/process title of the workspace owning `panel_id`, fed by a
/// terminal surface's title changes (xterm `onTitleChange`). Emits
/// `cmux://session-changed` and returns the snapshot ONLY when the title
/// actually changed, so the high-frequency title stream never floods the event
/// bus with no-op churn. `panelId` (camelCase) maps to the `panel_id` param.
#[tauri::command]
pub fn session_set_process_title(
    app: AppHandle,
    state: State<'_, SessionState>,
    panel_id: String,
    title: String,
) -> Result<AppSessionSnapshot, String> {
    state.transact_snapshot_if_changed(&app, |snapshot| {
        apply_set_process_title(snapshot, &panel_id, &title)
    })
}

/// Split the pane holding `panel_id` in `orientation`, allocating a fresh panel
/// id for the new pane. Emits `cmux://session-changed` and returns the snapshot.
#[tauri::command]
pub fn session_split(
    app: AppHandle,
    state: State<'_, SessionState>,
    panel_id: String,
    orientation: SessionSplitOrientation,
    insert_first: Option<bool>,
    initial_terminal_command: Option<String>,
    initial_terminal_input: Option<String>,
    initial_terminal_environment: Option<BTreeMap<String, String>>,
) -> Result<AppSessionSnapshot, String> {
    split_panel_for_control(
        &app,
        &state,
        &panel_id,
        orientation,
        insert_first.unwrap_or(false),
        initial_terminal_command.as_deref(),
        initial_terminal_input.as_deref(),
        initial_terminal_environment,
    )
    .map_err(|error| error.to_string())
}

/// Add a new terminal tab beside `panel_id` in the same pane.
#[tauri::command]
pub fn session_new_terminal_tab(
    app: AppHandle,
    state: State<'_, SessionState>,
    panel_id: String,
    initial_terminal_command: Option<String>,
    initial_terminal_input: Option<String>,
    initial_terminal_environment: Option<BTreeMap<String, String>>,
) -> Result<AppSessionSnapshot, String> {
    new_terminal_tab_for_control(
        &app,
        &state,
        &panel_id,
        initial_terminal_command.as_deref(),
        initial_terminal_input.as_deref(),
        initial_terminal_environment,
    )
    .map_err(|error| error.to_string())
}

/// Split the pane holding `panel_id`, making the new pane a browser surface.
#[tauri::command]
pub fn session_split_browser(
    app: AppHandle,
    state: State<'_, SessionState>,
    panel_id: String,
    orientation: SessionSplitOrientation,
    insert_first: Option<bool>,
    url: Option<String>,
) -> Result<AppSessionSnapshot, String> {
    split_browser_for_control(
        &app,
        &state,
        &panel_id,
        orientation,
        insert_first.unwrap_or(false),
        url.as_deref(),
    )
    .map_err(|error| error.to_string())
}

/// Close the pane/panel `panel_id`. Emits `cmux://session-changed` and returns
/// the snapshot. A no-op (still Ok) if the panel is already gone.
#[tauri::command]
pub fn session_close(
    app: AppHandle,
    state: State<'_, SessionState>,
    panel_id: String,
) -> AppSessionSnapshot {
    let (closed_browser_tab, outcome, snapshot) = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        let closed_browser_tab = closed_browser_tab_for_active_panel(&guard, &panel_id);
        let outcome = apply_close(&mut guard, &panel_id);
        (closed_browser_tab, outcome, guard.clone())
    };
    if outcome != CloseOutcome::NotFound {
        app.state::<crate::remote_proxy::RemoteProxyBrokerState>()
            .stop_panel_broker(&panel_id);
        if let Some(tab) = closed_browser_tab {
            let mut history = state
                .closed_browser_tabs
                .lock()
                .expect("closed browser history mutex poisoned");
            push_closed_browser_tabs(&mut history, [tab]);
        }
    }
    notify_session_changed(&app, &snapshot);
    snapshot
}

/// Set the divider ratio of the split at `path` (array of `"first"`/`"second"`).
/// Emits `cmux://session-changed` and returns the snapshot.
#[tauri::command]
pub fn session_set_divider(
    app: AppHandle,
    state: State<'_, SessionState>,
    path: Vec<SplitChild>,
    position: f64,
) -> Result<AppSessionSnapshot, String> {
    state.transact_snapshot_always(&app, |snapshot| {
        apply_set_divider(snapshot, &path, position)
    })
}

/// Equalize every split divider in the active workspace layout so panes share
/// space evenly by span. Emits `cmux://session-changed` and returns the
/// snapshot. A no-op (still returns the snapshot) on absent/single-pane layouts.
#[tauri::command]
pub fn session_equalize_dividers(
    app: AppHandle,
    state: State<'_, SessionState>,
) -> Result<AppSessionSnapshot, String> {
    let snapshot = equalize_dividers_for_control(&app, &state)?;
    Ok(snapshot)
}

/// Toggle split zoom for the active pane. When zoomed, the web workspace renders
/// only that pane at full size while preserving the underlying split tree.
#[tauri::command]
pub fn session_toggle_split_zoom(
    app: AppHandle,
    state: State<'_, SessionState>,
    panel_id: String,
) -> Result<AppSessionSnapshot, String> {
    let snapshot = toggle_split_zoom_for_control(&app, &state, &panel_id)?;
    Ok(snapshot)
}

/// Set the active workspace layout mode. `"canvas"` enables freeform canvas
/// mode and seeds `canvas_panes`; `null`, `"split"`, or `"default"` returns to
/// the canonical split layout while preserving the canvas seed for later.
#[tauri::command]
pub fn session_set_layout_mode(
    app: AppHandle,
    state: State<'_, SessionState>,
    mode: Option<String>,
) -> Result<AppSessionSnapshot, String> {
    state.transact_snapshot_if_changed(&app, |snapshot| {
        apply_set_layout_mode(snapshot, mode.as_deref())
    })
}

#[tauri::command]
pub fn session_set_canvas_pane_frame(
    app: AppHandle,
    state: State<'_, SessionState>,
    panel_id: String,
    x: i64,
    y: i64,
    width: i64,
    height: i64,
) -> Result<AppSessionSnapshot, String> {
    state.transact_snapshot_if_changed(&app, |snapshot| {
        apply_set_canvas_pane_frame(snapshot, &panel_id, x, y, width, height)
    })
}

#[tauri::command]
pub fn session_apply_canvas_action(
    app: AppHandle,
    state: State<'_, SessionState>,
    action: String,
    pane_gap: Option<i64>,
) -> Result<AppSessionSnapshot, String> {
    state.transact_snapshot_if_changed(&app, |snapshot| {
        apply_canvas_action(snapshot, &action, pane_gap)
    })
}

/// Set (or clear) the surface kind of the pane holding `panelId`: `"agent"` for
/// a canonical agent session, or `null`/absent to revert to a terminal. Emits
/// `cmux://session-changed` and returns the snapshot. A no-op (still returns the
/// snapshot) if no pane holds the id.
#[tauri::command]
pub fn session_set_surface_kind(
    app: AppHandle,
    state: State<'_, SessionState>,
    panel_id: String,
    kind: Option<String>,
) -> Result<AppSessionSnapshot, String> {
    state.transact_snapshot_always(&app, |snapshot| {
        apply_set_surface_kind(snapshot, &panel_id, kind)
    })
}

/// Select the next/previous tab inside the pane holding `panelId`.
#[tauri::command]
pub fn session_select_adjacent_panel(
    app: AppHandle,
    state: State<'_, SessionState>,
    panel_id: String,
    next: bool,
) -> Result<AppSessionSnapshot, String> {
    let snapshot = select_adjacent_panel_for_control(&app, &state, &panel_id, next)
        .map_err(collapse_infallible_publication_error)?;
    Ok(snapshot)
}

/// Select a workspace by id and focus a panel/tab inside it.
#[tauri::command]
pub fn session_select_workspace_surface(
    app: AppHandle,
    state: State<'_, SessionState>,
    workspace_id: String,
    panel_id: String,
) -> Result<AppSessionSnapshot, String> {
    let (_, snapshot) = select_workspace_surface(&app, &state, &workspace_id, &panel_id)
        .map_err(collapse_infallible_publication_error)?;
    Ok(snapshot)
}

#[tauri::command]
pub fn session_focus_panel(
    app: AppHandle,
    state: State<'_, SessionState>,
    panel_id: String,
) -> Result<AppSessionSnapshot, String> {
    state.transact_snapshot_if_changed(&app, |snapshot| apply_focus_panel(snapshot, &panel_id))
}

pub(crate) fn select_workspace_surface(
    app: &AppHandle,
    state: &SessionState,
    workspace_id: &str,
    panel_id: &str,
) -> Result<(bool, AppSessionSnapshot), PaneTopologyControlError<std::convert::Infallible>> {
    state.transact_value_if_changed(app, |snapshot| {
        let changed = apply_select_workspace_surface(snapshot, workspace_id, panel_id);
        Ok((changed, changed))
    })
}

pub(super) fn collapse_infallible_publication_error(
    error: PaneTopologyControlError<std::convert::Infallible>,
) -> String {
    match error {
        PaneTopologyControlError::Publication(error) => error,
        PaneTopologyControlError::Operation(error) => match error {},
    }
}

pub(crate) fn workspace_surface_is_selected(
    snapshot: &AppSessionSnapshot,
    workspace_id: &str,
    panel_id: &str,
) -> bool {
    let Some(window) = snapshot.windows.first() else {
        return false;
    };
    let Some(index) = window.tab_manager.selected_workspace_index else {
        return false;
    };
    let Ok(index) = usize::try_from(index) else {
        return false;
    };
    let Some(workspace) = window.tab_manager.workspaces.get(index) else {
        return false;
    };
    if workspace.workspace_id.as_deref() != Some(workspace_id) {
        return false;
    }
    workspace
        .layout
        .as_ref()
        .is_some_and(|layout| layout_selects_panel(layout, panel_id))
}

fn layout_selects_panel(layout: &SessionWorkspaceLayoutSnapshot, panel_id: &str) -> bool {
    match layout {
        SessionWorkspaceLayoutSnapshot::Pane(pane) => {
            pane.selected_panel_id.as_deref() == Some(panel_id)
                && pane.panel_ids.iter().any(|id| id == panel_id)
        }
        SessionWorkspaceLayoutSnapshot::Split(split) => {
            layout_selects_panel(&split.first, panel_id)
                || layout_selects_panel(&split.second, panel_id)
        }
    }
}

pub(crate) fn workspace_id_for_panel(state: &SessionState, panel_id: &str) -> Option<String> {
    let panel_id = panel_id.trim();
    if panel_id.is_empty() {
        return None;
    }
    let guard = state
        .snapshot
        .lock()
        .expect("session snapshot mutex poisoned");
    guard.windows.iter().find_map(|window| {
        window.tab_manager.workspaces.iter().find_map(|workspace| {
            let layout = workspace.layout.as_ref()?;
            if session_ops::contains_panel(layout, panel_id) {
                workspace.workspace_id.clone()
            } else {
                None
            }
        })
    })
}

pub(super) fn workspace_is_selected(snapshot: &AppSessionSnapshot, workspace_id: &str) -> bool {
    let Some(window) = snapshot.windows.first() else {
        return false;
    };
    let Some(index) = window.tab_manager.selected_workspace_index else {
        return false;
    };
    let Ok(index) = usize::try_from(index) else {
        return false;
    };
    window
        .tab_manager
        .workspaces
        .get(index)
        .is_some_and(|workspace| workspace.workspace_id.as_deref() == Some(workspace_id))
}

fn select_workspace_by_id(
    app: &AppHandle,
    state: &SessionState,
    workspace_id: &str,
) -> Result<(bool, AppSessionSnapshot), PaneTopologyControlError<std::convert::Infallible>> {
    state.transact_value_if_changed(app, |snapshot| {
        let changed = apply_select_workspace_by_id(snapshot, workspace_id);
        Ok((changed, changed))
    })
}

#[derive(Debug, PartialEq, Eq)]
enum OpenSshUrlControlError {
    NotFound(String),
    Publication(String),
}

impl std::fmt::Display for OpenSshUrlControlError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound(message) | Self::Publication(message) => formatter.write_str(message),
        }
    }
}

fn open_ssh_url_request(
    app: &AppHandle,
    state: &SessionState,
    request: &cmux_ssh::CmuxSSHURLRequest,
) -> Result<(bool, AppSessionSnapshot, String), OpenSshUrlControlError> {
    let _transact_guard = state.snapshot.lock_gate();
    let current = transaction_current_snapshot(&state.snapshot)
        .map_err(OpenSshUrlControlError::Publication)?;
    let anchor_panel_id = active_panel_id(&current).ok_or_else(|| {
        OpenSshUrlControlError::NotFound(
            "no active terminal pane is available for SSH URL".to_string(),
        )
    })?;
    let mut ids = DeferredPanelIds::new(&state.next_panel);
    let new_panel_id = ids.next();
    let mut candidate = current.clone();
    if !apply_ssh_url_request(&mut candidate, &anchor_panel_id, &new_panel_id, request) {
        return Err(OpenSshUrlControlError::NotFound(format!(
            "no pane holds panel id {anchor_panel_id}"
        )));
    }
    let mut publication =
        ProductionSnapshotPublicationOperations::with_deferred_next_panel_reseed(app, state);
    let snapshot = publish_snapshot_transaction(
        &state.snapshot,
        Some(&current),
        &candidate,
        &mut publication,
    )
    .map_err(OpenSshUrlControlError::Publication)?;
    ids.commit(&state.next_panel);
    Ok((true, snapshot, new_panel_id))
}

pub(super) fn parse_ssh_uri(uri: &str) -> Result<cmux_ssh::CmuxSSHURLRequest, String> {
    cmux_ssh::CmuxSSHURLRequest::parse(uri, &cmux_ssh::ssh_url::SUPPORTED_SCHEMES)
        .map_err(|error| format!("invalid SSH URI: {error}"))?
        .ok_or_else(|| "URI is not a supported cmux SSH route".to_string())
}

pub(super) fn parse_session_navigation_uri(uri: &str) -> Result<SessionNavigationTarget, String> {
    let (scheme, rest) = uri
        .split_once("://")
        .ok_or_else(|| "navigation URI must use <scheme>://workspace/...".to_owned())?;
    let supported_schemes = crate::auth_environment::active_navigation_schemes();
    if !supported_schemes
        .iter()
        .any(|candidate| candidate == scheme)
    {
        return Err(format!(
            "navigation URI used scheme {scheme:?}, expected one of {:?}",
            supported_schemes
        ));
    }

    let path = rest
        .split_once('?')
        .map_or(rest, |(path, _query)| path)
        .trim_matches('/');
    let segments = path.split('/').collect::<Vec<_>>();
    match segments.as_slice() {
        ["workspace", workspace_id] if !workspace_id.is_empty() => Ok(SessionNavigationTarget {
            workspace_id: percent_decode_path_segment(workspace_id)?,
            panel_id: None,
        }),
        ["workspace", workspace_id, "pane" | "surface", panel_id]
            if !workspace_id.is_empty() && !panel_id.is_empty() =>
        {
            Ok(SessionNavigationTarget {
                workspace_id: percent_decode_path_segment(workspace_id)?,
                panel_id: Some(percent_decode_path_segment(panel_id)?),
            })
        }
        _ => Err(
            "navigation URI must be <scheme>://workspace/{id}, /pane/{id}, or /surface/{id}"
                .to_owned(),
        ),
    }
}

fn percent_decode_path_segment(value: &str) -> Result<String, String> {
    let mut decoded = Vec::with_capacity(value.len());
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'%' {
            decoded.push(bytes[index]);
            index += 1;
            continue;
        }
        if index + 2 >= bytes.len() {
            return Err(format!(
                "navigation URI percent encoding is incomplete near {:?}",
                &value[index..]
            ));
        }
        let Some(high) = path_hex_value(bytes[index + 1]) else {
            return Err(format!(
                "navigation URI percent encoding is invalid near {:?}",
                &value[index..index + 3]
            ));
        };
        let Some(low) = path_hex_value(bytes[index + 2]) else {
            return Err(format!(
                "navigation URI percent encoding is invalid near {:?}",
                &value[index..index + 3]
            ));
        };
        decoded.push((high << 4) | low);
        index += 3;
    }
    String::from_utf8(decoded)
        .map_err(|_| "navigation URI percent encoding is not valid UTF-8".to_owned())
}

fn path_hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// Open a markdown file in the pane holding `panelId`, switching that pane to
/// the markdown surface and binding the file path into the pane-local session
/// snapshot. Returns an error when no pane holds the id or the path is blank.
#[tauri::command]
pub fn session_open_markdown_file(
    app: AppHandle,
    state: State<'_, SessionState>,
    panel_id: String,
    file_path: String,
) -> Result<AppSessionSnapshot, String> {
    open_markdown_file_in_panel(&app, &state, &panel_id, &file_path)?
        .ok_or_else(|| format!("unable to open markdown file in pane {panel_id}"))
}

/// Open a plain-text file in the pane holding `panelId`, switching that pane to
/// the file editor surface and binding the file path into the pane-local
/// session snapshot.
#[tauri::command]
pub fn session_open_file(
    app: AppHandle,
    state: State<'_, SessionState>,
    panel_id: String,
    file_path: String,
) -> Result<AppSessionSnapshot, String> {
    open_file_in_panel(&app, &state, &panel_id, &file_path)?
        .ok_or_else(|| format!("unable to open file in pane {panel_id}"))
}

/// Open a registered diff-viewer session in the pane holding `panelId`,
/// switching that pane to the diff surface and binding the token + request path
/// into the pane-local session snapshot. Returns an error when the pane id is
/// unknown or the diff request is not currently restorable from the native
/// registry/manifest.
#[tauri::command]
pub fn session_open_diff_viewer(
    app: AppHandle,
    state: State<'_, SessionState>,
    diff_state: State<'_, crate::diff::DiffState>,
    panel_id: String,
    token: String,
    request_path: Option<String>,
) -> Result<AppSessionSnapshot, String> {
    open_diff_viewer_in_panel(
        &app,
        &state,
        &diff_state,
        &panel_id,
        &token,
        request_path.as_deref().unwrap_or("/index.html"),
    )?
    .ok_or_else(|| format!("unable to open diff viewer in pane {panel_id}"))
}

/// Open a URL in the pane holding `panelId`, switching that pane to the browser
/// surface and persisting the pane-local browser state.
#[tauri::command]
pub fn session_open_browser_url(
    app: AppHandle,
    state: State<'_, SessionState>,
    panel_id: String,
    url: Option<String>,
) -> Result<AppSessionSnapshot, String> {
    open_browser_url_in_panel(&app, &state, &panel_id, url.as_deref())?
        .ok_or_else(|| format!("unable to open browser in pane {panel_id}"))
}

/// Navigate the pane-local browser back if it has persisted history.
#[tauri::command]
pub fn session_browser_go_back(
    app: AppHandle,
    state: State<'_, SessionState>,
    panel_id: String,
) -> Result<AppSessionSnapshot, String> {
    let snapshot = browser_go_back_for_control(&app, &state, &panel_id)?;
    Ok(snapshot)
}

/// Navigate the pane-local browser forward if it has persisted history.
#[tauri::command]
pub fn session_browser_go_forward(
    app: AppHandle,
    state: State<'_, SessionState>,
    panel_id: String,
) -> Result<AppSessionSnapshot, String> {
    let snapshot = browser_go_forward_for_control(&app, &state, &panel_id)?;
    Ok(snapshot)
}

/// Clear the pane-local browser history while preserving the current page.
#[tauri::command]
pub fn session_clear_browser_history(
    app: AppHandle,
    state: State<'_, SessionState>,
    panel_id: String,
) -> Result<AppSessionSnapshot, String> {
    let snapshot = clear_browser_history_for_control(&app, &state, &panel_id)?;
    Ok(snapshot)
}

/// Toggle the pane-local browser omnibar/toolbar visibility.
#[tauri::command]
pub fn session_toggle_browser_omnibar(
    app: AppHandle,
    state: State<'_, SessionState>,
    panel_id: String,
) -> Result<AppSessionSnapshot, String> {
    let snapshot = toggle_browser_omnibar_for_control(&app, &state, &panel_id)?;
    Ok(snapshot)
}

/// Toggle browser focus mode for the pane holding `panelId`.
#[tauri::command]
pub fn session_toggle_browser_focus_mode(
    app: AppHandle,
    state: State<'_, SessionState>,
    panel_id: String,
) -> Result<AppSessionSnapshot, String> {
    let snapshot = toggle_browser_focus_mode_for_control(&app, &state, &panel_id)?;
    Ok(snapshot)
}

/// Toggle the browser developer-tools drawer for the pane holding `panelId`.
#[tauri::command]
pub fn session_toggle_browser_developer_tools(
    app: AppHandle,
    state: State<'_, SessionState>,
    panel_id: String,
) -> Result<AppSessionSnapshot, String> {
    let snapshot = toggle_browser_developer_tools_for_control(&app, &state, &panel_id)?;
    Ok(snapshot)
}

/// Show the browser developer-tools drawer on a specific panel/lane.
#[tauri::command]
pub fn session_show_browser_developer_tools(
    app: AppHandle,
    state: State<'_, SessionState>,
    panel_id: String,
    panel: String,
) -> Result<AppSessionSnapshot, String> {
    let snapshot = show_browser_developer_tools_for_control(&app, &state, &panel_id, &panel)?;
    Ok(snapshot)
}

/// Persist the browser zoom factor for the pane holding `panelId`.
#[tauri::command]
pub fn session_set_browser_zoom(
    app: AppHandle,
    state: State<'_, SessionState>,
    panel_id: String,
    zoom: f64,
) -> Result<AppSessionSnapshot, String> {
    let (_, snapshot) = state
        .transact_pane_topology(&app, |snapshot| {
            apply_set_browser_zoom(snapshot, &panel_id, zoom)
                .then_some(())
                .ok_or_else(|| format!("unable to set browser zoom for pane {panel_id}"))
        })
        .map_err(|error| match error {
            PaneTopologyControlError::Operation(message)
            | PaneTopologyControlError::Publication(message) => message,
        })?;
    Ok(snapshot)
}

/// Create a new workspace (fresh single-pane terminal) and select it. Emits
/// `cmux://session-changed` and returns the snapshot.
#[tauri::command]
pub fn session_new_workspace(
    app: AppHandle,
    state: State<'_, SessionState>,
    current_directory: Option<String>,
    initial_terminal_command: Option<String>,
    initial_terminal_input: Option<String>,
    initial_terminal_environment: Option<BTreeMap<String, String>>,
) -> Result<AppSessionSnapshot, String> {
    new_workspace_for_control(
        &app,
        &state,
        current_directory.as_deref(),
        initial_terminal_command.as_deref(),
        initial_terminal_input.as_deref(),
        initial_terminal_environment,
    )
}

/// Create a new browser workspace, select it, and bind its initial URL.
#[tauri::command]
pub fn session_new_browser_workspace(
    app: AppHandle,
    state: State<'_, SessionState>,
    url: Option<String>,
) -> Result<AppSessionSnapshot, String> {
    new_browser_workspace_for_control(&app, &state, url.as_deref())
}

/// Reopen the most recent browser pane/workspace that disappeared, restoring
/// its URL into a fresh browser workspace. This is the first host-wired slice of
/// the canonical recently-closed history; full item-menu persistence remains a
/// separate layer.
#[tauri::command]
pub fn session_reopen_closed_browser_tab(
    app: AppHandle,
    state: State<'_, SessionState>,
) -> Result<AppSessionSnapshot, String> {
    reopen_closed_browser_tab_for_control(&app, &state)
}

/// Reopen the most recently closed workspace with its complete session model.
#[tauri::command]
pub fn session_reopen_closed_workspace(
    app: AppHandle,
    state: State<'_, SessionState>,
) -> Result<AppSessionSnapshot, String> {
    reopen_closed_workspace_for_control(&app, &state)?
        .ok_or_else(|| "No recently closed workspace".to_string())
}

/// Move `panelId` from the active workspace into a newly-created workspace.
#[tauri::command]
pub fn session_move_panel_to_new_workspace(
    app: AppHandle,
    state: State<'_, SessionState>,
    panel_id: String,
) -> Result<AppSessionSnapshot, String> {
    move_panel_to_new_workspace_for_control(&app, &state, &panel_id)
}

#[tauri::command]
pub fn session_handle_navigation_uri(
    app: AppHandle,
    state: State<'_, SessionState>,
    uri: String,
) -> Result<SessionNavigationHandleReply, String> {
    let target = parse_session_navigation_uri(&uri)?;
    let (changed, snapshot) = if let Some(panel_id) = target.panel_id.as_deref() {
        select_workspace_surface(&app, &state, &target.workspace_id, panel_id)
            .map_err(collapse_infallible_publication_error)?
    } else {
        select_workspace_by_id(&app, &state, &target.workspace_id)
            .map_err(collapse_infallible_publication_error)?
    };
    let handled = match target.panel_id.as_deref() {
        Some(panel_id) => {
            changed || workspace_surface_is_selected(&snapshot, &target.workspace_id, panel_id)
        }
        None => changed || workspace_is_selected(&snapshot, &target.workspace_id),
    };
    let message = if handled {
        "Navigation URI routed to the current session.".to_owned()
    } else {
        "Navigation URI target was not found in the current session.".to_owned()
    };

    Ok(SessionNavigationHandleReply {
        handled,
        changed,
        workspace_id: target.workspace_id,
        panel_id: target.panel_id,
        message,
    })
}

#[tauri::command]
pub fn session_handle_ssh_uri(
    app: AppHandle,
    state: State<'_, SessionState>,
    uri: String,
) -> Result<SessionSshUriHandleReply, String> {
    let request = parse_ssh_uri(&uri)?;
    let (changed, _snapshot, panel_id) =
        open_ssh_url_request(&app, &state, &request).map_err(|error| error.to_string())?;
    Ok(SessionSshUriHandleReply {
        handled: true,
        changed,
        panel_id,
        destination: request.display_target(),
        no_focus: request.no_focus,
        message: "SSH URI opened in a terminal tab.".to_string(),
    })
}

/// Select the workspace at `index`. Emits `cmux://session-changed` and returns
/// the snapshot. A no-op (still returns the snapshot) if the index is invalid.
#[tauri::command]
pub fn session_select_workspace(
    app: AppHandle,
    state: State<'_, SessionState>,
    index: i64,
) -> Result<AppSessionSnapshot, String> {
    let snapshot = select_workspace_for_control(&app, &state, index)?;
    Ok(snapshot)
}

/// Close the workspace at `index`. Closing the sole remaining workspace is a
/// no-op (canonical parity). Emits `cmux://session-changed` and returns the
/// snapshot.
#[tauri::command]
pub fn session_close_workspace(
    app: AppHandle,
    state: State<'_, SessionState>,
    index: i64,
) -> AppSessionSnapshot {
    let (closed_browser_tabs, changed, snapshot) = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        let closed_browser_tabs = closed_browser_tabs_for_workspace_index(&guard, index);
        let changed = apply_close_workspace(&mut guard, index);
        (closed_browser_tabs, changed, guard.clone())
    };
    if changed && !closed_browser_tabs.is_empty() {
        let mut history = state
            .closed_browser_tabs
            .lock()
            .expect("closed browser history mutex poisoned");
        push_closed_browser_tabs(&mut history, closed_browser_tabs);
    }
    notify_session_changed(&app, &snapshot);
    snapshot
}

/// Close multiple workspaces addressed by their ORIGINAL indices in the first
/// window. Canonicalized into current tab order Rust-side, so callers can pass
/// ranges like "close everything above/below" without racing index shifts.
/// Emits `cmux://session-changed` only when at least one workspace actually
/// closed, and returns the snapshot either way.
#[tauri::command]
pub fn session_close_workspaces(
    app: AppHandle,
    state: State<'_, SessionState>,
    indices: Vec<i64>,
) -> AppSessionSnapshot {
    let (closed_browser_tabs, changed, snapshot) = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        let candidates: Vec<_> = indices
            .iter()
            .filter_map(|index| {
                if *index < 0 {
                    return None;
                }
                let workspace = guard
                    .windows
                    .first()
                    .and_then(|window| window.tab_manager.workspaces.get(*index as usize))?;
                let tabs = closed_browser_tabs_for_workspace(workspace);
                (!tabs.is_empty()).then(|| (workspace.workspace_id.clone(), tabs))
            })
            .collect();
        let changed = apply_close_workspaces(&mut guard, &indices);
        let remaining_ids: HashSet<String> = guard
            .windows
            .first()
            .into_iter()
            .flat_map(|window| window.tab_manager.workspaces.iter())
            .filter_map(|workspace| workspace.workspace_id.clone())
            .collect();
        let closed_browser_tabs = if changed {
            candidates
                .into_iter()
                .filter(|(workspace_id, _tabs)| {
                    workspace_id
                        .as_ref()
                        .is_none_or(|id| !remaining_ids.contains(id))
                })
                .flat_map(|(_workspace_id, tabs)| tabs)
                .collect()
        } else {
            Vec::new()
        };
        (closed_browser_tabs, changed, guard.clone())
    };
    if !closed_browser_tabs.is_empty() {
        let mut history = state
            .closed_browser_tabs
            .lock()
            .expect("closed browser history mutex poisoned");
        push_closed_browser_tabs(&mut history, closed_browser_tabs);
    }
    if changed {
        notify_session_changed(&app, &snapshot);
    }
    snapshot
}

/// Set (collapse/expand) a workspace group's collapsed flag. Canonical parity:
/// the pure-data `setWorkspaceGroupCollapsed` used by socket/CLI paths —
/// never moves selection. Emits `cmux://session-changed` only when the flag
/// actually changed (unknown id / already-at-value are no-ops) and returns
/// the snapshot. `groupId` (camelCase) maps to the `group_id` param.
#[tauri::command]
pub fn session_set_group_collapsed(
    app: AppHandle,
    state: State<'_, SessionState>,
    group_id: String,
    collapsed: bool,
) -> Result<AppSessionSnapshot, String> {
    let snapshot = set_group_collapsed_for_control(&app, &state, &group_id, collapsed)?;
    Ok(snapshot)
}

/// Rename the workspace at `index`. Canonical `Workspace.setCustomTitle`
/// (user-source) semantics: the raw title is trimmed here — a single mutation
/// path, the web sends the input verbatim — and an empty/whitespace-only title
/// clears `custom_title` (restores the process-title fallback); non-empty
/// stamps `custom_title_source = "user"`. Emits `cmux://session-changed` only
/// when the title actually changed (unknown index / identical title are
/// no-ops) and returns the snapshot.
#[tauri::command]
pub fn session_rename_workspace(
    app: AppHandle,
    state: State<'_, SessionState>,
    index: i64,
    title: String,
) -> Result<AppSessionSnapshot, String> {
    state.transact_snapshot_if_changed(&app, |snapshot| {
        apply_rename_workspace(snapshot, index, &title)
    })
}

/// Set or clear the workspace description at `index`. Blank/whitespace-only
/// descriptions clear `custom_description`; non-empty descriptions preserve
/// their edge whitespace after canonical line-ending normalization. Emits
/// `cmux://session-changed` only when the description actually changed
/// (unknown index / identical description are no-ops) and returns the
/// snapshot.
#[tauri::command]
pub fn session_set_workspace_description(
    app: AppHandle,
    state: State<'_, SessionState>,
    index: i64,
    description: String,
) -> Result<AppSessionSnapshot, String> {
    let snapshot = set_workspace_description_for_control(&app, &state, index, &description)?;
    Ok(snapshot)
}

/// Clear the custom workspace tab color at `index`. Emits
/// `cmux://session-changed` only when a color was actually cleared.
#[tauri::command]
pub fn session_reset_workspace_color(
    app: AppHandle,
    state: State<'_, SessionState>,
    index: i64,
) -> Result<AppSessionSnapshot, String> {
    let snapshot = reset_workspace_color_for_control(&app, &state, index)?;
    Ok(snapshot)
}

/// Set or clear a panel/tab custom title in the active workspace. Empty or
/// whitespace-only `title` clears the custom name.
#[tauri::command]
pub fn session_set_panel_title(
    app: AppHandle,
    state: State<'_, SessionState>,
    panel_id: String,
    title: String,
) -> Result<AppSessionSnapshot, String> {
    let snapshot = set_panel_title_for_control(&app, &state, &panel_id, &title)?;
    Ok(snapshot)
}

/// Pin or unpin a panel/tab in the active workspace.
#[tauri::command]
pub fn session_set_panel_pinned(
    app: AppHandle,
    state: State<'_, SessionState>,
    panel_id: String,
    pinned: bool,
) -> Result<AppSessionSnapshot, String> {
    let snapshot = set_panel_pinned_for_control(&app, &state, &panel_id, pinned)?;
    Ok(snapshot)
}

/// Mark a panel/tab read or unread in the active workspace.
#[tauri::command]
pub fn session_set_panel_unread(
    app: AppHandle,
    state: State<'_, SessionState>,
    panel_id: String,
    unread: bool,
) -> Result<AppSessionSnapshot, String> {
    let snapshot = set_panel_unread_for_control(&app, &state, &panel_id, unread)?;
    Ok(snapshot)
}

/// Mark a workspace read or unread.
#[tauri::command]
pub fn session_set_workspace_unread(
    app: AppHandle,
    state: State<'_, SessionState>,
    index: i64,
    unread: bool,
    preferred_panel_id: Option<String>,
) -> Result<AppSessionSnapshot, String> {
    let snapshot = set_workspace_unread_for_control(
        &app,
        &state,
        index,
        preferred_panel_id.as_deref(),
        unread,
    )?;
    Ok(snapshot)
}

/// Pin/unpin the workspace at `index`. Canonical
/// `WorkspaceReorderCoordinator.setPinned` semantics
/// (`WorkspaceReorderCoordinator.swift:467-472` + the pinned-ahead
/// normalization `reorderTabForPinnedState`, `:529-539`): pin floats the tab
/// to the end of the pinned prefix, unpin drops it to the front of the
/// unpinned segment; grouped tabs flip the flag only. Pin persists as
/// `Some(true)`, unpin as `None` (never `Some(false)`) — the port's golden
/// byte-stability decision (see `cmux-core/src/session.rs`). Emits
/// `cmux://session-changed` only when the pin state actually changed
/// (already-at-value / unknown index are no-ops) and returns the snapshot.
#[tauri::command]
pub fn session_set_workspace_pinned(
    app: AppHandle,
    state: State<'_, SessionState>,
    index: i64,
    pinned: bool,
) -> Result<AppSessionSnapshot, String> {
    let snapshot = set_workspace_pinned_for_control(&app, &state, index, pinned)?;
    Ok(snapshot)
}

/// Reorder the workspace at `index` toward `toIndex`. Canonical
/// `WorkspaceReorderCoordinator.reorderSidebarWorkspace`
/// (`WorkspaceReorderCoordinator.swift:243-257`) routing: a group-anchor mover
/// relocates its WHOLE group (all member rows, contiguous, anchor-first) via
/// the top-level path, any other mover takes the plain path with the
/// grouped-member section clamp and the global pin-tier clamp.
///
/// INDEX-SPACE CONTRACT: `index` identifies the mover as a position in
/// `tabs.workspaces` (matching select/close/rename/pin); `toIndex` is
/// interpreted in the row space canonical uses for that mover — a
/// `tabs.workspaces` index for non-anchors, a TOP-LEVEL row index for group
/// anchors (canonical UI feeds indices from the matching space via
/// `sidebarReorderWorkspaceIds`, Coordinator:171-183; the web drag lane does
/// the same). `usesTopLevelRows` (camelCase → `uses_top_level_rows`) enables
/// the canonical grouped-child promotion path too: a sidebar drag can lift a
/// grouped non-anchor into top-level row space before reordering it. Emits
/// `cmux://session-changed` only when the order actually changed
/// (out-of-range / clamped-back-to-place / normalization-reverted moves are
/// no-ops) and returns the snapshot. `toIndex` (camelCase) maps to the
/// `to_index` param.
#[tauri::command]
pub fn session_reorder_workspaces(
    app: AppHandle,
    state: State<'_, SessionState>,
    index: i64,
    to_index: i64,
    uses_top_level_rows: Option<bool>,
) -> Result<AppSessionSnapshot, String> {
    let snapshot = reorder_workspaces_for_control(
        &app,
        &state,
        index,
        to_index,
        uses_top_level_rows.unwrap_or(false),
    )?;
    Ok(snapshot)
}
