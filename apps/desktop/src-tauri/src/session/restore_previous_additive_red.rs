//! Production-connected RED contract for canonical additive manual restore.
//!
//! These tests intentionally stop at the existing snapshot transaction seam.
//! Socket replies, real WebView effects, file-path immutability, lifecycle
//! event ordering, and product activation require a neutral production seam
//! before they can be tested without simulating the behavior under test.

use super::*;

const MAX_RESTORED_WINDOWS: usize = 12;

fn id(value: u128) -> String {
    Uuid::from_u128(value).to_string()
}

fn window_fixture(seed: u128) -> SessionWindowSnapshot {
    let window_id = id(seed);
    let workspace_id = id(seed + 0x1000);
    let pane_id = id(seed + 0x2000);
    let surface_id = id(seed + 0x3000);
    let mut snapshot = initial_snapshot(&surface_id);
    let window = &mut snapshot.windows[0];
    window.window_id = Some(window_id);
    window.selected_workspace_id = Some(workspace_id.clone());
    let workspace = &mut window.tab_manager.workspaces[0];
    workspace.workspace_id = Some(workspace_id);
    workspace.process_title = format!("window-{seed:x}");
    let Some(SessionWorkspaceLayoutSnapshot::Pane(pane)) = workspace.layout.as_mut() else {
        panic!("single-pane fixture")
    };
    pane.pane_id = Some(pane_id.clone());
    pane.panel_ids = vec![surface_id.clone()];
    pane.selected_panel_id = Some(surface_id.clone());
    let surface = workspace
        .surfaces
        .as_mut()
        .and_then(|surfaces| surfaces.first_mut())
        .expect("initial surface record");
    surface.surface_id = surface_id;
    surface.pane_id = pane_id;
    snapshot.windows.remove(0)
}

fn snapshot(windows: Vec<SessionWindowSnapshot>) -> AppSessionSnapshot {
    AppSessionSnapshot {
        version: SESSION_SNAPSHOT_SCHEMA_VERSION,
        created_at: 1,
        windows,
    }
}

fn snapshot_fixture(seed: u128, count: usize) -> AppSessionSnapshot {
    snapshot(
        (0..count)
            .map(|offset| window_fixture(seed + offset as u128))
            .collect(),
    )
}

fn crash_directory(seed: u128) -> String {
    let home = std::env::var("USERPROFILE").unwrap_or_else(|_| "C:\\Users\\cmux".to_string());
    PathBuf::from(home)
        .join(".local")
        .join("state")
        .join("cmux")
        .join("crash")
        .join(format!("report-{seed:x}"))
        .to_string_lossy()
        .into_owned()
}

fn crash_diagnostic_window(seed: u128) -> SessionWindowSnapshot {
    let mut window = window_fixture(seed);
    window.tab_manager.workspaces[0].current_directory = Some(crash_directory(seed));
    window
}

fn pane(window: &SessionWindowSnapshot) -> &cmux_core::session::SessionPaneLayoutSnapshot {
    let Some(SessionWorkspaceLayoutSnapshot::Pane(pane)) =
        window.tab_manager.workspaces[0].layout.as_ref()
    else {
        panic!("single-pane fixture")
    };
    pane
}

fn surface_id(window: &SessionWindowSnapshot) -> &str {
    pane(window)
        .panel_ids
        .first()
        .map(String::as_str)
        .expect("surface id")
}

fn dock_fixture(seed: u128) -> cmux_core::session::SessionDockSnapshot {
    let pane_id = id(seed + 0x4000);
    let surface_id = id(seed + 0x5000);
    cmux_core::session::SessionDockSnapshot {
        workspace_id: format!("dock:{}", id(seed)),
        layout: Some(SessionWorkspaceLayoutSnapshot::Pane(
            cmux_core::session::SessionPaneLayoutSnapshot {
                pane_id: Some(pane_id.clone()),
                panel_ids: vec![surface_id.clone()],
                selected_panel_id: Some(surface_id.clone()),
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
            },
        )),
        surfaces: vec![cmux_core::session::SessionSurfaceSnapshot {
            surface_id: surface_id.clone(),
            pane_id,
            generation: 1,
            kind: SessionSurfaceKindSnapshot::Terminal,
            metadata: Default::default(),
            terminal_startup: None,
        }],
        focused_surface_id: Some(surface_id),
    }
}

#[derive(Default)]
struct RecordingPublication {
    calls: Vec<&'static str>,
    persisted: Vec<AppSessionSnapshot>,
    baseline: Option<AppSessionSnapshot>,
    emitted: Vec<AppSessionSnapshot>,
}

impl SnapshotPublicationOperations for RecordingPublication {
    fn persist(&mut self, candidate: &AppSessionSnapshot) -> Result<(), String> {
        self.calls.push("persist");
        self.persisted.push(candidate.clone());
        Ok(())
    }

    fn update_event_baseline(&mut self, candidate: &AppSessionSnapshot) {
        self.calls.push("baseline");
        self.baseline = Some(candidate.clone());
    }

    fn emit(&mut self, candidate: &AppSessionSnapshot) -> Result<(), String> {
        self.calls.push("emit");
        self.emitted.push(candidate.clone());
        Ok(())
    }
}

fn restore(
    current: AppSessionSnapshot,
    previous: Option<AppSessionSnapshot>,
    next_panel_value: u64,
) -> (
    Result<AppSessionSnapshot, String>,
    AppSessionSnapshot,
    u64,
    RecordingPublication,
) {
    let authority = GatedSnapshot::new(current);
    let next_panel = AtomicU64::new(next_panel_value);
    let mut publication = RecordingPublication::default();
    let result =
        restore_previous_launch_transaction(&authority, &next_panel, &mut publication, || previous);
    let authoritative = authority.lock().unwrap().clone();
    (
        result,
        authoritative,
        next_panel.load(Ordering::Relaxed),
        publication,
    )
}

#[test]
fn additive_restore_keeps_live_windows_byte_stable_and_appends_in_order() {
    let current = snapshot_fixture(0x100, 2);
    let previous = snapshot_fixture(0x200, 2);
    let live = current.windows.clone();

    let (result, authoritative, _, _) = restore(current, Some(previous), 50);
    let committed = result.expect("valid previous snapshot");

    assert_eq!(committed, authoritative);
    assert_eq!(committed.windows.len(), 4);
    assert_eq!(&committed.windows[..2], live.as_slice());
    assert_eq!(
        committed.windows[2..]
            .iter()
            .map(|window| window.tab_manager.workspaces[0].process_title.as_str())
            .collect::<Vec<_>>(),
        ["window-200", "window-201"]
    );
}

#[test]
fn crash_pruning_precedes_the_twelve_restored_window_cap() {
    let current = snapshot_fixture(0x300, 1);
    let mut previous_windows = (0..15)
        .map(|offset| window_fixture(0x400 + offset))
        .collect::<Vec<_>>();
    previous_windows[1] = crash_diagnostic_window(0x401);
    previous_windows[6] = crash_diagnostic_window(0x406);
    let expected = previous_windows
        .iter()
        .filter(|window| {
            !window.tab_manager.workspaces[0]
                .current_directory
                .as_deref()
                .is_some_and(|path| path.contains("\\cmux\\crash\\"))
        })
        .take(MAX_RESTORED_WINDOWS)
        .map(|window| window.tab_manager.workspaces[0].process_title.clone())
        .collect::<Vec<_>>();

    let (result, _, _, _) = restore(current, Some(snapshot(previous_windows)), 50);
    let committed = result.expect("restorable previous snapshot");

    assert_eq!(committed.windows.len(), 1 + MAX_RESTORED_WINDOWS);
    assert_eq!(
        committed.windows[1..]
            .iter()
            .map(|window| window.tab_manager.workspaces[0].process_title.clone())
            .collect::<Vec<_>>(),
        expected
    );
}

#[test]
fn crash_pruning_removes_only_diagnostic_workspaces_and_repairs_selection() {
    let current = snapshot_fixture(0x500, 1);
    let mut mixed = window_fixture(0x600);
    let mut crash_workspace = mixed.tab_manager.workspaces[0].clone();
    crash_workspace.current_directory = Some(crash_directory(0x600));
    let survivor = window_fixture(0x601).tab_manager.workspaces.remove(0);
    let survivor_id = survivor.workspace_id.clone();
    mixed.tab_manager.workspaces = vec![crash_workspace, survivor];
    mixed.tab_manager.selected_workspace_index = Some(0);
    mixed.selected_workspace_id = mixed.tab_manager.workspaces[0].workspace_id.clone();

    let (result, _, _, _) = restore(current, Some(snapshot(vec![mixed])), 50);
    let committed = result.expect("mixed window remains restorable");
    let restored = &committed.windows[1];

    assert_eq!(restored.tab_manager.workspaces.len(), 1);
    assert_eq!(restored.tab_manager.selected_workspace_index, Some(0));
    assert_eq!(restored.selected_workspace_id, survivor_id);
}

#[test]
fn restore_strips_persisted_docks_and_caps_only_restored_windows() {
    let current = snapshot_fixture(0x700, 2);
    let mut previous = snapshot_fixture(0x800, MAX_RESTORED_WINDOWS + 2);
    for (index, window) in previous.windows.iter_mut().enumerate() {
        window.dock = Some(dock_fixture(0x900 + index as u128));
    }

    let (result, _, _, _) = restore(current, Some(previous), 50);
    let committed = result.expect("valid previous snapshot");

    assert_eq!(committed.windows.len(), 2 + MAX_RESTORED_WINDOWS);
    assert!(committed.windows[2..]
        .iter()
        .all(|window| window.dock.is_none()));
}

#[test]
fn live_main_and_dock_identities_exclude_restored_collisions_coherently() {
    let mut current = snapshot_fixture(0xa00, 1);
    current.windows[0].dock = Some(dock_fixture(0xa01));
    let live_window_id = current.windows[0].window_id.clone().expect("window id");
    let live_workspace_id = current.windows[0].tab_manager.workspaces[0]
        .workspace_id
        .clone()
        .expect("workspace id");
    let live_surface_id = surface_id(&current.windows[0]).to_string();
    let live_dock_surface_id = current.windows[0]
        .dock
        .as_ref()
        .and_then(|dock| dock.surfaces.first())
        .map(|surface| surface.surface_id.clone())
        .expect("dock surface");

    let mut restored = window_fixture(0xb00);
    restored.window_id = Some(live_window_id.clone());
    restored.tab_manager.workspaces[0].workspace_id = Some(live_workspace_id.clone());
    restored.selected_workspace_id = Some(live_workspace_id.clone());
    let pane_id = pane(&restored).pane_id.clone().expect("pane id");
    let Some(SessionWorkspaceLayoutSnapshot::Pane(layout)) =
        restored.tab_manager.workspaces[0].layout.as_mut()
    else {
        panic!("single pane")
    };
    layout.panel_ids = vec![live_surface_id.clone(), live_dock_surface_id.clone()];
    layout.selected_panel_id = Some(live_surface_id.clone());
    restored.tab_manager.workspaces[0].focused_panel_id = Some(live_dock_surface_id.clone());
    restored.tab_manager.workspaces[0].surfaces = Some(vec![
        cmux_core::session::SessionSurfaceSnapshot {
            surface_id: live_surface_id.clone(),
            pane_id: pane_id.clone(),
            generation: 1,
            kind: SessionSurfaceKindSnapshot::Terminal,
            metadata: Default::default(),
            terminal_startup: None,
        },
        cmux_core::session::SessionSurfaceSnapshot {
            surface_id: live_dock_surface_id.clone(),
            pane_id,
            generation: 1,
            kind: SessionSurfaceKindSnapshot::Terminal,
            metadata: Default::default(),
            terminal_startup: None,
        },
    ]);

    let live = current.windows[0].clone();
    let (result, _, _, _) = restore(current, Some(snapshot(vec![restored])), 50);
    let committed = result.expect("collisions are reminted");

    assert_eq!(committed.windows[0], live);
    let restored = &committed.windows[1];
    assert_ne!(restored.window_id.as_deref(), Some(live_window_id.as_str()));
    assert_ne!(
        restored.tab_manager.workspaces[0].workspace_id.as_deref(),
        Some(live_workspace_id.as_str())
    );
    let layout = pane(restored);
    assert!(layout
        .panel_ids
        .iter()
        .all(|id| id != &live_surface_id && id != &live_dock_surface_id));
    assert!(layout
        .selected_panel_id
        .as_ref()
        .is_some_and(|selected| layout.panel_ids.contains(selected)));
    let surfaces = restored.tab_manager.workspaces[0]
        .surfaces
        .as_ref()
        .expect("surface records");
    assert_eq!(
        surfaces
            .iter()
            .map(|surface| surface.surface_id.clone())
            .collect::<Vec<_>>(),
        layout.panel_ids
    );
    assert!(surfaces.iter().all(|surface| {
        layout.pane_id.as_ref() == Some(&surface.pane_id)
            && ![&live_surface_id, &live_dock_surface_id].contains(&&surface.surface_id)
    }));
    assert!(restored.tab_manager.workspaces[0]
        .focused_panel_id
        .as_ref()
        .is_some_and(|focused| layout.panel_ids.contains(focused)));
}

#[test]
fn uncollided_workspace_and_surface_stable_ids_are_adopted() {
    let current = snapshot_fixture(0xc00, 1);
    let previous = snapshot_fixture(0xd00, 1);
    let persisted_window = previous.windows[0].window_id.clone();
    let persisted_workspace = previous.windows[0].tab_manager.workspaces[0]
        .workspace_id
        .clone();
    let persisted_surface = surface_id(&previous.windows[0]).to_string();

    let (result, _, _, _) = restore(current, Some(previous), 50);
    let committed = result.expect("valid previous snapshot");
    let restored = &committed.windows[1];

    assert_ne!(restored.window_id, persisted_window);
    assert_eq!(
        restored.tab_manager.workspaces[0].workspace_id,
        persisted_workspace
    );
    assert_eq!(surface_id(restored), persisted_surface);
}

#[test]
fn restored_legacy_surface_counter_cannot_regress_the_live_allocator() {
    let current = initial_snapshot("surface-80");
    let mut previous = snapshot_fixture(0xe00, 1);
    let old = surface_id(&previous.windows[0]).to_string();
    let workspace = &mut previous.windows[0].tab_manager.workspaces[0];
    let Some(SessionWorkspaceLayoutSnapshot::Pane(layout)) = workspace.layout.as_mut() else {
        panic!("single pane")
    };
    layout.panel_ids = vec!["surface-250".to_string()];
    layout.selected_panel_id = Some("surface-250".to_string());
    if let Some(surfaces) = &mut workspace.surfaces {
        surfaces[0].surface_id = "surface-250".to_string();
    }
    assert_ne!(old, "surface-250");

    let (result, _, next_panel, _) = restore(current, Some(previous), 81);
    result.expect("valid previous snapshot");

    assert!(next_panel >= 251, "allocator regressed to {next_panel}");
}

#[test]
fn missing_wrong_schema_empty_and_all_crash_previous_are_exact_noops() {
    let current = snapshot_fixture(0xf00, 1);
    let mut wrong_schema = snapshot_fixture(0x1000, 1);
    wrong_schema.version = SESSION_SNAPSHOT_SCHEMA_VERSION + 1;
    let cases = [
        ("missing", None),
        ("wrong-schema", Some(wrong_schema)),
        ("empty", Some(snapshot(Vec::new()))),
        (
            "all-crash",
            Some(snapshot(vec![crash_diagnostic_window(0x1100)])),
        ),
    ];

    for (name, previous) in cases {
        let (result, authoritative, next_panel, publication) =
            restore(current.clone(), previous, 70);
        assert_eq!(result.expect(name), current, "{name}: returned snapshot");
        assert_eq!(authoritative, current, "{name}: authority");
        assert_eq!(next_panel, 70, "{name}: allocator");
        assert!(publication.calls.is_empty(), "{name}: publication");
    }
}
