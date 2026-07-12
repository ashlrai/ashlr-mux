use cmux_core::session::{
    AppSessionSnapshot, SessionSplitOrientation, SessionSurfaceKindSnapshot,
    SessionSurfaceTerminalStartupSnapshot, SessionWorkspaceLayoutSnapshot,
};
use cmux_core::session_ops::{self, PaneResizeDirection};
use cmux_core::surface_lifecycle::{CloseIntent, SurfaceLifecycleModel};
use cmux_ipc::{ControlCallResult, JsonValue};
use serde_json::{json, Map, Value};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq)]
pub(super) struct LifecycleEvent {
    pub name: &'static str,
    pub payload: Value,
}

#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)] // Variants are introduced together; dispatch branches consume them incrementally.
pub(super) enum LifecycleEffect {
    TerminalCreate {
        surface_id: String,
        generation: u64,
        command: Option<String>,
        working_directory: Option<String>,
    },
    TerminalReplace {
        surface_id: String,
        previous_generation: u64,
        generation: u64,
        command: String,
        working_directory: Option<String>,
    },
    BrowserAttach {
        surface_id: String,
        generation: u64,
        url: Option<String>,
    },
    RuntimeTeardown {
        surface_id: String,
        generation: u64,
    },
    DockCreate {
        dock_surface_id: String,
        kind: String,
        url: Option<String>,
    },
    RemoteCreate {
        remote_session_id: String,
        kind: String,
    },
    ActivateWindow {
        window_id: String,
    },
    PersistSession,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct LifecycleDispatchContext {
    pub viewport_size: Option<(f64, f64)>,
    pub browser_enabled: bool,
    pub dock_available: bool,
}

#[derive(Debug)]
pub(super) struct LifecycleTransition {
    pub snapshot: AppSessionSnapshot,
    pub result: ControlCallResult,
    pub changed: bool,
    pub events: Vec<LifecycleEvent>,
    pub effects: Vec<LifecycleEffect>,
}

pub(super) trait LifecycleEffectExecutor {
    type Error;

    /// Validate and acquire all resources needed by an effect without making
    /// it externally visible. The transition is committed only after every
    /// effect has staged successfully.
    fn stage(&mut self, effect: &LifecycleEffect) -> Result<(), Self::Error>;
    fn commit_staged(&mut self) -> Result<(), Self::Error>;
    fn rollback_staged(&mut self);
}

pub(super) fn commit_lifecycle_transition<E: LifecycleEffectExecutor>(
    target: &mut AppSessionSnapshot,
    transition: LifecycleTransition,
    executor: &mut E,
) -> Result<ControlCallResult, E::Error> {
    for effect in &transition.effects {
        if let Err(error) = executor.stage(effect) {
            executor.rollback_staged();
            return Err(error);
        }
    }
    if let Err(error) = executor.commit_staged() {
        executor.rollback_staged();
        return Err(error);
    }
    if transition.changed {
        *target = transition.snapshot;
    }
    Ok(transition.result)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct RuntimeArrival {
    pub window_id: String,
    pub workspace_id: String,
    pub pane_id: String,
    pub surface_id: String,
    pub remote_session_id: String,
    pub generation: u64,
}

impl RuntimeArrival {
    pub fn remote(
        window_id: impl Into<String>,
        workspace_id: impl Into<String>,
        pane_id: impl Into<String>,
        surface_id: impl Into<String>,
        remote_session_id: impl Into<String>,
        generation: u64,
    ) -> Self {
        Self {
            window_id: window_id.into(),
            workspace_id: workspace_id.into(),
            pane_id: pane_id.into(),
            surface_id: surface_id.into(),
            remote_session_id: remote_session_id.into(),
            generation,
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct RuntimeReconciliation {
    pub snapshot: AppSessionSnapshot,
}

impl RuntimeReconciliation {
    pub fn directory_apply_count(&self, surface_id: &str) -> usize {
        self.snapshot
            .windows
            .iter()
            .flat_map(|window| &window.tab_manager.workspaces)
            .flat_map(|workspace| workspace.surfaces.as_deref().unwrap_or_default())
            .filter(|record| {
                record.surface_id == surface_id
                    && record.metadata.directory_provenance.as_deref() == Some("remote_report")
            })
            .count()
    }
}

pub(super) fn reconcile_runtime_arrival(
    snapshot: &AppSessionSnapshot,
    arrival: RuntimeArrival,
) -> RuntimeReconciliation {
    let mut next = snapshot.clone();
    let Some(window_index) = next
        .windows
        .iter()
        .position(|window| window.window_id.as_deref() == Some(&arrival.window_id))
    else {
        return RuntimeReconciliation { snapshot: next };
    };
    let Some(workspace_index) = next.windows[window_index]
        .tab_manager
        .workspaces
        .iter()
        .position(|workspace| workspace.workspace_id.as_deref() == Some(&arrival.workspace_id))
    else {
        return RuntimeReconciliation { snapshot: next };
    };
    let workspace = &mut next.windows[window_index].tab_manager.workspaces[workspace_index];
    if workspace
        .surfaces
        .as_deref()
        .unwrap_or_default()
        .iter()
        .any(|record| record.surface_id == arrival.surface_id)
    {
        return RuntimeReconciliation { snapshot: next };
    }
    let pending_path = workspace.pending_remote_pwds.as_mut().and_then(|pending| {
        pending
            .iter()
            .position(|item| item.remote_session_id == arrival.surface_id)
            .map(|index| pending.remove(index).path)
    });
    let mut layout = session_ops::single_pane(&arrival.surface_id);
    let SessionWorkspaceLayoutSnapshot::Pane(pane) = &mut layout else {
        unreachable!()
    };
    pane.pane_id = Some(arrival.pane_id.clone());
    workspace.layout = Some(layout);
    workspace.focused_panel_id = Some(arrival.surface_id.clone());
    workspace.surfaces = Some(vec![cmux_core::session::SessionSurfaceSnapshot {
        surface_id: arrival.surface_id,
        pane_id: arrival.pane_id,
        generation: arrival.generation,
        kind: SessionSurfaceKindSnapshot::RemoteTerminal {
            remote_session_id: Some(arrival.remote_session_id),
            remote_context: None,
            arrival_generation: Some(arrival.generation),
        },
        metadata: cmux_core::session::SessionSurfaceMetadataSnapshot {
            reported_directory: pending_path,
            directory_provenance: Some("remote_report".into()),
            ..Default::default()
        },
        terminal_startup: None,
    }]);
    RuntimeReconciliation { snapshot: next }
}

pub(super) fn dispatch_lifecycle_request(
    snapshot: &AppSessionSnapshot,
    method: &str,
    params: &Map<String, Value>,
    context: &LifecycleDispatchContext,
) -> LifecycleTransition {
    match method {
        "surface.current" => surface_current(snapshot, params),
        "surface.list" => surface_list(snapshot, params),
        "surface.create" => surface_create(snapshot, params, context),
        "surface.action" | "tab.action" => surface_action(snapshot, params),
        "surface.report_pwd" => surface_report_pwd(snapshot, params),
        "surface.respawn" => surface_respawn(snapshot, params),
        "surface.close" => surface_close(snapshot, params),
        "surface.focus" => surface_focus(snapshot, params),
        "surface.move" => surface_move(snapshot, params),
        "pane.resize" => pane_resize(snapshot, params, context),
        "pane.focus" => pane_focus(snapshot, params),
        "pane.create" | "surface.split" => pane_create(snapshot, method, params, context),
        _ => error(
            snapshot,
            "method_not_found",
            "Unknown lifecycle method",
            None,
        ),
    }
}

fn json_result(value: Value) -> ControlCallResult {
    ControlCallResult::Ok(JsonValue::try_from(value).expect("lifecycle payload is valid JSON"))
}

fn ok_transition(
    snapshot: AppSessionSnapshot,
    value: Value,
    events: Vec<LifecycleEvent>,
    effects: Vec<LifecycleEffect>,
) -> LifecycleTransition {
    let changed = !events.is_empty() || !effects.is_empty();
    LifecycleTransition {
        snapshot,
        result: json_result(value),
        changed,
        events,
        effects,
    }
}

fn read_transition(snapshot: &AppSessionSnapshot, value: Value) -> LifecycleTransition {
    LifecycleTransition {
        snapshot: snapshot.clone(),
        result: json_result(value),
        changed: false,
        events: Vec::new(),
        effects: Vec::new(),
    }
}

fn error(
    snapshot: &AppSessionSnapshot,
    code: &str,
    message: &str,
    data: Option<Value>,
) -> LifecycleTransition {
    #[cfg(test)]
    eprintln!("lifecycle error {code}: {message}");
    LifecycleTransition {
        snapshot: snapshot.clone(),
        result: ControlCallResult::Err {
            code: code.into(),
            message: message.into(),
            data: data.and_then(|value| JsonValue::try_from(value).ok()),
        },
        changed: false,
        events: Vec::new(),
        effects: Vec::new(),
    }
}

#[derive(Clone)]
struct Scope {
    window_index: usize,
    workspace_index: usize,
    window_id: String,
    workspace_id: String,
}

fn scope(
    snapshot: &AppSessionSnapshot,
    params: &Map<String, Value>,
) -> Result<Scope, (&'static str, &'static str)> {
    let window_index = if let Some(Value::String(requested)) = params.get("window_id") {
        snapshot
            .windows
            .iter()
            .position(|window| window.window_id.as_deref() == Some(requested))
            .ok_or(("unavailable", "TabManager not available"))?
    } else if let Some(surface) = params
        .get("surface_id")
        .or_else(|| params.get("tab_id"))
        .and_then(Value::as_str)
    {
        snapshot
            .windows
            .iter()
            .position(|window| {
                window.tab_manager.workspaces.iter().any(|workspace| {
                    workspace
                        .surfaces
                        .as_deref()
                        .unwrap_or_default()
                        .iter()
                        .any(|record| record.surface_id == surface)
                        || workspace.layout.as_ref().is_some_and(|layout| {
                            layout_surface_ids(layout).iter().any(|id| id == surface)
                        })
                })
            })
            .unwrap_or(0)
    } else {
        0
    };
    let window = snapshot
        .windows
        .get(window_index)
        .ok_or(("unavailable", "TabManager not available"))?;
    let workspace_index =
        if let Some(requested) = params.get("workspace_id").and_then(Value::as_str) {
            window
                .tab_manager
                .workspaces
                .iter()
                .position(|workspace| workspace.workspace_id.as_deref() == Some(requested))
                .ok_or(("not_found", "Workspace not found"))?
        } else if let Some(surface) = params
            .get("surface_id")
            .or_else(|| params.get("tab_id"))
            .and_then(Value::as_str)
        {
            window
                .tab_manager
                .workspaces
                .iter()
                .position(|workspace| {
                    workspace
                        .surfaces
                        .as_deref()
                        .unwrap_or_default()
                        .iter()
                        .any(|record| record.surface_id == surface)
                        || workspace.layout.as_ref().is_some_and(|layout| {
                            layout_surface_ids(layout).iter().any(|id| id == surface)
                        })
                })
                .unwrap_or_else(|| {
                    usize::try_from(window.tab_manager.selected_workspace_index.unwrap_or(0))
                        .unwrap_or(0)
                })
        } else {
            usize::try_from(window.tab_manager.selected_workspace_index.unwrap_or(0)).unwrap_or(0)
        };
    let workspace = window
        .tab_manager
        .workspaces
        .get(workspace_index)
        .ok_or(("not_found", "Workspace not found"))?;
    Ok(Scope {
        window_index,
        workspace_index,
        window_id: window
            .window_id
            .clone()
            .unwrap_or_else(|| format!("window:{window_index}")),
        workspace_id: workspace
            .workspace_id
            .clone()
            .unwrap_or_else(|| format!("workspace:{workspace_index}")),
    })
}

fn layout_surface_ids(layout: &SessionWorkspaceLayoutSnapshot) -> Vec<String> {
    match layout {
        SessionWorkspaceLayoutSnapshot::Pane(pane) => pane.panel_ids.clone(),
        SessionWorkspaceLayoutSnapshot::Split(split) => {
            let mut ids = layout_surface_ids(&split.first);
            ids.extend(layout_surface_ids(&split.second));
            ids
        }
    }
}

fn kind_name(kind: &SessionSurfaceKindSnapshot) -> &'static str {
    match kind {
        SessionSurfaceKindSnapshot::Terminal
        | SessionSurfaceKindSnapshot::RemoteTerminal { .. } => "terminal",
        SessionSurfaceKindSnapshot::Browser { .. } => "browser",
        SessionSurfaceKindSnapshot::AgentSession { .. } => "agentSession",
        SessionSurfaceKindSnapshot::Markdown { .. } => "markdown",
        SessionSurfaceKindSnapshot::File { .. } => "filePreview",
        SessionSurfaceKindSnapshot::Diff { .. } => "diff",
        SessionSurfaceKindSnapshot::ProjectSidebar => "projectSidebar",
        SessionSurfaceKindSnapshot::RightSidebarTool => "rightSidebarTool",
    }
}

fn parse_kind(
    params: &Map<String, Value>,
) -> Result<SessionSurfaceKindSnapshot, (&'static str, &'static str, Option<Value>)> {
    let raw = params
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("terminal");
    match raw.to_ascii_lowercase().replace(['-', '_'], "").as_str() {
        "browser" => Ok(SessionSurfaceKindSnapshot::Browser {
            url: params.get("url").and_then(Value::as_str).map(str::to_owned),
            proxy_url: None,
            back_history: None,
            forward_history: None,
            omnibar_visible: None,
            focus_mode_active: None,
            developer_tools_visible: None,
            developer_tools_panel: None,
            page_zoom: None,
        }),
        "agentsession" => {
            let provider = params
                .get("provider_id")
                .or_else(|| params.get("provider"))
                .and_then(Value::as_str)
                .unwrap_or("codex");
            if !matches!(
                provider.to_ascii_lowercase().as_str(),
                "codex" | "claude" | "claudecode" | "opencode"
            ) {
                return Err((
                    "invalid_params",
                    "Invalid provider (codex|claude|opencode)",
                    Some(json!({"provider": provider})),
                ));
            }
            Ok(SessionSurfaceKindSnapshot::AgentSession {
                provider: Some(provider.to_owned()),
                renderer: Some("react".into()),
                working_directory: params
                    .get("working_directory")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                session_id: None,
                lifecycle: None,
                restorable_agent: None,
            })
        }
        _ => Ok(SessionSurfaceKindSnapshot::Terminal),
    }
}

fn event(name: &'static str, payload: Value) -> LifecycleEvent {
    LifecycleEvent { name, payload }
}

fn surface_current(
    snapshot: &AppSessionSnapshot,
    params: &Map<String, Value>,
) -> LifecycleTransition {
    let scope = match scope(snapshot, params) {
        Ok(scope) => scope,
        Err((code, message)) => return error(snapshot, code, message, None),
    };
    let model = match SurfaceLifecycleModel::from_app_session(snapshot) {
        Ok(model) => model,
        Err(_) => {
            return error(
                snapshot,
                "internal_error",
                "Invalid surface lifecycle state",
                None,
            )
        }
    };
    let projected = model
        .to_app_session(snapshot)
        .unwrap_or_else(|_| snapshot.clone());
    let workspace =
        &projected.windows[scope.window_index].tab_manager.workspaces[scope.workspace_index];
    let surface_id = model
        .focused_surface(&scope.workspace_id)
        .map(str::to_owned)
        .or_else(|| {
            workspace
                .surfaces
                .as_deref()
                .and_then(|rows| rows.first())
                .map(|row| row.surface_id.clone())
        });
    let (pane_id, surface_type) = surface_id
        .as_deref()
        .and_then(|id| {
            let owner = model.owner_of_surface(id)?;
            let record = model.surface(id)?;
            Some((Some(owner.pane_id.clone()), Some(kind_name(&record.kind))))
        })
        .unwrap_or((None, None));
    read_transition(
        snapshot,
        json!({
            "window_id": scope.window_id,
            "workspace_id": scope.workspace_id,
            "pane_id": pane_id,
            "surface_id": surface_id,
            "surface_type": surface_type,
        }),
    )
}

fn surface_list(snapshot: &AppSessionSnapshot, params: &Map<String, Value>) -> LifecycleTransition {
    let scope = match scope(snapshot, params) {
        Ok(scope) => scope,
        Err((code, message)) => return error(snapshot, code, message, None),
    };
    let model = match SurfaceLifecycleModel::from_app_session(snapshot) {
        Ok(model) => model,
        Err(_) => {
            return error(
                snapshot,
                "internal_error",
                "Invalid surface lifecycle state",
                None,
            )
        }
    };
    let projected = model
        .to_app_session(snapshot)
        .unwrap_or_else(|_| snapshot.clone());
    let workspace =
        &projected.windows[scope.window_index].tab_manager.workspaces[scope.workspace_index];
    let mut rows = Vec::new();
    for (index, record) in workspace
        .surfaces
        .as_deref()
        .unwrap_or_default()
        .iter()
        .enumerate()
    {
        let Some(owner) = model.owner_of_surface(&record.surface_id) else {
            continue;
        };
        let pane = model.pane(&owner.pane_id);
        let index_in_pane = pane
            .and_then(|pane| {
                pane.surface_ids
                    .iter()
                    .position(|id| id == &record.surface_id)
            })
            .unwrap_or(0);
        let selected = pane.is_some_and(|pane| pane.selected_surface_id == record.surface_id);
        let mut row = json!({
            "id": record.surface_id,
            "index": index,
            "type": kind_name(&record.kind),
            "title": record.metadata.custom_title,
            "focused": workspace.focused_panel_id.as_deref() == Some(record.surface_id.as_str()),
            "pane_id": owner.pane_id,
            "index_in_pane": index_in_pane,
            "selected_in_pane": selected,
        });
        if let Value::Object(object) = &mut row {
            match &record.kind {
                SessionSurfaceKindSnapshot::Terminal
                | SessionSurfaceKindSnapshot::RemoteTerminal { .. } => {
                    object.insert(
                        "requested_working_directory".into(),
                        json!(record
                            .terminal_startup
                            .as_ref()
                            .and_then(|startup| startup.working_directory.clone())),
                    );
                    object.insert(
                        "initial_command".into(),
                        json!(record
                            .terminal_startup
                            .as_ref()
                            .and_then(|startup| startup.command.clone())),
                    );
                    object.insert(
                        "tmux_start_command".into(),
                        json!(record
                            .terminal_startup
                            .as_ref()
                            .and_then(|startup| startup.tmux_start_command.clone())),
                    );
                    object.insert("resume_binding".into(), Value::Null);
                }
                SessionSurfaceKindSnapshot::Browser {
                    developer_tools_visible,
                    ..
                } => {
                    if let Some(visible) = developer_tools_visible {
                        object.insert("developer_tools_visible".into(), json!(visible));
                    }
                }
                _ => {}
            }
        }
        rows.push(row);
    }
    read_transition(
        snapshot,
        json!({"window_id": scope.window_id, "workspace_id": scope.workspace_id, "surfaces": rows}),
    )
}

fn surface_create(
    snapshot: &AppSessionSnapshot,
    params: &Map<String, Value>,
    context: &LifecycleDispatchContext,
) -> LifecycleTransition {
    let kind = match parse_kind(params) {
        Ok(kind) => kind,
        Err((code, message, data)) => return error(snapshot, code, message, data),
    };
    if params.get("placement").and_then(Value::as_str) == Some("dock") {
        if !context.dock_available {
            return error(snapshot, "unavailable", "Dock unavailable", None);
        }
        let id = Uuid::new_v4().to_string();
        return ok_transition(
            snapshot.clone(),
            json!({"placement":"dock", "pane_id": null, "surface_id": null, "dock_pane_id": null, "dock_surface_id": id, "type": kind_name(&kind)}),
            vec![],
            vec![LifecycleEffect::DockCreate {
                dock_surface_id: id,
                kind: kind_name(&kind).into(),
                url: params.get("url").and_then(Value::as_str).map(str::to_owned),
            }],
        );
    }
    let scope = match scope(snapshot, params) {
        Ok(scope) => scope,
        Err((code, message)) => return error(snapshot, code, message, None),
    };
    let mut next = snapshot.clone();
    let workspace =
        &mut next.windows[scope.window_index].tab_manager.workspaces[scope.workspace_index];
    let pane_id = params
        .get("pane_id")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| {
            workspace
                .focused_panel_id
                .as_deref()
                .and_then(|id| session_ops::pane_id_containing_surface(workspace, id))
                .map(str::to_owned)
        });
    let Some(pane_id) = pane_id else {
        return error(snapshot, "not_found", "Pane not found", None);
    };
    let anchor = find_pane(workspace.layout.as_ref(), &pane_id)
        .and_then(|pane| pane.panel_ids.last())
        .cloned();
    let Some(anchor) = anchor else {
        return error(snapshot, "not_found", "Pane not found", None);
    };
    let surface_id = Uuid::new_v4().to_string();
    if !workspace
        .layout
        .as_mut()
        .is_some_and(|layout| session_ops::add_panel_to_pane(layout, &anchor, &surface_id))
    {
        return error(snapshot, "internal_error", "Failed to create surface", None);
    }
    if let Some(records) = workspace.surfaces.as_mut() {
        records.push(cmux_core::session::SessionSurfaceSnapshot {
            surface_id: surface_id.clone(),
            pane_id: pane_id.clone(),
            generation: 1,
            kind: kind.clone(),
            metadata: Default::default(),
            terminal_startup: None,
        });
    }
    let mut model = match SurfaceLifecycleModel::from_app_session(&next) {
        Ok(model) => model,
        Err(_) => return error(snapshot, "internal_error", "Failed to create surface", None),
    };
    let generation = if model
        .surface(&surface_id)
        .is_some_and(|record| record.kind == kind)
    {
        model.surface(&surface_id).unwrap().generation
    } else {
        match model.replace_kind(&surface_id, kind.clone()) {
            Ok(value) => value.generation,
            Err(_) => return error(snapshot, "internal_error", "Failed to create surface", None),
        }
    };
    if matches!(kind, SessionSurfaceKindSnapshot::Terminal) {
        let _ = model.set_terminal_startup(
            &surface_id,
            SessionSurfaceTerminalStartupSnapshot {
                command: params
                    .get("initial_command")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                working_directory: params
                    .get("working_directory")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                ..Default::default()
            },
        );
    }
    if params
        .get("focus")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        let _ = model.focus_surface(&surface_id);
    }
    next = match model.to_app_session(&next) {
        Ok(value) => value,
        Err(_) => return error(snapshot, "internal_error", "Failed to create surface", None),
    };
    let effect = if matches!(kind, SessionSurfaceKindSnapshot::Browser { .. }) {
        LifecycleEffect::BrowserAttach {
            surface_id: surface_id.clone(),
            generation,
            url: params.get("url").and_then(Value::as_str).map(str::to_owned),
        }
    } else {
        LifecycleEffect::TerminalCreate {
            surface_id: surface_id.clone(),
            generation,
            command: params
                .get("initial_command")
                .and_then(Value::as_str)
                .map(str::to_owned),
            working_directory: params
                .get("working_directory")
                .and_then(Value::as_str)
                .map(str::to_owned),
        }
    };
    ok_transition(
        next,
        json!({"window_id":scope.window_id,"workspace_id":scope.workspace_id,"pane_id":pane_id,"surface_id":surface_id,"type":kind_name(&kind)}),
        vec![event("surface.created", json!({"surface_id":surface_id}))],
        vec![effect, LifecycleEffect::PersistSession],
    )
}

fn find_pane<'a>(
    layout: Option<&'a SessionWorkspaceLayoutSnapshot>,
    pane_id: &str,
) -> Option<&'a cmux_core::session::SessionPaneLayoutSnapshot> {
    match layout? {
        SessionWorkspaceLayoutSnapshot::Pane(pane) => {
            (pane.pane_id.as_deref() == Some(pane_id)).then_some(pane)
        }
        SessionWorkspaceLayoutSnapshot::Split(split) => find_pane(Some(&split.first), pane_id)
            .or_else(|| find_pane(Some(&split.second), pane_id)),
    }
}

fn surface_action(
    snapshot: &AppSessionSnapshot,
    params: &Map<String, Value>,
) -> LifecycleTransition {
    let action = params
        .get("action")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase()
        .replace('-', "_");
    if action.is_empty() {
        return error(snapshot, "invalid_params", "Missing action", None);
    }
    let surface_id = params
        .get("surface_id")
        .or_else(|| params.get("tab_id"))
        .and_then(Value::as_str)
        .map(str::to_owned);
    let Some(surface_id) = surface_id else {
        return error(snapshot, "not_found", "No focused surface", None);
    };
    let mut model = match SurfaceLifecycleModel::from_app_session(snapshot) {
        Ok(model) => model,
        Err(_) => {
            return error(
                snapshot,
                "internal_error",
                "Invalid surface lifecycle state",
                None,
            )
        }
    };
    let Some(owner) = model.owner_of_surface(&surface_id).cloned() else {
        return error(snapshot, "not_found", "Tab not found", None);
    };
    let mut extras = Map::new();
    match action.as_str() {
        "rename" => {
            let title = params
                .get("title")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_owned();
            if title.is_empty() {
                return error(snapshot, "invalid_params", "Missing title", None);
            }
            if model
                .set_custom_title(&surface_id, Some(title.clone()))
                .is_err()
            {
                return error(snapshot, "internal_error", "Failed to rename tab", None);
            }
            extras.insert("title".into(), json!(title));
        }
        "clear_name" => {
            let _ = model.set_custom_title(&surface_id, None);
        }
        "pin" | "unpin" => {
            let pinned = action == "pin";
            let _ = model.update_metadata(&surface_id, |metadata| metadata.pinned = pinned);
            extras.insert("pinned".into(), json!(pinned));
        }
        "mark_read" | "mark_unread" => {
            let unread = action == "mark_unread";
            let _ = model.update_metadata(&surface_id, |metadata| metadata.unread = unread);
        }
        "close_right" | "close_left" | "close_others" => {
            let pane = model.pane(&owner.pane_id).cloned().expect("owner pane");
            let target = pane
                .surface_ids
                .iter()
                .position(|id| id == &surface_id)
                .unwrap_or(0);
            let candidates: Vec<String> = pane
                .surface_ids
                .iter()
                .enumerate()
                .filter(|(index, id)| match action.as_str() {
                    "close_right" => *index > target,
                    "close_left" => *index < target,
                    _ => *id != &surface_id,
                })
                .map(|(_, id)| id.clone())
                .collect();
            let mut closed = 0_u64;
            let mut skipped = 0_u64;
            for candidate in candidates {
                if model
                    .surface(&candidate)
                    .is_some_and(|record| record.metadata.pinned)
                {
                    skipped += 1;
                    continue;
                }
                if model.close_surface(&candidate, CloseIntent::Range).is_ok() {
                    closed += 1;
                }
            }
            extras.insert("closed".into(), json!(closed));
            extras.insert("skipped_pinned".into(), json!(skipped));
        }
        _ => {
            return error(
                snapshot,
                "invalid_params",
                "Unknown tab action",
                Some(json!({"action": action})),
            )
        }
    }
    let next = match model.to_app_session(snapshot) {
        Ok(value) => value,
        Err(_) => {
            return error(
                snapshot,
                "internal_error",
                "Failed to commit tab action",
                None,
            )
        }
    };
    let mut payload = Map::from_iter([
        ("action".into(), json!(action)),
        ("window_id".into(), json!(owner.window_id)),
        ("workspace_id".into(), json!(owner.workspace_id)),
        ("pane_id".into(), json!(owner.pane_id)),
        ("surface_id".into(), json!(surface_id)),
        ("tab_id".into(), json!(surface_id)),
    ]);
    payload.extend(extras);
    ok_transition(
        next,
        Value::Object(payload),
        vec![event(
            "surface.action",
            json!({"surface_id":surface_id,"action":action}),
        )],
        vec![LifecycleEffect::PersistSession],
    )
}

fn surface_report_pwd(
    snapshot: &AppSessionSnapshot,
    params: &Map<String, Value>,
) -> LifecycleTransition {
    let Some(workspace_id) = params.get("workspace_id").and_then(Value::as_str) else {
        return error(
            snapshot,
            "invalid_params",
            "Missing or invalid workspace_id",
            None,
        );
    };
    let paths: Vec<&str> = ["path", "directory", "cwd"]
        .iter()
        .filter_map(|key| params.get(*key).and_then(Value::as_str))
        .filter(|value| !value.trim().is_empty())
        .collect();
    let Some(path) = paths.first().copied() else {
        return error(snapshot, "invalid_params", "Missing path", None);
    };
    if paths.iter().any(|value| *value != path) {
        return error(
            snapshot,
            "invalid_params",
            "Conflicting path parameters",
            None,
        );
    }
    let surface_id = params.get("surface_id").and_then(Value::as_str);
    let mut next = snapshot.clone();
    let mut model = match SurfaceLifecycleModel::from_app_session(&next) {
        Ok(model) => model,
        Err(_) => {
            return error(
                snapshot,
                "internal_error",
                "Invalid surface lifecycle state",
                None,
            )
        }
    };
    if let Some(id) = surface_id.filter(|id| model.surface(id).is_some()) {
        let owner = model.owner_of_surface(id).cloned().unwrap();
        if owner.workspace_id != workspace_id {
            return error(snapshot, "not_found", "Surface not found", None);
        }
        let _ = model.update_metadata(id, |metadata| {
            metadata.reported_directory = Some(path.into());
            metadata.directory_provenance = Some("reported".into());
        });
        next = model.to_app_session(&next).unwrap();
        return ok_transition(
            next,
            json!({"window_id":owner.window_id,"workspace_id":workspace_id,"surface_id":id,"path":path}),
            vec![],
            vec![LifecycleEffect::PersistSession],
        );
    }
    let mut found = false;
    for window in &mut next.windows {
        if let Some(workspace) = window
            .tab_manager
            .workspaces
            .iter_mut()
            .find(|workspace| workspace.workspace_id.as_deref() == Some(workspace_id))
        {
            found = true;
            workspace
                .pending_remote_pwds
                .get_or_insert_with(Vec::new)
                .push(cmux_core::session::SessionPendingRemotePwdSnapshot {
                    remote_session_id: surface_id.unwrap_or("").into(),
                    path: path.into(),
                });
            break;
        }
    }
    if !found {
        return error(snapshot, "not_found", "Workspace not found", None);
    }
    ok_transition(
        next,
        json!({"workspace_id":workspace_id,"surface_id":surface_id,"path":path,"pending":true}),
        vec![],
        vec![LifecycleEffect::PersistSession],
    )
}

fn surface_respawn(
    snapshot: &AppSessionSnapshot,
    params: &Map<String, Value>,
) -> LifecycleTransition {
    if params.contains_key("focus") && params.get("focus").and_then(Value::as_bool).is_none() {
        return error(snapshot, "invalid_params", "Missing or invalid focus", None);
    }
    let Some(surface_id) = params.get("surface_id").and_then(Value::as_str) else {
        return error(snapshot, "not_found", "No focused surface", None);
    };
    let mut model = match SurfaceLifecycleModel::from_app_session(snapshot) {
        Ok(model) => model,
        Err(_) => {
            return error(
                snapshot,
                "internal_error",
                "Invalid surface lifecycle state",
                None,
            )
        }
    };
    let Some(record) = model.surface(surface_id).cloned() else {
        return error(
            snapshot,
            "not_found",
            "Surface not found for the given surface_id",
            Some(json!({"surface_id":surface_id})),
        );
    };
    if !matches!(
        record.kind,
        SessionSurfaceKindSnapshot::Terminal | SessionSurfaceKindSnapshot::RemoteTerminal { .. }
    ) {
        return error(
            snapshot,
            "invalid_params",
            "Surface is not a terminal",
            Some(json!({"surface_id":surface_id})),
        );
    }
    let command = params
        .get("command")
        .and_then(Value::as_str)
        .or_else(|| params.get("initial_command").and_then(Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("cmd.exe")
        .to_owned();
    let working_directory = params.get("working_directory").and_then(Value::as_str);
    let reservation = model
        .begin_respawn(surface_id, &command, working_directory)
        .unwrap();
    if params.get("focus").and_then(Value::as_bool) == Some(true) {
        let _ = model.focus_surface(surface_id);
    }
    let owner = model.owner_of_surface(surface_id).cloned().unwrap();
    let next = model.to_app_session(snapshot).unwrap();
    ok_transition(
        next,
        json!({"window_id":owner.window_id,"workspace_id":owner.workspace_id,"surface_id":surface_id,"type":"terminal"}),
        vec![event("surface.respawned", json!({"surface_id":surface_id}))],
        vec![
            LifecycleEffect::TerminalReplace {
                surface_id: surface_id.into(),
                previous_generation: record.generation,
                generation: reservation.generation,
                command,
                working_directory: working_directory.map(str::to_owned),
            },
            LifecycleEffect::PersistSession,
        ],
    )
}

fn surface_close(
    snapshot: &AppSessionSnapshot,
    params: &Map<String, Value>,
) -> LifecycleTransition {
    let mut model = match SurfaceLifecycleModel::from_app_session(snapshot) {
        Ok(model) => model,
        Err(_) => {
            return error(
                snapshot,
                "internal_error",
                "Invalid surface lifecycle state",
                None,
            )
        }
    };
    let surface_id = params
        .get("surface_id")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| {
            scope(snapshot, params).ok().and_then(|scope| {
                model
                    .focused_surface(&scope.workspace_id)
                    .map(str::to_owned)
            })
        });
    let Some(surface_id) = surface_id else {
        return error(snapshot, "not_found", "No focused surface", None);
    };
    let Some(owner) = model.owner_of_surface(&surface_id).cloned() else {
        return error(
            snapshot,
            "not_found",
            "Surface not found",
            Some(json!({"surface_id":surface_id})),
        );
    };
    let generation = model.surface(&surface_id).unwrap().generation;
    if let Err(problem) = model.close_surface(&surface_id, CloseIntent::Explicit) {
        return if problem.to_string().contains("last surface") {
            error(
                snapshot,
                "invalid_state",
                "Cannot close the last surface",
                None,
            )
        } else {
            error(
                snapshot,
                "internal_error",
                "Failed to close surface",
                Some(json!({"surface_id":surface_id})),
            )
        };
    }
    let next = model.to_app_session(snapshot).unwrap();
    ok_transition(
        next,
        json!({"window_id":owner.window_id,"workspace_id":owner.workspace_id,"surface_id":surface_id}),
        vec![event("surface.closed", json!({"surface_id":surface_id}))],
        vec![
            LifecycleEffect::RuntimeTeardown {
                surface_id,
                generation,
            },
            LifecycleEffect::PersistSession,
        ],
    )
}

fn surface_focus(
    snapshot: &AppSessionSnapshot,
    params: &Map<String, Value>,
) -> LifecycleTransition {
    let Some(surface_id) = params.get("surface_id").and_then(Value::as_str) else {
        return error(
            snapshot,
            "invalid_params",
            "Missing or invalid surface_id",
            None,
        );
    };
    let mut model = match SurfaceLifecycleModel::from_app_session(snapshot) {
        Ok(model) => model,
        Err(_) => {
            return error(
                snapshot,
                "internal_error",
                "Invalid surface lifecycle state",
                None,
            )
        }
    };
    let Some(owner) = model.owner_of_surface(surface_id).cloned() else {
        return error(
            snapshot,
            "not_found",
            "Surface not found",
            Some(json!({"surface_id":surface_id})),
        );
    };
    let _ = model.focus_surface(surface_id);
    let next = model.to_app_session(snapshot).unwrap();
    ok_transition(
        next,
        json!({"window_id":owner.window_id,"workspace_id":owner.workspace_id,"surface_id":surface_id}),
        vec![event("surface.focused", json!({"surface_id":surface_id}))],
        vec![
            LifecycleEffect::ActivateWindow {
                window_id: owner.window_id,
            },
            LifecycleEffect::PersistSession,
        ],
    )
}

fn surface_move(snapshot: &AppSessionSnapshot, params: &Map<String, Value>) -> LifecycleTransition {
    let Some(surface_id) = params.get("surface_id").and_then(Value::as_str) else {
        return error(
            snapshot,
            "invalid_params",
            "Missing or invalid surface_id",
            None,
        );
    };
    let mut model = match SurfaceLifecycleModel::from_app_session(snapshot) {
        Ok(model) => model,
        Err(_) => {
            return error(
                snapshot,
                "internal_error",
                "Invalid surface lifecycle state",
                None,
            )
        }
    };
    if model.surface(surface_id).is_none() {
        return error(
            snapshot,
            "not_found",
            "Surface not found",
            Some(json!({"surface_id":surface_id})),
        );
    }
    let destination_scope = match scope(snapshot, params) {
        Ok(scope) => scope,
        Err((code, message)) => return error(snapshot, code, message, None),
    };
    let destination_pane = params
        .get("pane_id")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| {
            model
                .focused_surface(&destination_scope.workspace_id)
                .and_then(|id| model.owner_of_surface(id))
                .map(|owner| owner.pane_id.clone())
        });
    let Some(destination_pane) = destination_pane else {
        return error(snapshot, "not_found", "Destination pane not found", None);
    };
    let index = params
        .get("index")
        .and_then(Value::as_u64)
        .unwrap_or(usize::MAX as u64) as usize;
    if model
        .move_surface_transactionally(surface_id, &destination_pane, index, |_, _| Ok::<_, ()>(()))
        .is_err()
    {
        return error(snapshot, "internal_error", "Failed to move surface", None);
    }
    if params
        .get("focus")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        let _ = model.focus_surface(surface_id);
    }
    let owner = model.owner_of_surface(surface_id).cloned().unwrap();
    let next = model.to_app_session(snapshot).unwrap();
    ok_transition(
        next,
        json!({"window_id":owner.window_id,"workspace_id":owner.workspace_id,"pane_id":owner.pane_id,"surface_id":surface_id}),
        vec![event("surface.moved", json!({"surface_id":surface_id}))],
        vec![LifecycleEffect::PersistSession],
    )
}

fn pane_focus(snapshot: &AppSessionSnapshot, params: &Map<String, Value>) -> LifecycleTransition {
    let Some(pane_id) = params.get("pane_id").and_then(Value::as_str) else {
        return error(
            snapshot,
            "invalid_params",
            "Missing or invalid pane_id",
            None,
        );
    };
    let mut model = match SurfaceLifecycleModel::from_app_session(snapshot) {
        Ok(model) => model,
        Err(_) => {
            return error(
                snapshot,
                "internal_error",
                "Invalid surface lifecycle state",
                None,
            )
        }
    };
    let Some(pane) = model.pane(pane_id).cloned() else {
        return error(snapshot, "not_found", "Pane not found", None);
    };
    let selected = pane.selected_surface_id.clone();
    if selected.is_empty() {
        return error(snapshot, "not_found", "Pane has no surface", None);
    }
    let _ = model.focus_surface(&selected);
    let next = model.to_app_session(snapshot).unwrap();
    ok_transition(
        next,
        json!({"window_id":pane.window_id,"workspace_id":pane.workspace_id,"pane_id":pane_id,"surface_id":selected}),
        vec![event("pane.focused", json!({"pane_id":pane_id}))],
        vec![
            LifecycleEffect::ActivateWindow {
                window_id: pane.window_id,
            },
            LifecycleEffect::PersistSession,
        ],
    )
}

fn pane_resize(
    snapshot: &AppSessionSnapshot,
    params: &Map<String, Value>,
    context: &LifecycleDispatchContext,
) -> LifecycleTransition {
    let absolute = params.contains_key("absolute_axis") || params.contains_key("target_pixels");
    let scope = match scope(snapshot, params) {
        Ok(scope) => scope,
        Err((code, message)) => return error(snapshot, code, message, None),
    };
    let mut next = snapshot.clone();
    let workspace =
        &mut next.windows[scope.window_index].tab_manager.workspaces[scope.workspace_index];
    let pane_id = params
        .get("pane_id")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| {
            workspace
                .focused_panel_id
                .as_deref()
                .and_then(|id| session_ops::pane_id_containing_surface(workspace, id))
                .map(str::to_owned)
        });
    let Some(pane_id) = pane_id else {
        return error(snapshot, "not_found", "No focused pane", None);
    };
    let (width, height) = context.viewport_size.unwrap_or((1_000.0, 800.0));
    let result = if absolute {
        let axis = match params.get("absolute_axis").and_then(Value::as_str) {
            Some("horizontal") => SessionSplitOrientation::Horizontal,
            Some("vertical") => SessionSplitOrientation::Vertical,
            _ => {
                return error(
                    snapshot,
                    "invalid_params",
                    "absolute_axis must be 'horizontal' or 'vertical'",
                    None,
                )
            }
        };
        let Some(target) = params
            .get("target_pixels")
            .and_then(Value::as_f64)
            .filter(|value| value.is_finite() && *value > 0.0)
        else {
            return error(
                snapshot,
                "invalid_params",
                "target_pixels must be > 0",
                None,
            );
        };
        session_ops::resize_pane_absolute(workspace, &pane_id, axis, target, width, height)
    } else {
        let direction = match params.get("direction").and_then(Value::as_str) {
            Some("left") => PaneResizeDirection::Left,
            Some("right") => PaneResizeDirection::Right,
            Some("up") => PaneResizeDirection::Up,
            Some("down") => PaneResizeDirection::Down,
            _ => {
                return error(
                    snapshot,
                    "invalid_params",
                    "direction must be one of left|right|up|down and amount must be > 0",
                    None,
                )
            }
        };
        let Some(amount) = params
            .get("amount")
            .map_or(Some(1), Value::as_u64)
            .filter(|value| *value > 0)
        else {
            return error(
                snapshot,
                "invalid_params",
                "direction must be one of left|right|up|down and amount must be > 0",
                None,
            );
        };
        session_ops::resize_pane_relative(workspace, &pane_id, direction, amount, width, height)
    };
    let result = match result {
        Ok(result) => result,
        Err(_) => {
            return error(
                snapshot,
                "invalid_state",
                "No split ancestor for absolute pane resize",
                None,
            )
        }
    };
    ok_transition(
        next,
        json!({"window_id":scope.window_id,"workspace_id":scope.workspace_id,"pane_id":pane_id,"split_id":result.split_id,"old_divider_position":result.old_divider_position,"new_divider_position":result.new_divider_position}),
        vec![event("pane.resized", json!({"pane_id":pane_id}))],
        vec![LifecycleEffect::PersistSession],
    )
}

fn pane_create(
    snapshot: &AppSessionSnapshot,
    method: &str,
    params: &Map<String, Value>,
    _context: &LifecycleDispatchContext,
) -> LifecycleTransition {
    let (orientation, insert_first) = match params
        .get("direction")
        .and_then(Value::as_str)
        .map(|value| value.to_ascii_lowercase())
    {
        Some(value) if matches!(value.as_str(), "left" | "l") => {
            (SessionSplitOrientation::Horizontal, true)
        }
        Some(value) if matches!(value.as_str(), "right" | "r") => {
            (SessionSplitOrientation::Horizontal, false)
        }
        Some(value) if matches!(value.as_str(), "up" | "u") => {
            (SessionSplitOrientation::Vertical, true)
        }
        Some(value) if matches!(value.as_str(), "down" | "d") => {
            (SessionSplitOrientation::Vertical, false)
        }
        _ => {
            return error(
                snapshot,
                "invalid_params",
                "Missing or invalid direction (left|right|up|down)",
                None,
            )
        }
    };
    let scope = match scope(snapshot, params) {
        Ok(scope) => scope,
        Err((code, message)) => return error(snapshot, code, message, None),
    };
    let workspace_value = serde_json::to_value(
        &snapshot.windows[scope.window_index].tab_manager.workspaces[scope.workspace_index],
    )
    .unwrap();
    if method == "pane.create"
        && workspace_value["remote"]["connected"] == json!(true)
        && workspace_value["remote"]["transport"] == json!("tmux")
    {
        let remote = workspace_value["remote"]["destination"]
            .as_str()
            .unwrap_or("remote")
            .to_owned();
        return ok_transition(
            snapshot.clone(),
            json!({"accepted":true,"routed":"remote-tmux","type":"terminal","window_id":scope.window_id,"workspace_id":scope.workspace_id,"pane_id":null,"surface_id":null}),
            vec![],
            vec![LifecycleEffect::RemoteCreate {
                remote_session_id: remote,
                kind: "terminal".into(),
            }],
        );
    }
    let kind = match parse_kind(params) {
        Ok(kind) => kind,
        Err((code, message, data)) => return error(snapshot, code, message, data),
    };
    let mut next = snapshot.clone();
    let workspace =
        &mut next.windows[scope.window_index].tab_manager.workspaces[scope.workspace_index];
    let source = params
        .get("surface_id")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| workspace.focused_panel_id.clone())
        .or_else(|| {
            workspace
                .layout
                .as_ref()
                .and_then(|layout| layout_surface_ids(layout).into_iter().next())
        });
    let Some(source) = source else {
        return error(snapshot, "not_found", "No source surface to split", None);
    };
    let surface_id = Uuid::new_v4().to_string();
    let Some(layout) = workspace.layout.as_mut() else {
        return error(snapshot, "not_found", "No source surface to split", None);
    };
    if !session_ops::split_pane(layout, &source, orientation, &surface_id, insert_first) {
        return error(snapshot, "internal_error", "Failed to create pane", None);
    }
    let pane_id = Uuid::new_v4().to_string();
    assign_created_ids(
        layout,
        &surface_id,
        &pane_id,
        params
            .get("initial_divider_position")
            .and_then(Value::as_f64),
    );
    if let Some(records) = workspace.surfaces.as_mut() {
        records.push(cmux_core::session::SessionSurfaceSnapshot {
            surface_id: surface_id.clone(),
            pane_id: pane_id.clone(),
            generation: 1,
            kind: kind.clone(),
            metadata: Default::default(),
            terminal_startup: None,
        });
    }
    let mut model = match SurfaceLifecycleModel::from_app_session(&next) {
        Ok(model) => model,
        Err(problem) => {
            #[cfg(test)]
            eprintln!("pane create model error: {problem}");
            return error(snapshot, "internal_error", "Failed to create pane", None);
        }
    };
    let generation = if model
        .surface(&surface_id)
        .is_some_and(|record| record.kind == kind)
    {
        model.surface(&surface_id).unwrap().generation
    } else {
        match model.replace_kind(&surface_id, kind.clone()) {
            Ok(value) => value.generation,
            Err(problem) => {
                #[cfg(test)]
                eprintln!("pane create kind error: {problem}");
                return error(snapshot, "internal_error", "Failed to create pane", None);
            }
        }
    };
    if params
        .get("focus")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        let _ = model.focus_surface(&surface_id);
    }
    next = model.to_app_session(&next).unwrap();
    let mut effects = vec![
        if matches!(kind, SessionSurfaceKindSnapshot::Browser { .. }) {
            LifecycleEffect::BrowserAttach {
                surface_id: surface_id.clone(),
                generation,
                url: params.get("url").and_then(Value::as_str).map(str::to_owned),
            }
        } else {
            LifecycleEffect::TerminalCreate {
                surface_id: surface_id.clone(),
                generation,
                command: params
                    .get("initial_command")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                working_directory: params
                    .get("working_directory")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
            }
        },
    ];
    if params
        .get("focus")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        effects.push(LifecycleEffect::ActivateWindow {
            window_id: scope.window_id.clone(),
        });
    }
    effects.push(LifecycleEffect::PersistSession);
    ok_transition(
        next,
        json!({"window_id":scope.window_id,"workspace_id":scope.workspace_id,"pane_id":pane_id,"surface_id":surface_id,"type":kind_name(&kind)}),
        vec![
            event("pane.created", json!({"pane_id":pane_id})),
            event("surface.created", json!({"surface_id":surface_id})),
        ],
        effects,
    )
}

fn assign_created_ids(
    layout: &mut SessionWorkspaceLayoutSnapshot,
    surface_id: &str,
    pane_id: &str,
    divider: Option<f64>,
) -> bool {
    match layout {
        SessionWorkspaceLayoutSnapshot::Pane(pane) => {
            if pane.panel_ids.iter().any(|id| id == surface_id) {
                pane.pane_id = Some(pane_id.into());
                true
            } else {
                false
            }
        }
        SessionWorkspaceLayoutSnapshot::Split(split) => {
            let found = assign_created_ids(&mut split.first, surface_id, pane_id, divider)
                || assign_created_ids(&mut split.second, surface_id, pane_id, divider);
            if found {
                split
                    .split_id
                    .get_or_insert_with(|| Uuid::new_v4().to_string());
                if let Some(value) = divider {
                    split.divider_position = value.clamp(0.1, 0.9);
                }
            }
            found
        }
    }
}
