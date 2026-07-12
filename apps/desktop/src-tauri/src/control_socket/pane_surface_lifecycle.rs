use cmux_core::session::{
    AppSessionSnapshot, SessionSplitOrientation, SessionSurfaceKindSnapshot,
    SessionSurfaceTerminalStartupSnapshot, SessionWorkspaceLayoutSnapshot,
};
use cmux_core::session_ops::{self, PaneResizeDirection};
use cmux_core::surface_lifecycle::{
    CloseIntent, ContainerKind, SurfaceLifecycleModel, SurfaceMetadata, SurfaceSeed,
    TerminalStartup,
};
use cmux_ipc::{ControlCallResult, JsonValue};
use serde_json::{json, Map, Value};
use uuid::Uuid;

use crate::dock::{
    DockCreateRequest, DockPlacement, DockRuntimeIntent, DockStore, DockSurfaceKind,
};

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub(super) struct LifecycleEvent {
    pub name: &'static str,
    pub category: &'static str,
    pub source: &'static str,
    pub window_id: Option<String>,
    pub workspace_id: Option<String>,
    pub pane_id: Option<String>,
    pub surface_id: Option<String>,
    pub payload: Value,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[allow(dead_code)] // Variants are introduced together; dispatch branches consume them incrementally.
pub(super) enum LifecycleEffect {
    TerminalCreate {
        surface_id: String,
        generation: u64,
        command: Option<String>,
        working_directory: Option<String>,
        failure_code: &'static str,
        failure_message: &'static str,
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
        failure_code: &'static str,
        failure_message: &'static str,
    },
    BrowserReload {
        surface_id: String,
        phase: &'static str,
        failure_code: &'static str,
        failure_message: &'static str,
    },
    ExternalBrowserOpen {
        url: String,
        phase: &'static str,
        failure_code: &'static str,
        failure_message: &'static str,
    },
    UiSurfaceAttach {
        surface_id: String,
        kind: String,
    },
    RuntimeTeardown {
        surface_id: String,
        generation: u64,
        owner_id: String,
        dock_intent: Option<DockRuntimeIntent>,
        must_succeed: bool,
        failure_code: &'static str,
        failure_message: &'static str,
        phase: &'static str,
    },
    DockCreate {
        owner_id: String,
        dock_surface_id: String,
        generation: u64,
        kind: String,
        intent: DockRuntimeIntent,
        failure_code: &'static str,
        failure_message: &'static str,
        visibility_phase: &'static str,
        rollback: &'static str,
    },
    DockReveal {
        owner_id: String,
    },
    DockChanged {
        owner_id: String,
        phase: &'static str,
    },
    RemoteCreate {
        remote_session_id: String,
        destination: String,
        window_id: String,
        workspace_id: String,
        target_pane_id: Option<String>,
        source_surface_id: Option<String>,
        source_remote_pane_id: Option<String>,
        source_pane_id: Option<String>,
        split_direction: Option<super::RemoteTmuxSplitDirection>,
        split_orientation: Option<SessionSplitOrientation>,
        kind: String,
        tmux_operation: &'static str,
        arrival_policy: &'static str,
        focus: bool,
        focus_mode: &'static str,
        activate_window: bool,
        placement: &'static str,
        working_directory: Option<String>,
        working_directory_source_surface_id: Option<String>,
        observation_source: &'static str,
        observation_phase: &'static str,
        pending_reconciliation: bool,
        observation_failure_policy: &'static str,
        commit_failure_policy: &'static str,
        failure_code: &'static str,
        failure_message: &'static str,
    },
    ActivateWindow {
        window_id: String,
    },
    PersistSession,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct LifecycleDispatchContext {
    pub viewport_size: Option<(f64, f64)>,
    pub browser_enabled: bool,
    pub dock_available: bool,
    pub active_window_id: Option<String>,
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
    type Error: From<String> + ToString;

    fn prepare_transition(&mut self, _candidate: &AppSessionSnapshot) -> Result<(), Self::Error> {
        Ok(())
    }

    /// Validate and acquire all resources needed by an effect without making
    /// it externally visible. The transition is committed only after every
    /// effect has staged successfully.
    fn stage(&mut self, effect: &LifecycleEffect) -> Result<(), Self::Error>;
    fn commit_staged(&mut self) -> Result<(), Self::Error>;
    fn rollback_staged(&mut self) -> Result<(), Self::Error>;

    fn rollback_committed(&mut self) -> Result<(), Self::Error> {
        self.rollback_staged()
    }
}

pub(super) fn commit_lifecycle_transition<E: LifecycleEffectExecutor>(
    target: &mut AppSessionSnapshot,
    transition: LifecycleTransition,
    executor: &mut E,
) -> Result<ControlCallResult, E::Error> {
    executor.prepare_transition(&transition.snapshot)?;
    for effect in &transition.effects {
        if let Err(error) = executor.stage(effect) {
            return match executor.rollback_staged() {
                Ok(()) => Err(error),
                Err(rollback_error) => Err(combine_transaction_errors(error, rollback_error)),
            };
        }
    }
    if let Err(error) = executor.commit_staged() {
        return match executor.rollback_committed() {
            Ok(()) => Err(error),
            Err(rollback_error) => Err(combine_transaction_errors(error, rollback_error)),
        };
    }
    if transition.changed {
        *target = transition.snapshot;
    }
    Ok(transition.result)
}

fn combine_transaction_errors<E>(primary: E, rollback: E) -> E
where
    E: From<String> + ToString,
{
    format!(
        "{}; rollback compensation failed: {}",
        primary.to_string(),
        rollback.to_string()
    )
    .into()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RuntimeArrival {
    pub window_id: String,
    pub workspace_id: String,
    pub pane_id: String,
    pub surface_id: String,
    pub remote_session_id: String,
    pub generation: u64,
    pub creates_pane: bool,
    pub anchor_surface_id: Option<String>,
    pub focused: bool,
    pub split_orientation: Option<SessionSplitOrientation>,
    pub source_pane_id: Option<String>,
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
            creates_pane: true,
            anchor_surface_id: None,
            focused: false,
            split_orientation: None,
            source_pane_id: None,
        }
    }

    pub fn remote_tab(
        window_id: impl Into<String>,
        workspace_id: impl Into<String>,
        pane_id: impl Into<String>,
        surface_id: impl Into<String>,
        remote_session_id: impl Into<String>,
        generation: u64,
        anchor_surface_id: impl Into<String>,
        focused: bool,
    ) -> Self {
        Self {
            window_id: window_id.into(),
            workspace_id: workspace_id.into(),
            pane_id: pane_id.into(),
            surface_id: surface_id.into(),
            remote_session_id: remote_session_id.into(),
            generation,
            creates_pane: false,
            anchor_surface_id: Some(anchor_surface_id.into()),
            focused,
            split_orientation: None,
            source_pane_id: None,
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct RuntimeReconciliation {
    pub snapshot: AppSessionSnapshot,
}

impl RuntimeReconciliation {
    #[cfg(test)]
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
    let Ok(existing_model) = SurfaceLifecycleModel::from_app_session(&next) else {
        return RuntimeReconciliation { snapshot: next };
    };
    if let Some(existing) = existing_model.surface(&arrival.surface_id) {
        // The same generation is an idempotent duplicate. Any other
        // generation is a stale callback; neither may rewrite topology.
        let _same_generation = existing.generation == arrival.generation;
        return RuntimeReconciliation { snapshot: next };
    }
    if arrival.generation == 0 {
        return RuntimeReconciliation { snapshot: next };
    }
    next = existing_model.to_app_session(&next).unwrap_or(next);
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
    let requested_anchor = arrival.anchor_surface_id.clone();
    let requested_orientation = arrival.split_orientation.clone();
    let pending_path = workspace.pending_remote_pwds.as_mut().and_then(|pending| {
        pending
            .iter()
            .position(|item| {
                item.remote_session_id == arrival.remote_session_id
                    || item.remote_session_id == arrival.surface_id
            })
            .map(|index| pending.remove(index).path)
    });
    match workspace.layout.as_mut() {
        None => {
            let mut layout = session_ops::single_pane(&arrival.surface_id);
            let SessionWorkspaceLayoutSnapshot::Pane(pane) = &mut layout else {
                unreachable!()
            };
            pane.pane_id = Some(arrival.pane_id.clone());
            workspace.layout = Some(layout);
        }
        Some(layout) if find_pane(Some(layout), &arrival.pane_id).is_some() => {
            let pane = find_pane(Some(layout), &arrival.pane_id);
            let anchor = arrival
                .anchor_surface_id
                .as_ref()
                .filter(|anchor| pane.is_some_and(|pane| pane.panel_ids.contains(anchor)))
                .cloned()
                .or_else(|| pane.and_then(|pane| pane.panel_ids.last()).cloned());
            if let Some(anchor) = anchor {
                session_ops::add_panel_to_pane(layout, &anchor, &arrival.surface_id);
            }
        }
        Some(_) if !arrival.creates_pane => return RuntimeReconciliation { snapshot: next },
        Some(layout) => {
            let surface_ids = layout_surface_ids(layout);
            let anchor = match requested_anchor.clone() {
                Some(anchor) if surface_ids.contains(&anchor) => Some(anchor),
                Some(_) => None,
                None => surface_ids.into_iter().next(),
            };
            let Some(anchor) = anchor else {
                return RuntimeReconciliation { snapshot: next };
            };
            if !session_ops::split_pane(
                layout,
                &anchor,
                requested_orientation.unwrap_or(SessionSplitOrientation::Horizontal),
                &arrival.surface_id,
                false,
            ) {
                return RuntimeReconciliation { snapshot: next };
            }
            assign_created_ids(layout, &arrival.surface_id, &arrival.pane_id, None);
        }
    }
    workspace.surfaces.get_or_insert_with(Vec::new).push(
        cmux_core::session::SessionSurfaceSnapshot {
            surface_id: arrival.surface_id.clone(),
            pane_id: arrival.pane_id.clone(),
            generation: arrival.generation,
            kind: SessionSurfaceKindSnapshot::RemoteTerminal {
                remote_session_id: Some(arrival.remote_session_id.clone()),
                remote_context: None,
                arrival_generation: Some(arrival.generation),
            },
            metadata: cmux_core::session::SessionSurfaceMetadataSnapshot {
                directory_provenance: pending_path.as_ref().map(|_| "remote_report".into()),
                reported_directory: pending_path,
                ..Default::default()
            },
            terminal_startup: None,
        },
    );
    let Ok(model) = SurfaceLifecycleModel::from_app_session(&next) else {
        return RuntimeReconciliation {
            snapshot: snapshot.clone(),
        };
    };
    let mut model = model;
    if !arrival.creates_pane {
        let Some(pane) = model.pane(&arrival.pane_id).cloned() else {
            return RuntimeReconciliation {
                snapshot: snapshot.clone(),
            };
        };
        let anchor_index = arrival
            .anchor_surface_id
            .as_ref()
            .and_then(|anchor| pane.surface_ids.iter().position(|id| id == anchor))
            .unwrap_or_else(|| pane.surface_ids.len().saturating_sub(1));
        let pinned_prefix = pane
            .surface_ids
            .iter()
            .take_while(|id| model.surface(id).is_some_and(|row| row.metadata.pinned))
            .count();
        if model
            .move_surface_transactionally(
                &arrival.surface_id,
                &arrival.pane_id,
                (anchor_index + 1).max(pinned_prefix),
                |_, _| Ok::<_, String>(()),
            )
            .is_err()
        {
            return RuntimeReconciliation {
                snapshot: snapshot.clone(),
            };
        }
    }
    if arrival.focused && model.focus_surface(&arrival.surface_id).is_err() {
        return RuntimeReconciliation {
            snapshot: snapshot.clone(),
        };
    }
    let Ok(projected) = model.to_app_session(&next) else {
        return RuntimeReconciliation {
            snapshot: snapshot.clone(),
        };
    };
    RuntimeReconciliation {
        snapshot: projected,
    }
}

pub(super) fn dispatch_lifecycle_request(
    snapshot: &AppSessionSnapshot,
    method: &str,
    params: &Map<String, Value>,
    context: &LifecycleDispatchContext,
) -> LifecycleTransition {
    let mut transition = match method {
        "surface.current" => surface_current(snapshot, params, context),
        "surface.list" => surface_list(snapshot, params, context),
        "surface.create" => surface_create(snapshot, params, context),
        "surface.action" | "tab.action" => surface_action(snapshot, method, params, context),
        "surface.report_pwd" => surface_report_pwd(snapshot, params),
        "surface.respawn" => surface_respawn(snapshot, params),
        "surface.close" => surface_close(snapshot, params, context),
        "surface.focus" => surface_focus(snapshot, params),
        "surface.move" => surface_move(snapshot, params, context),
        "pane.resize" => pane_resize(snapshot, params, context),
        "pane.focus" => pane_focus(snapshot, params),
        "pane.create" | "surface.split" => pane_create(snapshot, method, params, context),
        _ => error(
            snapshot,
            "method_not_found",
            "Unknown lifecycle method",
            None,
        ),
    };
    transition.changed = transition.snapshot != *snapshot;
    transition
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
    context: &LifecycleDispatchContext,
) -> Result<Scope, (&'static str, &'static str)> {
    let surface_selector = params
        .get("surface_id")
        .or_else(|| params.get("terminal_id"))
        .or_else(|| params.get("tab_id"))
        .and_then(Value::as_str);
    let pane_selector = params.get("pane_id").and_then(Value::as_str);
    let workspace_selector = params.get("workspace_id").and_then(Value::as_str);
    let group_selector = params.get("group_id").and_then(Value::as_str);
    let contains_surface = |workspace: &cmux_core::session::SessionWorkspaceSnapshot, id: &str| {
        workspace
            .surfaces
            .as_deref()
            .unwrap_or_default()
            .iter()
            .any(|record| record.surface_id == id)
            || workspace.layout.as_ref().is_some_and(|layout| {
                layout_surface_ids(layout)
                    .iter()
                    .any(|candidate| candidate == id)
            })
    };
    let contains_pane = |workspace: &cmux_core::session::SessionWorkspaceSnapshot, id: &str| {
        find_pane(workspace.layout.as_ref(), id).is_some()
    };

    let window_index =
        if let Some(explicit) = params.get("window_id").filter(|value| !value.is_null()) {
            let requested = explicit
                .as_str()
                .ok_or(("unavailable", "TabManager not available"))?;
            snapshot
                .windows
                .iter()
                .position(|window| window.window_id.as_deref() == Some(requested))
                .ok_or(("unavailable", "TabManager not available"))?
        } else {
            group_selector
                .and_then(|group| {
                    snapshot.windows.iter().position(|window| {
                        window
                            .tab_manager
                            .workspace_groups
                            .as_deref()
                            .unwrap_or_default()
                            .iter()
                            .any(|candidate| candidate.id == group)
                    })
                })
                .or_else(|| {
                    workspace_selector.and_then(|workspace| {
                        snapshot.windows.iter().position(|window| {
                            window.tab_manager.workspaces.iter().any(|candidate| {
                                candidate.workspace_id.as_deref() == Some(workspace)
                            })
                        })
                    })
                })
                .or_else(|| {
                    surface_selector.and_then(|surface| {
                        snapshot.windows.iter().position(|window| {
                            window
                                .tab_manager
                                .workspaces
                                .iter()
                                .any(|workspace| contains_surface(workspace, surface))
                        })
                    })
                })
                .or_else(|| {
                    pane_selector.and_then(|pane| {
                        snapshot.windows.iter().position(|window| {
                            window
                                .tab_manager
                                .workspaces
                                .iter()
                                .any(|workspace| contains_pane(workspace, pane))
                        })
                    })
                })
                .or_else(|| {
                    context.active_window_id.as_deref().and_then(|active| {
                        snapshot
                            .windows
                            .iter()
                            .position(|window| window.window_id.as_deref() == Some(active))
                    })
                })
                .unwrap_or(0)
        };
    let window = snapshot
        .windows
        .get(window_index)
        .ok_or(("unavailable", "TabManager not available"))?;
    let workspace_index = if let Some(requested) = workspace_selector {
        window
            .tab_manager
            .workspaces
            .iter()
            .position(|workspace| workspace.workspace_id.as_deref() == Some(requested))
            .ok_or(("not_found", "Workspace not found"))?
    } else {
        let surface_index = surface_selector.and_then(|surface| {
            window
                .tab_manager
                .workspaces
                .iter()
                .position(|workspace| contains_surface(workspace, surface))
        });
        let pane_index = pane_selector.and_then(|pane| {
            window
                .tab_manager
                .workspaces
                .iter()
                .position(|workspace| contains_pane(workspace, pane))
        });
        surface_index.or(pane_index).unwrap_or(
            usize::try_from(window.tab_manager.selected_workspace_index.unwrap_or(0))
                .map_err(|_| ("not_found", "Workspace not found"))?,
        )
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

fn creation_origin(kind: &SessionSurfaceKindSnapshot, split: bool) -> &'static str {
    match (kind, split) {
        (SessionSurfaceKindSnapshot::Browser { .. }, true) => "browser_split",
        (SessionSurfaceKindSnapshot::Browser { .. }, false) => "browser_tab",
        (SessionSurfaceKindSnapshot::Markdown { .. }, true) => "markdown_split",
        (SessionSurfaceKindSnapshot::Markdown { .. }, false) => "markdown_tab",
        (SessionSurfaceKindSnapshot::File { .. }, true) => "file_preview_split",
        (SessionSurfaceKindSnapshot::File { .. }, false) => "file_preview_tab",
        (SessionSurfaceKindSnapshot::RightSidebarTool, true) => "right_sidebar_tool_split",
        (SessionSurfaceKindSnapshot::RightSidebarTool, false) => "right_sidebar_tool_tab",
        (_, true) => "terminal_split",
        (_, false) => "terminal_tab",
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
            profile: params
                .get("browser_profile")
                .or_else(|| params.get("profile"))
                .and_then(Value::as_str)
                .map(str::to_owned),
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
            let provider = if provider.eq_ignore_ascii_case("claudecode") {
                "claude"
            } else {
                provider
            };
            let renderer = params
                .get("renderer_kind")
                .or_else(|| params.get("renderer"))
                .and_then(Value::as_str)
                .unwrap_or("react");
            if !matches!(renderer.to_ascii_lowercase().as_str(), "react" | "solid") {
                return Err((
                    "invalid_params",
                    "Invalid renderer (react|solid)",
                    Some(json!({"renderer": renderer})),
                ));
            }
            Ok(SessionSurfaceKindSnapshot::AgentSession {
                provider: Some(provider.to_ascii_lowercase()),
                renderer: Some(renderer.to_ascii_lowercase()),
                working_directory: params
                    .get("working_directory")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                session_id: None,
                lifecycle: None,
                restorable_agent: None,
            })
        }
        "markdown" => Ok(SessionSurfaceKindSnapshot::Markdown {
            path: params
                .get("path")
                .and_then(Value::as_str)
                .map(str::to_owned),
        }),
        "filepreview" | "file" => Ok(SessionSurfaceKindSnapshot::File {
            path: params
                .get("path")
                .and_then(Value::as_str)
                .map(str::to_owned),
        }),
        "rightsidebartool" => Ok(SessionSurfaceKindSnapshot::RightSidebarTool),
        "projectsidebar" => Ok(SessionSurfaceKindSnapshot::ProjectSidebar),
        "diff" => Ok(SessionSurfaceKindSnapshot::Diff {
            token: params
                .get("token")
                .and_then(Value::as_str)
                .map(str::to_owned),
            request_path: params
                .get("request_path")
                .and_then(Value::as_str)
                .map(str::to_owned),
        }),
        _ => Ok(SessionSurfaceKindSnapshot::Terminal),
    }
}

fn owned_event(
    name: &'static str,
    window_id: &str,
    workspace_id: &str,
    pane_id: Option<&str>,
    surface_id: Option<&str>,
    extra: Value,
) -> LifecycleEvent {
    LifecycleEvent {
        name,
        category: if name.starts_with("pane.") {
            "pane"
        } else {
            "surface"
        },
        source: "workspace.lifecycle",
        window_id: Some(window_id.to_owned()),
        workspace_id: Some(workspace_id.to_owned()),
        pane_id: pane_id.map(str::to_owned),
        surface_id: surface_id.map(str::to_owned),
        payload: extra,
    }
}

fn socket_completion_event(
    name: &'static str,
    method: &'static str,
    params: &Map<String, Value>,
    result: &Value,
    owner: &cmux_core::surface_lifecycle::Owner,
) -> LifecycleEvent {
    LifecycleEvent {
        name,
        category: if name.starts_with("pane.") {
            "pane"
        } else {
            "surface"
        },
        source: "socket.v2",
        window_id: Some(owner.window_id.clone()),
        workspace_id: Some(owner.workspace_id.clone()),
        pane_id: Some(owner.pane_id.clone()),
        surface_id: Some(owner.surface_id.clone()),
        payload: json!({"method":method,"params":params,"result":result}),
    }
}

fn public_owner_ids(
    model: &SurfaceLifecycleModel,
    owner: &cmux_core::surface_lifecycle::Owner,
) -> (String, String) {
    let window_id = owner.window_id.clone();
    let workspace_id = if model
        .pane(&owner.pane_id)
        .is_some_and(|pane| pane.container == ContainerKind::Dock)
    {
        window_id.clone()
    } else {
        owner.workspace_id.clone()
    };
    (window_id, workspace_id)
}

fn dock_owner_from_workspace_selector(
    snapshot: &AppSessionSnapshot,
    params: &Map<String, Value>,
) -> Option<String> {
    if let Some(selector) = params.get("workspace_id").and_then(Value::as_str) {
        if let Some(owner) = snapshot
            .windows
            .iter()
            .find(|window| {
                window.window_id.as_deref() == Some(selector) && window.dock.as_ref().is_some()
            })
            .and_then(|window| window.window_id.clone())
        {
            return Some(owner);
        }
    }
    let model = SurfaceLifecycleModel::from_app_session(snapshot).ok()?;
    let owner = params
        .get("surface_id")
        .and_then(Value::as_str)
        .and_then(|id| model.owner_of_surface(id))
        .or_else(|| {
            let pane_id = params.get("pane_id").and_then(Value::as_str)?;
            let pane = model.pane(pane_id)?;
            (pane.container == ContainerKind::Dock)
                .then(|| pane.surface_ids.first())
                .flatten()
                .and_then(|id| model.owner_of_surface(id))
        })?;
    model
        .pane(&owner.pane_id)
        .is_some_and(|pane| pane.container == ContainerKind::Dock)
        .then(|| owner.window_id.clone())
}

fn dock_surface_current(snapshot: &AppSessionSnapshot, owner_id: &str) -> LifecycleTransition {
    let current = DockStore.current(snapshot, owner_id);
    read_transition(
        snapshot,
        json!({
            "window_id":owner_id,
            "workspace_id":owner_id,
            "pane_id":current.as_ref().map(|surface| surface.pane_id.to_string()),
            "surface_id":current.as_ref().map(|surface| surface.surface_id.to_string()),
            "surface_type":current.as_ref().map(|surface| if surface.kind == DockSurfaceKind::Browser { "browser" } else { "terminal" }),
        }),
    )
}

fn dock_surface_list(snapshot: &AppSessionSnapshot, owner_id: &str) -> LifecycleTransition {
    let dock = DockStore.snapshot(snapshot, owner_id);
    let rows = dock
        .surfaces
        .iter()
        .enumerate()
        .map(|(index, surface)| {
            json!({
                "id":surface.surface_id.to_string(),
                "index":index,
                "type":if surface.kind == DockSurfaceKind::Browser { "browser" } else { "terminal" },
                "title":surface.title,
                "focused":dock.focused_pane_id == Some(surface.pane_id) && DockStore.current(snapshot, owner_id).is_some_and(|current| current.surface_id == surface.surface_id),
                "pane_id":surface.pane_id.to_string(),
            })
        })
        .collect::<Vec<_>>();
    read_transition(
        snapshot,
        json!({"window_id":owner_id,"workspace_id":owner_id,"surfaces":rows}),
    )
}

fn surface_current(
    snapshot: &AppSessionSnapshot,
    params: &Map<String, Value>,
    context: &LifecycleDispatchContext,
) -> LifecycleTransition {
    if let Some(owner_id) = dock_owner_from_workspace_selector(snapshot, params) {
        return dock_surface_current(snapshot, &owner_id);
    }
    let scope = match scope(snapshot, params, context) {
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

fn surface_list(
    snapshot: &AppSessionSnapshot,
    params: &Map<String, Value>,
    context: &LifecycleDispatchContext,
) -> LifecycleTransition {
    if let Some(owner_id) = dock_owner_from_workspace_selector(snapshot, params) {
        return dock_surface_list(snapshot, &owner_id);
    }
    let scope = match scope(snapshot, params, context) {
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

fn requested_placement(
    params: &Map<String, Value>,
) -> Result<&str, (&'static str, &'static str, Option<Value>)> {
    let placement = params
        .get("placement")
        .and_then(Value::as_str)
        .unwrap_or("workspace");
    if matches!(placement, "workspace" | "dock") {
        Ok(placement)
    } else {
        Err((
            "invalid_params",
            "placement must be one of: workspace, dock",
            Some(json!({"placement":placement})),
        ))
    }
}

fn dock_owner_id(
    snapshot: &AppSessionSnapshot,
    params: &Map<String, Value>,
    context: &LifecycleDispatchContext,
) -> Result<String, (&'static str, &'static str)> {
    let model = SurfaceLifecycleModel::from_app_session(snapshot)
        .map_err(|_| ("internal_error", "Invalid surface lifecycle state"))?;
    let mut implied = Vec::new();
    if let Some(workspace_id) = params.get("workspace_id").and_then(Value::as_str) {
        if let Some(window_id) = snapshot.windows.iter().find_map(|window| {
            (window.window_id.as_deref() == Some(workspace_id)
                || window
                    .tab_manager
                    .workspaces
                    .iter()
                    .any(|workspace| workspace.workspace_id.as_deref() == Some(workspace_id)))
            .then(|| window.window_id.clone())
            .flatten()
        }) {
            implied.push(window_id);
        }
    }
    if let Some(surface_id) = params.get("surface_id").and_then(Value::as_str) {
        if let Some(owner) = model.owner_of_surface(surface_id) {
            implied.push(owner.window_id.clone());
        }
    }
    if let Some(pane_id) = params.get("pane_id").and_then(Value::as_str) {
        if let Some(pane) = model.pane(pane_id) {
            implied.push(pane.window_id.clone());
        } else if Uuid::parse_str(pane_id).is_ok() {
            return Err(("not_found", "Pane not found"));
        }
    }
    if implied.windows(2).any(|owners| owners[0] != owners[1]) {
        return Err(("invalid_params", "Conflicting Dock routing selectors"));
    }
    let implied = implied.into_iter().next();
    if let Some(explicit) = params.get("window_id").filter(|value| !value.is_null()) {
        let requested = explicit
            .as_str()
            .ok_or(("unavailable", "TabManager not available"))?;
        if !snapshot
            .windows
            .iter()
            .any(|window| window.window_id.as_deref() == Some(requested))
        {
            return Err(("unavailable", "TabManager not available"));
        }
        if implied.as_deref().is_some_and(|owner| owner != requested) {
            return Err(("invalid_params", "Conflicting Dock routing selectors"));
        }
        return Ok(requested.to_owned());
    }
    if let Some(implied) = implied {
        return Ok(implied);
    }
    context
        .active_window_id
        .as_deref()
        .and_then(|active| {
            snapshot
                .windows
                .iter()
                .any(|window| window.window_id.as_deref() == Some(active))
                .then(|| active.to_owned())
        })
        .or_else(|| snapshot.windows.first()?.window_id.clone())
        .ok_or(("unavailable", "TabManager not available"))
}

fn parse_dock_kind(
    params: &Map<String, Value>,
) -> Result<DockSurfaceKind, (&'static str, &'static str, Option<Value>)> {
    let raw = params
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("terminal");
    match raw.to_ascii_lowercase().replace(['-', '_'], "").as_str() {
        "terminal" => Ok(DockSurfaceKind::Terminal),
        "browser" => Ok(DockSurfaceKind::Browser),
        "agentsession" | "markdown" | "filepreview" | "file" | "rightsidebartool"
        | "projectsidebar" | "diff" => Err((
            "invalid_params",
            "Dock placement supports only terminal and browser surfaces",
            Some(json!({"type":raw})),
        )),
        _ => Ok(DockSurfaceKind::Terminal),
    }
}

fn dock_placement_for_method(
    method: &str,
    params: &Map<String, Value>,
) -> Result<DockPlacement, (&'static str, &'static str)> {
    if method != "pane.create" {
        return Ok(DockPlacement::Tab);
    }
    match params.get("direction").and_then(Value::as_str) {
        Some(direction) if matches!(direction.to_ascii_lowercase().as_str(), "left" | "l") => {
            Ok(DockPlacement::SplitLeft)
        }
        Some(direction) if matches!(direction.to_ascii_lowercase().as_str(), "right" | "r") => {
            Ok(DockPlacement::SplitRight)
        }
        Some(direction) if matches!(direction.to_ascii_lowercase().as_str(), "up" | "u") => {
            Ok(DockPlacement::SplitUp)
        }
        Some(direction) if matches!(direction.to_ascii_lowercase().as_str(), "down" | "d") => {
            Ok(DockPlacement::SplitDown)
        }
        _ => Err((
            "invalid_params",
            "Missing or invalid direction (left|right|up|down)",
        )),
    }
}

fn dock_request(
    method: &str,
    params: &Map<String, Value>,
    kind: DockSurfaceKind,
) -> Result<DockCreateRequest, (&'static str, &'static str)> {
    let parse_uuid = |key: &str| {
        params
            .get(key)
            .and_then(Value::as_str)
            .and_then(|id| Uuid::parse_str(id).ok())
    };
    Ok(DockCreateRequest {
        kind,
        title: params
            .get("title")
            .and_then(Value::as_str)
            .map(str::to_owned),
        pane_id: parse_uuid("pane_id"),
        source_surface_id: parse_uuid("surface_id"),
        placement: dock_placement_for_method(method, params)?,
        initial_divider_position: params
            .get("initial_divider_position")
            .and_then(Value::as_f64),
        working_directory: params
            .get("working_directory")
            .and_then(Value::as_str)
            .map(str::to_owned),
        command: params
            .get("initial_command")
            .or_else(|| params.get("command"))
            .and_then(Value::as_str)
            .map(str::to_owned),
        url: params.get("url").and_then(Value::as_str).map(str::to_owned),
        browser_profile: params
            .get("browser_profile")
            .or_else(|| params.get("profile"))
            .and_then(Value::as_str)
            .map(str::to_owned),
        focus: super::bool_param(params, &["focus"]).unwrap_or(false),
        ..DockCreateRequest::default()
    })
}

fn dock_create(
    snapshot: &AppSessionSnapshot,
    method: &str,
    params: &Map<String, Value>,
    context: &LifecycleDispatchContext,
) -> LifecycleTransition {
    let kind = match parse_dock_kind(params) {
        Ok(kind) => kind,
        Err((code, message, data)) => return error(snapshot, code, message, data),
    };
    if !context.dock_available {
        return error(
            snapshot,
            "invalid_params",
            "Dock placement is disabled",
            Some(json!({"placement":"dock"})),
        );
    }
    let owner_id = match dock_owner_id(snapshot, params, context) {
        Ok(owner_id) => owner_id,
        Err((code, message)) => return error(snapshot, code, message, None),
    };
    if kind == DockSurfaceKind::Browser {
        if let Some(raw) = params.get("url").and_then(Value::as_str) {
            if url::Url::parse("https://cmux.invalid/")
                .ok()
                .and_then(|base| base.join(raw).ok())
                .is_none()
            {
                return error(
                    snapshot,
                    "invalid_params",
                    "Invalid URL",
                    Some(json!({"url":raw})),
                );
            }
        }
    }
    if kind == DockSurfaceKind::Browser && !context.browser_enabled {
        let Some(url) = params.get("url").and_then(Value::as_str) else {
            return error(
                snapshot,
                "browser_disabled",
                "cmux browser is disabled",
                None,
            );
        };
        let result = json!({"window_id":owner_id,"workspace_id":null,"pane_id":null,"surface_id":null,"created_split":false,"opened_externally":true,"browser_disabled":true,"placement_strategy":"external_browser_disabled","url":url});
        return ok_transition(
            snapshot.clone(),
            result.clone(),
            vec![],
            vec![LifecycleEffect::ExternalBrowserOpen {
                url: url.into(),
                phase: "commit",
                failure_code: "external_open_failed",
                failure_message: "Failed to open URL externally",
            }],
        );
    }
    let request = match dock_request(method, params, kind) {
        Ok(request) => request,
        Err((code, message)) => return error(snapshot, code, message, None),
    };
    let mut next = snapshot.clone();
    let created = match DockStore.create(&mut next, &owner_id, request) {
        Ok(created) => created,
        Err(_) => {
            return error(
                snapshot,
                "internal_error",
                "Failed to create Dock surface",
                None,
            )
        }
    };
    let Some(surface) = DockStore
        .snapshot(&next, &owner_id)
        .surfaces
        .into_iter()
        .find(|surface| surface.surface_id == created.surface_id)
    else {
        return error(
            snapshot,
            "internal_error",
            "Failed to create Dock surface",
            None,
        );
    };
    let pane_id = created.pane_id.to_string();
    let surface_id = created.surface_id.to_string();
    let type_name = if kind == DockSurfaceKind::Browser {
        "browser"
    } else {
        "terminal"
    };
    let origin = if method == "pane.create" {
        format!("{type_name}_split")
    } else {
        format!("{type_name}_tab")
    };
    let result = json!({
        "window_id":owner_id,
        "workspace_id":owner_id,
        "placement":"dock",
        "pane_id":null,
        "pane_ref":null,
        "surface_id":null,
        "surface_ref":null,
        "dock_pane_id":pane_id,
        "dock_surface_id":surface_id,
        "type":type_name,
    });
    let mut events = Vec::new();
    if method == "pane.create" {
        events.push(owned_event(
            "pane.created",
            &owner_id,
            &owner_id,
            Some(&pane_id),
            Some(&surface_id),
            json!({"pane_id":pane_id,"surface_id":surface_id,"origin":origin}),
        ));
    }
    events.push(owned_event(
        "surface.created",
        &owner_id,
        &owner_id,
        Some(&pane_id),
        Some(&surface_id),
        json!({"surface_id":surface_id,"pane_id":pane_id,"kind":type_name,"origin":origin,"focused":super::bool_param(params, &["focus"]).unwrap_or(false)}),
    ));
    let mut effects = vec![LifecycleEffect::DockCreate {
        owner_id: owner_id.clone(),
        dock_surface_id: surface_id,
        generation: created.generation,
        kind: type_name.into(),
        intent: surface.runtime,
        failure_code: "internal_error",
        failure_message: if method == "pane.create" {
            "Failed to create pane"
        } else {
            "Failed to create surface"
        },
        visibility_phase: "post_persist",
        rollback: "teardown",
    }];
    if super::bool_param(params, &["focus"]) == Some(true) {
        effects.push(LifecycleEffect::DockReveal {
            owner_id: owner_id.clone(),
        });
    }
    effects.extend([
        LifecycleEffect::DockChanged {
            owner_id: owner_id.clone(),
            phase: "post_persist",
        },
        LifecycleEffect::PersistSession,
    ]);
    ok_transition(next, result, events, effects)
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
    let placement = match requested_placement(params) {
        Ok(placement) => placement,
        Err((code, message, data)) => return error(snapshot, code, message, data),
    };
    if placement == "dock" {
        return dock_create(snapshot, "surface.create", params, context);
    }
    let scope = match scope(snapshot, params, context) {
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
    if super::bool_param(params, &["focus"]).unwrap_or(false) {
        let _ = model.focus_surface(&surface_id);
    }
    next = match model.to_app_session(&next) {
        Ok(value) => value,
        Err(_) => return error(snapshot, "internal_error", "Failed to create surface", None),
    };
    let effect = match &kind {
        SessionSurfaceKindSnapshot::Browser { .. } => LifecycleEffect::BrowserAttach {
            surface_id: surface_id.clone(),
            generation,
            url: params.get("url").and_then(Value::as_str).map(str::to_owned),
            failure_code: "internal_error",
            failure_message: "Failed to create surface",
        },
        SessionSurfaceKindSnapshot::Terminal
        | SessionSurfaceKindSnapshot::RemoteTerminal { .. } => LifecycleEffect::TerminalCreate {
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
            failure_code: "internal_error",
            failure_message: "Failed to create surface",
        },
        _ => LifecycleEffect::UiSurfaceAttach {
            surface_id: surface_id.clone(),
            kind: kind_name(&kind).into(),
        },
    };
    ok_transition(
        next,
        json!({"window_id":scope.window_id,"workspace_id":scope.workspace_id,"pane_id":pane_id,"surface_id":surface_id,"type":kind_name(&kind)}),
        vec![owned_event(
            "surface.created",
            &scope.window_id,
            &scope.workspace_id,
            Some(&pane_id),
            Some(&surface_id),
            json!({"surface_id":surface_id,"pane_id":pane_id,"kind":kind_name(&kind),"origin":creation_origin(&kind, false),"focused":params.get("focus").and_then(Value::as_bool).unwrap_or(false)}),
        )],
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

const SUPPORTED_SURFACE_ACTIONS: [&str; 17] = [
    "rename",
    "clear_name",
    "close_left",
    "close_right",
    "close_others",
    "new_terminal_right",
    "new_browser_right",
    "reload",
    "duplicate",
    "move_to_new_workspace",
    "detach_to_workspace",
    "detach_to_new_workspace",
    "pin",
    "unpin",
    "mark_read",
    "mark_unread",
    "toggle_full_width_tab",
];

fn normalize_surface_action(params: &Map<String, Value>) -> (String, String) {
    let action = params
        .get("action")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase()
        .replace('-', "_");
    let canonical = match action.as_str() {
        "reload_tab" => "reload",
        "duplicate_tab" => "duplicate",
        "toggle_full_width" | "toggle_full_width_tab_mode" => "toggle_full_width_tab",
        "close_to_left" => "close_left",
        "close_to_right" => "close_right",
        "close_other_tabs" => "close_others",
        "new_terminal_to_right" | "new_terminal_tab_to_right" => "new_terminal_right",
        "new_browser_to_right" | "new_browser_tab_to_right" => "new_browser_right",
        "mark_as_unread" => "mark_unread",
        "detach_to_workspace" | "detach_to_new_workspace" => "move_to_new_workspace",
        other => other,
    }
    .to_owned();
    (action, canonical)
}

fn action_target(
    model: &SurfaceLifecycleModel,
    workspace_id: &str,
    params: &Map<String, Value>,
) -> Result<
    (String, cmux_core::surface_lifecycle::Owner),
    (&'static str, &'static str, Option<Value>),
> {
    let valid_selector = |key: &str| {
        params
            .get(key)
            .and_then(Value::as_str)
            .filter(|id| Uuid::parse_str(id).is_ok() || model.surface(id).is_some())
    };
    let explicit = valid_selector("surface_id").or_else(|| valid_selector("tab_id"));
    let surface_id = explicit
        .map(str::to_owned)
        .or_else(|| model.focused_surface(workspace_id).map(str::to_owned))
        .ok_or(("not_found", "No focused tab", None))?;
    let owner = model.owner_of_surface(&surface_id).cloned().ok_or((
        "not_found",
        "Tab not found",
        Some(json!({"surface_id":surface_id,"tab_id":surface_id})),
    ))?;
    if owner.workspace_id != workspace_id {
        return Err((
            "not_found",
            "Tab not found",
            Some(json!({"surface_id":surface_id,"tab_id":surface_id})),
        ));
    }
    Ok((surface_id, owner))
}

fn foundation_url_is_valid(raw: &str) -> bool {
    !raw.trim().is_empty()
        && (url::Url::parse(raw).is_ok()
            || url::Url::parse("https://cmux.invalid/")
                .ok()
                .and_then(|base| base.join(raw).ok())
                .is_some())
}

fn close_action_range(
    model: &mut SurfaceLifecycleModel,
    action_kind: &str,
    surface_id: &str,
    owner: &cmux_core::surface_lifecycle::Owner,
    extras: &mut Map<String, Value>,
    effects: &mut Vec<LifecycleEffect>,
    lifecycle_events: &mut Vec<LifecycleEvent>,
) -> Result<(), ()> {
    let pane = model.pane(&owner.pane_id).cloned().ok_or(())?;
    let target = pane
        .surface_ids
        .iter()
        .position(|id| id == surface_id)
        .ok_or(())?;
    let candidates: Vec<String> = pane
        .surface_ids
        .iter()
        .enumerate()
        .filter(|(index, id)| match action_kind {
            "close_right" => *index > target,
            "close_left" => *index < target,
            _ => id.as_str() != surface_id,
        })
        .map(|(_, id)| id.clone())
        .collect();
    let mut closed = 0_u64;
    let mut skipped = 0_u64;
    for candidate in candidates {
        let Some(record) = model.surface(&candidate).cloned() else {
            continue;
        };
        if record.metadata.pinned {
            skipped += 1;
            continue;
        }
        if model.close_surface(&candidate, CloseIntent::Range).is_ok() {
            closed += 1;
            effects.push(LifecycleEffect::RuntimeTeardown {
                surface_id: candidate.clone(),
                generation: record.generation,
                owner_id: owner.window_id.clone(),
                dock_intent: (pane.container == ContainerKind::Dock)
                    .then(|| DockRuntimeIntent::from_record(&record)),
                must_succeed: false,
                failure_code: "internal_error",
                failure_message: "Failed to close tab",
                phase: "commit",
            });
            lifecycle_events.push(owned_event(
                "surface.closed",
                &owner.window_id,
                &owner.workspace_id,
                Some(&owner.pane_id),
                Some(&candidate),
                json!({"surface_id":candidate,"pane_id":owner.pane_id,"origin":"tab_close"}),
            ));
        }
    }
    extras.insert("closed".into(), json!(closed));
    extras.insert("skipped_pinned".into(), json!(skipped));
    Ok(())
}

fn browser_disabled_outcome(
    snapshot: &AppSessionSnapshot,
    method: &str,
    params: &Map<String, Value>,
    owner: &cmux_core::surface_lifecycle::Owner,
    url: Option<&str>,
) -> LifecycleTransition {
    let Some(url) = url else {
        return error(
            snapshot,
            "browser_disabled",
            "cmux browser is disabled",
            None,
        );
    };
    let result = json!({"window_id":owner.window_id,"workspace_id":null,"pane_id":null,"surface_id":null,"created_split":false,"opened_externally":true,"browser_disabled":true,"placement_strategy":"external_browser_disabled","url":url});
    let mut completion = action_completion(method, params, &result, owner);
    completion.workspace_id = None;
    completion.pane_id = None;
    completion.surface_id = None;
    ok_transition(
        snapshot.clone(),
        result,
        vec![completion],
        vec![LifecycleEffect::ExternalBrowserOpen {
            url: url.into(),
            phase: "commit",
            failure_code: "external_open_failed",
            failure_message: "Failed to open URL externally",
        }],
    )
}

fn normalized_local_action_working_directory(
    reported_directory: Option<&str>,
    startup_directory: Option<&str>,
    workspace_directory: Option<&str>,
) -> Option<String> {
    [reported_directory, startup_directory, workspace_directory]
        .into_iter()
        .flatten()
        .map(str::trim)
        .find(|directory| !directory.is_empty())
        .map(str::to_owned)
}

#[allow(clippy::too_many_arguments)]
fn apply_create_right_action(
    snapshot: &AppSessionSnapshot,
    method: &str,
    params: &Map<String, Value>,
    context: &LifecycleDispatchContext,
    scope: &Scope,
    owner: &cmux_core::surface_lifecycle::Owner,
    surface_id: &str,
    action: &str,
    action_kind: &str,
    mut model: SurfaceLifecycleModel,
) -> LifecycleTransition {
    let Some(source_record) = model.surface(surface_id).cloned() else {
        return error(snapshot, "internal_error", "Failed to create tab", None);
    };
    if action_kind == "duplicate"
        && !matches!(
            source_record.kind,
            SessionSurfaceKindSnapshot::Browser { .. }
        )
    {
        return error(
            snapshot,
            "invalid_state",
            "Duplicate is only available for browser tabs",
            None,
        );
    }
    let browser = matches!(action_kind, "duplicate" | "new_browser_right");
    let source_url = match &source_record.kind {
        SessionSurfaceKindSnapshot::Browser { url, .. } => url.as_deref(),
        _ => None,
    };
    let raw_url = params
        .get("url")
        .and_then(Value::as_str)
        .or_else(|| (action_kind == "duplicate").then_some(source_url).flatten());
    if browser && raw_url.is_some_and(|raw| !foundation_url_is_valid(raw)) {
        return error(
            snapshot,
            "invalid_params",
            "Invalid URL",
            Some(json!({"url":raw_url})),
        );
    }
    if browser && !context.browser_enabled {
        return browser_disabled_outcome(snapshot, method, params, owner, raw_url);
    }

    let mut effects = Vec::new();
    let local_terminal_working_directory = if action_kind == "new_terminal_right" {
        let Some(workspace) = snapshot
            .windows
            .get(scope.window_index)
            .and_then(|window| window.tab_manager.workspaces.get(scope.workspace_index))
        else {
            return error(snapshot, "internal_error", "Failed to create tab", None);
        };
        let local_working_directory = normalized_local_action_working_directory(
            source_record.metadata.reported_directory.as_deref(),
            source_record.terminal_startup.working_directory.as_deref(),
            workspace.current_directory.as_deref(),
        );
        if workspace.remote.as_ref().is_some_and(|remote| {
            remote.enabled && remote.connected && remote.transport.as_deref() == Some("tmux")
        }) {
            let focused = super::bool_param(params, &["focus"]).unwrap_or(false);
            let source_remote_pane_id = match &source_record.kind {
                SessionSurfaceKindSnapshot::RemoteTerminal {
                    remote_session_id, ..
                } => remote_session_id.clone(),
                _ => None,
            };
            let working_directory = source_remote_pane_id.as_ref().and_then(|_| {
                source_record
                    .metadata
                    .directory_provenance
                    .as_deref()
                    .is_some_and(|source| source == "remote_report")
                    .then(|| source_record.metadata.reported_directory.clone())
                    .flatten()
            });
            let destination = workspace
                .remote
                .as_ref()
                .and_then(|remote| remote.destination.clone())
                .unwrap_or_else(|| "remote".into());
            effects.push(LifecycleEffect::RemoteCreate {
                remote_session_id: workspace
                    .remote
                    .as_ref()
                    .and_then(|remote| remote.persistent_daemon_slot.clone())
                    .unwrap_or_else(|| destination.clone()),
                destination,
                window_id: owner.window_id.clone(),
                workspace_id: owner.workspace_id.clone(),
                target_pane_id: Some(owner.pane_id.clone()),
                source_surface_id: Some(surface_id.to_string()),
                source_remote_pane_id: source_remote_pane_id.clone(),
                source_pane_id: Some(owner.pane_id.clone()),
                split_direction: None,
                split_orientation: None,
                kind: "terminal".into(),
                tmux_operation: "new-window",
                arrival_policy: "runtime-window-add",
                focus: focused,
                focus_mode: if focused { "focused" } else { "background" },
                activate_window: focused,
                placement: if source_remote_pane_id.is_some() {
                    "after-source-window"
                } else {
                    "end"
                },
                working_directory: working_directory.clone(),
                working_directory_source_surface_id: working_directory
                    .as_ref()
                    .map(|_| surface_id.to_string()),
                observation_source: "tmux-new-window-output",
                observation_phase: "after-action-completion",
                pending_reconciliation: true,
                observation_failure_policy: "retain-pending-and-report",
                commit_failure_policy: "retain-pending-and-retry",
                failure_code: "internal_error",
                failure_message: "Failed to create tab",
            });
            let extras = Map::from_iter([
                ("accepted".into(), json!(true)),
                ("routed".into(), json!("remote-tmux")),
                ("created_surface_id".into(), Value::Null),
                ("created_tab_id".into(), Value::Null),
            ]);
            let result = action_result(action, owner, surface_id, extras);
            let completion = action_completion(method, params, &result, owner);
            return ok_transition(snapshot.clone(), result, vec![completion], effects);
        }
        local_working_directory
    } else {
        None
    };

    let kind = if action_kind == "duplicate" {
        source_record.kind
    } else if browser {
        SessionSurfaceKindSnapshot::Browser {
            url: Some(raw_url.unwrap_or("about:blank").into()),
            profile: params
                .get("browser_profile")
                .or_else(|| params.get("profile"))
                .and_then(Value::as_str)
                .map(str::to_owned),
            proxy_url: None,
            back_history: None,
            forward_history: None,
            omnibar_visible: None,
            focus_mode_active: None,
            developer_tools_visible: None,
            developer_tools_panel: None,
            page_zoom: None,
        }
    } else {
        SessionSurfaceKindSnapshot::Terminal
    };
    let created = Uuid::new_v4().to_string();
    let Ok(reservation) = model.reserve_surface(SurfaceSeed {
        surface_id: created.clone(),
        pane_id: owner.pane_id.clone(),
        kind: kind.clone(),
        metadata: SurfaceMetadata::default(),
    }) else {
        return error(snapshot, "internal_error", "Failed to create tab", None);
    };
    let Some(pane) = model.pane(&owner.pane_id).cloned() else {
        return error(snapshot, "internal_error", "Failed to create tab", None);
    };
    let Some(target) = pane.surface_ids.iter().position(|id| id == surface_id) else {
        return error(snapshot, "internal_error", "Failed to create tab", None);
    };
    let pinned_prefix = pane
        .surface_ids
        .iter()
        .take_while(|id| model.surface(id).is_some_and(|row| row.metadata.pinned))
        .count();
    if model
        .move_surface_transactionally(
            &created,
            &owner.pane_id,
            (target + 1).max(pinned_prefix),
            |_, _| Ok::<_, String>(()),
        )
        .is_err()
    {
        return error(snapshot, "internal_error", "Failed to create tab", None);
    }
    let focused = super::bool_param(params, &["focus"]).unwrap_or(false);
    if focused {
        if model.focus_surface(&created).is_err() {
            return error(snapshot, "internal_error", "Failed to create tab", None);
        }
        effects.push(LifecycleEffect::ActivateWindow {
            window_id: owner.window_id.clone(),
        });
    }
    if browser {
        let url = match &kind {
            SessionSurfaceKindSnapshot::Browser { url, .. } => url.clone(),
            _ => None,
        };
        effects.push(LifecycleEffect::BrowserAttach {
            surface_id: created.clone(),
            generation: reservation.generation,
            url,
            failure_code: "internal_error",
            failure_message: if action_kind == "duplicate" {
                "Failed to duplicate tab"
            } else {
                "Failed to create tab"
            },
        });
    } else {
        let startup = TerminalStartup {
            working_directory: local_terminal_working_directory.clone(),
            ..Default::default()
        };
        if model.set_terminal_startup(&created, startup).is_err() {
            return error(snapshot, "internal_error", "Failed to create tab", None);
        }
        effects.push(LifecycleEffect::TerminalCreate {
            surface_id: created.clone(),
            generation: reservation.generation,
            command: None,
            working_directory: local_terminal_working_directory,
            failure_code: "internal_error",
            failure_message: "Failed to create tab",
        });
    }
    let Ok(next) = model.to_app_session(snapshot) else {
        return error(
            snapshot,
            "internal_error",
            "Failed to commit tab action",
            None,
        );
    };
    effects.push(LifecycleEffect::PersistSession);
    let result = action_result(
        action,
        owner,
        surface_id,
        Map::from_iter([
            ("created_surface_id".into(), json!(created)),
            ("created_tab_id".into(), json!(created)),
        ]),
    );
    let events = vec![
        owned_event(
            "surface.created",
            &owner.window_id,
            &owner.workspace_id,
            Some(&owner.pane_id),
            Some(&created),
            json!({"surface_id":created,"pane_id":owner.pane_id,"kind":kind_name(&kind),"origin":creation_origin(&kind,false),"focused":focused}),
        ),
        action_completion(method, params, &result, owner),
    ];
    ok_transition(next, result, events, effects)
}

#[allow(clippy::too_many_arguments)]
fn apply_move_to_workspace_action(
    snapshot: &AppSessionSnapshot,
    method: &str,
    params: &Map<String, Value>,
    scope: &Scope,
    owner: &cmux_core::surface_lifecycle::Owner,
    surface_id: &str,
    action: &str,
    model: SurfaceLifecycleModel,
) -> LifecycleTransition {
    let workspace_count = model
        .snapshot()
        .panes
        .iter()
        .filter(|pane| pane.workspace_id == owner.workspace_id)
        .flat_map(|pane| &pane.surface_ids)
        .count();
    if workspace_count <= 1 {
        return error(
            snapshot,
            "invalid_state",
            "Tab cannot be moved to a new workspace because it is the only tab in its workspace",
            None,
        );
    }
    let Ok(mut next) = model.to_app_session(snapshot) else {
        return error(
            snapshot,
            "internal_error",
            "Failed to move tab to new workspace",
            None,
        );
    };
    let Some(window) = next.windows.get_mut(scope.window_index) else {
        return error(
            snapshot,
            "internal_error",
            "Failed to move tab to new workspace",
            None,
        );
    };
    let selected_before = window.tab_manager.selected_workspace_index;
    let selected_id_before = window.selected_workspace_id.clone();
    let Some(moved_record) = window
        .tab_manager
        .workspaces
        .get_mut(scope.workspace_index)
        .and_then(|workspace| workspace.surfaces.as_mut())
        .and_then(|rows| {
            rows.iter()
                .position(|row| row.surface_id == surface_id)
                .map(|index| rows.remove(index))
        })
    else {
        return error(
            snapshot,
            "internal_error",
            "Failed to move tab to new workspace",
            None,
        );
    };
    if !session_ops::move_panel_to_new_workspace(&mut window.tab_manager, surface_id) {
        return error(
            snapshot,
            "internal_error",
            "Failed to move tab to new workspace",
            None,
        );
    }
    let Some(source_workspace) = window
        .tab_manager
        .workspaces
        .iter_mut()
        .find(|workspace| workspace.workspace_id.as_deref() == Some(&owner.workspace_id))
    else {
        return error(
            snapshot,
            "internal_error",
            "Failed to move tab to new workspace",
            None,
        );
    };
    if source_workspace.focused_panel_id.as_deref() == Some(surface_id) {
        source_workspace.focused_panel_id = source_workspace
            .layout
            .as_ref()
            .and_then(|layout| layout_surface_ids(layout).into_iter().next());
    }
    let Some(destination_index) = window.tab_manager.workspaces.iter().position(|workspace| {
        workspace
            .layout
            .as_ref()
            .is_some_and(|layout| layout_surface_ids(layout).iter().any(|id| id == surface_id))
    }) else {
        return error(
            snapshot,
            "internal_error",
            "Failed to move tab to new workspace",
            None,
        );
    };
    let destination = &mut window.tab_manager.workspaces[destination_index];
    let workspace_id = destination
        .workspace_id
        .get_or_insert_with(|| Uuid::new_v4().to_string())
        .clone();
    destination.focused_panel_id = Some(surface_id.to_owned());
    if let Some(title) = params
        .get("title")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|title| !title.is_empty())
    {
        destination.custom_title = Some(title.into());
    }
    let Some(SessionWorkspaceLayoutSnapshot::Pane(pane)) = destination.layout.as_mut() else {
        return error(
            snapshot,
            "internal_error",
            "Failed to move tab to new workspace",
            None,
        );
    };
    let pane_id = pane
        .pane_id
        .get_or_insert_with(|| Uuid::new_v4().to_string())
        .clone();
    let mut moved_record = moved_record;
    moved_record.pane_id = pane_id.clone();
    destination.surfaces = Some(vec![moved_record]);
    if super::bool_param(params, &["focus"]).unwrap_or(false) {
        window.tab_manager.selected_workspace_index = i64::try_from(destination_index).ok();
        window.selected_workspace_id = Some(workspace_id.clone());
    } else {
        window.tab_manager.selected_workspace_index = selected_id_before
            .as_deref()
            .and_then(|id| {
                window
                    .tab_manager
                    .workspaces
                    .iter()
                    .position(|workspace| workspace.workspace_id.as_deref() == Some(id))
            })
            .and_then(|index| i64::try_from(index).ok())
            .or(selected_before);
        window.selected_workspace_id = selected_id_before;
    }
    let Ok(restored) = SurfaceLifecycleModel::from_app_session(&next) else {
        return error(
            snapshot,
            "internal_error",
            "Failed to move tab to new workspace",
            None,
        );
    };
    let Some(owner_after) = restored.owner_of_surface(surface_id).cloned() else {
        return error(
            snapshot,
            "internal_error",
            "Failed to move tab to new workspace",
            None,
        );
    };
    let result = json!({"action":action,"source_window_id":owner.window_id,"source_workspace_id":owner.workspace_id,"window_id":owner_after.window_id,"workspace_id":workspace_id,"created_workspace_id":workspace_id,"pane_id":pane_id,"surface_id":surface_id,"tab_id":surface_id});
    let completion = action_completion(method, params, &result, &owner_after);
    ok_transition(
        next,
        result,
        vec![completion],
        vec![LifecycleEffect::PersistSession],
    )
}

fn surface_action(
    snapshot: &AppSessionSnapshot,
    method: &str,
    params: &Map<String, Value>,
    context: &LifecycleDispatchContext,
) -> LifecycleTransition {
    let (action, action_kind) = normalize_surface_action(params);
    if snapshot.windows.is_empty() {
        return error(snapshot, "unavailable", "TabManager not available", None);
    }
    if action.is_empty() {
        return error(snapshot, "invalid_params", "Missing action", None);
    }
    let scope = match scope(snapshot, params, context) {
        Ok(scope) => scope,
        Err((code, message)) => return error(snapshot, code, message, None),
    };
    if action_kind == "toggle_full_width_tab" {
        let workspace =
            &snapshot.windows[scope.window_index].tab_manager.workspaces[scope.workspace_index];
        let requested = params
            .get("surface_id")
            .or_else(|| params.get("tab_id"))
            .and_then(Value::as_str)
            .or(workspace.focused_panel_id.as_deref());
        let persisted_pane = requested.and_then(|surface_id| {
            workspace
                .surfaces
                .as_deref()
                .unwrap_or_default()
                .iter()
                .find(|record| record.surface_id == surface_id)
                .map(|record| record.pane_id.as_str())
        });
        if persisted_pane
            .is_some_and(|pane_id| find_pane(workspace.layout.as_ref(), pane_id).is_none())
        {
            return error(snapshot, "not_found", "Tab pane not found", None);
        }
    }
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
    let (surface_id, owner) = match action_target(&model, &scope.workspace_id, params) {
        Ok(target) => target,
        Err((code, message, data)) => return error(snapshot, code, message, data),
    };
    if !SUPPORTED_SURFACE_ACTIONS.contains(&action_kind.as_str()) {
        return error(
            snapshot,
            "invalid_params",
            "Unknown tab action",
            Some(json!({"action":action,"supported_actions":SUPPORTED_SURFACE_ACTIONS})),
        );
    }
    let mut extras = Map::new();
    let mut effects = Vec::new();
    let mut lifecycle_events = Vec::new();
    match action_kind.as_str() {
        "rename" => {
            let title = params
                .get("title")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_owned();
            if title.is_empty() {
                return error(snapshot, "invalid_params", "Missing or invalid title", None);
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
            if model.set_custom_title(&surface_id, None).is_err() {
                return error(snapshot, "internal_error", "Failed to clear tab name", None);
            }
        }
        "pin" | "unpin" => {
            let pinned = action == "pin";
            if model
                .update_metadata(&surface_id, |metadata| metadata.pinned = pinned)
                .is_err()
            {
                return error(snapshot, "internal_error", "Failed to update tab", None);
            }
            let Some(pane) = model.pane(&owner.pane_id).cloned() else {
                return error(snapshot, "not_found", "Tab pane not found", None);
            };
            let mut normalized = pane.surface_ids.clone();
            normalized.sort_by_key(|candidate| {
                !model
                    .surface(candidate)
                    .is_some_and(|record| record.metadata.pinned)
            });
            for (index, candidate) in normalized.iter().enumerate() {
                if model
                    .move_surface_transactionally(candidate, &owner.pane_id, index, |_, _| {
                        Ok::<_, String>(())
                    })
                    .is_err()
                {
                    return error(snapshot, "internal_error", "Failed to update tab", None);
                }
            }
            extras.insert("pinned".into(), json!(pinned));
        }
        "mark_read" | "mark_unread" => {
            let unread = action_kind == "mark_unread";
            if model
                .update_metadata(&surface_id, |metadata| metadata.unread = unread)
                .is_err()
            {
                return error(snapshot, "internal_error", "Failed to update tab", None);
            }
        }
        "close_right" | "close_left" | "close_others" => {
            if close_action_range(
                &mut model,
                &action_kind,
                &surface_id,
                &owner,
                &mut extras,
                &mut effects,
                &mut lifecycle_events,
            )
            .is_err()
            {
                return error(snapshot, "internal_error", "Failed to close tabs", None);
            }
        }
        "reload" => {
            if !model.surface(&surface_id).is_some_and(|surface| {
                matches!(surface.kind, SessionSurfaceKindSnapshot::Browser { .. })
            }) {
                return error(
                    snapshot,
                    "invalid_state",
                    "Reload is only available for browser tabs",
                    None,
                );
            }
            effects.push(LifecycleEffect::BrowserReload {
                surface_id: surface_id.clone(),
                phase: "commit",
                failure_code: "internal_error",
                failure_message: "Failed to reload tab",
            });
        }
        "toggle_full_width_tab" => {
            let pane_exists = snapshot
                .windows
                .get(scope.window_index)
                .and_then(|window| window.tab_manager.workspaces.get(scope.workspace_index))
                .and_then(|workspace| find_pane(workspace.layout.as_ref(), &owner.pane_id))
                .is_some();
            if !pane_exists {
                return error(snapshot, "not_found", "Tab pane not found", None);
            }
            if model.focus_surface(&surface_id).is_err() {
                return error(snapshot, "not_found", "Tab not found", None);
            }
            // Split zoom is projected below after authoritative focus changes.
        }
        "duplicate" | "new_terminal_right" | "new_browser_right" => {
            return apply_create_right_action(
                snapshot,
                method,
                params,
                context,
                &scope,
                &owner,
                &surface_id,
                &action,
                &action_kind,
                model,
            );
        }
        "move_to_new_workspace" => {
            return apply_move_to_workspace_action(
                snapshot,
                method,
                params,
                &scope,
                &owner,
                &surface_id,
                &action,
                model,
            );
        }
        _ => unreachable!(),
    }
    let mut next = match model.to_app_session(snapshot) {
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
    if action_kind == "toggle_full_width_tab" {
        let workspace =
            &mut next.windows[scope.window_index].tab_manager.workspaces[scope.workspace_index];
        let enabled = workspace.zoomed_panel_id.as_deref() != Some(surface_id.as_str());
        workspace.zoomed_panel_id = enabled.then(|| surface_id.clone());
        extras.insert("full_width_tab_mode".into(), json!(enabled));
    }
    if next != *snapshot {
        effects.push(LifecycleEffect::PersistSession);
    }
    let result = action_result(&action, &owner, &surface_id, extras);
    let completion = action_completion(method, params, &result, &owner);
    lifecycle_events.push(completion);
    ok_transition(next, result, lifecycle_events, effects)
}

fn action_result(
    action: &str,
    owner: &cmux_core::surface_lifecycle::Owner,
    surface_id: &str,
    extras: Map<String, Value>,
) -> Value {
    let mut payload = Map::from_iter([
        ("action".into(), json!(action)),
        ("window_id".into(), json!(owner.window_id)),
        ("workspace_id".into(), json!(owner.workspace_id)),
        ("surface_id".into(), json!(surface_id)),
        ("tab_id".into(), json!(surface_id)),
        ("pane_id".into(), json!(owner.pane_id)),
    ]);
    payload.extend(extras);
    Value::Object(payload)
}

fn action_completion(
    method: &str,
    params: &Map<String, Value>,
    result: &Value,
    owner: &cmux_core::surface_lifecycle::Owner,
) -> LifecycleEvent {
    socket_completion_event(
        "surface.action",
        if method == "tab.action" {
            "tab.action"
        } else {
            "surface.action"
        },
        params,
        result,
        owner,
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
        vec![],
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
    context: &LifecycleDispatchContext,
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
            dock_owner_from_workspace_selector(snapshot, params).and_then(|owner_id| {
                model
                    .focused_surface(&format!("dock:{owner_id}"))
                    .map(str::to_owned)
            })
        })
        .or_else(|| {
            scope(snapshot, params, context).ok().and_then(|scope| {
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
    let record = model.surface(&surface_id).unwrap().clone();
    let generation = record.generation;
    let is_dock = model
        .pane(&owner.pane_id)
        .is_some_and(|pane| pane.container == ContainerKind::Dock);
    let (window_id, workspace_id) = public_owner_ids(&model, &owner);
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
    let mut effects = vec![LifecycleEffect::RuntimeTeardown {
        surface_id: surface_id.clone(),
        generation,
        owner_id: owner.window_id.clone(),
        dock_intent: is_dock.then(|| DockRuntimeIntent::from_record(&record)),
        must_succeed: true,
        failure_code: "internal_error",
        failure_message: "Failed to close surface",
        phase: "pre_publish",
    }];
    if is_dock {
        effects.push(LifecycleEffect::DockChanged {
            owner_id: window_id.clone(),
            phase: "post_persist",
        });
    }
    effects.push(LifecycleEffect::PersistSession);
    ok_transition(
        next,
        json!({"window_id":window_id,"workspace_id":workspace_id,"surface_id":surface_id}),
        vec![owned_event(
            "surface.closed",
            &window_id,
            &workspace_id,
            Some(&owner.pane_id),
            Some(&surface_id),
            json!({}),
        )],
        effects,
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
    let (window_id, workspace_id) = public_owner_ids(&model, &owner);
    let is_dock = model
        .pane(&owner.pane_id)
        .is_some_and(|pane| pane.container == ContainerKind::Dock);
    let _ = model.focus_surface(surface_id);
    let next = model.to_app_session(snapshot).unwrap();
    let mut effects = vec![LifecycleEffect::ActivateWindow {
        window_id: window_id.clone(),
    }];
    if is_dock {
        effects.extend([
            LifecycleEffect::DockReveal {
                owner_id: window_id.clone(),
            },
            LifecycleEffect::DockChanged {
                owner_id: window_id.clone(),
                phase: "post_persist",
            },
        ]);
    }
    effects.push(LifecycleEffect::PersistSession);
    ok_transition(
        next,
        json!({"window_id":window_id,"workspace_id":workspace_id,"surface_id":surface_id}),
        vec![owned_event(
            "surface.focused",
            &window_id,
            &workspace_id,
            Some(&owner.pane_id),
            Some(surface_id),
            json!({}),
        )],
        effects,
    )
}

fn surface_move(
    snapshot: &AppSessionSnapshot,
    params: &Map<String, Value>,
    context: &LifecycleDispatchContext,
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
    if model.surface(surface_id).is_none() {
        return error(
            snapshot,
            "not_found",
            "Surface not found",
            Some(json!({"surface_id":surface_id})),
        );
    }
    let source_owner = model.owner_of_surface(surface_id).cloned();
    let source_is_dock = source_owner
        .as_ref()
        .and_then(|owner| model.pane(&owner.pane_id))
        .is_some_and(|pane| pane.container == ContainerKind::Dock);
    let destination_pane = params
        .get("pane_id")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| {
            let mut destination_params = params.clone();
            destination_params.remove("surface_id");
            scope(snapshot, &destination_params, context)
                .ok()
                .and_then(|scope| model.focused_surface(&scope.workspace_id))
                .and_then(|id| model.owner_of_surface(id))
                .map(|owner| owner.pane_id.clone())
        });
    let Some(destination_pane) = destination_pane else {
        return error(snapshot, "not_found", "Destination pane not found", None);
    };
    let destination_is_dock = model
        .pane(&destination_pane)
        .is_some_and(|pane| pane.container == ContainerKind::Dock);
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
    if super::bool_param(params, &["focus"]).unwrap_or(false) {
        let _ = model.focus_surface(surface_id);
    }
    let owner = model.owner_of_surface(surface_id).cloned().unwrap();
    let (window_id, workspace_id) = public_owner_ids(&model, &owner);
    let next = model.to_app_session(snapshot).unwrap();
    let result = json!({"window_id":window_id,"workspace_id":workspace_id,"pane_id":owner.pane_id,"surface_id":surface_id});
    let public_owner = cmux_core::surface_lifecycle::Owner {
        window_id,
        workspace_id,
        ..owner
    };
    let completion = socket_completion_event(
        "surface.moved",
        "surface.move",
        params,
        &result,
        &public_owner,
    );
    let mut effects = Vec::new();
    let mut dock_owners = Vec::new();
    if source_is_dock {
        if let Some(owner_id) = source_owner.as_ref().map(|owner| owner.window_id.clone()) {
            dock_owners.push(owner_id);
        }
    }
    if destination_is_dock && !dock_owners.contains(&public_owner.window_id) {
        dock_owners.push(public_owner.window_id.clone());
    }
    for owner_id in dock_owners {
        effects.push(LifecycleEffect::DockChanged {
            owner_id,
            phase: "post_persist",
        });
    }
    effects.push(LifecycleEffect::PersistSession);
    ok_transition(next, result, vec![completion], effects)
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
        vec![owned_event(
            "pane.focused",
            &pane.window_id,
            &pane.workspace_id,
            Some(pane_id),
            Some(&selected),
            json!({}),
        )],
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
    let scope = match scope(snapshot, params, context) {
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
    let result_payload = json!({"window_id":scope.window_id,"workspace_id":scope.workspace_id,"pane_id":pane_id,"split_id":result.split_id,"old_divider_position":result.old_divider_position,"new_divider_position":result.new_divider_position});
    let completion = LifecycleEvent {
        name: "pane.resized",
        category: "pane",
        source: "socket.v2",
        window_id: Some(scope.window_id.clone()),
        workspace_id: Some(scope.workspace_id.clone()),
        pane_id: Some(pane_id.clone()),
        surface_id: None,
        payload: json!({"method":"pane.resize","params":params,"result":result_payload}),
    };
    ok_transition(
        next,
        result_payload,
        vec![completion],
        vec![LifecycleEffect::PersistSession],
    )
}

fn remote_tmux_unsupported_options(
    params: &Map<String, Value>,
    insert_first: bool,
    initial_divider_position: Option<f64>,
) -> Vec<String> {
    let mut unsupported = Vec::new();
    if insert_first {
        unsupported.push("direction=left/up".to_string());
    }
    if super::string_param(params, &["working_directory"]).is_some() {
        unsupported.push("working_directory".into());
    }
    if super::string_param(params, &["initial_command"]).is_some() {
        unsupported.push("initial_command".into());
    }
    if super::string_param(params, &["tmux_start_command"]).is_some() {
        unsupported.push("tmux_start_command".into());
    }
    if super::first_present_trimmed_string_map_param(
        params,
        &["startup_environment", "initial_env"],
    )
    .is_some_and(|environment| !environment.is_empty())
    {
        unsupported.push("startup_environment".into());
    }
    if initial_divider_position.is_some() {
        unsupported.push("initial_divider_position".into());
    }
    unsupported
}

fn pane_create(
    snapshot: &AppSessionSnapshot,
    method: &str,
    params: &Map<String, Value>,
    context: &LifecycleDispatchContext,
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
    let initial_divider_position = match super::initial_divider_position_param(params) {
        Ok(value) => value,
        Err(()) => {
            return error(
                snapshot,
                "invalid_params",
                "initial_divider_position must be numeric",
                None,
            )
        }
    };
    let placement = match requested_placement(params) {
        Ok(placement) => placement,
        Err((code, message, data)) => return error(snapshot, code, message, data),
    };
    if placement == "dock" {
        return dock_create(snapshot, "pane.create", params, context);
    }
    let scope = match scope(snapshot, params, context) {
        Ok(scope) => scope,
        Err((code, message)) => return error(snapshot, code, message, None),
    };
    let kind = match parse_kind(params) {
        Ok(kind) => kind,
        Err((code, message, data)) => return error(snapshot, code, message, data),
    };
    let workspace =
        &snapshot.windows[scope.window_index].tab_manager.workspaces[scope.workspace_index];
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
    let Some(source) = source.filter(|source| {
        workspace
            .layout
            .as_ref()
            .is_some_and(|layout| layout_surface_ids(layout).contains(source))
    }) else {
        return error(snapshot, "not_found", "No source surface to split", None);
    };
    let source_pane_id =
        session_ops::pane_id_containing_surface(workspace, &source).map(str::to_owned);
    let source_record = workspace
        .surfaces
        .as_deref()
        .unwrap_or_default()
        .iter()
        .find(|record| record.surface_id == source);
    let source_remote_pane_id = source_record.and_then(|record| match &record.kind {
        SessionSurfaceKindSnapshot::RemoteTerminal {
            remote_session_id: Some(remote_session_id),
            ..
        } if super::valid_tmux_identity(remote_session_id, '%') => Some(remote_session_id.clone()),
        _ => None,
    });
    let remote = workspace.remote.as_ref();
    let remote_tmux = method == "pane.create"
        && remote
            .is_some_and(|remote| remote.enabled && remote.transport.as_deref() == Some("tmux"))
        && matches!(kind, SessionSurfaceKindSnapshot::Terminal);
    if remote_tmux {
        let unsupported =
            remote_tmux_unsupported_options(params, insert_first, initial_divider_position);
        if !unsupported.is_empty() {
            return error(
                snapshot,
                "invalid_params",
                &format!("Not supported when targeting a remote tmux mirror workspace (the request is routed to tmux and these options cannot be applied): {}", unsupported.join(", ")),
                Some(json!({"unsupported":unsupported,"routed_target":"remote-tmux"})),
            );
        }
    }
    if remote_tmux
        && remote.is_some_and(|remote| remote.connected)
        && source_remote_pane_id.is_some()
    {
        let (split_direction, split_orientation) = match orientation {
            SessionSplitOrientation::Horizontal => (
                super::RemoteTmuxSplitDirection::Horizontal,
                SessionSplitOrientation::Horizontal,
            ),
            SessionSplitOrientation::Vertical => (
                super::RemoteTmuxSplitDirection::Vertical,
                SessionSplitOrientation::Vertical,
            ),
        };
        let destination = remote
            .and_then(|remote| remote.destination.clone())
            .unwrap_or_else(|| "remote".into());
        return ok_transition(
            snapshot.clone(),
            json!({"accepted":true,"routed":"remote-tmux","type":"terminal","window_id":scope.window_id,"workspace_id":scope.workspace_id,"pane_id":null,"surface_id":null}),
            vec![],
            vec![LifecycleEffect::RemoteCreate {
                remote_session_id: destination.clone(),
                destination,
                window_id: scope.window_id,
                workspace_id: scope.workspace_id,
                target_pane_id: None,
                source_surface_id: Some(source.clone()),
                source_remote_pane_id,
                source_pane_id,
                split_direction: Some(split_direction),
                split_orientation: Some(split_orientation),
                kind: "terminal".into(),
                tmux_operation: "split-window",
                arrival_policy: "runtime-pane-add",
                focus: true,
                focus_mode: "tmux-active",
                activate_window: false,
                placement: "split",
                working_directory: None,
                working_directory_source_surface_id: None,
                observation_source: "tmux-split-window-output",
                observation_phase: "after-action-completion",
                pending_reconciliation: true,
                observation_failure_policy: "fail-action",
                commit_failure_policy: "rollback",
                failure_code: "internal_error",
                failure_message: "Failed to create pane",
            }],
        );
    }
    if remote_tmux {
        return error(snapshot, "internal_error", "Failed to create pane", None);
    }
    let mut next = snapshot.clone();
    let workspace =
        &mut next.windows[scope.window_index].tab_manager.workspaces[scope.workspace_index];
    let source_pane_id =
        session_ops::pane_id_containing_surface(workspace, &source).map(str::to_owned);
    let surface_id = Uuid::new_v4().to_string();
    let Some(layout) = workspace.layout.as_mut() else {
        return error(snapshot, "not_found", "No source surface to split", None);
    };
    if !session_ops::split_pane(layout, &source, orientation, &surface_id, insert_first) {
        return error(snapshot, "internal_error", "Failed to create pane", None);
    }
    let pane_id = Uuid::new_v4().to_string();
    assign_created_ids(layout, &surface_id, &pane_id, initial_divider_position);
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
        Err(_) => {
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
            Err(_) => {
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
    let mut effects = vec![match &kind {
        SessionSurfaceKindSnapshot::Browser { .. } => LifecycleEffect::BrowserAttach {
            surface_id: surface_id.clone(),
            generation,
            url: params.get("url").and_then(Value::as_str).map(str::to_owned),
            failure_code: "internal_error",
            failure_message: "Failed to create pane",
        },
        SessionSurfaceKindSnapshot::Terminal
        | SessionSurfaceKindSnapshot::RemoteTerminal { .. } => LifecycleEffect::TerminalCreate {
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
            failure_code: "internal_error",
            failure_message: "Failed to create pane",
        },
        _ => LifecycleEffect::UiSurfaceAttach {
            surface_id: surface_id.clone(),
            kind: kind_name(&kind).into(),
        },
    }];
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
            owned_event(
                "pane.created",
                &scope.window_id,
                &scope.workspace_id,
                Some(&pane_id),
                Some(&surface_id),
                json!({"pane_id":pane_id,"source_pane_id":source_pane_id,"orientation":params.get("direction"),"surface_id":surface_id,"origin":creation_origin(&kind, true)}),
            ),
            owned_event(
                "surface.created",
                &scope.window_id,
                &scope.workspace_id,
                Some(&pane_id),
                Some(&surface_id),
                json!({"surface_id":surface_id,"pane_id":pane_id,"kind":kind_name(&kind),"origin":creation_origin(&kind, true),"focused":params.get("focus").and_then(Value::as_bool).unwrap_or(false)}),
            ),
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
