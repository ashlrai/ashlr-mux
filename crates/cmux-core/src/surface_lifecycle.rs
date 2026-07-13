//! Authoritative pane/surface lifecycle state.
//!
//! `SessionWorkspaceSnapshot::surfaces` is the persistence authority. Layout
//! panes retain only topology, order, and pane-local selection. Runtime and
//! owner maps here are derived indexes and are never serialized.

use crate::session::{
    AppSessionSnapshot, SessionDockSnapshot, SessionPaneLayoutSnapshot,
    SessionPanelTerminalStartupSnapshot, SessionPendingRemotePwdSnapshot,
    SessionPendingSurfacePwdSnapshot, SessionSurfaceSnapshot, SessionTabManagerSnapshot,
    SessionWorkspaceLayoutSnapshot,
};
pub use crate::session::{
    SessionSurfaceKindSnapshot as SurfaceKind, SessionSurfaceMetadataSnapshot as SurfaceMetadata,
    SessionSurfaceTerminalStartupSnapshot as TerminalStartup,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use thiserror::Error;

fn synthetic_workspace_id(window_id: &str, index: usize) -> String {
    const LEGACY_WORKSPACE_NAMESPACE: uuid::Uuid =
        uuid::Uuid::from_u128(0x42f5_73ab_631f_5f20_8e1c_a654_9384_21ca);
    uuid::Uuid::new_v5(
        &LEGACY_WORKSPACE_NAMESPACE,
        format!("{window_id}:workspace:{index}").as_bytes(),
    )
    .to_string()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContainerKind {
    Workspace,
    Dock,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeHandle(String);
impl RuntimeHandle {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }
    pub fn id(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaneRecord {
    pub pane_id: String,
    pub window_id: String,
    pub workspace_id: String,
    pub container: ContainerKind,
    pub surface_ids: Vec<String>,
    pub selected_surface_id: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SurfaceRecord {
    pub surface_id: String,
    pub pane_id: String,
    pub generation: u64,
    pub kind: SurfaceKind,
    pub metadata: SurfaceMetadata,
    pub terminal_startup: TerminalStartup,
    pub runtime: Option<RuntimeHandle>,
    pub is_workspace_focused: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct PersistedSurfaceRecord {
    surface_id: String,
    pane_id: String,
    generation: u64,
    kind: SurfaceKind,
    metadata: SurfaceMetadata,
    #[serde(default)]
    terminal_startup: TerminalStartup,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LifecycleSnapshot {
    #[serde(default = "snapshot_version")]
    pub version: u32,
    #[serde(default)]
    pub panes: Vec<PaneRecord>,
    #[serde(default)]
    pub focused_surfaces: BTreeMap<String, String>,
    #[serde(default)]
    surfaces: Vec<PersistedSurfaceRecord>,
}
fn snapshot_version() -> u32 {
    2
}
impl LifecycleSnapshot {
    pub fn from_json(value: serde_json::Value) -> Result<Self, serde_json::Error> {
        serde_json::from_value(value)
    }
}

#[derive(Debug, Clone)]
pub struct PaneSeed {
    pub pane_id: String,
    pub window_id: String,
    pub workspace_id: String,
    pub container: ContainerKind,
}
#[derive(Debug, Clone)]
pub struct SurfaceSeed {
    pub surface_id: String,
    pub pane_id: String,
    pub kind: SurfaceKind,
    pub metadata: SurfaceMetadata,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Owner {
    pub window_id: String,
    pub workspace_id: String,
    pub pane_id: String,
    pub surface_id: String,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reservation {
    pub surface_id: String,
    pub generation: u64,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttachOutcome {
    Attached,
    StaleCleaned,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MoveTransactionError<E> {
    Model(LifecycleError),
    Effect(E),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseIntent {
    Explicit,
    Range,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum LifecycleError {
    #[error("pane not found: {0}")]
    PaneNotFound(String),
    #[error("surface not found: {0}")]
    SurfaceNotFound(String),
    #[error("duplicate pane: {0}")]
    DuplicatePane(String),
    #[error("duplicate surface: {0}")]
    DuplicateSurface(String),
    #[error("invalid lifecycle snapshot: {0}")]
    Invalid(String),
}

#[derive(Debug, Clone)]
pub struct LegacyPaneSnapshot {
    pub pane_id: String,
    pub window_id: String,
    pub workspace_id: String,
    pub panel_ids: Vec<String>,
    pub selected_panel_id: Option<String>,
    pub pane_surface_kind: Option<String>,
    pub per_panel_kinds: Vec<(String, String)>,
    pub panel_titles: Vec<(String, String)>,
    pub panel_pins: Vec<(String, bool)>,
    pub browser_urls: Vec<(String, String)>,
}

#[derive(Debug, Clone, Default)]
pub struct SurfaceLifecycleModel {
    panes: BTreeMap<String, PaneRecord>,
    pane_order: Vec<String>,
    surfaces: BTreeMap<String, SurfaceRecord>,
    focused_surfaces: BTreeMap<String, String>,
    surface_owners: HashMap<String, Owner>,
    runtime_owners: HashMap<String, String>,
    last_generation: HashMap<String, u64>,
    pending_pwd: BTreeMap<String, (u64, String)>,
    pending_remote_pwd: BTreeMap<(String, String), String>,
    reconciled_remote_generation: HashMap<String, u64>,
    collapsed_panes: HashSet<String>,
}

impl SurfaceLifecycleModel {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_pane(&mut self, seed: PaneSeed) -> Result<(), LifecycleError> {
        if self.panes.contains_key(&seed.pane_id) {
            return Err(LifecycleError::DuplicatePane(seed.pane_id));
        }
        let pane_id = seed.pane_id.clone();
        self.panes.insert(
            pane_id.clone(),
            PaneRecord {
                pane_id: seed.pane_id,
                window_id: seed.window_id,
                workspace_id: seed.workspace_id,
                container: seed.container,
                surface_ids: Vec::new(),
                selected_surface_id: String::new(),
            },
        );
        self.pane_order.push(pane_id);
        Ok(())
    }

    pub fn reserve_surface(&mut self, seed: SurfaceSeed) -> Result<Reservation, LifecycleError> {
        if self.surfaces.contains_key(&seed.surface_id) {
            return Err(LifecycleError::DuplicateSurface(seed.surface_id));
        }
        let pane = self
            .panes
            .get_mut(&seed.pane_id)
            .ok_or_else(|| LifecycleError::PaneNotFound(seed.pane_id.clone()))?;
        let generation = self
            .last_generation
            .get(&seed.surface_id)
            .copied()
            .unwrap_or(0)
            + 1;
        self.last_generation
            .insert(seed.surface_id.clone(), generation);
        if !pane.surface_ids.contains(&seed.surface_id) {
            pane.surface_ids.push(seed.surface_id.clone());
        }
        if pane.selected_surface_id.is_empty() {
            pane.selected_surface_id = seed.surface_id.clone();
        }
        let owner = Owner {
            window_id: pane.window_id.clone(),
            workspace_id: pane.workspace_id.clone(),
            pane_id: pane.pane_id.clone(),
            surface_id: seed.surface_id.clone(),
        };
        self.surface_owners.insert(seed.surface_id.clone(), owner);
        self.surfaces.insert(
            seed.surface_id.clone(),
            SurfaceRecord {
                surface_id: seed.surface_id.clone(),
                pane_id: seed.pane_id,
                generation,
                kind: seed.kind,
                metadata: seed.metadata,
                terminal_startup: TerminalStartup::default(),
                runtime: None,
                is_workspace_focused: false,
            },
        );
        Ok(Reservation {
            surface_id: seed.surface_id,
            generation,
        })
    }

    pub fn reserve_remote_arrival(
        &mut self,
        surface_id: &str,
        pane_id: &str,
        remote_session_id: &str,
    ) -> Result<Reservation, LifecycleError> {
        self.reserve_surface(SurfaceSeed {
            surface_id: surface_id.into(),
            pane_id: pane_id.into(),
            kind: SurfaceKind::RemoteTerminal {
                remote_session_id: Some(remote_session_id.into()),
                remote_context: None,
                arrival_generation: None,
            },
            metadata: SurfaceMetadata::default(),
        })
    }

    pub fn pane(&self, id: &str) -> Option<&PaneRecord> {
        self.panes.get(id)
    }
    pub fn surface(&self, id: &str) -> Option<&SurfaceRecord> {
        self.surfaces.get(id)
    }
    pub fn set_custom_title(
        &mut self,
        id: &str,
        title: Option<String>,
    ) -> Result<(), LifecycleError> {
        self.update_metadata(id, |metadata| metadata.custom_title = title)
    }
    pub fn update_metadata<R>(
        &mut self,
        id: &str,
        update: impl FnOnce(&mut SurfaceMetadata) -> R,
    ) -> Result<R, LifecycleError> {
        let record = self
            .surfaces
            .get_mut(id)
            .ok_or_else(|| LifecycleError::SurfaceNotFound(id.into()))?;
        Ok(update(&mut record.metadata))
    }
    pub fn set_terminal_startup(
        &mut self,
        id: &str,
        startup: TerminalStartup,
    ) -> Result<(), LifecycleError> {
        let record = self
            .surfaces
            .get_mut(id)
            .ok_or_else(|| LifecycleError::SurfaceNotFound(id.into()))?;
        if !matches!(
            record.kind,
            SurfaceKind::Terminal | SurfaceKind::RemoteTerminal { .. }
        ) {
            return Err(LifecycleError::Invalid(
                "terminal startup requires terminal".into(),
            ));
        }
        record.terminal_startup = startup;
        Ok(())
    }
    pub fn replace_kind(
        &mut self,
        id: &str,
        kind: SurfaceKind,
    ) -> Result<Reservation, LifecycleError> {
        let owner = self
            .surface_owners
            .get(id)
            .ok_or_else(|| LifecycleError::SurfaceNotFound(id.into()))?
            .clone();
        let record = self.surfaces.get_mut(id).unwrap();
        if let SurfaceKind::RemoteTerminal {
            remote_session_id: Some(remote),
            ..
        } = &record.kind
        {
            self.pending_remote_pwd
                .remove(&(owner.workspace_id, remote.clone()));
        }
        if let Some(runtime) = record.runtime.take() {
            self.runtime_owners.remove(runtime.id());
        }
        self.pending_pwd.remove(id);
        self.reconciled_remote_generation.remove(id);
        record.kind = kind;
        record.generation += 1;
        if !matches!(
            record.kind,
            SurfaceKind::Terminal | SurfaceKind::RemoteTerminal { .. }
        ) {
            record.terminal_startup = TerminalStartup::default();
        }
        self.last_generation.insert(id.into(), record.generation);
        Ok(Reservation {
            surface_id: id.into(),
            generation: record.generation,
        })
    }
    pub fn focused_surface(&self, workspace_id: &str) -> Option<&str> {
        self.focused_surfaces.get(workspace_id).map(String::as_str)
    }
    pub fn owner_of_surface(&self, id: &str) -> Option<&Owner> {
        self.surface_owners.get(id)
    }
    pub fn owner_of_runtime(&self, id: &str) -> Option<&Owner> {
        self.runtime_owners
            .get(id)
            .and_then(|surface| self.surface_owners.get(surface))
    }

    pub fn select_in_pane(&mut self, surface_id: &str) -> Result<(), LifecycleError> {
        let owner = self
            .surface_owners
            .get(surface_id)
            .ok_or_else(|| LifecycleError::SurfaceNotFound(surface_id.into()))?
            .clone();
        self.panes
            .get_mut(&owner.pane_id)
            .unwrap()
            .selected_surface_id = surface_id.into();
        Ok(())
    }

    pub fn focus_surface(&mut self, surface_id: &str) -> Result<(), LifecycleError> {
        let owner = self
            .surface_owners
            .get(surface_id)
            .ok_or_else(|| LifecycleError::SurfaceNotFound(surface_id.into()))?
            .clone();
        if let Some(old) = self
            .focused_surfaces
            .insert(owner.workspace_id, surface_id.into())
        {
            if let Some(record) = self.surfaces.get_mut(&old) {
                record.is_workspace_focused = false;
            }
        }
        self.surfaces
            .get_mut(surface_id)
            .unwrap()
            .is_workspace_focused = true;
        self.select_in_pane(surface_id)
    }

    pub fn attach_runtime(
        &mut self,
        surface_id: &str,
        generation: u64,
        runtime: RuntimeHandle,
    ) -> AttachOutcome {
        if self
            .runtime_owners
            .get(runtime.id())
            .is_some_and(|owner| owner != surface_id)
        {
            return AttachOutcome::StaleCleaned;
        }
        let Some(record) = self.surfaces.get_mut(surface_id) else {
            return AttachOutcome::StaleCleaned;
        };
        if record.generation != generation {
            return AttachOutcome::StaleCleaned;
        }
        if let Some(old) = record.runtime.replace(runtime.clone()) {
            self.runtime_owners.remove(old.id());
        }
        self.runtime_owners.insert(runtime.0, surface_id.into());
        if let Some((pending_generation, path)) = self.pending_pwd.remove(surface_id) {
            if pending_generation == generation {
                record.metadata.reported_directory = Some(path);
                record.metadata.directory_provenance = Some("arrival_report".into());
            }
        }
        AttachOutcome::Attached
    }

    pub fn close_surface(
        &mut self,
        surface_id: &str,
        intent: CloseIntent,
    ) -> Result<SurfaceRecord, LifecycleError> {
        let owner = self
            .surface_owners
            .get(surface_id)
            .ok_or_else(|| LifecycleError::SurfaceNotFound(surface_id.into()))?;
        let count = self
            .surface_owners
            .values()
            .filter(|candidate| {
                candidate.window_id == owner.window_id
                    && candidate.workspace_id == owner.workspace_id
            })
            .count();
        let is_dock = self
            .panes
            .get(&owner.pane_id)
            .is_some_and(|pane| pane.container == ContainerKind::Dock);
        if count <= 1 && !is_dock {
            return Err(LifecycleError::Invalid(
                "cannot close the last surface".into(),
            ));
        }
        if intent == CloseIntent::Range && self.surfaces[surface_id].metadata.pinned {
            return Err(LifecycleError::Invalid(
                "pinned surface is protected from range close".into(),
            ));
        }
        self.close_surface_unchecked(surface_id)
    }

    fn close_surface_unchecked(
        &mut self,
        surface_id: &str,
    ) -> Result<SurfaceRecord, LifecycleError> {
        let owner = self
            .surface_owners
            .remove(surface_id)
            .ok_or_else(|| LifecycleError::SurfaceNotFound(surface_id.into()))?;
        let record = self.surfaces.remove(surface_id).unwrap();
        if let Some(runtime) = &record.runtime {
            self.runtime_owners.remove(runtime.id());
        }
        self.pending_pwd.remove(surface_id);
        if let SurfaceKind::RemoteTerminal {
            remote_session_id: Some(remote_session_id),
            ..
        } = &record.kind
        {
            self.pending_remote_pwd
                .remove(&(owner.workspace_id.clone(), remote_session_id.clone()));
        }
        self.reconciled_remote_generation.remove(surface_id);
        let pane = self.panes.get_mut(&owner.pane_id).unwrap();
        pane.surface_ids.retain(|id| id != surface_id);
        if pane.selected_surface_id == surface_id {
            pane.selected_surface_id = pane.surface_ids.first().cloned().unwrap_or_default();
        }
        if pane.surface_ids.is_empty() {
            self.collapsed_panes.insert(owner.pane_id.clone());
        }
        if self
            .focused_surfaces
            .get(&owner.workspace_id)
            .is_some_and(|id| id == surface_id)
        {
            // The removed record is returned to the caller, but it is no longer
            // a live focused surface.
            self.focused_surfaces.remove(&owner.workspace_id);
            if let Some(next) = self
                .pane_order
                .iter()
                .filter_map(|id| self.panes.get(id))
                .filter(|p| p.workspace_id == owner.workspace_id)
                .flat_map(|p| p.surface_ids.iter())
                .next()
                .cloned()
            {
                self.focused_surfaces
                    .insert(owner.workspace_id, next.clone());
                if let Some(row) = self.surfaces.get_mut(&next) {
                    row.is_workspace_focused = true;
                }
            }
        }
        Ok(record)
    }

    /// Stage a move, run the desktop's real destination attach effect, and
    /// commit only after that effect succeeds.
    pub fn move_surface_transactionally<E, F>(
        &mut self,
        surface_id: &str,
        pane_id: &str,
        index: usize,
        attach: F,
    ) -> Result<(), MoveTransactionError<E>>
    where
        F: FnOnce(&SurfaceRecord, &Owner) -> Result<(), E>,
    {
        let mut next = self.clone();
        next.move_surface_inner(surface_id, pane_id, index)
            .map_err(MoveTransactionError::Model)?;
        next.validate_indexes()
            .map_err(MoveTransactionError::Model)?;
        attach(&next.surfaces[surface_id], &next.surface_owners[surface_id])
            .map_err(MoveTransactionError::Effect)?;
        *self = next;
        Ok(())
    }
    fn move_surface_inner(
        &mut self,
        surface_id: &str,
        pane_id: &str,
        index: usize,
    ) -> Result<(), LifecycleError> {
        let old = self
            .surface_owners
            .get(surface_id)
            .ok_or_else(|| LifecycleError::SurfaceNotFound(surface_id.into()))?
            .clone();
        if !self.panes.contains_key(pane_id) {
            return Err(LifecycleError::PaneNotFound(pane_id.into()));
        }
        let same_pane = old.pane_id == pane_id;
        let was_selected = self.panes[&old.pane_id].selected_surface_id == surface_id;
        let was_focused = self
            .focused_surfaces
            .get(&old.workspace_id)
            .is_some_and(|id| id == surface_id);
        let old_pane = self.panes.get_mut(&old.pane_id).unwrap();
        old_pane.surface_ids.retain(|id| id != surface_id);
        if was_selected && !same_pane {
            old_pane.selected_surface_id =
                old_pane.surface_ids.first().cloned().unwrap_or_default();
        }
        let source_became_empty = !same_pane && old_pane.surface_ids.is_empty();
        let destination = self.panes.get_mut(pane_id).unwrap();
        let at = index.min(destination.surface_ids.len());
        destination.surface_ids.insert(at, surface_id.into());
        self.collapsed_panes.remove(pane_id);
        if destination.selected_surface_id.is_empty() || (same_pane && was_selected) {
            destination.selected_surface_id = surface_id.into();
        }
        if source_became_empty {
            self.collapsed_panes.insert(old.pane_id.clone());
        }
        let destination_workspace = destination.workspace_id.clone();
        let owner = self.surface_owners.get_mut(surface_id).unwrap();
        owner.window_id = destination.window_id.clone();
        owner.workspace_id = destination.workspace_id.clone();
        owner.pane_id = pane_id.into();
        self.surfaces.get_mut(surface_id).unwrap().pane_id = pane_id.into();
        if was_focused && old.workspace_id != destination_workspace {
            self.focused_surfaces.remove(&old.workspace_id);
            self.surfaces
                .get_mut(surface_id)
                .unwrap()
                .is_workspace_focused = false;
            if let Some(fallback) = self
                .pane_order
                .iter()
                .filter_map(|id| self.panes.get(id))
                .filter(|pane| pane.workspace_id == old.workspace_id)
                .flat_map(|pane| pane.surface_ids.iter())
                .next()
                .cloned()
            {
                self.focused_surfaces
                    .insert(old.workspace_id.clone(), fallback.clone());
                self.surfaces
                    .get_mut(&fallback)
                    .unwrap()
                    .is_workspace_focused = true;
            }
        }
        if old.workspace_id != destination_workspace {
            if let SurfaceKind::RemoteTerminal {
                remote_session_id: Some(remote_session_id),
                ..
            } = &self.surfaces[surface_id].kind
            {
                if let Some(path) = self
                    .pending_remote_pwd
                    .remove(&(old.workspace_id, remote_session_id.clone()))
                {
                    self.pending_remote_pwd
                        .insert((destination_workspace, remote_session_id.clone()), path);
                }
            }
        }
        Ok(())
    }

    pub fn begin_respawn(
        &mut self,
        surface_id: &str,
        command: &str,
        working_directory: Option<&str>,
        tmux_start_command: Option<&str>,
    ) -> Result<Reservation, LifecycleError> {
        let owner_workspace = self
            .surface_owners
            .get(surface_id)
            .ok_or_else(|| LifecycleError::SurfaceNotFound(surface_id.into()))?
            .workspace_id
            .clone();
        let record = self
            .surfaces
            .get_mut(surface_id)
            .ok_or_else(|| LifecycleError::SurfaceNotFound(surface_id.into()))?;
        if !matches!(
            record.kind,
            SurfaceKind::Terminal | SurfaceKind::RemoteTerminal { .. }
        ) {
            return Err(LifecycleError::Invalid("respawn requires terminal".into()));
        }
        if let Some(runtime) = record.runtime.take() {
            self.runtime_owners.remove(runtime.id());
        }
        if let SurfaceKind::RemoteTerminal {
            remote_session_id: Some(remote_session_id),
            ..
        } = &record.kind
        {
            self.pending_remote_pwd
                .remove(&(owner_workspace, remote_session_id.clone()));
        }
        self.pending_pwd.remove(surface_id);
        self.reconciled_remote_generation.remove(surface_id);
        record.generation += 1;
        self.last_generation
            .insert(surface_id.into(), record.generation);
        record.terminal_startup.command = Some(command.into());
        record.terminal_startup.working_directory = working_directory.map(str::to_owned);
        record.terminal_startup.tmux_start_command = tmux_start_command.map(str::to_owned);
        Ok(Reservation {
            surface_id: surface_id.into(),
            generation: record.generation,
        })
    }

    pub fn queue_pending_pwd(
        &mut self,
        surface_id: &str,
        path: &str,
    ) -> Result<(), LifecycleError> {
        let generation = self
            .surfaces
            .get(surface_id)
            .ok_or_else(|| LifecycleError::SurfaceNotFound(surface_id.into()))?
            .generation;
        self.pending_pwd
            .insert(surface_id.into(), (generation, path.into()));
        Ok(())
    }
    pub fn has_pending_pwd(&self, surface_id: &str) -> bool {
        self.pending_pwd.contains_key(surface_id)
    }
    pub fn queue_remote_pwd(
        &mut self,
        workspace_id: &str,
        remote_id: Option<&str>,
        path: &str,
    ) -> Result<(), LifecycleError> {
        let key =
            remote_id.ok_or_else(|| LifecycleError::Invalid("remote identity required".into()))?;
        self.pending_remote_pwd
            .insert((workspace_id.into(), key.into()), path.into());
        Ok(())
    }
    pub fn has_pending_remote_pwd(&self, workspace_id: &str, remote_id: &str) -> bool {
        self.pending_remote_pwd
            .contains_key(&(workspace_id.into(), remote_id.into()))
    }
    pub fn reconcile_remote_arrival(&mut self, surface_id: &str, generation: u64) -> AttachOutcome {
        let Some(record) = self.surfaces.get(surface_id) else {
            return AttachOutcome::StaleCleaned;
        };
        if record.generation != generation {
            return AttachOutcome::StaleCleaned;
        }
        if self.reconciled_remote_generation.get(surface_id) == Some(&generation) {
            return AttachOutcome::Attached;
        }
        let remote_id = match &record.kind {
            SurfaceKind::RemoteTerminal {
                remote_session_id: Some(remote_session_id),
                ..
            } => remote_session_id.clone(),
            _ => return AttachOutcome::StaleCleaned,
        };
        let workspace = self.surface_owners[surface_id].workspace_id.clone();
        if let Some(path) = self.pending_remote_pwd.remove(&(workspace, remote_id)) {
            let metadata = &mut self.surfaces.get_mut(surface_id).unwrap().metadata;
            metadata.reported_directory = Some(path);
            metadata.directory_provenance = Some("remote_report".into());
        }
        self.reconciled_remote_generation
            .insert(surface_id.into(), generation);
        AttachOutcome::Attached
    }

    pub fn snapshot(&self) -> LifecycleSnapshot {
        LifecycleSnapshot {
            version: 2,
            panes: self
                .pane_order
                .iter()
                .filter_map(|id| self.panes.get(id).cloned())
                .collect(),
            focused_surfaces: self.focused_surfaces.clone(),
            surfaces: self
                .surfaces
                .values()
                .map(|s| PersistedSurfaceRecord {
                    surface_id: s.surface_id.clone(),
                    pane_id: s.pane_id.clone(),
                    generation: s.generation,
                    kind: s.kind.clone(),
                    metadata: s.metadata.clone(),
                    terminal_startup: s.terminal_startup.clone(),
                })
                .collect(),
        }
    }
    pub fn restore(snapshot: LifecycleSnapshot) -> Result<Self, LifecycleError> {
        let mut model = Self::new();
        for pane in snapshot.panes {
            let pane_id = pane.pane_id.clone();
            model.add_pane(PaneSeed {
                pane_id: pane_id.clone(),
                window_id: pane.window_id.clone(),
                workspace_id: pane.workspace_id.clone(),
                container: pane.container.clone(),
            })?;
            model.panes.insert(pane_id, pane);
        }
        for persisted in snapshot.surfaces {
            if !model.panes.contains_key(&persisted.pane_id) {
                return Err(LifecycleError::PaneNotFound(persisted.pane_id));
            }
            if persisted.generation == 0
                || model
                    .last_generation
                    .insert(persisted.surface_id.clone(), persisted.generation)
                    .is_some()
                || model
                    .surfaces
                    .insert(
                        persisted.surface_id.clone(),
                        SurfaceRecord {
                            surface_id: persisted.surface_id.clone(),
                            pane_id: persisted.pane_id.clone(),
                            generation: persisted.generation,
                            kind: persisted.kind,
                            metadata: persisted.metadata,
                            terminal_startup: persisted.terminal_startup,
                            runtime: None,
                            is_workspace_focused: false,
                        },
                    )
                    .is_some()
            {
                return Err(LifecycleError::Invalid(format!(
                    "duplicate or invalid surface {}",
                    persisted.surface_id
                )));
            }
            let p = &model.panes[&persisted.pane_id];
            if model
                .surface_owners
                .insert(
                    persisted.surface_id.clone(),
                    Owner {
                        window_id: p.window_id.clone(),
                        workspace_id: p.workspace_id.clone(),
                        pane_id: p.pane_id.clone(),
                        surface_id: persisted.surface_id,
                    },
                )
                .is_some()
            {
                return Err(LifecycleError::Invalid("duplicate surface owner".into()));
            }
        }
        model.focused_surfaces = snapshot.focused_surfaces;
        for id in model.focused_surfaces.values() {
            if let Some(surface) = model.surfaces.get_mut(id) {
                surface.is_workspace_focused = true;
            }
        }
        model.validate_indexes()?;
        Ok(model)
    }

    pub fn migrate_legacy(panes: Vec<LegacyPaneSnapshot>) -> Result<Self, LifecycleError> {
        let mut model = Self::new();
        for legacy in panes {
            model.add_pane(PaneSeed {
                pane_id: legacy.pane_id.clone(),
                window_id: legacy.window_id,
                workspace_id: legacy.workspace_id,
                container: ContainerKind::Workspace,
            })?;
            for id in &legacy.panel_ids {
                let explicit = legacy
                    .per_panel_kinds
                    .iter()
                    .find(|(panel, _)| panel == id)
                    .map(|(_, kind)| kind.as_str())
                    .or(legacy.pane_surface_kind.as_deref())
                    .unwrap_or("terminal");
                let browser_url = legacy
                    .browser_urls
                    .iter()
                    .find(|(panel, _)| panel == id)
                    .map(|(_, url)| url.clone());
                let kind = kind_from_legacy(explicit, browser_url);
                let metadata = SurfaceMetadata {
                    custom_title: legacy
                        .panel_titles
                        .iter()
                        .find(|(panel, _)| panel == id)
                        .map(|(_, title)| title.clone()),
                    pinned: legacy
                        .panel_pins
                        .iter()
                        .find(|(panel, _)| panel == id)
                        .is_some_and(|(_, value)| *value),
                    ..SurfaceMetadata::default()
                };
                model.reserve_surface(SurfaceSeed {
                    surface_id: id.clone(),
                    pane_id: legacy.pane_id.clone(),
                    kind,
                    metadata,
                })?;
            }
            if let Some(selected) = legacy.selected_panel_id {
                if model.surfaces.contains_key(&selected) {
                    model.select_in_pane(&selected)?;
                }
            }
        }
        Ok(model)
    }

    pub fn from_session_snapshot(
        window_id: &str,
        tabs: &SessionTabManagerSnapshot,
    ) -> Result<Self, LifecycleError> {
        let mut model = Self::new();
        for (workspace_index, workspace) in tabs.workspaces.iter().enumerate() {
            let workspace_id = workspace
                .workspace_id
                .clone()
                .unwrap_or_else(|| synthetic_workspace_id(window_id, workspace_index));
            if let Some(layout) = &workspace.layout {
                add_layout_panes(&mut model, layout, window_id, &workspace_id)?;
            }
            if let Some(records) = &workspace.surfaces {
                for persisted in records {
                    let pane = model
                        .panes
                        .get(&persisted.pane_id)
                        .ok_or_else(|| LifecycleError::PaneNotFound(persisted.pane_id.clone()))?;
                    if !pane.surface_ids.contains(&persisted.surface_id) {
                        return Err(LifecycleError::Invalid(format!(
                            "surface {} missing from pane order {}",
                            persisted.surface_id, persisted.pane_id
                        )));
                    }
                    if persisted.generation == 0
                        || model
                            .last_generation
                            .insert(persisted.surface_id.clone(), persisted.generation)
                            .is_some()
                        || model
                            .surfaces
                            .insert(persisted.surface_id.clone(), record_from_session(persisted))
                            .is_some()
                        || model
                            .surface_owners
                            .insert(
                                persisted.surface_id.clone(),
                                Owner {
                                    window_id: window_id.into(),
                                    workspace_id: workspace_id.clone(),
                                    pane_id: persisted.pane_id.clone(),
                                    surface_id: persisted.surface_id.clone(),
                                },
                            )
                            .is_some()
                    {
                        return Err(LifecycleError::Invalid(format!(
                            "duplicate or invalid surface {}",
                            persisted.surface_id
                        )));
                    }
                }
            } else {
                migrate_workspace_legacy(&mut model, workspace, &workspace_id)?;
            }
            if let Some(focused) = &workspace.focused_panel_id {
                if !model
                    .surface_owners
                    .get(focused)
                    .is_some_and(|owner| owner.workspace_id == workspace_id)
                {
                    return Err(LifecycleError::Invalid(format!(
                        "stale focused surface {focused}"
                    )));
                }
                model
                    .focused_surfaces
                    .insert(workspace_id.clone(), focused.clone());
                model
                    .surfaces
                    .get_mut(focused)
                    .unwrap()
                    .is_workspace_focused = true;
            }
            for pending in workspace.pending_remote_pwds.as_deref().unwrap_or_default() {
                let key = (workspace_id.clone(), pending.remote_session_id.clone());
                if model
                    .pending_remote_pwd
                    .insert(key, pending.path.clone())
                    .is_some()
                {
                    return Err(LifecycleError::Invalid(format!(
                        "duplicate pending remote pwd {}",
                        pending.remote_session_id
                    )));
                }
            }
            for pending in workspace
                .pending_surface_pwds
                .as_deref()
                .unwrap_or_default()
            {
                let owner = model
                    .surface_owners
                    .get(&pending.surface_id)
                    .ok_or_else(|| {
                        LifecycleError::Invalid(format!(
                            "pending pwd references missing surface {}",
                            pending.surface_id
                        ))
                    })?;
                if owner.workspace_id != workspace_id
                    || model.surfaces[&pending.surface_id].generation != pending.generation
                    || model
                        .pending_pwd
                        .insert(
                            pending.surface_id.clone(),
                            (pending.generation, pending.path.clone()),
                        )
                        .is_some()
                {
                    return Err(LifecycleError::Invalid(format!(
                        "invalid pending pwd {}",
                        pending.surface_id
                    )));
                }
            }
        }
        model.validate_indexes()?;
        Ok(model)
    }

    /// Build one application-wide authority. Public surface/pane identities and
    /// their reverse indexes are validated across all windows, not per window.
    pub fn from_app_session(snapshot: &AppSessionSnapshot) -> Result<Self, LifecycleError> {
        let mut merged = Self::new();
        for (index, window) in snapshot.windows.iter().enumerate() {
            let window_id = window
                .window_id
                .clone()
                .unwrap_or_else(|| format!("window:{index}"));
            let part = Self::from_session_snapshot(&window_id, &window.tab_manager)?;
            merged.merge(part)?;
            if let Some(dock) = &window.dock {
                merged.merge(Self::from_dock_snapshot(&window_id, dock)?)?;
            }
        }
        merged.validate_indexes()?;
        Ok(merged)
    }

    fn from_dock_snapshot(
        window_id: &str,
        dock: &SessionDockSnapshot,
    ) -> Result<Self, LifecycleError> {
        let mut model = Self::new();
        if let Some(layout) = &dock.layout {
            add_layout_panes_with_container(
                &mut model,
                layout,
                window_id,
                &dock.workspace_id,
                ContainerKind::Dock,
            )?;
        }
        for persisted in &dock.surfaces {
            let pane = model
                .panes
                .get(&persisted.pane_id)
                .ok_or_else(|| LifecycleError::PaneNotFound(persisted.pane_id.clone()))?;
            if !pane.surface_ids.contains(&persisted.surface_id)
                || persisted.generation == 0
                || model
                    .last_generation
                    .insert(persisted.surface_id.clone(), persisted.generation)
                    .is_some()
                || model
                    .surfaces
                    .insert(persisted.surface_id.clone(), record_from_session(persisted))
                    .is_some()
                || model
                    .surface_owners
                    .insert(
                        persisted.surface_id.clone(),
                        Owner {
                            window_id: window_id.into(),
                            workspace_id: dock.workspace_id.clone(),
                            pane_id: persisted.pane_id.clone(),
                            surface_id: persisted.surface_id.clone(),
                        },
                    )
                    .is_some()
            {
                return Err(LifecycleError::Invalid(format!(
                    "duplicate or invalid Dock surface {}",
                    persisted.surface_id
                )));
            }
        }
        if let Some(focused) = &dock.focused_surface_id {
            if !model.surface_owners.contains_key(focused) {
                return Err(LifecycleError::Invalid(format!(
                    "stale Dock focus {focused}"
                )));
            }
            model
                .focused_surfaces
                .insert(dock.workspace_id.clone(), focused.clone());
            model
                .surfaces
                .get_mut(focused)
                .unwrap()
                .is_workspace_focused = true;
        }
        model.validate_indexes()?;
        Ok(model)
    }

    /// Explicitly named compatibility constructor used by desktop lifecycle
    /// callers to distinguish the application-wide authority from the
    /// single-window session constructor.
    pub fn from_app_session_snapshot(
        snapshot: &AppSessionSnapshot,
    ) -> Result<Self, LifecycleError> {
        Self::from_app_session(snapshot)
    }

    fn merge(&mut self, other: Self) -> Result<(), LifecycleError> {
        for (id, pane) in other.panes {
            if self.panes.insert(id.clone(), pane).is_some() {
                return Err(LifecycleError::DuplicatePane(id));
            }
        }
        self.pane_order.extend(other.pane_order);
        for (id, surface) in other.surfaces {
            if self.surfaces.insert(id.clone(), surface).is_some() {
                return Err(LifecycleError::DuplicateSurface(id));
            }
        }
        for (id, owner) in other.surface_owners {
            if self.surface_owners.insert(id.clone(), owner).is_some() {
                return Err(LifecycleError::DuplicateSurface(id));
            }
        }
        for (id, generation) in other.last_generation {
            if self
                .last_generation
                .insert(id.clone(), generation)
                .is_some()
            {
                return Err(LifecycleError::DuplicateSurface(id));
            }
        }
        for (workspace, focused) in other.focused_surfaces {
            if self
                .focused_surfaces
                .insert(workspace.clone(), focused)
                .is_some()
            {
                return Err(LifecycleError::Invalid(format!(
                    "duplicate workspace identity {workspace}"
                )));
            }
        }
        for (key, path) in other.pending_remote_pwd {
            if self.pending_remote_pwd.insert(key.clone(), path).is_some() {
                return Err(LifecycleError::Invalid(format!(
                    "duplicate pending remote identity {}",
                    key.1
                )));
            }
        }
        for (surface_id, pending) in other.pending_pwd {
            if self
                .pending_pwd
                .insert(surface_id.clone(), pending)
                .is_some()
            {
                return Err(LifecycleError::Invalid(format!(
                    "duplicate pending surface identity {surface_id}"
                )));
            }
        }
        Ok(())
    }

    pub fn to_session_snapshot(
        &self,
        base: &SessionTabManagerSnapshot,
    ) -> Result<SessionTabManagerSnapshot, LifecycleError> {
        let window_id = self
            .panes
            .values()
            .next()
            .map(|pane| pane.window_id.as_str());
        self.project_tab_manager(window_id, base)
    }

    pub fn to_app_session(
        &self,
        base: &AppSessionSnapshot,
    ) -> Result<AppSessionSnapshot, LifecycleError> {
        let mut projected = base.clone();
        for (index, window) in projected.windows.iter_mut().enumerate() {
            let window_id = window
                .window_id
                .clone()
                .unwrap_or_else(|| format!("window:{index}"));
            window.tab_manager = self.project_tab_manager(Some(&window_id), &window.tab_manager)?;
            if let Some(dock) = window.dock.as_mut() {
                dock.surfaces = self
                    .pane_order
                    .iter()
                    .filter_map(|id| self.panes.get(id))
                    .filter(|pane| {
                        pane.container == ContainerKind::Dock
                            && pane.window_id == window_id
                            && pane.workspace_id == dock.workspace_id
                    })
                    .flat_map(|pane| pane.surface_ids.iter())
                    .filter_map(|id| self.surfaces.get(id))
                    .map(record_to_session)
                    .collect();
                dock.focused_surface_id = self.focused_surfaces.get(&dock.workspace_id).cloned();
                if let Some(layout) = dock.layout.take() {
                    dock.layout = project_layout(layout, &self.panes, &self.collapsed_panes);
                }
            }
        }
        Ok(projected)
    }

    fn project_tab_manager(
        &self,
        window_id: Option<&str>,
        base: &SessionTabManagerSnapshot,
    ) -> Result<SessionTabManagerSnapshot, LifecycleError> {
        let mut projected = base.clone();
        for (workspace_index, workspace) in projected.workspaces.iter_mut().enumerate() {
            let workspace_id = workspace.workspace_id.clone().unwrap_or_else(|| {
                synthetic_workspace_id(window_id.unwrap_or("window:0"), workspace_index)
            });
            workspace.workspace_id = Some(workspace_id.clone());
            workspace.surfaces = Some(
                self.pane_order
                    .iter()
                    .filter_map(|id| self.panes.get(id))
                    .filter(|pane| {
                        pane.workspace_id == workspace_id
                            && window_id.is_none_or(|window| pane.window_id == window)
                    })
                    .flat_map(|pane| pane.surface_ids.iter())
                    .filter_map(|id| self.surfaces.get(id))
                    .map(record_to_session)
                    .collect(),
            );
            workspace.focused_panel_id = self.focused_surfaces.get(&workspace_id).cloned();
            let pending = self
                .pending_remote_pwd
                .iter()
                .filter(|((owner, _), _)| owner == &workspace_id)
                .map(
                    |((_, remote_session_id), path)| SessionPendingRemotePwdSnapshot {
                        remote_session_id: remote_session_id.clone(),
                        path: path.clone(),
                    },
                )
                .collect::<Vec<_>>();
            workspace.pending_remote_pwds = (!pending.is_empty()).then_some(pending);
            let pending_surface = self
                .pending_pwd
                .iter()
                .filter_map(|(surface_id, (generation, path))| {
                    self.surface_owners
                        .get(surface_id)
                        .filter(|owner| {
                            owner.workspace_id == workspace_id
                                && window_id.is_none_or(|window| owner.window_id == window)
                        })
                        .map(|_| SessionPendingSurfacePwdSnapshot {
                            surface_id: surface_id.clone(),
                            generation: *generation,
                            path: path.clone(),
                        })
                })
                .collect::<Vec<_>>();
            workspace.pending_surface_pwds =
                (!pending_surface.is_empty()).then_some(pending_surface);
            workspace.panel_titles = None;
            workspace.panel_pins = None;
            workspace.panel_unreads = None;
            workspace.panel_terminal_startups = None;
            workspace.restorable_agent_snapshots = None;
            if let Some(layout) = workspace.layout.take() {
                workspace.layout = project_layout(layout, &self.panes, &self.collapsed_panes);
            }
        }
        Ok(projected)
    }

    pub fn validate_indexes(&self) -> Result<(), LifecycleError> {
        let pane_order_set = self.pane_order.iter().collect::<HashSet<_>>();
        if pane_order_set.len() != self.pane_order.len()
            || pane_order_set.len() != self.panes.len()
            || self.panes.keys().any(|id| !pane_order_set.contains(id))
        {
            return Err(LifecycleError::Invalid("pane order/index mismatch".into()));
        }
        let mut ordered = HashSet::new();
        for pane in self.panes.values() {
            let mut within_pane = HashSet::new();
            for id in &pane.surface_ids {
                if !within_pane.insert(id) || !ordered.insert(id) {
                    return Err(LifecycleError::Invalid(format!(
                        "duplicate pane order surface {id}"
                    )));
                }
                let surface = self.surfaces.get(id).ok_or_else(|| {
                    LifecycleError::Invalid(format!("pane order references missing surface {id}"))
                })?;
                let owner = self
                    .surface_owners
                    .get(id)
                    .ok_or_else(|| LifecycleError::Invalid(format!("missing owner for {id}")))?;
                if surface.pane_id != pane.pane_id
                    || owner.pane_id != pane.pane_id
                    || owner.workspace_id != pane.workspace_id
                    || owner.window_id != pane.window_id
                    || owner.surface_id != *id
                {
                    return Err(LifecycleError::Invalid(format!(
                        "owner/order mismatch for {id}"
                    )));
                }
            }
            if pane.surface_ids.is_empty() {
                if !pane.selected_surface_id.is_empty() {
                    return Err(LifecycleError::Invalid(format!(
                        "empty pane {} has selection",
                        pane.pane_id
                    )));
                }
            } else if !pane.surface_ids.contains(&pane.selected_surface_id) {
                return Err(LifecycleError::Invalid(format!(
                    "stale pane selection {}",
                    pane.selected_surface_id
                )));
            }
        }
        if ordered.len() != self.surfaces.len() || self.surface_owners.len() != self.surfaces.len()
        {
            return Err(LifecycleError::Invalid(
                "surface/index cardinality mismatch".into(),
            ));
        }
        for (id, surface) in &self.surfaces {
            let owner = self
                .surface_owners
                .get(id)
                .ok_or_else(|| LifecycleError::Invalid(format!("missing owner for {id}")))?;
            if owner.pane_id != surface.pane_id
                || !self
                    .panes
                    .get(&owner.pane_id)
                    .is_some_and(|pane| pane.surface_ids.contains(id))
            {
                return Err(LifecycleError::Invalid(format!(
                    "owner/order mismatch for {id}"
                )));
            }
        }
        for (surface_id, (generation, _)) in &self.pending_pwd {
            if !self
                .surfaces
                .get(surface_id)
                .is_some_and(|surface| surface.generation == *generation)
            {
                return Err(LifecycleError::Invalid(format!(
                    "stale pending pwd {surface_id}"
                )));
            }
        }
        let mut runtime_ids = HashSet::new();
        for (id, surface) in &self.surfaces {
            if let Some(runtime) = &surface.runtime {
                if !runtime_ids.insert(runtime.id())
                    || self.runtime_owners.get(runtime.id()).map(String::as_str)
                        != Some(id.as_str())
                {
                    return Err(LifecycleError::Invalid(format!(
                        "duplicate runtime {}",
                        runtime.id()
                    )));
                }
            }
        }
        if runtime_ids.len() != self.runtime_owners.len() {
            return Err(LifecycleError::Invalid(
                "runtime index cardinality mismatch".into(),
            ));
        }
        for (runtime, surface) in &self.runtime_owners {
            if self
                .surfaces
                .get(surface)
                .and_then(|row| row.runtime.as_ref())
                .map(RuntimeHandle::id)
                != Some(runtime.as_str())
            {
                return Err(LifecycleError::Invalid(format!(
                    "runtime index mismatch for {runtime}"
                )));
            }
        }
        for (workspace, focused) in &self.focused_surfaces {
            let owner = self.surface_owners.get(focused).ok_or_else(|| {
                LifecycleError::Invalid(format!("stale focused surface {focused}"))
            })?;
            if &owner.workspace_id != workspace
                || !self
                    .surfaces
                    .get(focused)
                    .is_some_and(|surface| surface.is_workspace_focused)
            {
                return Err(LifecycleError::Invalid(format!(
                    "focused owner mismatch for {focused}"
                )));
            }
        }
        for (id, surface) in &self.surfaces {
            let should_focus = self.focused_surfaces.values().any(|focused| focused == id);
            if surface.is_workspace_focused != should_focus {
                return Err(LifecycleError::Invalid(format!(
                    "focused flag mismatch for {id}"
                )));
            }
        }
        Ok(())
    }
}

fn add_layout_panes(
    model: &mut SurfaceLifecycleModel,
    layout: &SessionWorkspaceLayoutSnapshot,
    window: &str,
    workspace: &str,
) -> Result<(), LifecycleError> {
    add_layout_panes_with_container(model, layout, window, workspace, ContainerKind::Workspace)
}

fn add_layout_panes_with_container(
    model: &mut SurfaceLifecycleModel,
    layout: &SessionWorkspaceLayoutSnapshot,
    window: &str,
    workspace: &str,
    container: ContainerKind,
) -> Result<(), LifecycleError> {
    match layout {
        SessionWorkspaceLayoutSnapshot::Pane(pane) => {
            let pane_id = pane.pane_id.clone().unwrap_or_else(|| {
                pane.panel_ids
                    .first()
                    .cloned()
                    .unwrap_or_else(|| format!("pane:{workspace}"))
            });
            model.add_pane(PaneSeed {
                pane_id: pane_id.clone(),
                window_id: window.into(),
                workspace_id: workspace.into(),
                container,
            })?;
            let row = model.panes.get_mut(&pane_id).unwrap();
            row.surface_ids = pane.panel_ids.clone();
            row.selected_surface_id = pane
                .selected_panel_id
                .clone()
                .or_else(|| pane.panel_ids.first().cloned())
                .unwrap_or_default();
        }
        SessionWorkspaceLayoutSnapshot::Split(split) => {
            add_layout_panes_with_container(
                model,
                &split.first,
                window,
                workspace,
                container.clone(),
            )?;
            add_layout_panes_with_container(model, &split.second, window, workspace, container)?;
        }
    }
    Ok(())
}

fn migrate_workspace_legacy(
    model: &mut SurfaceLifecycleModel,
    workspace: &crate::session::SessionWorkspaceSnapshot,
    workspace_id: &str,
) -> Result<(), LifecycleError> {
    let pane_ids: Vec<String> = model
        .pane_order
        .iter()
        .filter_map(|id| model.panes.get(id))
        .filter(|p| p.workspace_id == workspace_id)
        .map(|p| p.pane_id.clone())
        .collect();
    for pane_id in pane_ids {
        let pane = model.panes[&pane_id].clone();
        let legacy_pane = find_pane(workspace.layout.as_ref(), &pane_id);
        for id in pane.surface_ids {
            let mut kind_name = legacy_pane
                .and_then(|p| p.surface_kind.as_deref())
                .unwrap_or("terminal");
            let browser_selected = legacy_pane.is_some_and(|p| {
                let target = p
                    .selected_panel_id
                    .as_deref()
                    .or_else(|| p.panel_ids.first().map(String::as_str));
                p.browser_url.is_some() && target == Some(id.as_str())
            });
            if browser_selected {
                kind_name = "browser";
            }
            let browser_url = if browser_selected {
                legacy_pane.and_then(|p| p.browser_url.clone())
            } else {
                None
            };
            let metadata = SurfaceMetadata {
                custom_title: workspace
                    .panel_titles
                    .as_ref()
                    .and_then(|rows| rows.iter().find(|r| r.panel_id == id))
                    .and_then(|r| r.custom_title.clone()),
                pinned: workspace
                    .panel_pins
                    .as_ref()
                    .and_then(|rows| rows.iter().find(|r| r.panel_id == id))
                    .is_some_and(|r| r.is_pinned),
                unread: workspace
                    .panel_unreads
                    .as_ref()
                    .and_then(|rows| rows.iter().find(|r| r.panel_id == id))
                    .is_some_and(|r| r.is_unread),
                unread_at: workspace
                    .panel_unreads
                    .as_ref()
                    .and_then(|rows| rows.iter().find(|r| r.panel_id == id))
                    .and_then(|r| r.unread_at),
                ..SurfaceMetadata::default()
            };
            let mut kind = kind_from_legacy(kind_name, browser_url);
            if let Some(legacy) = legacy_pane {
                kind = match kind {
                    SurfaceKind::Browser { url, .. } => SurfaceKind::Browser {
                        url,
                        profile: None,
                        proxy_url: legacy.browser_proxy_url.clone(),
                        back_history: legacy.browser_back_history.clone(),
                        forward_history: legacy.browser_forward_history.clone(),
                        omnibar_visible: legacy.browser_omnibar_visible,
                        focus_mode_active: legacy.browser_focus_mode_active,
                        developer_tools_visible: legacy.browser_developer_tools_visible,
                        developer_tools_panel: legacy.browser_developer_tools_panel.clone(),
                        page_zoom: legacy.browser_page_zoom,
                    },
                    SurfaceKind::Markdown { .. } => SurfaceKind::Markdown {
                        path: legacy.markdown_file_path.clone(),
                    },
                    SurfaceKind::File { .. } => SurfaceKind::File {
                        path: legacy.file_path.clone(),
                    },
                    SurfaceKind::Diff { .. } => SurfaceKind::Diff {
                        token: legacy.diff_viewer_token.clone(),
                        request_path: legacy.diff_viewer_request_path.clone(),
                    },
                    other => other,
                };
            }
            if matches!(kind, SurfaceKind::AgentSession { .. }) {
                if let Some(agent) = workspace
                    .restorable_agent_snapshots
                    .as_ref()
                    .and_then(|rows| rows.iter().find(|row| row.panel_id == id))
                    .map(|row| row.snapshot.clone())
                {
                    kind = SurfaceKind::AgentSession {
                        provider: Some(agent.kind.clone()),
                        renderer: None,
                        working_directory: agent.working_directory.clone(),
                        session_id: Some(agent.session_id.clone()),
                        lifecycle: None,
                        restorable_agent: Some(Box::new(agent)),
                    };
                }
            }
            let reservation = model.reserve_surface(SurfaceSeed {
                surface_id: id.clone(),
                pane_id: pane_id.clone(),
                kind,
                metadata,
            })?;
            let record = model.surfaces.get_mut(&id).unwrap();
            record.generation = reservation.generation;
            if let Some(startup) = workspace
                .panel_terminal_startups
                .as_ref()
                .and_then(|rows| rows.iter().find(|r| r.panel_id == id))
            {
                record.terminal_startup = startup_from_legacy(startup);
            }
            if matches!(
                record.kind,
                SurfaceKind::Terminal | SurfaceKind::RemoteTerminal { .. }
            ) {
                record.terminal_startup.resume_binding = workspace
                    .restorable_agent_snapshots
                    .as_ref()
                    .and_then(|rows| rows.iter().find(|row| row.panel_id == id))
                    .map(|row| Box::new(row.snapshot.clone()));
            }
        }
        if !pane.selected_surface_id.is_empty()
            && model.surfaces.contains_key(&pane.selected_surface_id)
        {
            model.select_in_pane(&pane.selected_surface_id)?;
        }
    }
    Ok(())
}

fn kind_from_legacy(kind: &str, browser_url: Option<String>) -> SurfaceKind {
    match kind {
        "browser" => SurfaceKind::Browser {
            url: browser_url,
            profile: None,
            proxy_url: None,
            back_history: None,
            forward_history: None,
            omnibar_visible: None,
            focus_mode_active: None,
            developer_tools_visible: None,
            developer_tools_panel: None,
            page_zoom: None,
        },
        "agent" | "agent_session" | "agent-session" => SurfaceKind::AgentSession {
            provider: None,
            renderer: None,
            working_directory: None,
            session_id: None,
            lifecycle: None,
            restorable_agent: None,
        },
        "markdown" => SurfaceKind::Markdown { path: None },
        "file" => SurfaceKind::File { path: None },
        "diff" => SurfaceKind::Diff {
            token: None,
            request_path: None,
        },
        "project_sidebar" => SurfaceKind::ProjectSidebar,
        "right_sidebar_tool" => SurfaceKind::RightSidebarTool,
        "remote_terminal" => SurfaceKind::RemoteTerminal {
            remote_session_id: None,
            remote_context: None,
            arrival_generation: None,
        },
        _ => SurfaceKind::Terminal,
    }
}

fn find_pane<'a>(
    layout: Option<&'a SessionWorkspaceLayoutSnapshot>,
    pane_id: &str,
) -> Option<&'a SessionPaneLayoutSnapshot> {
    match layout? {
        SessionWorkspaceLayoutSnapshot::Pane(p)
            if p.pane_id.as_deref() == Some(pane_id)
                || (p.pane_id.is_none()
                    && p.panel_ids.first().map(String::as_str) == Some(pane_id)) =>
        {
            Some(p)
        }
        SessionWorkspaceLayoutSnapshot::Pane(_) => None,
        SessionWorkspaceLayoutSnapshot::Split(s) => {
            find_pane(Some(&s.first), pane_id).or_else(|| find_pane(Some(&s.second), pane_id))
        }
    }
}

fn project_layout(
    layout: SessionWorkspaceLayoutSnapshot,
    panes: &BTreeMap<String, PaneRecord>,
    collapsed_panes: &HashSet<String>,
) -> Option<SessionWorkspaceLayoutSnapshot> {
    match layout {
        SessionWorkspaceLayoutSnapshot::Pane(mut old) => {
            let id = old
                .pane_id
                .clone()
                .or_else(|| old.panel_ids.first().cloned())?;
            let pane = panes.get(&id)?;
            if collapsed_panes.contains(&id) {
                return None;
            }
            old.panel_ids = pane.surface_ids.clone();
            old.pane_id = Some(id);
            old.selected_panel_id =
                Some(pane.selected_surface_id.clone()).filter(|s| !s.is_empty());
            old.surface_kind = None;
            old.markdown_file_path = None;
            old.file_path = None;
            old.diff_viewer_token = None;
            old.diff_viewer_request_path = None;
            old.browser_url = None;
            old.browser_proxy_url = None;
            old.browser_back_history = None;
            old.browser_forward_history = None;
            old.browser_omnibar_visible = None;
            old.browser_focus_mode_active = None;
            old.browser_developer_tools_visible = None;
            old.browser_developer_tools_panel = None;
            old.browser_page_zoom = None;
            Some(SessionWorkspaceLayoutSnapshot::Pane(old))
        }
        SessionWorkspaceLayoutSnapshot::Split(mut split) => {
            let first = project_layout(*split.first, panes, collapsed_panes);
            let second = project_layout(*split.second, panes, collapsed_panes);
            match (first, second) {
                (Some(a), Some(b)) => {
                    split.first = Box::new(a);
                    split.second = Box::new(b);
                    Some(SessionWorkspaceLayoutSnapshot::Split(split))
                }
                (Some(one), None) | (None, Some(one)) => Some(one),
                (None, None) => None,
            }
        }
    }
}

fn startup_from_legacy(value: &SessionPanelTerminalStartupSnapshot) -> TerminalStartup {
    TerminalStartup {
        command: value.initial_terminal_command.clone(),
        working_directory: None,
        initial_input: value.initial_terminal_input.clone(),
        environment: value.initial_terminal_environment.clone(),
        tmux_start_command: None,
        remote_pty_session_id: None,
        resume_binding: None,
    }
}

fn record_from_session(value: &SessionSurfaceSnapshot) -> SurfaceRecord {
    SurfaceRecord {
        surface_id: value.surface_id.clone(),
        pane_id: value.pane_id.clone(),
        generation: value.generation,
        kind: value.kind.clone(),
        metadata: value.metadata.clone(),
        terminal_startup: value.terminal_startup.clone().unwrap_or_default(),
        runtime: None,
        is_workspace_focused: false,
    }
}
fn record_to_session(value: &SurfaceRecord) -> SessionSurfaceSnapshot {
    SessionSurfaceSnapshot {
        surface_id: value.surface_id.clone(),
        pane_id: value.pane_id.clone(),
        generation: value.generation,
        kind: value.kind.clone(),
        metadata: value.metadata.clone(),
        terminal_startup: (matches!(
            value.kind,
            SurfaceKind::Terminal | SurfaceKind::RemoteTerminal { .. }
        ) && value.terminal_startup != TerminalStartup::default())
        .then(|| value.terminal_startup.clone()),
    }
}
