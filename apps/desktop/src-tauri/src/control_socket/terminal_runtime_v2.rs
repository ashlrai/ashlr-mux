use cmux_core::session::{
    AppSessionSnapshot, SessionPaneLayoutSnapshot, SessionSurfaceKindSnapshot,
    SessionWorkspaceLayoutSnapshot, SessionWorkspaceSnapshot,
};
use serde_json::{Map, Value};
use uuid::Uuid;

use crate::terminal::TerminalMaterializationEvent;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum TerminalRequestPlan {
    Create {
        window_index: usize,
        workspace_index: usize,
        pane_id: String,
        requested_workspace_id: Option<String>,
    },
    Input {
        window_index: usize,
        workspace_index: usize,
        surface_id: String,
        events: Vec<TerminalMaterializationEvent>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TerminalRequestError {
    pub(super) code: &'static str,
    pub(super) message: &'static str,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct TerminalSetFontPlan {
    pub(super) font_size: f64,
    pub(super) surface_id: Option<String>,
    pub(super) workspace_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct TerminalSetFontError {
    pub(super) code: &'static str,
    pub(super) message: &'static str,
    pub(super) font_size: Option<f64>,
}

pub(super) fn plan_terminal_set_font_request(
    params: &Map<String, Value>,
) -> Result<TerminalSetFontPlan, TerminalSetFontError> {
    let font_size = match params.get("font_size") {
        Some(Value::Bool(value)) => f64::from(u8::from(*value)),
        Some(Value::Number(value)) => value.as_f64().ok_or_else(missing_font_size)?,
        Some(Value::String(value)) => value.parse::<f64>().map_err(|_| missing_font_size())?,
        _ => return Err(missing_font_size()),
    };
    if !font_size.is_finite() || font_size <= 0.0 {
        return Err(TerminalSetFontError {
            code: "invalid_params",
            message: "font_size must be a positive number of points",
            font_size: Some(font_size),
        });
    }
    Ok(TerminalSetFontPlan {
        font_size,
        surface_id: params
            .get("surface_id")
            .and_then(Value::as_str)
            .map(str::to_owned),
        workspace_id: params
            .get("workspace_id")
            .and_then(Value::as_str)
            .map(str::to_owned),
    })
}

fn missing_font_size() -> TerminalSetFontError {
    TerminalSetFontError {
        code: "invalid_params",
        message: "Missing or invalid font_size",
        font_size: None,
    }
}

pub(super) fn plan_terminal_request_with_active_window(
    snapshot: &AppSessionSnapshot,
    method: &str,
    params: &Map<String, Value>,
    active_window_id: Option<&str>,
) -> Result<TerminalRequestPlan, TerminalRequestError> {
    let method = canonical_terminal_method(method)
        .ok_or_else(|| error("method_not_found", "Unknown terminal method"))?;

    if method == "terminal.input" {
        return plan_input(snapshot, params, active_window_id);
    }
    plan_create(snapshot, params, active_window_id)
}

fn plan_create(
    snapshot: &AppSessionSnapshot,
    params: &Map<String, Value>,
    active_window_id: Option<&str>,
) -> Result<TerminalRequestPlan, TerminalRequestError> {
    let explicit_window = explicit_window_index(snapshot, params)?;
    let workspace_id = parse_optional_uuid(params, "workspace_id")?.map(|id| id.to_string());
    let terminal_id = first_valid_terminal_alias(params).map(|id| id.to_string());
    let window_index = route_window(
        snapshot,
        params,
        explicit_window,
        workspace_id.as_deref(),
        terminal_id.as_deref(),
        active_window_id,
    )
    .ok_or_else(|| error("unavailable", "Workspace context is unavailable"))?;
    let workspace_index = resolve_workspace(
        snapshot,
        window_index,
        params,
        workspace_id.as_deref(),
        terminal_id.as_deref(),
    )
    .ok_or_else(|| error("not_found", "Workspace not found"))?;
    let workspace = &snapshot.windows[window_index].tab_manager.workspaces[workspace_index];
    let pane_id =
        focused_or_first_pane_id(workspace).ok_or_else(|| error("not_found", "Pane not found"))?;
    Ok(TerminalRequestPlan::Create {
        window_index,
        workspace_index,
        pane_id,
        requested_workspace_id: workspace_id,
    })
}

fn plan_input(
    snapshot: &AppSessionSnapshot,
    params: &Map<String, Value>,
    active_window_id: Option<&str>,
) -> Result<TerminalRequestPlan, TerminalRequestError> {
    let text = params
        .get("text")
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
        .ok_or_else(|| error("invalid_params", "Missing text"))?;
    let workspace_id = parse_optional_uuid(params, "workspace_id")?.map(|id| id.to_string());
    let terminal_id = parse_terminal_aliases(params)?.map(|id| id.to_string());
    let explicit_window = explicit_window_index(snapshot, params).ok().flatten();
    let Some(window_index) = route_window(
        snapshot,
        params,
        explicit_window,
        workspace_id.as_deref(),
        terminal_id.as_deref(),
        active_window_id,
    ) else {
        return Err(error("not_found", "Terminal surface not found"));
    };
    let Some(workspace_index) = resolve_workspace(
        snapshot,
        window_index,
        params,
        workspace_id.as_deref(),
        terminal_id.as_deref(),
    ) else {
        return Err(error("not_found", "Terminal surface not found"));
    };
    let workspace = &snapshot.windows[window_index].tab_manager.workspaces[workspace_index];
    let surface_id = resolve_terminal_surface(workspace, terminal_id.as_deref())
        .ok_or_else(|| error("not_found", "Terminal surface not found"))?;
    Ok(TerminalRequestPlan::Input {
        window_index,
        workspace_index,
        surface_id,
        events: parse_terminal_input(text),
    })
}

fn canonical_terminal_method(method: &str) -> Option<&str> {
    match method {
        "terminal.create" | "mobile.terminal.create" => Some("terminal.create"),
        "terminal.input" | "mobile.terminal.input" => Some("terminal.input"),
        _ => None,
    }
}

fn error(code: &'static str, message: &'static str) -> TerminalRequestError {
    TerminalRequestError { code, message }
}

fn parse_optional_uuid(
    params: &Map<String, Value>,
    key: &str,
) -> Result<Option<Uuid>, TerminalRequestError> {
    let Some(value) = params.get(key) else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }
    value
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .and_then(|value| Uuid::parse_str(value).ok())
        .map(Some)
        .ok_or_else(|| error("invalid_params", "Missing or invalid workspace_id"))
}

fn parse_terminal_aliases(
    params: &Map<String, Value>,
) -> Result<Option<Uuid>, TerminalRequestError> {
    let mut selected = None;
    for key in ["surface_id", "terminal_id", "tab_id"] {
        let Some(value) = params.get(key) else {
            continue;
        };
        if value.is_null() {
            continue;
        }
        let candidate = value
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .and_then(|value| Uuid::parse_str(value).ok())
            .ok_or_else(|| error("invalid_params", "Missing or invalid terminal_id"))?;
        if selected.is_some_and(|selected| selected != candidate) {
            return Err(error("invalid_params", "Conflicting terminal identifiers"));
        }
        selected = Some(candidate);
    }
    Ok(selected)
}

pub(super) fn terminal_create_response_terminal_id(
    params: &Map<String, Value>,
) -> Result<Option<String>, TerminalRequestError> {
    parse_terminal_aliases(params).map(|terminal_id| terminal_id.map(|id| id.to_string()))
}

fn first_valid_terminal_alias(params: &Map<String, Value>) -> Option<Uuid> {
    ["surface_id", "terminal_id", "tab_id"]
        .into_iter()
        .find_map(|key| {
            params
                .get(key)
                .and_then(Value::as_str)
                .map(str::trim)
                .and_then(|value| Uuid::parse_str(value).ok())
        })
}

fn explicit_window_index(
    snapshot: &AppSessionSnapshot,
    params: &Map<String, Value>,
) -> Result<Option<usize>, TerminalRequestError> {
    let Some(value) = params.get("window_id") else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }
    let window_id = value
        .as_str()
        .map(str::trim)
        .filter(|value| Uuid::parse_str(value).is_ok())
        .ok_or_else(|| error("unavailable", "Workspace context is unavailable"))?;
    snapshot
        .windows
        .iter()
        .position(|window| window.window_id.as_deref() == Some(window_id))
        .map(Some)
        .ok_or_else(|| error("unavailable", "Workspace context is unavailable"))
}

fn route_window(
    snapshot: &AppSessionSnapshot,
    params: &Map<String, Value>,
    explicit_window: Option<usize>,
    workspace_id: Option<&str>,
    terminal_id: Option<&str>,
    active_window_id: Option<&str>,
) -> Option<usize> {
    if params
        .get("window_id")
        .is_some_and(|value| !value.is_null())
    {
        return explicit_window;
    }
    if let Some(group_id) = valid_uuid_param(params, "group_id") {
        if let Some(index) = snapshot.windows.iter().position(|window| {
            window
                .tab_manager
                .workspace_groups
                .as_deref()
                .unwrap_or_default()
                .iter()
                .any(|group| group.id == group_id)
        }) {
            return Some(index);
        }
    }
    if let Some(workspace_id) = workspace_id {
        if let Some(index) = snapshot.windows.iter().position(|window| {
            window
                .tab_manager
                .workspaces
                .iter()
                .any(|workspace| workspace.workspace_id.as_deref() == Some(workspace_id))
        }) {
            return Some(index);
        }
    }
    if let Some(terminal_id) = terminal_id {
        if let Some(index) = snapshot.windows.iter().position(|window| {
            window
                .tab_manager
                .workspaces
                .iter()
                .any(|workspace| workspace_has_surface(workspace, terminal_id))
        }) {
            return Some(index);
        }
    }
    if let Some(pane_id) = valid_uuid_param(params, "pane_id") {
        if let Some(index) = snapshot.windows.iter().position(|window| {
            window
                .tab_manager
                .workspaces
                .iter()
                .any(|workspace| workspace_has_pane(workspace, &pane_id))
        }) {
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

fn resolve_workspace(
    snapshot: &AppSessionSnapshot,
    window_index: usize,
    params: &Map<String, Value>,
    workspace_id: Option<&str>,
    terminal_id: Option<&str>,
) -> Option<usize> {
    let window = snapshot.windows.get(window_index)?;
    if let Some(workspace_id) = workspace_id {
        return window
            .tab_manager
            .workspaces
            .iter()
            .position(|workspace| workspace.workspace_id.as_deref() == Some(workspace_id));
    }
    if let Some(terminal_id) = terminal_id {
        return window
            .tab_manager
            .workspaces
            .iter()
            .position(|workspace| workspace_has_surface(workspace, terminal_id));
    }
    if let Some(pane_id) = valid_uuid_param(params, "pane_id") {
        return window
            .tab_manager
            .workspaces
            .iter()
            .position(|workspace| workspace_has_pane(workspace, &pane_id));
    }
    window
        .tab_manager
        .selected_workspace_index
        .and_then(|index| usize::try_from(index).ok())
        .filter(|index| *index < window.tab_manager.workspaces.len())
}

fn valid_uuid_param(params: &Map<String, Value>, key: &str) -> Option<String> {
    params
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| Uuid::parse_str(value).is_ok())
        .map(str::to_owned)
}

fn resolve_terminal_surface(
    workspace: &SessionWorkspaceSnapshot,
    requested: Option<&str>,
) -> Option<String> {
    if let Some(requested) = requested {
        return surface_is_terminal(workspace, requested).then(|| requested.to_string());
    }
    workspace
        .focused_panel_id
        .as_deref()
        .filter(|surface_id| surface_is_terminal(workspace, surface_id))
        .map(str::to_owned)
        .or_else(|| {
            ordered_panel_ids(workspace.layout.as_ref()).find_map(|surface_id| {
                surface_is_terminal(workspace, surface_id).then(|| surface_id.to_string())
            })
        })
}

fn workspace_has_surface(workspace: &SessionWorkspaceSnapshot, surface_id: &str) -> bool {
    workspace
        .surfaces
        .as_deref()
        .unwrap_or_default()
        .iter()
        .any(|surface| surface.surface_id == surface_id)
}

fn surface_is_terminal(workspace: &SessionWorkspaceSnapshot, surface_id: &str) -> bool {
    workspace
        .surfaces
        .as_deref()
        .unwrap_or_default()
        .iter()
        .find(|surface| surface.surface_id == surface_id)
        .is_some_and(|surface| {
            matches!(
                surface.kind,
                SessionSurfaceKindSnapshot::Terminal
                    | SessionSurfaceKindSnapshot::RemoteTerminal { .. }
            )
        })
}

fn workspace_has_pane(workspace: &SessionWorkspaceSnapshot, pane_id: &str) -> bool {
    find_pane(workspace.layout.as_ref(), pane_id).is_some()
}

fn focused_or_first_pane_id(workspace: &SessionWorkspaceSnapshot) -> Option<String> {
    workspace
        .focused_pane_id
        .clone()
        .or_else(|| {
            workspace
                .focused_panel_id
                .as_deref()
                .and_then(|surface_id| pane_containing(workspace.layout.as_ref(), surface_id))
                .map(str::to_owned)
        })
        .or_else(|| first_pane_id(workspace.layout.as_ref()).map(str::to_owned))
}

fn find_pane<'a>(
    layout: Option<&'a SessionWorkspaceLayoutSnapshot>,
    pane_id: &str,
) -> Option<&'a SessionPaneLayoutSnapshot> {
    match layout? {
        SessionWorkspaceLayoutSnapshot::Pane(pane) => {
            (pane.pane_id.as_deref() == Some(pane_id)).then_some(pane)
        }
        SessionWorkspaceLayoutSnapshot::Split(split) => find_pane(Some(&split.first), pane_id)
            .or_else(|| find_pane(Some(&split.second), pane_id)),
    }
}

fn pane_containing<'a>(
    layout: Option<&'a SessionWorkspaceLayoutSnapshot>,
    surface_id: &str,
) -> Option<&'a str> {
    match layout? {
        SessionWorkspaceLayoutSnapshot::Pane(pane) => pane
            .panel_ids
            .iter()
            .any(|candidate| candidate == surface_id)
            .then_some(pane.pane_id.as_deref())
            .flatten(),
        SessionWorkspaceLayoutSnapshot::Split(split) => {
            pane_containing(Some(&split.first), surface_id)
                .or_else(|| pane_containing(Some(&split.second), surface_id))
        }
    }
}

fn first_pane_id(layout: Option<&SessionWorkspaceLayoutSnapshot>) -> Option<&str> {
    match layout? {
        SessionWorkspaceLayoutSnapshot::Pane(pane) => pane.pane_id.as_deref(),
        SessionWorkspaceLayoutSnapshot::Split(split) => {
            first_pane_id(Some(&split.first)).or_else(|| first_pane_id(Some(&split.second)))
        }
    }
}

fn ordered_panel_ids(
    layout: Option<&SessionWorkspaceLayoutSnapshot>,
) -> impl Iterator<Item = &str> {
    fn collect<'a>(layout: Option<&'a SessionWorkspaceLayoutSnapshot>, ids: &mut Vec<&'a str>) {
        match layout {
            Some(SessionWorkspaceLayoutSnapshot::Pane(pane)) => {
                ids.extend(pane.panel_ids.iter().map(String::as_str));
            }
            Some(SessionWorkspaceLayoutSnapshot::Split(split)) => {
                collect(Some(&split.first), ids);
                collect(Some(&split.second), ids);
            }
            None => {}
        }
    }
    let mut ids = Vec::new();
    collect(layout, &mut ids);
    ids.into_iter()
}

fn parse_terminal_input(text: &str) -> Vec<TerminalMaterializationEvent> {
    let source = text.as_bytes();
    let mut input = Vec::with_capacity(source.len());
    let mut events = Vec::new();
    let mut index = 0;
    while index < source.len() {
        match source[index] {
            b'\r' => {
                input.push(b'\r');
                index += 1;
                if source.get(index) == Some(&b'\n') {
                    index += 1;
                }
            }
            b'\n' => {
                input.push(b'\r');
                index += 1;
            }
            0x08 | 0x7f => {
                input.push(0x7f);
                index += 1;
            }
            0x1b => {
                if matches!(source.get(index + 1), Some(0x08) | Some(0x7f)) {
                    input.extend_from_slice(&[0x1b, 0x7f]);
                    index += 2;
                } else if let Some((length, bytes)) = navigation_input(source, index) {
                    input.extend_from_slice(&bytes);
                    index += length;
                } else if let Some(length) = terminal_control_sequence_length(source, index) {
                    if !input.is_empty() {
                        events.push(TerminalMaterializationEvent::Input(std::mem::take(
                            &mut input,
                        )));
                    }
                    events.push(TerminalMaterializationEvent::ProcessOutput(
                        source[index..index + length].to_vec(),
                    ));
                    index += length;
                } else {
                    input.push(0x1b);
                    index += 1;
                }
            }
            byte => {
                input.push(byte);
                index += 1;
            }
        }
    }
    if !input.is_empty() {
        events.push(TerminalMaterializationEvent::Input(input));
    }
    events
}

fn navigation_input(source: &[u8], start: usize) -> Option<(usize, Vec<u8>)> {
    let introducer = *source.get(start + 1)?;
    if !matches!(introducer, b'[' | b'O') {
        return None;
    }
    let final_byte = *source.get(start + 2)?;
    if matches!(final_byte, b'A' | b'B' | b'C' | b'D' | b'H' | b'F') {
        return Some((3, vec![0x1b, b'[', final_byte]));
    }
    if introducer == b'['
        && matches!(final_byte, b'1' | b'3' | b'4' | b'5' | b'6')
        && source.get(start + 3) == Some(&b'~')
    {
        return Some((4, source[start..start + 4].to_vec()));
    }
    None
}

fn terminal_control_sequence_length(source: &[u8], start: usize) -> Option<usize> {
    let introducer = *source.get(start + 1)?;
    let allows_bel = introducer == b']';
    if !allows_bel && !matches!(introducer, b'P' | b'^' | b'_') {
        return None;
    }
    let mut index = start + 2;
    while index < source.len() {
        if allows_bel && source[index] == 0x07 {
            return Some(index - start + 1);
        }
        if source[index] == 0x1b && source.get(index + 1) == Some(&b'\\') {
            return Some(index - start + 2);
        }
        index += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use cmux_core::session::{
        SessionSurfaceSnapshot, SessionTabManagerSnapshot, SessionWindowSnapshot,
        SessionWorkspaceGroupSnapshot, SESSION_SNAPSHOT_SCHEMA_VERSION,
    };
    use serde_json::json;

    const WINDOW: &str = "11111111-1111-4111-8111-111111111111";
    const WORKSPACE: &str = "22222222-2222-4222-8222-222222222222";
    const PANE: &str = "33333333-3333-4333-8333-333333333333";
    const SURFACE: &str = "44444444-4444-4444-8444-444444444444";
    const OTHER_WINDOW: &str = "55555555-5555-4555-8555-555555555555";
    const OTHER_WORKSPACE: &str = "66666666-6666-4666-8666-666666666666";
    const OTHER_PANE: &str = "77777777-7777-4777-8777-777777777777";
    const OTHER_SURFACE: &str = "88888888-8888-4888-8888-888888888888";
    const GROUP: &str = "99999999-9999-4999-8999-999999999999";

    fn pane(pane_id: &str, surface_id: &str) -> SessionPaneLayoutSnapshot {
        SessionPaneLayoutSnapshot {
            pane_id: Some(pane_id.into()),
            panel_ids: vec![surface_id.into()],
            selected_panel_id: Some(surface_id.into()),
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
        }
    }

    fn workspace(workspace_id: &str, pane_id: &str, surface_id: &str) -> SessionWorkspaceSnapshot {
        SessionWorkspaceSnapshot {
            workspace_id: Some(workspace_id.into()),
            process_title: "shell".into(),
            current_directory: Some("C:/repo".into()),
            focused_panel_id: Some(surface_id.into()),
            focused_pane_id: Some(pane_id.into()),
            layout: Some(SessionWorkspaceLayoutSnapshot::Pane(pane(
                pane_id, surface_id,
            ))),
            surfaces: Some(vec![SessionSurfaceSnapshot {
                surface_id: surface_id.into(),
                pane_id: pane_id.into(),
                generation: 1,
                kind: SessionSurfaceKindSnapshot::Terminal,
                metadata: Default::default(),
                terminal_startup: None,
                scrollback: None,
            }]),
            ..Default::default()
        }
    }

    fn snapshot() -> AppSessionSnapshot {
        let first = workspace(WORKSPACE, PANE, SURFACE);
        let second = workspace(OTHER_WORKSPACE, OTHER_PANE, OTHER_SURFACE);
        AppSessionSnapshot {
            version: SESSION_SNAPSHOT_SCHEMA_VERSION,
            created_at: 0,
            windows: vec![
                SessionWindowSnapshot {
                    window_id: Some(WINDOW.into()),
                    selected_workspace_id: Some(WORKSPACE.into()),
                    dock: None,
                    tab_manager: SessionTabManagerSnapshot {
                        selected_workspace_index: Some(0),
                        workspaces: vec![first],
                        workspace_groups: None,
                    },
                },
                SessionWindowSnapshot {
                    window_id: Some(OTHER_WINDOW.into()),
                    selected_workspace_id: Some(OTHER_WORKSPACE.into()),
                    dock: None,
                    tab_manager: SessionTabManagerSnapshot {
                        selected_workspace_index: Some(0),
                        workspaces: vec![second],
                        workspace_groups: Some(vec![SessionWorkspaceGroupSnapshot {
                            id: GROUP.into(),
                            name: "Group".into(),
                            ..Default::default()
                        }]),
                    },
                },
            ],
        }
    }

    fn params(value: Value) -> Map<String, Value> {
        value.as_object().expect("object params").clone()
    }

    #[test]
    fn aliases_share_exact_create_and_input_plans() {
        let snapshot = snapshot();
        for (bare, mobile, value) in [
            (
                "terminal.create",
                "mobile.terminal.create",
                json!({"workspace_id": WORKSPACE}),
            ),
            (
                "terminal.input",
                "mobile.terminal.input",
                json!({"workspace_id": WORKSPACE, "surface_id": SURFACE, "text": "hello"}),
            ),
        ] {
            let params = params(value);
            assert_eq!(
                plan_terminal_request_with_active_window(&snapshot, bare, &params, Some(WINDOW)),
                plan_terminal_request_with_active_window(&snapshot, mobile, &params, Some(WINDOW))
            );
        }
    }

    #[test]
    fn input_validation_order_and_messages_are_exact() {
        let snapshot = snapshot();
        for (value, expected) in [
            (
                json!({"workspace_id":"bad", "surface_id":"bad"}),
                ("invalid_params", "Missing text"),
            ),
            (
                json!({"workspace_id":"bad", "text":"x"}),
                ("invalid_params", "Missing or invalid workspace_id"),
            ),
            (
                json!({"surface_id":"bad", "text":"x"}),
                ("invalid_params", "Missing or invalid terminal_id"),
            ),
            (
                json!({"surface_id":SURFACE, "terminal_id":OTHER_SURFACE, "text":"x"}),
                ("invalid_params", "Conflicting terminal identifiers"),
            ),
        ] {
            let error = plan_terminal_request_with_active_window(
                &snapshot,
                "terminal.input",
                &params(value),
                Some(WINDOW),
            )
            .unwrap_err();
            assert_eq!((error.code, error.message), expected);
        }
    }

    #[test]
    fn create_response_validates_terminal_aliases_after_routing() {
        assert_eq!(
            terminal_create_response_terminal_id(&params(json!({"surface_id":"bad"}))),
            Err(error("invalid_params", "Missing or invalid terminal_id"))
        );
        assert_eq!(
            terminal_create_response_terminal_id(&params(json!({
                "surface_id": SURFACE,
                "terminal_id": OTHER_SURFACE,
            }))),
            Err(error("invalid_params", "Conflicting terminal identifiers"))
        );
        assert_eq!(
            terminal_create_response_terminal_id(&params(json!({
                "surface_id": SURFACE,
                "terminal_id": SURFACE,
            }))),
            Ok(Some(SURFACE.into()))
        );
    }

    #[test]
    fn routing_precedence_covers_explicit_window_group_workspace_terminal_pane_and_active() {
        let snapshot = snapshot();
        let cases = [
            json!({"window_id":OTHER_WINDOW, "text":"x"}),
            json!({"group_id":GROUP, "text":"x"}),
            json!({"workspace_id":OTHER_WORKSPACE, "text":"x"}),
            json!({"surface_id":OTHER_SURFACE, "text":"x"}),
            json!({"pane_id":OTHER_PANE, "text":"x"}),
            json!({"text":"x"}),
        ];
        for value in cases {
            assert!(matches!(
                plan_terminal_request_with_active_window(
                    &snapshot,
                    "terminal.input",
                    &params(value),
                    Some(OTHER_WINDOW),
                ),
                Ok(TerminalRequestPlan::Input {
                    window_index: 1,
                    workspace_index: 0,
                    ref surface_id,
                    ..
                }) if surface_id == OTHER_SURFACE
            ));
        }
    }

    #[test]
    fn explicit_terminal_is_workspace_scoped_and_terminal_kind_required() {
        let mut snapshot = snapshot();
        let cross_workspace = plan_terminal_request_with_active_window(
            &snapshot,
            "terminal.input",
            &params(json!({
                "workspace_id":WORKSPACE,
                "surface_id":OTHER_SURFACE,
                "text":"x"
            })),
            Some(WINDOW),
        )
        .unwrap_err();
        assert_eq!(
            (cross_workspace.code, cross_workspace.message),
            ("not_found", "Terminal surface not found")
        );
        snapshot.windows[0].tab_manager.workspaces[0]
            .surfaces
            .as_mut()
            .unwrap()[0]
            .kind = SessionSurfaceKindSnapshot::Markdown { path: None };
        let wrong_kind = plan_terminal_request_with_active_window(
            &snapshot,
            "terminal.input",
            &params(json!({"surface_id":SURFACE, "text":"x"})),
            Some(WINDOW),
        )
        .unwrap_err();
        assert_eq!(
            (wrong_kind.code, wrong_kind.message),
            ("not_found", "Terminal surface not found")
        );
    }

    #[test]
    fn create_uses_focused_pane_and_invalid_explicit_window_wins() {
        let snapshot = snapshot();
        assert_eq!(
            plan_terminal_request_with_active_window(
                &snapshot,
                "terminal.create",
                &params(json!({"workspace_id":WORKSPACE})),
                Some(OTHER_WINDOW),
            ),
            Ok(TerminalRequestPlan::Create {
                window_index: 0,
                workspace_index: 0,
                pane_id: PANE.into(),
                requested_workspace_id: Some(WORKSPACE.into()),
            })
        );
        let error = plan_terminal_request_with_active_window(
            &snapshot,
            "terminal.create",
            &params(json!({"window_id":"bad", "workspace_id":"bad"})),
            Some(WINDOW),
        )
        .unwrap_err();
        assert_eq!(
            (error.code, error.message),
            ("unavailable", "Workspace context is unavailable")
        );
    }

    #[test]
    fn input_grammar_preserves_utf8_normalizes_keys_and_splits_control_strings() {
        let text = "hé\r\nllo\n\t\u{8}\u{7f}\u{1b}\u{7f}\u{1b}[A\u{1b}[5~\u{1b}]0;title\u{7}";
        assert_eq!(
            parse_terminal_input(text),
            vec![
                TerminalMaterializationEvent::Input(
                    "hé\rllo\r\t"
                        .as_bytes()
                        .iter()
                        .copied()
                        .chain([0x7f, 0x7f, 0x1b, 0x7f])
                        .chain(b"\x1b[A\x1b[5~".iter().copied())
                        .collect(),
                ),
                TerminalMaterializationEvent::ProcessOutput(b"\x1b]0;title\x07".to_vec()),
            ]
        );
    }

    #[test]
    fn incomplete_string_control_is_input_not_process_output() {
        assert_eq!(
            parse_terminal_input("\u{1b}]0;partial"),
            vec![TerminalMaterializationEvent::Input(
                b"\x1b]0;partial".to_vec()
            )]
        );
    }
}
