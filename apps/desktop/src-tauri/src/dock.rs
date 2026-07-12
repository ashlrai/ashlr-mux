use std::collections::BTreeMap;

use cmux_core::{
    session::{
        AppSessionSnapshot, SessionDockSnapshot, SessionPaneLayoutSnapshot,
        SessionSplitLayoutSnapshot, SessionSplitOrientation, SessionSurfaceKindSnapshot,
        SessionSurfaceMetadataSnapshot, SessionWorkspaceLayoutSnapshot,
    },
    surface_lifecycle::{
        CloseIntent, ContainerKind, SurfaceLifecycleModel, SurfaceSeed, TerminalStartup,
    },
};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DockSurfaceKind {
    #[default]
    Terminal,
    Browser,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DockPlacement {
    #[default]
    Tab,
    SplitLeft,
    SplitRight,
    SplitUp,
    SplitDown,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct DockCreateRequest {
    pub kind: DockSurfaceKind,
    pub title: Option<String>,
    pub pane_id: Option<Uuid>,
    pub source_surface_id: Option<Uuid>,
    pub placement: DockPlacement,
    pub initial_divider_position: Option<f64>,
    pub working_directory: Option<String>,
    pub command: Option<String>,
    pub environment: BTreeMap<String, String>,
    pub tmux_start_command: Option<String>,
    pub url: Option<String>,
    pub browser_profile: Option<String>,
    pub focus: bool,
    pub surface_id: Option<Uuid>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum DockRuntimeIntent {
    Terminal {
        working_directory: Option<String>,
        command: Option<String>,
        environment: BTreeMap<String, String>,
        tmux_start_command: Option<String>,
    },
    Browser {
        url: String,
        profile: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct DockSurfaceSnapshot {
    #[serde(rename = "id")]
    pub surface_id: Uuid,
    pub pane_id: Uuid,
    pub generation: u64,
    pub kind: DockSurfaceKind,
    pub title: String,
    pub runtime: DockRuntimeIntent,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DockPaneSnapshot {
    pub id: Uuid,
    pub surface_ids: Vec<Uuid>,
    pub selected_surface_id: Option<Uuid>,
    pub placement: String,
    pub divider_position: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DockSnapshot {
    pub owner_id: String,
    pub focused_pane_id: Option<Uuid>,
    pub panes: Vec<DockPaneSnapshot>,
    pub surfaces: Vec<DockSurfaceSnapshot>,
}

impl DockSnapshot {
    #[cfg(test)]
    fn pane(&self, id: Uuid) -> Option<&DockPaneSnapshot> {
        self.panes.iter().find(|pane| pane.id == id)
    }

    fn surface(&self, id: Uuid) -> Option<&DockSurfaceSnapshot> {
        self.surfaces
            .iter()
            .find(|surface| surface.surface_id == id)
    }

    #[cfg(test)]
    fn focused_surface_id(&self) -> Option<Uuid> {
        self.pane(self.focused_pane_id?)?.selected_surface_id
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DockCreateResult {
    pub pane_id: Uuid,
    pub surface_id: Uuid,
    pub generation: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DockRuntimeOperation {
    Create {
        surface_id: Uuid,
        generation: u64,
        intent: DockRuntimeIntent,
    },
    Teardown {
        surface_id: Uuid,
        generation: u64,
    },
}

/// Stateless adapter over the single `AppSessionSnapshot` lifecycle authority.
#[derive(Default)]
pub(crate) struct DockStore;

impl DockStore {
    /// Socket/runtime integration seam: stage the real terminal/browser effect
    /// against a cloned global snapshot and publish identities only on success.
    pub(crate) fn create_transactionally<E>(
        &self,
        session: &mut AppSessionSnapshot,
        owner_id: &str,
        request: DockCreateRequest,
        stage_runtime: impl FnOnce(&DockRuntimeOperation) -> Result<(), E>,
    ) -> Result<DockCreateResult, String>
    where
        E: ToString,
    {
        let mut next = session.clone();
        let created = self.create(&mut next, owner_id, request)?;
        let surface = self
            .snapshot(&next, owner_id)
            .surface(created.surface_id)
            .cloned()
            .ok_or_else(|| "Created Dock surface is unavailable".to_string())?;
        stage_runtime(&DockRuntimeOperation::Create {
            surface_id: created.surface_id,
            generation: created.generation,
            intent: surface.runtime,
        })
        .map_err(|error| error.to_string())?;
        *session = next;
        Ok(created)
    }

    pub(crate) fn close_transactionally<E>(
        &self,
        session: &mut AppSessionSnapshot,
        owner_id: &str,
        surface_id: Uuid,
        stage_runtime: impl FnOnce(&DockRuntimeOperation) -> Result<(), E>,
    ) -> Result<(), String>
    where
        E: ToString,
    {
        let generation = model(session)?
            .surface(&surface_id.to_string())
            .ok_or_else(|| "Dock surface not found".to_string())?
            .generation;
        let mut next = session.clone();
        self.close(&mut next, owner_id, surface_id)?;
        stage_runtime(&DockRuntimeOperation::Teardown {
            surface_id,
            generation,
        })
        .map_err(|error| error.to_string())?;
        *session = next;
        Ok(())
    }

    pub(crate) fn create(
        &self,
        session: &mut AppSessionSnapshot,
        owner_id: &str,
        request: DockCreateRequest,
    ) -> Result<DockCreateResult, String> {
        ensure_dock(session, owner_id)?;
        let before = model(session)?;
        let pane_id = match request.placement {
            DockPlacement::Tab => request
                .pane_id
                .or_else(|| focused_pane_id(&before, owner_id))
                .or_else(|| first_pane_id(session, owner_id))
                .unwrap_or_else(Uuid::new_v4),
            DockPlacement::SplitLeft
            | DockPlacement::SplitRight
            | DockPlacement::SplitUp
            | DockPlacement::SplitDown => Uuid::new_v4(),
        };
        if before.pane(&pane_id.to_string()).is_none() {
            add_dock_pane(
                session,
                owner_id,
                pane_id,
                request.placement,
                request.source_surface_id,
                request.initial_divider_position,
            )?;
        } else {
            ensure_dock_pane(&before, owner_id, pane_id)?;
        }
        let mut lifecycle = model(session)?;
        let surface_id = request.surface_id.unwrap_or_else(Uuid::new_v4);
        let kind = match request.kind {
            DockSurfaceKind::Terminal => SessionSurfaceKindSnapshot::Terminal,
            DockSurfaceKind::Browser => SessionSurfaceKindSnapshot::Browser {
                url: Some(request.url.clone().unwrap_or_else(|| "about:blank".into())),
                profile: request.browser_profile,
                proxy_url: None,
                back_history: None,
                forward_history: None,
                omnibar_visible: None,
                focus_mode_active: None,
                developer_tools_visible: None,
                developer_tools_panel: None,
                page_zoom: None,
            },
        };
        let reservation = lifecycle
            .reserve_surface(SurfaceSeed {
                surface_id: surface_id.to_string(),
                pane_id: pane_id.to_string(),
                kind,
                metadata: SessionSurfaceMetadataSnapshot {
                    custom_title: request.title,
                    ..SessionSurfaceMetadataSnapshot::default()
                },
            })
            .map_err(|error| error.to_string())?;
        if request.kind == DockSurfaceKind::Terminal {
            lifecycle
                .set_terminal_startup(
                    &surface_id.to_string(),
                    TerminalStartup {
                        command: request.command,
                        working_directory: request.working_directory,
                        environment: (!request.environment.is_empty())
                            .then_some(request.environment),
                        tmux_start_command: request.tmux_start_command,
                        ..TerminalStartup::default()
                    },
                )
                .map_err(|error| error.to_string())?;
        }
        if request.focus {
            lifecycle
                .focus_surface(&surface_id.to_string())
                .map_err(|error| error.to_string())?;
        }
        *session = lifecycle
            .to_app_session(session)
            .map_err(|error| error.to_string())?;
        Ok(DockCreateResult {
            pane_id,
            surface_id,
            generation: reservation.generation,
        })
    }

    pub(crate) fn select(
        &self,
        session: &mut AppSessionSnapshot,
        owner_id: &str,
        pane_id: Uuid,
        surface_id: Uuid,
    ) -> Result<(), String> {
        self.mutate(session, |lifecycle| {
            ensure_dock_pane(lifecycle, owner_id, pane_id)?;
            let owner = lifecycle
                .owner_of_surface(&surface_id.to_string())
                .ok_or_else(|| "Dock surface not found".to_string())?;
            if owner.pane_id != pane_id.to_string() {
                return Err("Dock surface does not belong to pane".into());
            }
            lifecycle
                .select_in_pane(&surface_id.to_string())
                .map_err(|error| error.to_string())
        })
    }

    pub(crate) fn focus(
        &self,
        session: &mut AppSessionSnapshot,
        owner_id: &str,
        surface_id: Uuid,
    ) -> Result<(), String> {
        self.mutate(session, |lifecycle| {
            ensure_dock_surface(lifecycle, owner_id, surface_id)?;
            lifecycle
                .focus_surface(&surface_id.to_string())
                .map_err(|error| error.to_string())
        })
    }

    pub(crate) fn close(
        &self,
        session: &mut AppSessionSnapshot,
        owner_id: &str,
        surface_id: Uuid,
    ) -> Result<(), String> {
        self.mutate(session, |lifecycle| {
            ensure_dock_surface(lifecycle, owner_id, surface_id)?;
            lifecycle
                .close_surface(&surface_id.to_string(), CloseIntent::Explicit)
                .map(|_| ())
                .map_err(|error| error.to_string())
        })
    }

    #[allow(dead_code, reason = "production lifecycle executor integration seam")]
    pub(crate) fn move_surface(
        &self,
        session: &mut AppSessionSnapshot,
        owner_id: &str,
        surface_id: &str,
        pane_id: Uuid,
        index: usize,
    ) -> Result<(), String> {
        self.mutate(session, |lifecycle| {
            ensure_dock_pane(lifecycle, owner_id, pane_id)?;
            lifecycle
                .move_surface_transactionally(surface_id, &pane_id.to_string(), index, |_, _| {
                    Ok::<_, String>(())
                })
                .map_err(|error| match error {
                    cmux_core::surface_lifecycle::MoveTransactionError::Model(error) => {
                        error.to_string()
                    }
                    cmux_core::surface_lifecycle::MoveTransactionError::Effect(error) => error,
                })
        })
    }

    #[allow(dead_code, reason = "production lifecycle executor integration seam")]
    pub(crate) fn current(
        &self,
        session: &AppSessionSnapshot,
        owner_id: &str,
    ) -> Option<DockSurfaceSnapshot> {
        let lifecycle = model(session).ok()?;
        let id = lifecycle.focused_surface(&dock_workspace_id(owner_id))?;
        surface_snapshot(&lifecycle, id)
    }

    #[allow(dead_code, reason = "production lifecycle executor integration seam")]
    pub(crate) fn list(
        &self,
        session: &AppSessionSnapshot,
        owner_id: &str,
    ) -> Vec<DockSurfaceSnapshot> {
        self.snapshot(session, owner_id).surfaces
    }

    pub(crate) fn snapshot(&self, session: &AppSessionSnapshot, owner_id: &str) -> DockSnapshot {
        model(session)
            .ok()
            .map(|lifecycle| snapshot_for_owner(session, &lifecycle, owner_id))
            .unwrap_or_else(|| empty_snapshot(owner_id))
    }

    fn mutate(
        &self,
        session: &mut AppSessionSnapshot,
        mutation: impl FnOnce(&mut SurfaceLifecycleModel) -> Result<(), String>,
    ) -> Result<(), String> {
        let mut lifecycle = model(session)?;
        mutation(&mut lifecycle)?;
        *session = lifecycle
            .to_app_session(session)
            .map_err(|error| error.to_string())?;
        Ok(())
    }
}

fn model(session: &AppSessionSnapshot) -> Result<SurfaceLifecycleModel, String> {
    SurfaceLifecycleModel::from_app_session(session).map_err(|error| error.to_string())
}

fn dock_workspace_id(owner_id: &str) -> String {
    format!("dock:{owner_id}")
}

fn ensure_dock(session: &mut AppSessionSnapshot, owner_id: &str) -> Result<(), String> {
    let window = session
        .windows
        .iter_mut()
        .find(|window| window.window_id.as_deref() == Some(owner_id))
        .ok_or_else(|| "Dock owner window not found".to_string())?;
    window.dock.get_or_insert_with(|| SessionDockSnapshot {
        workspace_id: dock_workspace_id(owner_id),
        layout: None,
        surfaces: Vec::new(),
        focused_surface_id: None,
    });
    Ok(())
}

fn dock_mut<'a>(
    session: &'a mut AppSessionSnapshot,
    owner_id: &str,
) -> Result<&'a mut SessionDockSnapshot, String> {
    session
        .windows
        .iter_mut()
        .find(|window| window.window_id.as_deref() == Some(owner_id))
        .and_then(|window| window.dock.as_mut())
        .ok_or_else(|| "Dock owner window not found".to_string())
}

fn empty_pane(id: Uuid) -> SessionWorkspaceLayoutSnapshot {
    SessionWorkspaceLayoutSnapshot::Pane(SessionPaneLayoutSnapshot {
        pane_id: Some(id.to_string()),
        panel_ids: Vec::new(),
        selected_panel_id: None,
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
    })
}

fn add_dock_pane(
    session: &mut AppSessionSnapshot,
    owner_id: &str,
    pane_id: Uuid,
    placement: DockPlacement,
    source_surface_id: Option<Uuid>,
    divider: Option<f64>,
) -> Result<(), String> {
    let source_pane = {
        let lifecycle = model(session)?;
        source_surface_id
            .and_then(|id| lifecycle.owner_of_surface(&id.to_string()))
            .and_then(|owner| Uuid::parse_str(&owner.pane_id).ok())
            .or_else(|| focused_pane_id(&lifecycle, owner_id))
            .or_else(|| first_pane_id(session, owner_id))
    };
    let dock = dock_mut(session, owner_id)?;
    let new_pane = empty_pane(pane_id);
    let Some(layout) = dock.layout.take() else {
        dock.layout = Some(new_pane);
        return Ok(());
    };
    if placement == DockPlacement::Tab {
        return Err("Dock pane not found".into());
    }
    let source = source_pane.ok_or_else(|| "Dock source pane not found".to_string())?;
    let orientation = if matches!(
        placement,
        DockPlacement::SplitLeft | DockPlacement::SplitRight
    ) {
        SessionSplitOrientation::Horizontal
    } else {
        SessionSplitOrientation::Vertical
    };
    let insert_first = matches!(placement, DockPlacement::SplitLeft | DockPlacement::SplitUp);
    dock.layout = Some(split_at(
        layout,
        source,
        new_pane,
        orientation,
        divider.unwrap_or(0.5).clamp(0.1, 0.9),
        insert_first,
    )?);
    Ok(())
}

fn split_at(
    layout: SessionWorkspaceLayoutSnapshot,
    source: Uuid,
    new_pane: SessionWorkspaceLayoutSnapshot,
    orientation: SessionSplitOrientation,
    divider: f64,
    insert_first: bool,
) -> Result<SessionWorkspaceLayoutSnapshot, String> {
    match layout {
        SessionWorkspaceLayoutSnapshot::Pane(pane)
            if pane.pane_id.as_deref() == Some(&source.to_string()) =>
        {
            let (first, second) = if insert_first {
                (new_pane, SessionWorkspaceLayoutSnapshot::Pane(pane))
            } else {
                (SessionWorkspaceLayoutSnapshot::Pane(pane), new_pane)
            };
            Ok(SessionWorkspaceLayoutSnapshot::Split(
                SessionSplitLayoutSnapshot {
                    split_id: Some(Uuid::new_v4().to_string()),
                    orientation,
                    divider_position: divider,
                    first: Box::new(first),
                    second: Box::new(second),
                },
            ))
        }
        SessionWorkspaceLayoutSnapshot::Pane(_) => Err("Dock source pane not found".into()),
        SessionWorkspaceLayoutSnapshot::Split(mut split) => {
            if layout_contains(&split.first, source) {
                split.first = Box::new(split_at(
                    *split.first,
                    source,
                    new_pane,
                    orientation,
                    divider,
                    insert_first,
                )?);
            } else {
                split.second = Box::new(split_at(
                    *split.second,
                    source,
                    new_pane,
                    orientation,
                    divider,
                    insert_first,
                )?);
            }
            Ok(SessionWorkspaceLayoutSnapshot::Split(split))
        }
    }
}

fn layout_contains(layout: &SessionWorkspaceLayoutSnapshot, pane_id: Uuid) -> bool {
    match layout {
        SessionWorkspaceLayoutSnapshot::Pane(pane) => {
            pane.pane_id.as_deref() == Some(&pane_id.to_string())
        }
        SessionWorkspaceLayoutSnapshot::Split(split) => {
            layout_contains(&split.first, pane_id) || layout_contains(&split.second, pane_id)
        }
    }
}

fn first_layout_pane(layout: &SessionWorkspaceLayoutSnapshot) -> Option<Uuid> {
    match layout {
        SessionWorkspaceLayoutSnapshot::Pane(pane) => {
            Uuid::parse_str(pane.pane_id.as_deref()?).ok()
        }
        SessionWorkspaceLayoutSnapshot::Split(split) => first_layout_pane(&split.first),
    }
}

fn first_pane_id(session: &AppSessionSnapshot, owner_id: &str) -> Option<Uuid> {
    let layout = session
        .windows
        .iter()
        .find(|window| window.window_id.as_deref() == Some(owner_id))?
        .dock
        .as_ref()?
        .layout
        .as_ref()?;
    first_layout_pane(layout)
}

fn focused_pane_id(lifecycle: &SurfaceLifecycleModel, owner_id: &str) -> Option<Uuid> {
    let surface = lifecycle.focused_surface(&dock_workspace_id(owner_id))?;
    Uuid::parse_str(&lifecycle.owner_of_surface(surface)?.pane_id).ok()
}

fn ensure_dock_pane(
    lifecycle: &SurfaceLifecycleModel,
    owner_id: &str,
    pane_id: Uuid,
) -> Result<(), String> {
    let pane = lifecycle
        .pane(&pane_id.to_string())
        .ok_or_else(|| "Dock pane not found".to_string())?;
    if pane.window_id != owner_id || pane.container != ContainerKind::Dock {
        return Err("Dock pane not found".into());
    }
    Ok(())
}

fn ensure_dock_surface(
    lifecycle: &SurfaceLifecycleModel,
    owner_id: &str,
    surface_id: Uuid,
) -> Result<(), String> {
    let owner = lifecycle
        .owner_of_surface(&surface_id.to_string())
        .ok_or_else(|| "Dock surface not found".to_string())?;
    ensure_dock_pane(
        lifecycle,
        owner_id,
        Uuid::parse_str(&owner.pane_id).map_err(|_| "Invalid Dock pane identity")?,
    )
}

fn snapshot_for_owner(
    session: &AppSessionSnapshot,
    lifecycle: &SurfaceLifecycleModel,
    owner_id: &str,
) -> DockSnapshot {
    let Some(dock) = session
        .windows
        .iter()
        .find(|window| window.window_id.as_deref() == Some(owner_id))
        .and_then(|window| window.dock.as_ref())
    else {
        return empty_snapshot(owner_id);
    };
    let focused_pane_id = lifecycle
        .focused_surface(&dock.workspace_id)
        .and_then(|id| lifecycle.owner_of_surface(id))
        .and_then(|owner| Uuid::parse_str(&owner.pane_id).ok());
    let mut panes = Vec::new();
    let mut surfaces = Vec::new();
    if let Some(layout) = &dock.layout {
        collect_layout(layout, "root", None, lifecycle, &mut panes, &mut surfaces);
    }
    DockSnapshot {
        owner_id: owner_id.to_string(),
        focused_pane_id,
        panes,
        surfaces,
    }
}

fn collect_layout(
    layout: &SessionWorkspaceLayoutSnapshot,
    placement: &str,
    divider: Option<f64>,
    lifecycle: &SurfaceLifecycleModel,
    panes: &mut Vec<DockPaneSnapshot>,
    surfaces: &mut Vec<DockSurfaceSnapshot>,
) {
    match layout {
        SessionWorkspaceLayoutSnapshot::Pane(raw) => {
            let Some(id) = raw
                .pane_id
                .as_deref()
                .and_then(|id| Uuid::parse_str(id).ok())
            else {
                return;
            };
            let Some(pane) = lifecycle.pane(&id.to_string()) else {
                return;
            };
            let surface_ids = pane
                .surface_ids
                .iter()
                .filter_map(|surface_id| {
                    let parsed = Uuid::parse_str(surface_id).ok()?;
                    surfaces.push(surface_snapshot(lifecycle, surface_id)?);
                    Some(parsed)
                })
                .collect();
            panes.push(DockPaneSnapshot {
                id,
                surface_ids,
                selected_surface_id: Uuid::parse_str(&pane.selected_surface_id).ok(),
                placement: placement.into(),
                divider_position: divider,
            });
        }
        SessionWorkspaceLayoutSnapshot::Split(split) => {
            let first_placement = if split.orientation == SessionSplitOrientation::Horizontal {
                "split_left"
            } else {
                "split_up"
            };
            collect_layout(
                &split.first,
                first_placement,
                Some(split.divider_position),
                lifecycle,
                panes,
                surfaces,
            );
            let second_placement = if split.orientation == SessionSplitOrientation::Horizontal {
                "split_right"
            } else {
                "split_down"
            };
            collect_layout(
                &split.second,
                second_placement,
                Some(split.divider_position),
                lifecycle,
                panes,
                surfaces,
            );
        }
    }
}

fn surface_snapshot(lifecycle: &SurfaceLifecycleModel, id: &str) -> Option<DockSurfaceSnapshot> {
    let record = lifecycle.surface(id)?;
    let title = record
        .metadata
        .custom_title
        .clone()
        .unwrap_or_else(|| match record.kind {
            SessionSurfaceKindSnapshot::Browser { .. } => "Browser".into(),
            _ => "Terminal".into(),
        });
    let (kind, runtime) = match &record.kind {
        SessionSurfaceKindSnapshot::Browser { url, profile, .. } => (
            DockSurfaceKind::Browser,
            DockRuntimeIntent::Browser {
                url: url.clone().unwrap_or_else(|| "about:blank".into()),
                profile: profile.clone(),
            },
        ),
        _ => (
            DockSurfaceKind::Terminal,
            DockRuntimeIntent::Terminal {
                working_directory: record.terminal_startup.working_directory.clone(),
                command: record.terminal_startup.command.clone(),
                environment: record
                    .terminal_startup
                    .environment
                    .clone()
                    .unwrap_or_default(),
                tmux_start_command: record.terminal_startup.tmux_start_command.clone(),
            },
        ),
    };
    Some(DockSurfaceSnapshot {
        surface_id: Uuid::parse_str(id).ok()?,
        pane_id: Uuid::parse_str(&record.pane_id).ok()?,
        generation: record.generation,
        kind,
        title,
        runtime,
    })
}

fn empty_snapshot(owner_id: &str) -> DockSnapshot {
    DockSnapshot {
        owner_id: owner_id.to_string(),
        focused_pane_id: None,
        panes: Vec::new(),
        surfaces: Vec::new(),
    }
}

pub(crate) const DOCK_CHANGED_EVENT: &str = "cmux://dock-changed";

fn emit_snapshot(
    app: &AppHandle,
    session: &AppSessionSnapshot,
    store: &DockStore,
    owner_id: &str,
) -> Result<DockSnapshot, String> {
    let snapshot = store.snapshot(session, owner_id);
    app.emit(DOCK_CHANGED_EVENT, &snapshot)
        .map_err(|error| error.to_string())?;
    Ok(snapshot)
}

#[tauri::command]
pub(crate) fn dock_snapshot(
    owner_id: String,
    state: State<'_, DockStore>,
    session: State<'_, crate::session::SessionState>,
) -> Result<DockSnapshot, String> {
    Ok(state.snapshot(&session.snapshot_for_lifecycle()?, &owner_id))
}

#[tauri::command]
pub(crate) fn dock_create(
    app: AppHandle,
    owner_id: String,
    request: DockCreateRequest,
    state: State<'_, DockStore>,
    session: State<'_, crate::session::SessionState>,
) -> Result<DockSnapshot, String> {
    session.transact_lifecycle(&app, |snapshot| {
        state
            .create_transactionally(snapshot, &owner_id, request, |_| Ok::<_, String>(()))
            .map(|_| ())
    })?;
    emit_snapshot(
        &app,
        &session.snapshot_for_lifecycle()?,
        state.inner(),
        &owner_id,
    )
}

#[tauri::command]
pub(crate) fn dock_select(
    app: AppHandle,
    owner_id: String,
    pane_id: String,
    surface_id: String,
    focus: bool,
    state: State<'_, DockStore>,
    session: State<'_, crate::session::SessionState>,
) -> Result<DockSnapshot, String> {
    let pane_id = Uuid::parse_str(&pane_id).map_err(|_| "Invalid Dock pane identity")?;
    let surface_id = Uuid::parse_str(&surface_id).map_err(|_| "Invalid Dock surface identity")?;
    session.transact_lifecycle(&app, |snapshot| {
        state.select(snapshot, &owner_id, pane_id, surface_id)?;
        if focus {
            state.focus(snapshot, &owner_id, surface_id)?;
        }
        Ok(())
    })?;
    emit_snapshot(
        &app,
        &session.snapshot_for_lifecycle()?,
        state.inner(),
        &owner_id,
    )
}

#[tauri::command]
pub(crate) fn dock_focus(
    app: AppHandle,
    owner_id: String,
    surface_id: String,
    state: State<'_, DockStore>,
    session: State<'_, crate::session::SessionState>,
) -> Result<DockSnapshot, String> {
    let surface_id = Uuid::parse_str(&surface_id).map_err(|_| "Invalid Dock surface identity")?;
    session.transact_lifecycle(&app, |snapshot| {
        state.focus(snapshot, &owner_id, surface_id)
    })?;
    emit_snapshot(
        &app,
        &session.snapshot_for_lifecycle()?,
        state.inner(),
        &owner_id,
    )
}

#[tauri::command]
pub(crate) fn dock_close(
    app: AppHandle,
    owner_id: String,
    surface_id: String,
    state: State<'_, DockStore>,
    session: State<'_, crate::session::SessionState>,
) -> Result<DockSnapshot, String> {
    let surface_id = Uuid::parse_str(&surface_id).map_err(|_| "Invalid Dock surface identity")?;
    session.transact_lifecycle(&app, |snapshot| {
        state.close_transactionally(snapshot, &owner_id, surface_id, |_| Ok::<_, String>(()))
    })?;
    emit_snapshot(
        &app,
        &session.snapshot_for_lifecycle()?,
        state.inner(),
        &owner_id,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use cmux_core::session::{decode_session, encode_session};

    fn app() -> (AppSessionSnapshot, String) {
        let state = crate::session::SessionState::default();
        let mut snapshot = state.snapshot_for_lifecycle().unwrap();
        let owner = Uuid::new_v4().to_string();
        snapshot.windows[0].window_id = Some(owner.clone());
        (snapshot, owner)
    }

    fn terminal(title: &str) -> DockCreateRequest {
        DockCreateRequest {
            kind: DockSurfaceKind::Terminal,
            title: Some(title.into()),
            working_directory: Some("C:\\repo".into()),
            command: Some("cargo test".into()),
            environment: [("CI".into(), "0".into())].into(),
            focus: true,
            ..DockCreateRequest::default()
        }
    }

    #[test]
    fn creates_orders_focuses_and_closes_heterogeneous_surfaces() {
        let (mut session, owner) = app();
        let store = DockStore;
        let first = store
            .create(&mut session, &owner, terminal("Tests"))
            .unwrap();
        let browser = store
            .create(
                &mut session,
                &owner,
                DockCreateRequest {
                    kind: DockSurfaceKind::Browser,
                    title: Some("Docs".into()),
                    url: Some("https://example.com".into()),
                    pane_id: Some(first.pane_id),
                    focus: false,
                    ..DockCreateRequest::default()
                },
            )
            .unwrap();
        let second = store
            .create(
                &mut session,
                &owner,
                DockCreateRequest {
                    placement: DockPlacement::SplitDown,
                    source_surface_id: Some(first.surface_id),
                    initial_divider_position: Some(0.4),
                    ..terminal("Logs")
                },
            )
            .unwrap();
        let snapshot = store.snapshot(&session, &owner);
        assert_eq!(snapshot.panes.len(), 2);
        assert_eq!(
            snapshot.panes[0].surface_ids,
            vec![first.surface_id, browser.surface_id]
        );
        assert_eq!(snapshot.focused_surface_id(), Some(second.surface_id));
        assert_eq!(snapshot.panes[1].divider_position, Some(0.4));
        assert!(matches!(
            snapshot.surface(browser.surface_id).unwrap().runtime,
            DockRuntimeIntent::Browser { .. }
        ));
        store.focus(&mut session, &owner, first.surface_id).unwrap();
        store
            .close(&mut session, &owner, browser.surface_id)
            .unwrap();
        assert_eq!(
            store.current(&session, &owner).unwrap().surface_id,
            first.surface_id
        );
        assert_eq!(store.list(&session, &owner).len(), 2);
    }

    #[test]
    fn all_split_directions_preserve_insertion_side_and_orientation() {
        for (placement, expected) in [
            (DockPlacement::SplitLeft, "split_left"),
            (DockPlacement::SplitRight, "split_right"),
            (DockPlacement::SplitUp, "split_up"),
            (DockPlacement::SplitDown, "split_down"),
        ] {
            let (mut session, owner) = app();
            let source = DockStore
                .create(&mut session, &owner, terminal("source"))
                .unwrap();
            let created = DockStore
                .create(
                    &mut session,
                    &owner,
                    DockCreateRequest {
                        placement,
                        source_surface_id: Some(source.surface_id),
                        focus: false,
                        ..terminal("split")
                    },
                )
                .unwrap();
            let snapshot = DockStore.snapshot(&session, &owner);
            assert_eq!(snapshot.pane(created.pane_id).unwrap().placement, expected);
        }
    }

    #[test]
    fn unfocused_create_stays_unfocused_and_browser_profile_round_trips() {
        let (mut session, owner) = app();
        let terminal = DockStore
            .create(
                &mut session,
                &owner,
                DockCreateRequest {
                    focus: false,
                    ..terminal("quiet")
                },
            )
            .unwrap();
        assert!(DockStore.current(&session, &owner).is_none());
        let browser = DockStore
            .create(
                &mut session,
                &owner,
                DockCreateRequest {
                    kind: DockSurfaceKind::Browser,
                    pane_id: Some(terminal.pane_id),
                    url: Some("https://profile.test".into()),
                    browser_profile: Some("isolated".into()),
                    focus: false,
                    ..DockCreateRequest::default()
                },
            )
            .unwrap();
        let restored = decode_session(&encode_session(&session).unwrap()).unwrap();
        assert_eq!(
            DockStore
                .snapshot(&restored, &owner)
                .surface(browser.surface_id)
                .unwrap()
                .runtime,
            DockRuntimeIntent::Browser {
                url: "https://profile.test".into(),
                profile: Some("isolated".into()),
            }
        );
        assert!(DockStore.current(&restored, &owner).is_none());
    }

    #[test]
    fn cross_container_move_and_app_session_round_trip_keep_one_owner_and_generation() {
        let (mut session, owner) = app();
        let store = DockStore;
        let dock = store
            .create(&mut session, &owner, terminal("Dock"))
            .unwrap();
        let before = model(&session).unwrap();
        let workspace_surface = before
            .snapshot()
            .panes
            .iter()
            .find(|pane| pane.container == ContainerKind::Workspace)
            .and_then(|pane| pane.surface_ids.first())
            .cloned()
            .unwrap();
        let generation = before.surface(&workspace_surface).unwrap().generation;
        store
            .move_surface(&mut session, &owner, &workspace_surface, dock.pane_id, 0)
            .unwrap();

        let bytes = encode_session(&session).unwrap();
        let decoded = decode_session(&bytes).unwrap();
        let restored = model(&decoded).unwrap();
        restored.validate_indexes().unwrap();
        let moved_owner = restored.owner_of_surface(&workspace_surface).unwrap();
        assert_eq!(moved_owner.pane_id, dock.pane_id.to_string());
        assert_eq!(
            restored.pane(&moved_owner.pane_id).unwrap().container,
            ContainerKind::Dock
        );
        assert_eq!(
            restored.surface(&workspace_surface).unwrap().generation,
            generation
        );
        assert_eq!(decoded.windows[0].dock.as_ref().unwrap().surfaces.len(), 2);
    }

    #[test]
    fn runtime_staging_failure_does_not_publish_dock_identity() {
        let (mut session, owner) = app();
        let before = session.clone();
        let error = DockStore
            .create_transactionally(&mut session, &owner, terminal("Fail"), |operation| {
                assert!(matches!(operation, DockRuntimeOperation::Create { .. }));
                Err::<(), _>("runtime unavailable")
            })
            .unwrap_err();
        assert_eq!(error, "runtime unavailable");
        assert_eq!(session, before);
    }

    #[test]
    fn production_main_window_label_is_preserved_as_dock_owner() {
        let state = crate::session::SessionState::default();
        let mut session = state.snapshot_for_lifecycle().unwrap();
        assert_eq!(session.windows[0].window_id.as_deref(), Some("main"));
        let created = DockStore
            .create(&mut session, "main", terminal("Main Dock"))
            .unwrap();
        assert_eq!(session.windows[0].window_id.as_deref(), Some("main"));
        assert_eq!(
            DockStore.current(&session, "main").unwrap().surface_id,
            created.surface_id
        );
        DockStore
            .close(&mut session, "main", created.surface_id)
            .unwrap();
        assert!(DockStore.snapshot(&session, "main").surfaces.is_empty());
        assert_eq!(session.windows[0].window_id.as_deref(), Some("main"));
    }
}
