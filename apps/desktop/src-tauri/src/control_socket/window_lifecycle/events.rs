use super::*;

/// Socket-normalized `publishCmuxWindowLifecycle` payload
/// (Sources/CmuxLifecycleEventPublishing.swift:281-300).
pub(crate) fn window_lifecycle_event(
    name: &'static str,
    origin: &'static str,
    window: &SessionWindowSnapshot,
    window_id: &str,
    is_key: bool,
) -> LifecycleEvent {
    let selected_index =
        usize::try_from(window.tab_manager.selected_workspace_index.unwrap_or(0)).unwrap_or(0);
    let workspace_id = window.selected_workspace_id.clone().or_else(|| {
        window
            .tab_manager
            .workspaces
            .get(selected_index)
            .and_then(|workspace| workspace.workspace_id.clone())
    });
    LifecycleEvent {
        name,
        category: "window",
        source: "window.lifecycle",
        window_id: Some(window_id.to_owned()),
        workspace_id: workspace_id.clone(),
        pane_id: None,
        surface_id: None,
        payload: json!({
            "window_id": window_id,
            "workspace_id": workspace_id,
            "workspace_count": window.tab_manager.workspaces.len(),
            "selected_workspace_index": selected_index,
            "is_key_window": is_key,
            "is_main_window": is_key,
            "origin": origin,
        }),
    }
}

pub(super) fn initial_workspace_events(window: &SessionWindowSnapshot) -> Vec<LifecycleEvent> {
    let workspace = &window.tab_manager.workspaces[0];
    let workspace_id = workspace
        .workspace_id
        .clone()
        .expect("fresh window workspace identity");
    let surface_id = workspace
        .focused_panel_id
        .clone()
        .expect("fresh window surface identity");
    let pane_id = match workspace.layout.as_ref().expect("fresh window layout") {
        cmux_core::session::SessionWorkspaceLayoutSnapshot::Pane(pane) => {
            pane.pane_id.clone().expect("fresh window pane identity")
        }
        cmux_core::session::SessionWorkspaceLayoutSnapshot::Split(_) => {
            unreachable!("fresh window layout is a single pane")
        }
    };
    let workspace_payload = json!({
        "workspace_id": workspace_id,
        "index": 0,
        "title": "Terminal 1",
        "custom_title": workspace.custom_title,
        "cwd": workspace.current_directory,
        "selected": true,
        "tab_count": 1,
        "previous_workspace_id": null,
    });
    vec![
        LifecycleEvent {
            name: "surface.selected",
            category: "surface",
            source: "workspace.lifecycle",
            window_id: None,
            workspace_id: Some(workspace_id.clone()),
            pane_id: Some(pane_id.clone()),
            surface_id: Some(surface_id.clone()),
            payload: json!({
                "surface_id": surface_id,
                "pane_id": pane_id,
                "kind": "terminal",
                "focused": true,
                "previous_surface_id": null,
                "origin": "bonsplit_selection",
            }),
        },
        LifecycleEvent {
            name: "pane.focused",
            category: "pane",
            source: "workspace.lifecycle",
            window_id: None,
            workspace_id: Some(workspace_id.clone()),
            pane_id: Some(pane_id.clone()),
            surface_id: Some(surface_id.clone()),
            payload: json!({
                "pane_id": pane_id,
                "selected_surface_id": surface_id,
                "origin": "bonsplit_selection",
            }),
        },
        LifecycleEvent {
            name: "surface.focused",
            category: "surface",
            source: "workspace.lifecycle",
            window_id: None,
            workspace_id: Some(workspace_id.clone()),
            pane_id: Some(pane_id.clone()),
            surface_id: Some(surface_id.clone()),
            payload: json!({
                "surface_id": surface_id,
                "pane_id": pane_id,
                "kind": "terminal",
                "origin": "bonsplit_selection",
            }),
        },
        LifecycleEvent {
            name: "workspace.created",
            category: "workspace",
            source: "workspace.lifecycle",
            window_id: None,
            workspace_id: Some(workspace_id.clone()),
            pane_id: None,
            surface_id: None,
            payload: workspace_payload.clone(),
        },
        LifecycleEvent {
            name: "surface.created",
            category: "surface",
            source: "workspace.lifecycle",
            window_id: None,
            workspace_id: Some(workspace_id.clone()),
            pane_id: Some(pane_id.clone()),
            surface_id: Some(surface_id.clone()),
            payload: json!({
                "surface_id": surface_id,
                "pane_id": pane_id,
                "kind": "terminal",
                "focused": true,
                "origin": "workspace_initial",
            }),
        },
        LifecycleEvent {
            name: "workspace.selected",
            category: "workspace",
            source: "workspace.lifecycle",
            window_id: None,
            workspace_id: Some(workspace_id),
            pane_id: None,
            surface_id: None,
            payload: workspace_payload,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_window_events_match_the_canonical_initial_sequence() {
        let workspace = crate::session::fresh_control_window_workspace("surface-test");
        let window = SessionWindowSnapshot {
            window_id: Some("window-test".into()),
            selected_workspace_id: workspace.workspace_id.clone(),
            dock: None,
            tab_manager: SessionTabManagerSnapshot {
                selected_workspace_index: Some(0),
                workspaces: vec![workspace],
                workspace_groups: None,
            },
        };
        let events = initial_workspace_events(&window);
        assert_eq!(
            events.iter().map(|event| event.name).collect::<Vec<_>>(),
            [
                "surface.selected",
                "pane.focused",
                "surface.focused",
                "workspace.created",
                "surface.created",
                "workspace.selected",
            ]
        );
        let workspace_id = window.selected_workspace_id.as_deref().unwrap();
        assert!(events.iter().all(|event| event.window_id.is_none()
            && event.workspace_id.as_deref() == Some(workspace_id)));
        assert_eq!(events[0].payload["origin"], "bonsplit_selection");
        assert_eq!(events[3].payload["title"], "Terminal 1");
        assert_eq!(events[4].payload["origin"], "workspace_initial");
        assert_eq!(events[5].payload, events[3].payload);
    }

    #[test]
    fn close_event_uses_key_window_state_not_active_routing_pointer() {
        let workspace = crate::session::fresh_control_window_workspace("surface-test");
        let selected_workspace_id = workspace.workspace_id.clone();
        let window = |window_id: &str| SessionWindowSnapshot {
            window_id: Some(window_id.into()),
            selected_workspace_id: selected_workspace_id.clone(),
            dock: None,
            tab_manager: SessionTabManagerSnapshot {
                selected_workspace_index: Some(0),
                workspaces: vec![workspace.clone()],
                workspace_groups: None,
            },
        };
        let snapshot = AppSessionSnapshot {
            windows: vec![window("key-window"), window("active-window")],
            ..Default::default()
        };
        let params = serde_json::Map::from_iter([("window_id".into(), json!("active-window"))]);
        let transition = super::window_close(
            &snapshot,
            &params,
            &WindowLifecycleContext {
                active_window_id: Some("active-window".into()),
                key_window_id: Some("key-window".into()),
                quit_confirmation_required: true,
                now_epoch_seconds: 0.0,
                new_window_id: None,
                new_surface_id: None,
            },
        );
        assert_eq!(transition.events[0].payload["is_key_window"], false);
    }
}
