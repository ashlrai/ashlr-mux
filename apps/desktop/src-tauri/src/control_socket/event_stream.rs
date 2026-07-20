use super::*;

#[path = "event_stream/manual_restore.rs"]
mod manual_restore;
pub(crate) use manual_restore::record_manual_restore_window_created;

pub(crate) fn record_session_changed_event(app: &AppHandle, snapshot: &AppSessionSnapshot) {
    record_session_changed_event_suppressing(app, snapshot, &HashSet::new());
}

pub(crate) fn replace_session_event_baseline(app: &AppHandle, snapshot: &AppSessionSnapshot) {
    let Some(state) = app.try_state::<ControlEventState>() else {
        return;
    };
    state
        .inner
        .lock()
        .expect("control event log mutex poisoned")
        .last_session_summaries = session_event_summaries(snapshot);
}

pub(super) fn record_session_changed_event_suppressing(
    app: &AppHandle,
    snapshot: &AppSessionSnapshot,
    suppressed_names: &HashSet<&str>,
) {
    let Some(state) = app.try_state::<ControlEventState>() else {
        return;
    };
    let current = session_event_summaries(snapshot);
    let previous = {
        let mut guard = state
            .inner
            .lock()
            .expect("control event log mutex poisoned");
        let previous = guard.last_session_summaries.clone();
        guard.last_session_summaries = current.clone();
        previous
    };
    for (key, summary) in &current {
        for event in derived_session_event_specs(previous.get(key), summary) {
            if suppressed_names.contains(event.name) {
                continue;
            }
            let pane_id = event
                .payload
                .get("pane_id")
                .and_then(Value::as_str)
                .map(str::to_owned);
            record_event(
                app,
                event.name,
                event.category,
                event.source,
                event.window_id,
                event.workspace_id,
                pane_id,
                event.surface_id,
                event.payload,
            );
        }
    }
    for (key, previous_summary) in &previous {
        if current.contains_key(key) {
            continue;
        }
        let empty = SessionEventSummary {
            window_id: previous_summary.window_id.clone(),
            selected_workspace_id: None,
            selected_workspace_index: None,
            workspaces: Vec::new(),
        };
        for event in derived_session_event_specs(Some(previous_summary), &empty) {
            if suppressed_names.contains(event.name) {
                continue;
            }
            let pane_id = event
                .payload
                .get("pane_id")
                .and_then(Value::as_str)
                .map(str::to_owned);
            record_event(
                app,
                event.name,
                event.category,
                event.source,
                event.window_id,
                event.workspace_id,
                pane_id,
                event.surface_id,
                event.payload,
            );
        }
    }
}

pub(super) fn session_event_summaries(
    snapshot: &AppSessionSnapshot,
) -> BTreeMap<String, SessionEventSummary> {
    snapshot
        .windows
        .iter()
        .enumerate()
        .map(|(index, window)| {
            let key = window
                .window_id
                .clone()
                .unwrap_or_else(|| format!("window-index:{index}"));
            (key, session_event_summary_for_window(window, index))
        })
        .collect()
}

pub(super) fn session_event_summary_for_window(
    window: &cmux_core::session::SessionWindowSnapshot,
    window_index: usize,
) -> SessionEventSummary {
    if window.tab_manager.workspaces.is_empty() {
        return SessionEventSummary {
            window_id: window.window_id.clone(),
            selected_workspace_id: None,
            selected_workspace_index: None,
            workspaces: Vec::new(),
        };
    }
    let selected_index = window
        .tab_manager
        .selected_workspace_index
        .and_then(|index| usize::try_from(index).ok())
        .filter(|index| *index < window.tab_manager.workspaces.len());
    let workspaces: Vec<WorkspaceEventSummary> = window
        .tab_manager
        .workspaces
        .iter()
        .enumerate()
        .map(|(index, workspace)| {
            let panes = pane_event_summaries(workspace);
            let surfaces = surfaces_for_workspace(workspace);
            let surface_ids: Vec<String> = surfaces
                .iter()
                .filter_map(|surface| surface.get("id").and_then(Value::as_str))
                .map(str::to_string)
                .collect();
            let selected_surface_id = surfaces
                .iter()
                .find(|surface| {
                    surface
                        .get("focused")
                        .and_then(Value::as_bool)
                        .unwrap_or(false)
                })
                .or_else(|| surfaces.first())
                .and_then(|surface| surface.get("id"))
                .and_then(Value::as_str)
                .map(str::to_string);
            WorkspaceEventSummary {
                key: workspace
                    .workspace_id
                    .clone()
                    .unwrap_or_else(|| format!("{}:{index}", window_ref(window_index))),
                id: workspace.workspace_id.clone(),
                title: workspace_display_name(workspace),
                index,
                panes,
                surface_ids,
                selected_surface_id,
                sidebar: sidebar_event_summary(workspace),
            }
        })
        .collect();
    let selected_workspace_id = selected_index
        .and_then(|index| workspaces.get(index))
        .and_then(|workspace| workspace.id.clone());
    SessionEventSummary {
        window_id: window.window_id.clone(),
        selected_workspace_id,
        selected_workspace_index: selected_index,
        workspaces,
    }
}

pub(super) fn derived_session_event_specs(
    previous: Option<&SessionEventSummary>,
    current: &SessionEventSummary,
) -> Vec<DerivedEventSpec> {
    if previous == Some(current) {
        return Vec::new();
    }
    let mut events = vec![session_changed_event_spec(current)];
    match previous {
        Some(previous) => {
            append_workspace_diff_events(&mut events, previous, current);
            append_pane_diff_events(&mut events, previous, current);
            append_surface_diff_events(&mut events, previous, current);
            append_sidebar_diff_events(&mut events, previous, current);
        }
        None => {
            for workspace in &current.workspaces {
                events.push(workspace_event_spec(
                    "workspace.created",
                    current,
                    workspace,
                    None,
                ));
                for pane in &workspace.panes {
                    events.push(pane_event_spec(
                        "pane.created",
                        current,
                        workspace,
                        pane,
                        None,
                    ));
                    if pane.selected_surface_id.is_some() {
                        events.push(pane_event_spec(
                            "pane.focused",
                            current,
                            workspace,
                            pane,
                            None,
                        ));
                    }
                }
                for surface_id in &workspace.surface_ids {
                    events.push(surface_event_spec(
                        "surface.created",
                        current,
                        workspace,
                        surface_id,
                        None,
                    ));
                }
            }
            if let Some(workspace) = selected_workspace(current) {
                events.push(workspace_event_spec(
                    "workspace.selected",
                    current,
                    workspace,
                    None,
                ));
                if let Some(surface_id) = workspace.selected_surface_id.as_deref() {
                    events.push(surface_event_spec(
                        "surface.selected",
                        current,
                        workspace,
                        surface_id,
                        None,
                    ));
                }
            }
        }
    }
    events
}

pub(super) fn sidebar_event_summary(
    workspace: &SessionWorkspaceSnapshot,
) -> WorkspaceSidebarEventSummary {
    WorkspaceSidebarEventSummary {
        progress: workspace
            .sidebar_progress
            .as_ref()
            .map(json_value_for_event),
        status_entries: workspace
            .sidebar_status_entries
            .as_deref()
            .unwrap_or_default()
            .iter()
            .map(|entry| (entry.key.clone(), json_value_for_event(entry)))
            .collect(),
        metadata_entries: workspace
            .sidebar_metadata_entries
            .as_deref()
            .unwrap_or_default()
            .iter()
            .map(|entry| (entry.key.clone(), json_value_for_event(entry)))
            .collect(),
        metadata_blocks: workspace
            .sidebar_metadata_blocks
            .as_deref()
            .unwrap_or_default()
            .iter()
            .map(|entry| (entry.key.clone(), json_value_for_event(entry)))
            .collect(),
        log_entries: workspace
            .sidebar_log_entries
            .as_deref()
            .unwrap_or_default()
            .iter()
            .map(json_value_for_event)
            .collect(),
    }
}

pub(super) fn json_value_for_event<T: serde::Serialize>(value: &T) -> Value {
    serde_json::to_value(value).unwrap_or(Value::Null)
}

pub(super) fn pane_event_summaries(workspace: &SessionWorkspaceSnapshot) -> Vec<PaneEventSummary> {
    let mut panes = Vec::new();
    if let Some(layout) = workspace.layout.as_ref() {
        collect_pane_event_summaries(layout, &mut panes);
    }
    panes
}

pub(super) fn collect_pane_event_summaries(
    layout: &SessionWorkspaceLayoutSnapshot,
    panes: &mut Vec<PaneEventSummary>,
) {
    match layout {
        SessionWorkspaceLayoutSnapshot::Pane(pane) => {
            let index = panes.len();
            let selected_surface_id = pane
                .selected_panel_id
                .clone()
                .or_else(|| pane.panel_ids.first().cloned());
            panes.push(PaneEventSummary {
                key: pane.pane_id.clone().unwrap_or_else(|| pane_ref(index)),
                id: pane.pane_id.clone(),
                index,
                surface_ids: pane.panel_ids.clone(),
                selected_surface_id,
            });
        }
        SessionWorkspaceLayoutSnapshot::Split(split) => {
            collect_pane_event_summaries(&split.first, panes);
            collect_pane_event_summaries(&split.second, panes);
        }
    }
}

pub(super) fn append_workspace_diff_events(
    events: &mut Vec<DerivedEventSpec>,
    previous: &SessionEventSummary,
    current: &SessionEventSummary,
) {
    let previous_by_key: HashMap<&str, &WorkspaceEventSummary> = previous
        .workspaces
        .iter()
        .map(|workspace| (workspace.key.as_str(), workspace))
        .collect();
    let current_by_key: HashMap<&str, &WorkspaceEventSummary> = current
        .workspaces
        .iter()
        .map(|workspace| (workspace.key.as_str(), workspace))
        .collect();

    if previous.selected_workspace_id != current.selected_workspace_id
        || previous.selected_workspace_index != current.selected_workspace_index
    {
        if let Some(workspace) = selected_workspace(current) {
            events.push(workspace_event_spec(
                "workspace.selected",
                current,
                workspace,
                previous.selected_workspace_id.as_deref(),
            ));
        }
    }

    for workspace in &current.workspaces {
        match previous_by_key.get(workspace.key.as_str()) {
            Some(previous_workspace) => {
                if previous_workspace.title != workspace.title {
                    events.push(workspace_renamed_event_spec(
                        current,
                        workspace,
                        &previous_workspace.title,
                    ));
                }
            }
            None => events.push(workspace_event_spec(
                "workspace.created",
                current,
                workspace,
                None,
            )),
        }
    }

    for workspace in &previous.workspaces {
        if !current_by_key.contains_key(workspace.key.as_str()) {
            events.push(workspace_event_spec(
                "workspace.closed",
                current,
                workspace,
                None,
            ));
        }
    }

    let previous_keys: Vec<&str> = previous
        .workspaces
        .iter()
        .map(|workspace| workspace.key.as_str())
        .collect();
    let current_keys: Vec<&str> = current
        .workspaces
        .iter()
        .map(|workspace| workspace.key.as_str())
        .collect();
    let previous_set: HashSet<&str> = previous_keys.iter().copied().collect();
    let current_set: HashSet<&str> = current_keys.iter().copied().collect();
    if previous_keys != current_keys && previous_set == current_set {
        let moved_workspace_ids: Vec<String> = current
            .workspaces
            .iter()
            .filter(|workspace| {
                previous_by_key
                    .get(workspace.key.as_str())
                    .is_some_and(|previous_workspace| previous_workspace.index != workspace.index)
            })
            .map(workspace_event_identifier)
            .collect();
        events.push(DerivedEventSpec {
            name: "workspace.reordered",
            category: "workspace",
            source: "session.model",
            window_id: current.window_id.clone(),
            workspace_id: current.selected_workspace_id.clone(),
            surface_id: None,
            payload: json!({
                "window_id": current.window_id,
                "workspace_id": current.selected_workspace_id,
                "workspace_count": current.workspaces.len(),
                "selected_workspace_index": current.selected_workspace_index,
                "workspace_ids": current.workspaces.iter().map(workspace_event_identifier).collect::<Vec<_>>(),
                "moved_workspace_ids": moved_workspace_ids,
                "count": current.workspaces.len(),
                "origin": "session.changed",
            }),
        });
    }
}

pub(super) fn append_surface_diff_events(
    events: &mut Vec<DerivedEventSpec>,
    previous: &SessionEventSummary,
    current: &SessionEventSummary,
) {
    let previous_by_key: HashMap<&str, &WorkspaceEventSummary> = previous
        .workspaces
        .iter()
        .map(|workspace| (workspace.key.as_str(), workspace))
        .collect();
    let current_by_key: HashMap<&str, &WorkspaceEventSummary> = current
        .workspaces
        .iter()
        .map(|workspace| (workspace.key.as_str(), workspace))
        .collect();
    let previous_owners = previous
        .workspaces
        .iter()
        .flat_map(|workspace| {
            workspace
                .surface_ids
                .iter()
                .map(move |id| (id.as_str(), workspace))
        })
        .collect::<HashMap<_, _>>();
    let current_owners = current
        .workspaces
        .iter()
        .flat_map(|workspace| {
            workspace
                .surface_ids
                .iter()
                .map(move |id| (id.as_str(), workspace))
        })
        .collect::<HashMap<_, _>>();
    let moved = current_owners
        .iter()
        .filter_map(|(surface_id, destination)| {
            previous_owners
                .get(surface_id)
                .filter(|source| source.key != destination.key)
                .map(|source| ((*surface_id).to_string(), *source, *destination))
        })
        .collect::<Vec<_>>();
    let moved_ids = moved
        .iter()
        .map(|(surface_id, _, _)| surface_id.as_str())
        .collect::<HashSet<_>>();
    for workspace in &current.workspaces {
        let Some(previous_workspace) = previous_by_key.get(workspace.key.as_str()) else {
            for surface_id in &workspace.surface_ids {
                events.push(surface_event_spec(
                    "surface.created",
                    current,
                    workspace,
                    surface_id,
                    None,
                ));
            }
            continue;
        };
        let previous_surfaces: HashSet<&str> = previous_workspace
            .surface_ids
            .iter()
            .map(String::as_str)
            .collect();
        let current_surfaces: HashSet<&str> =
            workspace.surface_ids.iter().map(String::as_str).collect();
        for surface_id in &workspace.surface_ids {
            if !moved_ids.contains(surface_id.as_str())
                && !previous_surfaces.contains(surface_id.as_str())
            {
                events.push(surface_event_spec(
                    "surface.created",
                    current,
                    workspace,
                    surface_id,
                    None,
                ));
            }
        }
        for surface_id in &previous_workspace.surface_ids {
            if !moved_ids.contains(surface_id.as_str())
                && !current_surfaces.contains(surface_id.as_str())
            {
                events.push(surface_event_spec(
                    "surface.closed",
                    current,
                    workspace,
                    surface_id,
                    None,
                ));
            }
        }
        if previous_workspace.selected_surface_id != workspace.selected_surface_id {
            if let Some(surface_id) = workspace.selected_surface_id.as_deref() {
                events.push(surface_event_spec(
                    "surface.selected",
                    current,
                    workspace,
                    surface_id,
                    previous_workspace.selected_surface_id.as_deref(),
                ));
            }
        }
    }
    for workspace in &previous.workspaces {
        if current_by_key.contains_key(workspace.key.as_str()) {
            continue;
        }
        for surface_id in &workspace.surface_ids {
            if moved_ids.contains(surface_id.as_str()) {
                continue;
            }
            events.push(surface_event_spec(
                "surface.closed",
                current,
                workspace,
                surface_id,
                None,
            ));
        }
    }
}

pub(super) fn append_pane_diff_events(
    events: &mut Vec<DerivedEventSpec>,
    previous: &SessionEventSummary,
    current: &SessionEventSummary,
) {
    let previous_by_key: HashMap<&str, &WorkspaceEventSummary> = previous
        .workspaces
        .iter()
        .map(|workspace| (workspace.key.as_str(), workspace))
        .collect();
    for workspace in &current.workspaces {
        let Some(previous_workspace) = previous_by_key.get(workspace.key.as_str()) else {
            for pane in &workspace.panes {
                events.push(pane_event_spec(
                    "pane.created",
                    current,
                    workspace,
                    pane,
                    None,
                ));
                if pane.selected_surface_id.is_some() {
                    events.push(pane_event_spec(
                        "pane.focused",
                        current,
                        workspace,
                        pane,
                        None,
                    ));
                }
            }
            continue;
        };
        let previous_panes: HashMap<&str, &PaneEventSummary> = previous_workspace
            .panes
            .iter()
            .map(|pane| (pane.key.as_str(), pane))
            .collect();
        let current_panes: HashMap<&str, &PaneEventSummary> = workspace
            .panes
            .iter()
            .map(|pane| (pane.key.as_str(), pane))
            .collect();
        for pane in &workspace.panes {
            match previous_panes.get(pane.key.as_str()) {
                Some(previous_pane) => {
                    if previous_pane.selected_surface_id != pane.selected_surface_id {
                        events.push(pane_event_spec(
                            "pane.focused",
                            current,
                            workspace,
                            pane,
                            previous_pane.selected_surface_id.as_deref(),
                        ));
                    }
                }
                None => {
                    events.push(pane_event_spec(
                        "pane.created",
                        current,
                        workspace,
                        pane,
                        None,
                    ));
                    if pane.selected_surface_id.is_some() {
                        events.push(pane_event_spec(
                            "pane.focused",
                            current,
                            workspace,
                            pane,
                            None,
                        ));
                    }
                }
            }
        }
        for pane in &previous_workspace.panes {
            if !current_panes.contains_key(pane.key.as_str()) {
                events.push(pane_event_spec(
                    "pane.closed",
                    current,
                    workspace,
                    pane,
                    None,
                ));
            }
        }
    }
    let current_by_key: HashMap<&str, &WorkspaceEventSummary> = current
        .workspaces
        .iter()
        .map(|workspace| (workspace.key.as_str(), workspace))
        .collect();
    for workspace in &previous.workspaces {
        if current_by_key.contains_key(workspace.key.as_str()) {
            continue;
        }
        for pane in &workspace.panes {
            events.push(pane_event_spec(
                "pane.closed",
                current,
                workspace,
                pane,
                None,
            ));
        }
    }
}

pub(super) fn append_sidebar_diff_events(
    events: &mut Vec<DerivedEventSpec>,
    previous: &SessionEventSummary,
    current: &SessionEventSummary,
) {
    let previous_by_key: HashMap<&str, &WorkspaceEventSummary> = previous
        .workspaces
        .iter()
        .map(|workspace| (workspace.key.as_str(), workspace))
        .collect();
    for workspace in &current.workspaces {
        let Some(previous_workspace) = previous_by_key.get(workspace.key.as_str()) else {
            continue;
        };
        append_sidebar_progress_diff(events, current, previous_workspace, workspace);
        append_sidebar_metadata_collection_diff(
            events,
            current,
            previous_workspace,
            workspace,
            "status",
            &previous_workspace.sidebar.status_entries,
            &workspace.sidebar.status_entries,
        );
        append_sidebar_metadata_collection_diff(
            events,
            current,
            previous_workspace,
            workspace,
            "metadata",
            &previous_workspace.sidebar.metadata_entries,
            &workspace.sidebar.metadata_entries,
        );
        append_sidebar_metadata_collection_diff(
            events,
            current,
            previous_workspace,
            workspace,
            "metadata_block",
            &previous_workspace.sidebar.metadata_blocks,
            &workspace.sidebar.metadata_blocks,
        );
        append_sidebar_log_diff(events, current, previous_workspace, workspace);
    }
}

pub(super) fn append_sidebar_progress_diff(
    events: &mut Vec<DerivedEventSpec>,
    current: &SessionEventSummary,
    previous_workspace: &WorkspaceEventSummary,
    workspace: &WorkspaceEventSummary,
) {
    match (
        previous_workspace.sidebar.progress.as_ref(),
        workspace.sidebar.progress.as_ref(),
    ) {
        (previous, Some(value)) if previous != Some(value) => {
            events.push(sidebar_event_spec(
                "sidebar.progress.updated",
                current,
                workspace,
                json!({
                    "kind": "progress",
                    "value": value,
                    "previous_value": previous.cloned(),
                }),
            ));
        }
        (Some(previous), None) => {
            events.push(sidebar_event_spec(
                "sidebar.progress.cleared",
                current,
                workspace,
                json!({
                    "kind": "progress",
                    "previous_value": previous,
                }),
            ));
        }
        _ => {}
    }
}

pub(super) fn append_sidebar_metadata_collection_diff(
    events: &mut Vec<DerivedEventSpec>,
    current: &SessionEventSummary,
    _previous_workspace: &WorkspaceEventSummary,
    workspace: &WorkspaceEventSummary,
    kind: &'static str,
    previous_entries: &BTreeMap<String, Value>,
    current_entries: &BTreeMap<String, Value>,
) {
    for (key, value) in current_entries {
        let previous_value = previous_entries.get(key);
        if previous_value != Some(value) {
            events.push(sidebar_event_spec(
                "sidebar.metadata.updated",
                current,
                workspace,
                json!({
                    "kind": kind,
                    "key": key,
                    "value": value,
                    "previous_value": previous_value.cloned(),
                    "count": current_entries.len(),
                }),
            ));
        }
    }
    for (key, previous_value) in previous_entries {
        if !current_entries.contains_key(key) {
            events.push(sidebar_event_spec(
                "sidebar.metadata.cleared",
                current,
                workspace,
                json!({
                    "kind": kind,
                    "key": key,
                    "previous_value": previous_value,
                    "count": current_entries.len(),
                }),
            ));
        }
    }
}

pub(super) fn append_sidebar_log_diff(
    events: &mut Vec<DerivedEventSpec>,
    current: &SessionEventSummary,
    previous_workspace: &WorkspaceEventSummary,
    workspace: &WorkspaceEventSummary,
) {
    let previous_entries = &previous_workspace.sidebar.log_entries;
    let current_entries = &workspace.sidebar.log_entries;
    if previous_entries == current_entries {
        return;
    }
    if current_entries.is_empty() {
        if !previous_entries.is_empty() {
            events.push(sidebar_event_spec(
                "sidebar.log.cleared",
                current,
                workspace,
                json!({
                    "kind": "log",
                    "previous_count": previous_entries.len(),
                    "count": 0,
                }),
            ));
        }
        return;
    }
    let appended = if current_entries.len() >= previous_entries.len()
        && current_entries.starts_with(previous_entries)
    {
        current_entries[previous_entries.len()..].to_vec()
    } else {
        current_entries.clone()
    };
    events.push(sidebar_event_spec(
        "sidebar.log.appended",
        current,
        workspace,
        json!({
            "kind": "log",
            "entries": appended,
            "appended_count": current_entries.len().saturating_sub(previous_entries.len()),
            "previous_count": previous_entries.len(),
            "count": current_entries.len(),
            "replaced": !current_entries.starts_with(previous_entries),
        }),
    ));
}

pub(super) fn session_changed_event_spec(current: &SessionEventSummary) -> DerivedEventSpec {
    DerivedEventSpec {
        name: "session.changed",
        category: "session",
        source: "session.model",
        window_id: current.window_id.clone(),
        workspace_id: current.selected_workspace_id.clone(),
        surface_id: selected_workspace(current)
            .and_then(|workspace| workspace.selected_surface_id.clone()),
        payload: json!({
            "window_id": current.window_id,
            "workspace_id": current.selected_workspace_id,
            "workspace_count": current.workspaces.len(),
            "selected_workspace_index": current.selected_workspace_index,
            "surface_id": selected_workspace(current).and_then(|workspace| workspace.selected_surface_id.clone()),
            "origin": "session.changed",
        }),
    }
}

pub(super) fn workspace_event_spec(
    name: &'static str,
    current: &SessionEventSummary,
    workspace: &WorkspaceEventSummary,
    previous_workspace_id: Option<&str>,
) -> DerivedEventSpec {
    let mut payload = workspace_event_payload(current, workspace);
    if let Some(previous_workspace_id) = previous_workspace_id {
        payload["previous_workspace_id"] = json!(previous_workspace_id);
    }
    DerivedEventSpec {
        name,
        category: "workspace",
        source: "session.model",
        window_id: current.window_id.clone(),
        workspace_id: workspace.id.clone(),
        surface_id: None,
        payload,
    }
}

pub(super) fn workspace_renamed_event_spec(
    current: &SessionEventSummary,
    workspace: &WorkspaceEventSummary,
    previous_title: &str,
) -> DerivedEventSpec {
    let mut payload = workspace_event_payload(current, workspace);
    payload["previous_title"] = json!(previous_title);
    DerivedEventSpec {
        name: "workspace.renamed",
        category: "workspace",
        source: "session.model",
        window_id: current.window_id.clone(),
        workspace_id: workspace.id.clone(),
        surface_id: None,
        payload,
    }
}

pub(super) fn workspace_event_payload(
    current: &SessionEventSummary,
    workspace: &WorkspaceEventSummary,
) -> Value {
    json!({
        "window_id": current.window_id,
        "workspace_id": workspace.id,
        "workspace_ref": workspace_ref(workspace.index),
        "workspace_key": workspace.key,
        "workspace_count": current.workspaces.len(),
        "selected_workspace_index": current.selected_workspace_index,
        "index": workspace.index,
        "title": workspace.title,
        "tab_count": workspace.surface_ids.len(),
        "origin": "session.changed",
    })
}

pub(super) fn sidebar_event_spec(
    name: &'static str,
    current: &SessionEventSummary,
    workspace: &WorkspaceEventSummary,
    detail: Value,
) -> DerivedEventSpec {
    let mut payload = workspace_event_payload(current, workspace);
    if let (Some(payload), Some(detail)) = (payload.as_object_mut(), detail.as_object()) {
        for (key, value) in detail {
            payload.insert(key.clone(), value.clone());
        }
    }
    DerivedEventSpec {
        name,
        category: "sidebar",
        source: "session.model",
        window_id: current.window_id.clone(),
        workspace_id: workspace.id.clone(),
        surface_id: None,
        payload,
    }
}

pub(super) fn pane_event_spec(
    name: &'static str,
    current: &SessionEventSummary,
    workspace: &WorkspaceEventSummary,
    pane: &PaneEventSummary,
    previous_surface_id: Option<&str>,
) -> DerivedEventSpec {
    let mut payload = json!({
        "window_id": current.window_id,
        "workspace_id": workspace.id,
        "workspace_ref": workspace_ref(workspace.index),
        "pane_id": pane.id,
        "pane_ref": pane_ref(pane.index),
        "pane_key": pane.key,
        "index": pane.index,
        "surface_ids": pane.surface_ids,
        "selected_surface_id": pane.selected_surface_id,
        "surface_id": pane.selected_surface_id,
        "origin": "session.changed",
    });
    if let Some(previous_surface_id) = previous_surface_id {
        payload["previous_surface_id"] = json!(previous_surface_id);
    }
    DerivedEventSpec {
        name,
        category: "pane",
        source: "session.model",
        window_id: current.window_id.clone(),
        workspace_id: workspace.id.clone(),
        surface_id: pane.selected_surface_id.clone(),
        payload,
    }
}

pub(super) fn surface_event_spec(
    name: &'static str,
    current: &SessionEventSummary,
    workspace: &WorkspaceEventSummary,
    surface_id: &str,
    previous_surface_id: Option<&str>,
) -> DerivedEventSpec {
    let mut payload = json!({
        "window_id": current.window_id,
        "workspace_id": workspace.id,
        "workspace_ref": workspace_ref(workspace.index),
        "surface_id": surface_id,
        "surface_ref": surface_ref_for_summary(workspace, surface_id),
        "selected_surface_id": workspace.selected_surface_id,
        "index": workspace.surface_ids.iter().position(|id| id == surface_id),
        "tab_count": workspace.surface_ids.len(),
        "focused": workspace.selected_surface_id.as_deref() == Some(surface_id),
        "origin": "session.changed",
    });
    if let Some(previous_surface_id) = previous_surface_id {
        payload["previous_surface_id"] = json!(previous_surface_id);
    }
    DerivedEventSpec {
        name,
        category: "surface",
        source: "session.model",
        window_id: current.window_id.clone(),
        workspace_id: workspace.id.clone(),
        surface_id: Some(surface_id.to_string()),
        payload,
    }
}

pub(super) fn selected_workspace(current: &SessionEventSummary) -> Option<&WorkspaceEventSummary> {
    current
        .selected_workspace_index
        .and_then(|index| current.workspaces.get(index))
}

pub(super) fn workspace_event_identifier(workspace: &WorkspaceEventSummary) -> String {
    workspace
        .id
        .clone()
        .unwrap_or_else(|| workspace.key.clone())
}

pub(super) fn surface_ref_for_summary(
    workspace: &WorkspaceEventSummary,
    surface_id: &str,
) -> Option<String> {
    workspace
        .surface_ids
        .iter()
        .position(|id| id == surface_id)
        .map(surface_ref)
}

pub(super) fn record_event(
    app: &AppHandle,
    name: &str,
    category: &str,
    source: &str,
    window_id: Option<String>,
    workspace_id: Option<String>,
    pane_id: Option<String>,
    surface_id: Option<String>,
    payload: Value,
) {
    let Some(state) = app.try_state::<ControlEventState>() else {
        return;
    };
    let mut guard = state
        .inner
        .lock()
        .expect("control event log mutex poisoned");
    let seq = guard.next_seq;
    guard.next_seq = guard.next_seq.saturating_add(1);
    let boot_id = guard.boot_id.clone();
    let event = json!({
        "type": "event",
        "protocol": EVENT_STREAM_PROTOCOL,
        "version": EVENT_STREAM_VERSION,
        "boot_id": boot_id,
        "seq": seq,
        "id": format!("{boot_id}-{seq}"),
        "name": name,
        "category": category,
        "source": source,
        "occurred_at": event_timestamp(),
        "workspace_id": workspace_id,
        "surface_id": surface_id,
        "pane_id": pane_id,
        "window_id": window_id,
        "payload": payload,
    });
    let frame = serde_json::to_string(&event).ok();
    guard.events.push_back(event);
    while guard.events.len() > EVENT_REPLAY_LIMIT {
        guard.events.pop_front();
    }
    if let Some(frame) = frame {
        let event = guard.events.back().cloned().unwrap_or(Value::Null);
        fan_out_event_to_subscribers(&mut guard.subscribers, &event, &frame);
    }
    let event = guard.events.back().cloned();
    drop(guard);
    if let Some(event) = event {
        let _ = app.emit(CONTROL_EVENTS_CHANGED_EVENT, event.clone());
        append_event_to_disk(&event);
    }
}

pub(super) fn emit_transient_control_event(
    app: &AppHandle,
    name: &str,
    category: &str,
    source: &str,
    window_id: Option<String>,
    workspace_id: Option<String>,
    pane_id: Option<String>,
    surface_id: Option<String>,
    payload: Value,
) -> bool {
    let Some(state) = app.try_state::<ControlEventState>() else {
        return false;
    };
    let mut guard = state
        .inner
        .lock()
        .expect("control event log mutex poisoned");
    let event = json!({
        "type": "event",
        "protocol": EVENT_STREAM_PROTOCOL,
        "version": EVENT_STREAM_VERSION,
        "boot_id": guard.boot_id.clone(),
        "name": name,
        "category": category,
        "source": source,
        "occurred_at": event_timestamp(),
        "workspace_id": workspace_id,
        "surface_id": surface_id,
        "pane_id": pane_id,
        "window_id": window_id,
        "payload": payload,
    });
    let Ok(frame) = serde_json::to_string(&event) else {
        return false;
    };
    fan_out_event_to_subscribers(&mut guard.subscribers, &event, &frame)
}

pub(crate) fn publish_notification_removal_effects(
    app: &AppHandle,
    effects: &crate::notifications::NotificationMutationEffects,
    lifecycle_name: &'static str,
    workspace_id: Option<&str>,
) {
    crate::notifications::clear_native_notifications(effects);
    if effects.cleared.is_empty() {
        return;
    }
    for event in notification_removal_lifecycle_events(effects, lifecycle_name, workspace_id) {
        record_event(
            app,
            event.name,
            event.category,
            event.source,
            event.window_id,
            event.workspace_id,
            None,
            event.surface_id,
            event.payload,
        );
    }
    let ids = effects
        .cleared
        .iter()
        .map(|notification| notification.id.clone())
        .collect::<Vec<_>>();
    let dismissed_payload = json!({
        "ids": ids,
        "unread_count": effects.center.unread_count,
    });
    record_event(
        app,
        "notification.dismissed",
        "notification",
        "notification.store",
        None,
        workspace_id.map(str::to_owned),
        None,
        None,
        dismissed_payload,
    );
}

pub(super) fn notification_removal_lifecycle_events(
    effects: &crate::notifications::NotificationMutationEffects,
    lifecycle_name: &'static str,
    workspace_id: Option<&str>,
) -> Vec<DerivedEventSpec> {
    if lifecycle_name == "notification.read" {
        return effects
            .cleared
            .iter()
            .map(|notification| DerivedEventSpec {
                name: lifecycle_name,
                category: "notification",
                source: "notification.store",
                window_id: None,
                workspace_id: Some(notification.tab_id.clone()),
                surface_id: notification.surface_id.clone(),
                payload: json!({
                    "notification_ids": [notification.id.clone()],
                    "count": 1,
                }),
            })
            .collect();
    }

    let notification_ids = effects
        .cleared
        .iter()
        .map(|notification| notification.id.clone())
        .collect::<Vec<_>>();
    vec![DerivedEventSpec {
        name: lifecycle_name,
        category: "notification",
        source: "notification.store",
        window_id: None,
        workspace_id: workspace_id.map(str::to_owned),
        surface_id: None,
        payload: json!({
            "count": notification_ids.len(),
            "notification_ids": notification_ids,
        }),
    }]
}

pub(super) fn append_event_to_disk(event: &Value) {
    let Some(home) = event_log_home_directory() else {
        return;
    };
    let Ok(line) = serde_json::to_string(event) else {
        return;
    };
    let dir = home.join(".cmuxterm");
    if let Err(error) = append_event_line_to_dir(&dir, &line, EVENT_LOG_MAX_BYTES) {
        eprintln!("[events] failed to append durable event log: {error}");
    }
}

pub(super) fn fan_out_event_to_subscribers(
    subscribers: &mut Vec<EventSubscriber>,
    event: &Value,
    frame: &str,
) -> bool {
    let mut delivered = false;
    subscribers.retain(|subscriber| {
        if !event_matches_filters(event, &subscriber.names, &subscriber.categories) {
            return true;
        }
        let sent = subscriber.sender.send(frame.to_string()).is_ok();
        delivered |= sent;
        sent
    });
    delivered
}

pub(super) fn event_log_home_directory() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
}

pub(super) fn append_event_line_to_dir(
    dir: &Path,
    line: &str,
    max_bytes: u64,
) -> std::io::Result<()> {
    fs::create_dir_all(dir)?;
    let current = dir.join(EVENT_LOG_FILE_NAME);
    let archive = dir.join(EVENT_LOG_ARCHIVE_FILE_NAME);
    let additional_bytes = line.len() as u64 + 1;
    if current
        .metadata()
        .map(|metadata| metadata.len().saturating_add(additional_bytes) > max_bytes)
        .unwrap_or(false)
    {
        match fs::remove_file(&archive) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        fs::rename(&current, &archive)?;
    }
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(current)?;
    file.write_all(line.as_bytes())?;
    file.write_all(b"\n")?;
    file.flush()
}

pub(super) fn events_snapshot_payload(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> Value {
    let (ack, events, heartbeat) = events_payload_parts(app, params);
    json!({
        "ack": ack,
        "events": events,
        "heartbeat": heartbeat,
    })
}

pub(super) fn events_live_stream(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlStream {
    let (ack, events, heartbeat, receiver) = events_live_stream_parts(app, params);
    let mut frames = event_stream_initial_frames(ack, events, heartbeat);
    if frames.is_empty() {
        frames.push("{}".to_string());
    }
    ControlStream::Live {
        initial_frames: frames,
        receiver,
    }
}

pub(super) fn event_stream_initial_frames(
    ack: Value,
    events: Vec<Value>,
    heartbeat: Value,
) -> Vec<String> {
    let mut frames = Vec::with_capacity(events.len() + 2);
    frames.push(serde_json::to_string(&ack).unwrap_or_else(|_| "{}".to_string()));
    frames.extend(
        events
            .into_iter()
            .map(|event| serde_json::to_string(&event).unwrap_or_else(|_| "{}".to_string())),
    );
    if !heartbeat.is_null() {
        frames.push(serde_json::to_string(&heartbeat).unwrap_or_else(|_| "{}".to_string()));
    }
    frames
}

pub(super) fn events_live_stream_parts(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> (
    Value,
    Vec<Value>,
    Value,
    cmux_ipc::stream_mpsc::UnboundedReceiver<String>,
) {
    let names = string_vec_param(params, &["names", "name"]).unwrap_or_default();
    let categories = string_vec_param(params, &["categories", "category"]).unwrap_or_default();
    let requested_after_seq =
        i64_param(params, &["after_seq", "after"]).map(|value| value.max(0) as u64);
    let limit = usize_param(params, &["limit"]).unwrap_or(EVENT_REPLAY_LIMIT);
    let include_heartbeats = bool_param(params, &["include_heartbeats", "heartbeat"])
        .unwrap_or_else(|| !bool_param(params, &["no_heartbeat", "no-heartbeat"]).unwrap_or(false));
    let (sender, receiver) = cmux_ipc::stream_mpsc::unbounded_channel();
    let heartbeat_sender = include_heartbeats.then(|| sender.clone());

    let (boot_id, next_seq, retained_events) = {
        let state = app.state::<ControlEventState>();
        let mut guard = state
            .inner
            .lock()
            .expect("control event log mutex poisoned");
        let retained_events = guard.events.iter().cloned().collect::<Vec<_>>();
        guard.subscribers.push(EventSubscriber {
            sender: sender.clone(),
            names: names.clone(),
            categories: categories.clone(),
        });
        (guard.boot_id.clone(), guard.next_seq, retained_events)
    };
    let (ack, events, heartbeat) = events_parts_from_retained(
        boot_id,
        next_seq,
        retained_events,
        requested_after_seq,
        limit,
        include_heartbeats,
        names,
        categories,
    );
    if let Some(sender) = heartbeat_sender {
        spawn_event_heartbeat_task(app.clone(), sender, ack["subscription_id"].clone());
    }
    (ack, events, heartbeat, receiver)
}

pub(super) fn spawn_event_heartbeat_task(
    app: AppHandle,
    sender: cmux_ipc::stream_mpsc::UnboundedSender<String>,
    subscription_id: Value,
) {
    tauri::async_runtime::spawn(async move {
        loop {
            cmux_ipc::stream_sleep(Duration::from_secs(15)).await;
            let Some(state) = app.try_state::<ControlEventState>() else {
                break;
            };
            let (boot_id, latest_seq) = {
                let guard = state
                    .inner
                    .lock()
                    .expect("control event log mutex poisoned");
                (guard.boot_id.clone(), guard.next_seq.saturating_sub(1))
            };
            let heartbeat = json!({
                "type": "heartbeat",
                "protocol": EVENT_STREAM_PROTOCOL,
                "version": EVENT_STREAM_VERSION,
                "boot_id": boot_id,
                "subscription_id": subscription_id,
                "latest_seq": latest_seq,
                "occurred_at": event_timestamp(),
            });
            let Ok(frame) = serde_json::to_string(&heartbeat) else {
                continue;
            };
            if sender.send(frame).is_err() {
                break;
            }
        }
    });
}

pub(super) fn events_payload_parts(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> (Value, Vec<Value>, Value) {
    let names = string_vec_param(params, &["names", "name"]).unwrap_or_default();
    let categories = string_vec_param(params, &["categories", "category"]).unwrap_or_default();
    let requested_after_seq =
        i64_param(params, &["after_seq", "after"]).map(|value| value.max(0) as u64);
    let limit = usize_param(params, &["limit"]).unwrap_or(EVENT_REPLAY_LIMIT);
    let include_heartbeats = bool_param(params, &["include_heartbeats", "heartbeat"])
        .unwrap_or_else(|| !bool_param(params, &["no_heartbeat", "no-heartbeat"]).unwrap_or(false));

    let (boot_id, next_seq, retained_events) = {
        let state = app.state::<ControlEventState>();
        let guard = state
            .inner
            .lock()
            .expect("control event log mutex poisoned");
        (
            guard.boot_id.clone(),
            guard.next_seq,
            guard.events.iter().cloned().collect::<Vec<_>>(),
        )
    };
    events_parts_from_retained(
        boot_id,
        next_seq,
        retained_events,
        requested_after_seq,
        limit,
        include_heartbeats,
        names,
        categories,
    )
}

pub(super) fn events_parts_from_retained(
    boot_id: String,
    next_seq: u64,
    retained_events: Vec<Value>,
    after_seq_param: Option<u64>,
    limit: usize,
    include_heartbeats: bool,
    names: Vec<String>,
    categories: Vec<String>,
) -> (Value, Vec<Value>, Value) {
    let latest_seq = next_seq.saturating_sub(1);
    // D8a: omitted after_seq subscribes at latest with no default replay.
    // The ack echoes the raw param and the resolved requested_after_seq.
    let requested_after_seq = after_seq_param.unwrap_or(latest_seq);
    let oldest_seq = retained_events
        .first()
        .and_then(|event| event.get("seq"))
        .and_then(Value::as_u64)
        .unwrap_or(next_seq);
    let gap = (requested_after_seq > latest_seq)
        || (!retained_events.is_empty() && requested_after_seq.saturating_add(1) < oldest_seq);
    let events: Vec<Value> = retained_events
        .into_iter()
        .filter(|event| {
            event.get("seq").and_then(Value::as_u64).unwrap_or(0) > requested_after_seq
                && event_matches_filters(event, &names, &categories)
        })
        .take(limit)
        .collect();
    let ack = json!({
        "type": "ack",
        "protocol": EVENT_STREAM_PROTOCOL,
        "version": EVENT_STREAM_VERSION,
        "boot_id": boot_id,
        "subscription_id": Uuid::new_v4().to_string(),
        "heartbeat_interval_seconds": 15,
        "replay_count": events.len(),
        "resume": {
            "after_seq": after_seq_param,
            "requested_after_seq": requested_after_seq,
            "oldest_seq": oldest_seq,
            "latest_seq": latest_seq,
            "next_seq": next_seq,
            "gap": gap,
        },
        "filters": {
            "names": names,
            "categories": categories,
        }
    });
    let heartbeat = if include_heartbeats {
        json!({
            "type": "heartbeat",
            "protocol": EVENT_STREAM_PROTOCOL,
            "version": EVENT_STREAM_VERSION,
            "boot_id": boot_id,
            "subscription_id": ack["subscription_id"].clone(),
            "latest_seq": latest_seq,
            "occurred_at": event_timestamp(),
        })
    } else {
        Value::Null
    };
    (ack, events, heartbeat)
}

pub(super) fn event_matches_filters(
    event: &Value,
    names: &[String],
    categories: &[String],
) -> bool {
    let name_matches = names.is_empty()
        || event
            .get("name")
            .and_then(Value::as_str)
            .is_some_and(|name| names.iter().any(|filter| filter == name));
    let category_matches = categories.is_empty()
        || event
            .get("category")
            .and_then(Value::as_str)
            .is_some_and(|category| categories.iter().any(|filter| filter == category));
    name_matches && category_matches
}

pub(super) fn event_timestamp() -> String {
    OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
}
