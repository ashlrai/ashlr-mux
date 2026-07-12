#[cfg(test)]
mod tests {
    use super::{
        DockCreateRequest, DockPlacement, DockRuntimeIntent, DockStore, DockSurfaceKind,
    };
    use uuid::Uuid;
    use cmux_core::session::{SessionSurfaceKindSnapshot, SessionSurfaceMetadataSnapshot};
    use cmux_core::surface_lifecycle::{ContainerKind, PaneSeed, SurfaceLifecycleModel, SurfaceSeed};

    fn terminal(title: &str) -> DockCreateRequest {
        DockCreateRequest {
            kind: DockSurfaceKind::Terminal,
            title: Some(title.to_string()),
            working_directory: Some("C:\\repo".to_string()),
            command: Some("cargo test".to_string()),
            environment: [("CI".to_string(), "0".to_string())].into(),
            ..DockCreateRequest::default()
        }
    }

    #[test]
    fn creates_ordered_heterogeneous_surfaces_with_runtime_intent() {
        let store = DockStore::default();
        let owner = Uuid::new_v4();
        let terminal = store.create(owner, terminal("Tests")).unwrap();
        let browser = store
            .create(
                owner,
                DockCreateRequest {
                    kind: DockSurfaceKind::Browser,
                    title: Some("Docs".into()),
                    url: Some("https://example.com/docs".into()),
                    pane_id: Some(terminal.pane_id),
                    focus: false,
                    ..DockCreateRequest::default()
                },
            )
            .unwrap();

        let snapshot = store.snapshot(owner);
        assert_eq!(snapshot.panes.len(), 1);
        assert_eq!(snapshot.panes[0].surface_ids, vec![terminal.surface_id, browser.surface_id]);
        assert_eq!(snapshot.panes[0].selected_surface_id, Some(terminal.surface_id));
        assert_eq!(snapshot.focused_pane_id, Some(terminal.pane_id));
        assert!(matches!(
            snapshot.surface(terminal.surface_id).unwrap().runtime,
            DockRuntimeIntent::Terminal { ref working_directory, ref command, .. }
                if working_directory.as_deref() == Some("C:\\repo")
                    && command.as_deref() == Some("cargo test")
        ));
        assert!(matches!(
            snapshot.surface(browser.surface_id).unwrap().runtime,
            DockRuntimeIntent::Browser { ref url, .. } if url == "https://example.com/docs"
        ));
    }

    #[test]
    fn split_select_focus_close_and_list_preserve_dock_invariants() {
        let store = DockStore::default();
        let owner = Uuid::new_v4();
        let first = store.create(owner, terminal("One")).unwrap();
        let second = store
            .create(
                owner,
                DockCreateRequest {
                    placement: DockPlacement::SplitRight,
                    source_surface_id: Some(first.surface_id),
                    initial_divider_position: Some(0.25),
                    ..terminal("Two")
                },
            )
            .unwrap();
        let background = store
            .create(
                owner,
                DockCreateRequest {
                    pane_id: Some(first.pane_id),
                    focus: false,
                    ..terminal("Background")
                },
            )
            .unwrap();

        let split = store.snapshot(owner);
        assert_eq!(split.panes.iter().map(|pane| pane.id).collect::<Vec<_>>(), vec![first.pane_id, second.pane_id]);
        assert_eq!(split.panes[1].divider_position, Some(0.25));
        assert_eq!(split.focused_surface_id(), Some(second.surface_id));
        assert_eq!(split.pane(first.pane_id).unwrap().selected_surface_id, Some(first.surface_id));

        store.select(owner, first.pane_id, background.surface_id).unwrap();
        assert_eq!(store.current(owner).unwrap().surface_id, second.surface_id);
        store.focus(owner, background.surface_id).unwrap();
        assert_eq!(store.current(owner).unwrap().surface_id, background.surface_id);
        assert_eq!(store.list(owner).len(), 3);

        store.close(owner, background.surface_id).unwrap();
        let closed = store.snapshot(owner);
        assert_eq!(closed.pane(first.pane_id).unwrap().selected_surface_id, Some(first.surface_id));
        assert_eq!(closed.focused_surface_id(), Some(first.surface_id));
    }

    #[test]
    fn snapshots_round_trip_without_replacing_public_identity_or_metadata() {
        let store = DockStore::default();
        let owner = Uuid::new_v4();
        let created = store.create(owner, terminal("Watcher")).unwrap();
        let json = store.snapshot_json(owner).unwrap();

        let restored = DockStore::default();
        restored.restore_json(&json).unwrap();
        let snapshot = restored.snapshot(owner);
        assert_eq!(snapshot.owner_id, owner);
        assert_eq!(snapshot.focused_surface_id(), Some(created.surface_id));
        assert_eq!(snapshot.surface(created.surface_id).unwrap().title, "Watcher");
    }

    #[test]
    fn persisted_registry_restores_independent_window_docks() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("dock-state.json");
        let store = DockStore::with_persistence(path.clone());
        let first_owner = Uuid::new_v4();
        let second_owner = Uuid::new_v4();
        let first = store.create(first_owner, terminal("First")).unwrap();
        let second = store.create(second_owner, terminal("Second")).unwrap();
        store.persist().unwrap();

        let restored = DockStore::with_persistence(path);
        restored.restore().unwrap();
        assert_eq!(restored.current(first_owner).unwrap().surface_id, first.surface_id);
        assert_eq!(restored.current(second_owner).unwrap().surface_id, second.surface_id);
        assert_ne!(restored.snapshot(first_owner).panes[0].id, restored.snapshot(second_owner).panes[0].id);
    }

    #[test]
    fn cross_container_move_uses_one_authority_and_survives_restore() {
        let owner = Uuid::new_v4();
        let workspace_id = Uuid::new_v4();
        let workspace_pane = Uuid::new_v4();
        let surface = Uuid::new_v4();
        let mut model = SurfaceLifecycleModel::new();
        model
            .add_pane(PaneSeed {
                pane_id: workspace_pane.to_string(),
                window_id: owner.to_string(),
                workspace_id: workspace_id.to_string(),
                container: ContainerKind::Workspace,
            })
            .unwrap();
        let generation = model
            .reserve_surface(SurfaceSeed {
                surface_id: surface.to_string(),
                pane_id: workspace_pane.to_string(),
                kind: SessionSurfaceKindSnapshot::Terminal,
                metadata: SessionSurfaceMetadataSnapshot::default(),
            })
            .unwrap()
            .generation;

        let store = DockStore::from_model(model);
        let dock_seed = store.create(owner, terminal("Dock seed")).unwrap();
        store
            .move_surface(owner, surface, dock_seed.pane_id, 0)
            .unwrap();
        let moved = store.authoritative_snapshot();
        let restored = SurfaceLifecycleModel::restore(moved).unwrap();
        let moved_owner = restored.owner_of_surface(&surface.to_string()).unwrap();
        assert_eq!(moved_owner.pane_id, dock_seed.pane_id.to_string());
        assert_eq!(restored.pane(&moved_owner.pane_id).unwrap().container, ContainerKind::Dock);
        assert_eq!(restored.surface(&surface.to_string()).unwrap().generation, generation);
        assert!(restored.pane(&workspace_pane.to_string()).unwrap().surface_ids.is_empty());
    }
}
