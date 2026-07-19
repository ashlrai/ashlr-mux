use super::*;

/// A single window / single workspace / single pane starting layout.
pub(super) fn initial_snapshot(first_panel_id: &str) -> AppSessionSnapshot {
    let mut snapshot = AppSessionSnapshot {
        version: SESSION_SNAPSHOT_SCHEMA_VERSION,
        created_at: 0,
        windows: vec![SessionWindowSnapshot {
            // D1: canonical windows carry UUID ids (live capture window:1 =
            // <uuid>); the "main" webview LABEL maps to the first session
            // window via session_window_id_for_label, not by identity.
            window_id: Some(Uuid::new_v4().to_string()),
            selected_workspace_id: None,
            dock: None,
            tab_manager: SessionTabManagerSnapshot {
                selected_workspace_index: Some(0),
                workspaces: vec![session_ops::fresh_terminal_workspace(first_panel_id)],
                workspace_groups: None,
            },
        }],
    };
    // Canonical Workspace init: currentDirectory = requested ?? home
    // (Workspace.swift:2885-2891 at pinned e1825d40d).
    snapshot.windows[0].tab_manager.workspaces[0].current_directory = default_workspace_directory();
    ensure_workspace_ids(&mut snapshot);
    ensure_pane_ids(&mut snapshot);
    seed_initial_surface_record(&mut snapshot.windows[0].tab_manager.workspaces[0]);
    snapshot
}

/// Rebuild the persisted identity graph with canonical owner-local semantics.
/// Opaque strings are never inspected. The candidate replaces `snapshot` only
/// after validation and normalization complete, so a rejected graph cannot
/// leak a partial migration.
pub(super) fn remint_noncanonical_identities(snapshot: &mut AppSessionSnapshot) -> bool {
    #[derive(Default)]
    struct WorkspaceAliases {
        surfaces: HashMap<String, String>,
        panes: HashMap<String, String>,
        surface_owners: HashMap<String, String>,
        last_selected_surface: Option<String>,
    }

    fn mint(used: &mut HashSet<String>) -> String {
        loop {
            let id = Uuid::new_v4().to_string();
            if used.insert(id.clone()) {
                return id;
            }
        }
    }

    fn retain_mapped_rows<T>(
        rows: &mut Option<Vec<T>>,
        aliases: &HashMap<String, String>,
        identity: fn(&mut T) -> &mut String,
    ) {
        if let Some(rows) = rows {
            rows.retain_mut(|row| {
                let id = identity(row);
                let Some(replacement) = aliases.get(id) else {
                    return false;
                };
                *id = replacement.clone();
                true
            });
        }
    }

    fn prune_surface_rows(
        workspace: &mut SessionWorkspaceSnapshot,
        aliases: &HashMap<String, String>,
    ) {
        retain_mapped_rows(&mut workspace.pending_surface_pwds, aliases, |row| {
            &mut row.surface_id
        });
        retain_mapped_rows(&mut workspace.panel_titles, aliases, |row| {
            &mut row.panel_id
        });
        retain_mapped_rows(&mut workspace.panel_pins, aliases, |row| &mut row.panel_id);
        retain_mapped_rows(&mut workspace.panel_unreads, aliases, |row| {
            &mut row.panel_id
        });
        retain_mapped_rows(&mut workspace.restorable_agent_snapshots, aliases, |row| {
            &mut row.panel_id
        });
        retain_mapped_rows(&mut workspace.surface_resume_bindings, aliases, |row| {
            &mut row.surface_id
        });
        retain_mapped_rows(&mut workspace.panel_git_branches, aliases, |row| {
            &mut row.panel_id
        });
        retain_mapped_rows(&mut workspace.panel_pull_requests, aliases, |row| {
            &mut row.panel_id
        });
        retain_mapped_rows(&mut workspace.panel_listening_ports, aliases, |row| {
            &mut row.panel_id
        });
        retain_mapped_rows(&mut workspace.panel_ttys, aliases, |row| &mut row.panel_id);
        retain_mapped_rows(&mut workspace.panel_shell_activity, aliases, |row| {
            &mut row.panel_id
        });
        retain_mapped_rows(&mut workspace.panel_terminal_startups, aliases, |row| {
            &mut row.panel_id
        });
    }

    fn stable_surface_id(old: &str, used: &mut HashSet<String>) -> String {
        if Uuid::parse_str(old).is_ok() && used.insert(old.to_string()) {
            old.to_string()
        } else {
            mint(used)
        }
    }

    fn normalize_layout(
        layout: SessionWorkspaceLayoutSnapshot,
        authoritative: Option<&HashMap<String, cmux_core::session::SessionSurfaceSnapshot>>,
        used_surfaces: &mut HashSet<String>,
        used_panes: &mut HashSet<String>,
        used_splits: &mut HashSet<String>,
        aliases: &mut WorkspaceAliases,
        created_surfaces: &mut Vec<cmux_core::session::SessionSurfaceSnapshot>,
    ) -> Option<SessionWorkspaceLayoutSnapshot> {
        match layout {
            SessionWorkspaceLayoutSnapshot::Pane(mut pane) => {
                let old_pane_id = pane.pane_id.clone();
                let new_pane_id = mint(used_panes);
                if let Some(old_pane_id) = old_pane_id {
                    aliases
                        .panes
                        .entry(old_pane_id)
                        .or_insert_with(|| new_pane_id.clone());
                }

                let selected = pane.selected_panel_id.take();
                let mut local_aliases = HashMap::new();
                let mut panel_ids = Vec::new();
                for old_surface_id in std::mem::take(&mut pane.panel_ids) {
                    let source = match authoritative {
                        Some(authoritative) => {
                            let Some(source) = authoritative.get(&old_surface_id) else {
                                continue;
                            };
                            Some(source)
                        }
                        None => None,
                    };
                    let new_surface_id = stable_surface_id(&old_surface_id, used_surfaces);
                    local_aliases
                        .entry(old_surface_id.clone())
                        .or_insert_with(|| new_surface_id.clone());
                    aliases
                        .surfaces
                        .insert(old_surface_id, new_surface_id.clone());
                    aliases
                        .surface_owners
                        .insert(new_surface_id.clone(), new_pane_id.clone());
                    panel_ids.push(new_surface_id.clone());

                    if let Some(source) = source {
                        let mut surface = source.clone();
                        surface.surface_id = new_surface_id;
                        surface.pane_id = new_pane_id.clone();
                        created_surfaces.push(surface);
                    }
                }
                let restored_saved_surface = !panel_ids.is_empty();
                if panel_ids.is_empty() {
                    let scaffold_surface_id = mint(used_surfaces);
                    aliases
                        .surface_owners
                        .insert(scaffold_surface_id.clone(), new_pane_id.clone());
                    panel_ids.push(scaffold_surface_id.clone());
                    created_surfaces.push(cmux_core::session::SessionSurfaceSnapshot {
                        surface_id: scaffold_surface_id,
                        pane_id: new_pane_id.clone(),
                        generation: 1,
                        kind: cmux_core::session::SessionSurfaceKindSnapshot::Terminal,
                        metadata: Default::default(),
                        terminal_startup: None,
                        scrollback: None,
                    });
                }
                pane.pane_id = Some(new_pane_id);
                let restored_selection = selected
                    .as_ref()
                    .and_then(|selected| local_aliases.get(selected).cloned());
                let advances_focus =
                    restored_selection.is_some() || (selected.is_none() && restored_saved_surface);
                pane.selected_panel_id = restored_selection.or_else(|| panel_ids.first().cloned());
                if advances_focus {
                    aliases.last_selected_surface = pane.selected_panel_id.clone();
                }
                pane.panel_ids = panel_ids;
                Some(SessionWorkspaceLayoutSnapshot::Pane(pane))
            }
            SessionWorkspaceLayoutSnapshot::Split(mut split) => {
                let first = normalize_layout(
                    *split.first,
                    authoritative,
                    used_surfaces,
                    used_panes,
                    used_splits,
                    aliases,
                    created_surfaces,
                );
                let second = normalize_layout(
                    *split.second,
                    authoritative,
                    used_surfaces,
                    used_panes,
                    used_splits,
                    aliases,
                    created_surfaces,
                );
                match (first, second) {
                    (Some(first), Some(second)) => {
                        split.split_id = Some(mint(used_splits));
                        split.first = Box::new(first);
                        split.second = Box::new(second);
                        Some(SessionWorkspaceLayoutSnapshot::Split(split))
                    }
                    (Some(layout), None) | (None, Some(layout)) => Some(layout),
                    (None, None) => None,
                }
            }
        }
    }

    fn normalize_workspace(
        workspace: &mut SessionWorkspaceSnapshot,
        group_aliases: &HashMap<String, String>,
        used_workspaces: &mut HashSet<String>,
        used_surfaces: &mut HashSet<String>,
        used_panes: &mut HashSet<String>,
        used_splits: &mut HashSet<String>,
    ) -> (Option<String>, String) {
        let old_workspace_id = workspace.workspace_id.clone();
        let new_workspace_id = mint(used_workspaces);
        workspace.workspace_id = Some(new_workspace_id.clone());

        let original_surfaces = workspace.surfaces.take();
        let authoritative = original_surfaces.as_ref().map(|surfaces| {
            surfaces
                .iter()
                .map(|surface| (surface.surface_id.clone(), surface.clone()))
                .collect::<HashMap<_, _>>()
        });
        let mut aliases = WorkspaceAliases::default();
        let mut created_surfaces = Vec::new();
        workspace.layout = workspace.layout.take().and_then(|layout| {
            normalize_layout(
                layout,
                authoritative.as_ref(),
                used_surfaces,
                used_panes,
                used_splits,
                &mut aliases,
                &mut created_surfaces,
            )
        });
        workspace.surfaces = original_surfaces.map(|original_surfaces| {
            let mut created_by_id = created_surfaces
                .iter()
                .cloned()
                .map(|surface| (surface.surface_id.clone(), surface))
                .collect::<HashMap<_, _>>();
            let mut ordered = original_surfaces
                .iter()
                .filter_map(|surface| aliases.surfaces.get(&surface.surface_id))
                .filter_map(|surface_id| created_by_id.remove(surface_id))
                .collect::<Vec<_>>();
            ordered.extend(
                created_surfaces
                    .into_iter()
                    .filter(|surface| created_by_id.remove(&surface.surface_id).is_some()),
            );
            ordered
        });

        workspace.group_id = workspace
            .group_id
            .as_ref()
            .and_then(|group_id| group_aliases.get(group_id).cloned());
        workspace.zoomed_panel_id = workspace
            .zoomed_panel_id
            .as_ref()
            .and_then(|panel_id| aliases.surfaces.get(panel_id).cloned());

        let focused_panel = workspace
            .focused_panel_id
            .as_ref()
            .and_then(|panel_id| aliases.surfaces.get(panel_id).cloned())
            .or_else(|| aliases.last_selected_surface.clone());
        workspace.focused_pane_id = focused_panel
            .as_ref()
            .and_then(|panel_id| aliases.surface_owners.get(panel_id).cloned());
        workspace.focused_panel_id = focused_panel;

        prune_surface_rows(workspace, &aliases.surfaces);

        if let Some(rows) = &mut workspace.published_pane_selections {
            rows.retain_mut(|row| {
                let Some(pane_id) = aliases.panes.get(&row.pane_id).cloned() else {
                    return false;
                };
                let Some(panel_id) = aliases.surfaces.get(&row.panel_id).cloned() else {
                    return false;
                };
                if aliases.surface_owners.get(&panel_id) != Some(&pane_id) {
                    return false;
                }
                row.pane_id = pane_id;
                row.panel_id = panel_id;
                true
            });
        }

        if let Some(canvas_panes) = &mut workspace.canvas_panes {
            canvas_panes.retain_mut(|canvas| {
                let old_panel_ids = canvas
                    .panel_ids
                    .take()
                    .unwrap_or_else(|| vec![canvas.panel_id.clone()]);
                let panel_ids = old_panel_ids
                    .iter()
                    .filter_map(|panel_id| aliases.surfaces.get(panel_id).cloned())
                    .collect::<Vec<_>>();
                let Some(first) = panel_ids.first().cloned() else {
                    return false;
                };
                let selected_source = canvas
                    .selected_panel_id
                    .as_ref()
                    .unwrap_or(&canvas.panel_id);
                let selected = aliases
                    .surfaces
                    .get(selected_source)
                    .cloned()
                    .filter(|panel_id| panel_ids.contains(panel_id))
                    .unwrap_or_else(|| first.clone());
                canvas.panel_id = first;
                canvas.panel_ids = Some(panel_ids);
                canvas.selected_panel_id = Some(selected);
                true
            });
        }

        (old_workspace_id, new_workspace_id)
    }

    fn normalize_dock(
        dock: &mut cmux_core::session::SessionDockSnapshot,
        window_id: &str,
        used_surfaces: &mut HashSet<String>,
        used_panes: &mut HashSet<String>,
        used_splits: &mut HashSet<String>,
    ) {
        let authoritative = dock
            .surfaces
            .iter()
            .map(|surface| (surface.surface_id.clone(), surface.clone()))
            .collect::<HashMap<_, _>>();
        let focused = dock.focused_surface_id.take();
        let mut aliases = WorkspaceAliases::default();
        let mut created_surfaces = Vec::new();
        dock.layout = dock.layout.take().and_then(|layout| {
            normalize_layout(
                layout,
                Some(&authoritative),
                used_surfaces,
                used_panes,
                used_splits,
                &mut aliases,
                &mut created_surfaces,
            )
        });
        dock.surfaces = created_surfaces;
        dock.focused_surface_id = focused
            .as_ref()
            .and_then(|surface_id| aliases.surfaces.get(surface_id).cloned())
            .or_else(|| {
                dock.surfaces
                    .first()
                    .map(|surface| surface.surface_id.clone())
            });
        dock.workspace_id = format!("dock:{window_id}");
    }

    for window in &snapshot.windows {
        for workspace in &window.tab_manager.workspaces {
            if let Some(surfaces) = workspace.surfaces.as_deref() {
                let mut seen = HashSet::new();
                if surfaces
                    .iter()
                    .any(|surface| !seen.insert(surface.surface_id.as_str()))
                {
                    return false;
                }
            }
        }
    }

    let mut candidate = snapshot.clone();
    let mut used_windows = HashSet::new();
    let mut used_workspaces = HashSet::new();
    let mut used_groups = candidate
        .windows
        .iter()
        .flat_map(|window| {
            window
                .tab_manager
                .workspace_groups
                .as_deref()
                .unwrap_or_default()
        })
        .filter(|group| Uuid::parse_str(&group.id).is_ok())
        .map(|group| group.id.clone())
        .collect::<HashSet<_>>();
    let mut used_panes = HashSet::new();
    let mut used_splits = HashSet::new();
    let mut used_surfaces = HashSet::new();

    for window in &mut candidate.windows {
        let window_id = match window.window_id.as_deref() {
            Some(id) if Uuid::parse_str(id).is_ok() && used_windows.insert(id.to_string()) => {
                id.to_string()
            }
            _ => mint(&mut used_windows),
        };
        window.window_id = Some(window_id);
    }

    for window in &mut candidate.windows {
        let old_selected_workspace = window.selected_workspace_id.clone();
        let mut group_aliases = HashMap::new();
        let mut seen_groups = HashSet::new();
        let groups = window
            .tab_manager
            .workspace_groups
            .take()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|mut group| {
                if !seen_groups.insert(group.id.clone()) {
                    return None;
                }
                let old_id = group.id.clone();
                if Uuid::parse_str(&group.id).is_err() {
                    group.id = mint(&mut used_groups);
                }
                group_aliases.insert(old_id, group.id.clone());
                Some(group)
            })
            .collect::<Vec<_>>();
        window.tab_manager.workspace_groups = (!groups.is_empty()).then_some(groups);

        let mut workspace_aliases = HashMap::new();
        for workspace in &mut window.tab_manager.workspaces {
            let (old_id, new_id) = normalize_workspace(
                workspace,
                &group_aliases,
                &mut used_workspaces,
                &mut used_surfaces,
                &mut used_panes,
                &mut used_splits,
            );
            if let Some(old_id) = old_id {
                workspace_aliases.entry(old_id).or_insert(new_id);
            }
        }

        let selected_index = window
            .tab_manager
            .selected_workspace_index
            .and_then(|index| usize::try_from(index).ok());
        window.selected_workspace_id = selected_index
            .and_then(|index| window.tab_manager.workspaces.get(index))
            .and_then(|workspace| workspace.workspace_id.clone())
            .or_else(|| {
                old_selected_workspace
                    .as_ref()
                    .and_then(|workspace_id| workspace_aliases.get(workspace_id).cloned())
            })
            .or_else(|| {
                window
                    .tab_manager
                    .workspaces
                    .first()
                    .and_then(|workspace| workspace.workspace_id.clone())
            });

        if let Some(groups) = window.tab_manager.workspace_groups.take() {
            let groups = groups
                .into_iter()
                .filter_map(|mut group| {
                    let members = window
                        .tab_manager
                        .workspaces
                        .iter()
                        .filter(|workspace| workspace.group_id.as_deref() == Some(&group.id))
                        .filter_map(|workspace| workspace.workspace_id.clone())
                        .collect::<Vec<_>>();
                    if members.is_empty() {
                        return None;
                    }
                    group.anchor_workspace_id = group
                        .anchor_member_index
                        .and_then(|index| usize::try_from(index).ok())
                        .and_then(|index| members.get(index).cloned())
                        .or_else(|| {
                            group
                                .anchor_workspace_id
                                .as_ref()
                                .and_then(|workspace_id| workspace_aliases.get(workspace_id))
                                .filter(|workspace_id| members.contains(workspace_id))
                                .cloned()
                        })
                        .or_else(|| members.first().cloned());
                    Some(group)
                })
                .collect::<Vec<_>>();
            window.tab_manager.workspace_groups = (!groups.is_empty()).then_some(groups);
        }

        if let Some(dock) = &mut window.dock {
            normalize_dock(
                dock,
                window.window_id.as_deref().expect("normalized window id"),
                &mut used_surfaces,
                &mut used_panes,
                &mut used_splits,
            );
        }
    }

    *snapshot = candidate;
    true
}

pub(crate) fn default_workspace_directory() -> Option<String> {
    ["USERPROFILE", "HOME"]
        .iter()
        .find_map(|key| std::env::var(*key).ok())
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

/// Materialize the authoritative surface record for a fresh single-pane
/// workspace so its first terminal carries `requested_working_directory`
/// from birth, like canonical's first-panel spawn with `initialDirectory`
/// (REMEDIATION.md divergence 6).
pub(super) fn seed_initial_surface_record(workspace: &mut SessionWorkspaceSnapshot) {
    if workspace.surfaces.is_some() {
        return;
    }
    let Some(SessionWorkspaceLayoutSnapshot::Pane(pane)) = workspace.layout.as_ref() else {
        return;
    };
    let (Some(pane_id), Some(panel_id)) = (pane.pane_id.clone(), pane.panel_ids.first().cloned())
    else {
        return;
    };
    workspace.surfaces = Some(vec![cmux_core::session::SessionSurfaceSnapshot {
        surface_id: panel_id,
        pane_id,
        generation: 1,
        kind: cmux_core::session::SessionSurfaceKindSnapshot::Terminal,
        metadata: Default::default(),
        terminal_startup: workspace.current_directory.clone().map(|directory| {
            cmux_core::session::SessionSurfaceTerminalStartupSnapshot {
                working_directory: Some(directory),
                ..Default::default()
            }
        }),
        scrollback: None,
    }]);
}

/// Mint a `window_id` and `workspace_id` for every owner that lacks one. Canonical parity:
/// the Swift restore mints a fresh UUID exactly once per workspace missing an id
/// (`TabManager.swift:5960-5975`), and live `Workspace`s carry an identity from
/// init. This stateful session layer is the sole owner of id synthesis — the
/// pure `session_ops` builders stay deterministic and stateless projections
/// (`sidebar_render`, the web sidebar) never re-mint, they skip id-less rows.
pub(super) fn ensure_workspace_ids(snapshot: &mut AppSessionSnapshot) {
    for window in &mut snapshot.windows {
        if window.window_id.is_none() {
            window.window_id = Some(Uuid::new_v4().to_string());
        }
        for workspace in &mut window.tab_manager.workspaces {
            if workspace.workspace_id.is_none() {
                workspace.workspace_id = Some(Uuid::new_v4().to_string());
            }
        }
        sync_window_selected_workspace_id(window);
    }
}

pub(super) fn sync_window_selected_workspace_id(window: &mut SessionWindowSnapshot) {
    let selected = window
        .tab_manager
        .selected_workspace_index
        .and_then(|index| usize::try_from(index).ok())
        .and_then(|index| window.tab_manager.workspaces.get(index))
        .and_then(|workspace| workspace.workspace_id.clone());
    if selected.is_some() {
        window.selected_workspace_id = selected;
    }
}

/// Mint stable pane and split identities for every layout node that lacks one. This mirrors the
/// workspace-id rule above: older snapshots decode without pane ids, and the
/// stateful desktop layer synthesizes them exactly once so downstream pure/UI
/// consumers can treat pane identity as stable.
pub(super) fn ensure_pane_ids(snapshot: &mut AppSessionSnapshot) {
    fn collect_pane_ids(layout: &SessionWorkspaceLayoutSnapshot, used: &mut HashSet<String>) {
        match layout {
            SessionWorkspaceLayoutSnapshot::Pane(pane) => {
                if let Some(pane_id) = &pane.pane_id {
                    used.insert(pane_id.clone());
                }
            }
            SessionWorkspaceLayoutSnapshot::Split(split) => {
                collect_pane_ids(&split.first, used);
                collect_pane_ids(&split.second, used);
            }
        }
    }

    fn ensure_layout_pane_ids(
        layout: &mut SessionWorkspaceLayoutSnapshot,
        surfaces: &mut [cmux_core::session::SessionSurfaceSnapshot],
        used: &mut HashSet<String>,
    ) {
        match layout {
            SessionWorkspaceLayoutSnapshot::Pane(pane) => {
                if pane.pane_id.is_none() {
                    let mut referenced_owner: Option<String> = None;
                    let mut conflicting_owners = false;
                    for panel_id in &pane.panel_ids {
                        let Some(owner) = surfaces
                            .iter()
                            .find(|surface| surface.surface_id == *panel_id)
                            .map(|surface| surface.pane_id.clone())
                        else {
                            continue;
                        };
                        match referenced_owner.as_deref() {
                            None => referenced_owner = Some(owner),
                            Some(existing) if existing == owner => {}
                            Some(_) => conflicting_owners = true,
                        }
                    }
                    let reusable = referenced_owner
                        .filter(|owner| !conflicting_owners && !used.contains(owner));
                    let pane_id = reusable.unwrap_or_else(|| loop {
                        let candidate = Uuid::new_v4().to_string();
                        if !used.contains(&candidate) {
                            break candidate;
                        }
                    });
                    pane.pane_id = Some(pane_id);
                }
                let pane_id = pane.pane_id.as_ref().expect("pane id assigned").clone();
                used.insert(pane_id.clone());
                for panel_id in &pane.panel_ids {
                    if let Some(surface) = surfaces
                        .iter_mut()
                        .find(|surface| surface.surface_id == *panel_id)
                    {
                        surface.pane_id.clone_from(&pane_id);
                    }
                }
            }
            SessionWorkspaceLayoutSnapshot::Split(split) => {
                if split.split_id.is_none() {
                    split.split_id = Some(Uuid::new_v4().to_string());
                }
                ensure_layout_pane_ids(&mut split.first, surfaces, used);
                ensure_layout_pane_ids(&mut split.second, surfaces, used);
            }
        }
    }

    let mut used = HashSet::new();
    for window in &snapshot.windows {
        for workspace in &window.tab_manager.workspaces {
            if let Some(layout) = &workspace.layout {
                collect_pane_ids(layout, &mut used);
            }
        }
        if let Some(layout) = window.dock.as_ref().and_then(|dock| dock.layout.as_ref()) {
            collect_pane_ids(layout, &mut used);
        }
    }
    for window in &mut snapshot.windows {
        for workspace in &mut window.tab_manager.workspaces {
            if let Some(layout) = workspace.layout.as_mut() {
                ensure_layout_pane_ids(
                    layout,
                    workspace.surfaces.as_deref_mut().unwrap_or_default(),
                    &mut used,
                );
            }
        }
        if let Some(dock) = window.dock.as_mut() {
            if let Some(layout) = dock.layout.as_mut() {
                ensure_layout_pane_ids(layout, &mut dock.surfaces, &mut used);
            }
        }
    }
}

pub(super) fn session_snapshot_paths(app: &AppHandle) -> Option<(PathBuf, PathBuf)> {
    let root = app.path().app_data_dir().ok()?.join("cmux");
    Some((
        root.join(CURRENT_SESSION_SNAPSHOT_FILENAME),
        root.join(PREVIOUS_SESSION_SNAPSHOT_FILENAME),
    ))
}

pub(super) fn load_snapshot_file(path: &Path) -> Option<AppSessionSnapshot> {
    std::fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<AppSessionSnapshot>(&bytes).ok())
}

pub(super) trait SnapshotFileOperations {
    fn create_parent(&mut self, parent: &Path) -> Result<(), String>;
    fn write_staged(&mut self, path: &Path, bytes: &[u8]) -> Result<(), String>;
    fn atomic_replace(&mut self, staged: &Path, destination: &Path) -> Result<(), String>;
}

pub(super) fn write_snapshot_file_strict(
    path: &Path,
    snapshot: &AppSessionSnapshot,
    operations: &mut impl SnapshotFileOperations,
) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(snapshot).map_err(|error| error.to_string())?;
    let parent = path
        .parent()
        .ok_or_else(|| "session snapshot path has no parent".to_string())?;
    let staged = path.with_extension(format!("json.{}.staged", Uuid::new_v4()));
    operations.create_parent(parent)?;
    operations.write_staged(&staged, &bytes)?;
    operations.atomic_replace(&staged, path)
}

pub(super) struct ProductionSnapshotFileOperations;

impl SnapshotFileOperations for ProductionSnapshotFileOperations {
    fn create_parent(&mut self, parent: &Path) -> Result<(), String> {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())
    }

    fn write_staged(&mut self, path: &Path, bytes: &[u8]) -> Result<(), String> {
        if let Err(error) = std::fs::write(path, bytes) {
            let _ = std::fs::remove_file(path);
            return Err(error.to_string());
        }
        Ok(())
    }

    fn atomic_replace(&mut self, staged: &Path, destination: &Path) -> Result<(), String> {
        if let Err(error) = replace_file_atomically(staged, destination) {
            let _ = std::fs::remove_file(staged);
            return Err(error);
        }
        Ok(())
    }
}

pub(super) fn write_snapshot_file(
    path: &Path,
    snapshot: &AppSessionSnapshot,
) -> Result<(), String> {
    write_snapshot_file_strict(path, snapshot, &mut ProductionSnapshotFileOperations)
}

pub(super) fn persist_current_snapshot(
    app: &AppHandle,
    snapshot: &AppSessionSnapshot,
) -> Result<(), String> {
    if let Some((current, _previous)) = session_snapshot_paths(app) {
        write_snapshot_file(&current, snapshot)?;
    }
    Ok(())
}

pub(super) fn next_panel_counter(snapshot: &AppSessionSnapshot) -> u64 {
    fn visit_layout(layout: &SessionWorkspaceLayoutSnapshot, max_seen: &mut u64) {
        match layout {
            SessionWorkspaceLayoutSnapshot::Pane(pane) => {
                for panel_id in &pane.panel_ids {
                    let Some(raw) = panel_id.strip_prefix("surface-") else {
                        continue;
                    };
                    let Ok(value) = raw.parse::<u64>() else {
                        continue;
                    };
                    *max_seen = (*max_seen).max(value);
                }
            }
            SessionWorkspaceLayoutSnapshot::Split(split) => {
                visit_layout(&split.first, max_seen);
                visit_layout(&split.second, max_seen);
            }
        }
    }

    let mut max_seen = 0u64;
    for window in &snapshot.windows {
        for workspace in &window.tab_manager.workspaces {
            if let Some(layout) = workspace.layout.as_ref() {
                visit_layout(layout, &mut max_seen);
            }
        }
    }
    max_seen.saturating_add(1).max(1)
}
