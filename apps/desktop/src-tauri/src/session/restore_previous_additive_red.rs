//! Additive manual-session restore product contract.
//!
//! Frozen canonical `e1825d40` creates bounded new windows without replacing
//! live windows. Windows `118fdc4e` still replaces the authoritative snapshot
//! and has no batch webview effect seam. These tests stay executable against
//! that rejected base so every failure describes an observable missing
//! behavior rather than an implementation-shape requirement.

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

fn crash_diagnostic_window(seed: u128) -> SessionWindowSnapshot {
    let mut window = window_fixture(seed);
    let home = std::env::var("USERPROFILE").unwrap_or_else(|_| "C:\\Users\\cmux".to_string());
    window.tab_manager.workspaces[0].current_directory = Some(
        PathBuf::from(home)
            .join(".local")
            .join("state")
            .join("cmux")
            .join("crash")
            .join(format!("report-{seed:x}"))
            .to_string_lossy()
            .into_owned(),
    );
    window
}

fn pane_id(window: &SessionWindowSnapshot) -> &str {
    let Some(SessionWorkspaceLayoutSnapshot::Pane(pane)) =
        window.tab_manager.workspaces[0].layout.as_ref()
    else {
        panic!("single-pane fixture")
    };
    pane.pane_id.as_deref().expect("pane id")
}

fn surface_id(window: &SessionWindowSnapshot) -> &str {
    let Some(SessionWorkspaceLayoutSnapshot::Pane(pane)) =
        window.tab_manager.workspaces[0].layout.as_ref()
    else {
        panic!("single-pane fixture")
    };
    pane.panel_ids
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
            kind: cmux_core::session::SessionSurfaceKindSnapshot::Terminal,
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
    persist_error: Option<String>,
    emit_error: Option<String>,
}

impl SnapshotPublicationOperations for RecordingPublication {
    fn persist(&mut self, candidate: &AppSessionSnapshot) -> Result<(), String> {
        self.calls.push("persist");
        self.persisted.push(candidate.clone());
        self.persist_error.take().map_or(Ok(()), Err)
    }

    fn update_event_baseline(&mut self, candidate: &AppSessionSnapshot) {
        self.calls.push("baseline");
        self.baseline = Some(candidate.clone());
    }

    fn emit(&mut self, candidate: &AppSessionSnapshot) -> Result<(), String> {
        self.calls.push("emit");
        self.emitted.push(candidate.clone());
        self.emit_error.take().map_or(Ok(()), Err)
    }
}

fn restore_with_current_production(
    current: AppSessionSnapshot,
    previous: Option<AppSessionSnapshot>,
    next_panel_value: u64,
    publication: &mut RecordingPublication,
) -> (Result<AppSessionSnapshot, String>, AppSessionSnapshot, u64) {
    let authority = GatedSnapshot::new(current);
    let next_panel = AtomicU64::new(next_panel_value);
    let result =
        restore_previous_launch_transaction(&authority, &next_panel, publication, || previous);
    let authoritative = authority.lock().unwrap().clone();
    (result, authoritative, next_panel.load(Ordering::Relaxed))
}

#[test]
fn additive_restore_retains_live_windows_and_appends_previous_in_order() {
    let current = snapshot_fixture(0x100, 2);
    let previous = snapshot_fixture(0x200, 2);
    let live = current.windows.clone();
    let restored = previous.windows.clone();
    let mut publication = RecordingPublication::default();

    let (result, authoritative, _) =
        restore_with_current_production(current, Some(previous), 50, &mut publication);
    let committed = result.expect("valid previous snapshot");

    assert_eq!(committed, authoritative);
    assert_eq!(committed.windows.len(), 4);
    assert_eq!(&committed.windows[..2], live.as_slice());
    assert_eq!(
        committed.windows[2..]
            .iter()
            .map(|window| window.window_id.as_deref())
            .collect::<Vec<_>>(),
        restored
            .iter()
            .map(|window| window.window_id.as_deref())
            .collect::<Vec<_>>()
    );
}

#[test]
fn restore_prunes_to_twelve_windows_and_never_restores_persisted_docks() {
    let current = snapshot_fixture(0x300, 1);
    let mut previous = snapshot_fixture(0x400, MAX_RESTORED_WINDOWS + 2);
    for (index, window) in previous.windows.iter_mut().enumerate() {
        window.dock = Some(dock_fixture(0x800 + index as u128));
    }
    let expected_ids = previous.windows[..MAX_RESTORED_WINDOWS]
        .iter()
        .map(|window| window.window_id.clone())
        .collect::<Vec<_>>();
    let mut publication = RecordingPublication::default();

    let (result, _, _) =
        restore_with_current_production(current, Some(previous), 50, &mut publication);
    let committed = result.expect("valid previous snapshot");

    assert_eq!(committed.windows.len(), 1 + MAX_RESTORED_WINDOWS);
    assert_eq!(
        committed.windows[1..]
            .iter()
            .map(|window| window.window_id.clone())
            .collect::<Vec<_>>(),
        expected_ids
    );
    assert!(committed.windows[1..]
        .iter()
        .all(|window| window.dock.is_none()));
}

#[test]
fn crash_diagnostic_windows_are_pruned_before_the_window_limit() {
    let current = snapshot_fixture(0x450, 1);
    let live_window_id = current.windows[0].window_id.clone();
    let normal = window_fixture(0x452);
    let normal_window_id = normal.window_id.clone();
    let previous = snapshot(vec![crash_diagnostic_window(0x451), normal]);
    let mut publication = RecordingPublication::default();

    let (result, _, _) =
        restore_with_current_production(current, Some(previous), 50, &mut publication);
    let committed = result.expect("one restorable window remains");

    assert_eq!(
        committed
            .windows
            .iter()
            .map(|window| window.window_id.clone())
            .collect::<Vec<_>>(),
        [live_window_id, normal_window_id]
    );
}

#[test]
fn fully_pruned_crash_snapshot_is_a_no_snapshot_noop() {
    let current = snapshot_fixture(0x460, 1);
    let previous = snapshot(vec![crash_diagnostic_window(0x461)]);
    let mut publication = RecordingPublication::default();

    let (result, authoritative, next_panel) =
        restore_with_current_production(current.clone(), Some(previous), 50, &mut publication);

    assert!(result.expect("no-snapshot outcome") == current);
    assert!(authoritative == current);
    assert_eq!(next_panel, 50);
    assert!(publication.calls.is_empty());
}

#[test]
fn live_window_main_surface_and_dock_identities_exclude_restored_collisions() {
    let mut current = snapshot_fixture(0x500, 1);
    let colliding_window_id = current.windows[0].window_id.clone().expect("window id");
    let live_surface_id = surface_id(&current.windows[0]).to_string();
    current.windows[0].dock = Some(dock_fixture(0x501));
    let live_dock_surface_id = current.windows[0]
        .dock
        .as_ref()
        .and_then(|dock| dock.surfaces.first())
        .map(|surface| surface.surface_id.clone())
        .expect("dock surface");

    let mut restored_window = window_fixture(0x600);
    restored_window.window_id = Some(colliding_window_id.clone());
    let Some(SessionWorkspaceLayoutSnapshot::Pane(pane)) =
        restored_window.tab_manager.workspaces[0].layout.as_mut()
    else {
        panic!("single-pane fixture")
    };
    pane.panel_ids = vec![live_surface_id.clone(), live_dock_surface_id.clone()];
    pane.selected_panel_id = Some(live_surface_id.clone());
    restored_window.tab_manager.workspaces[0].surfaces = None;
    let previous = snapshot(vec![restored_window]);
    let live_window = current.windows[0].clone();
    let mut publication = RecordingPublication::default();

    let (result, _, _) =
        restore_with_current_production(current, Some(previous), 50, &mut publication);
    let committed = result.expect("valid previous snapshot");

    assert_eq!(committed.windows.len(), 2);
    assert_eq!(committed.windows[0], live_window);
    let restored = &committed.windows[1];
    assert_ne!(
        restored.window_id.as_deref(),
        Some(colliding_window_id.as_str())
    );
    let restored_surface_ids = match restored.tab_manager.workspaces[0].layout.as_ref() {
        Some(SessionWorkspaceLayoutSnapshot::Pane(pane)) => pane.panel_ids.as_slice(),
        _ => panic!("restored pane"),
    };
    assert!(!restored_surface_ids.contains(&live_surface_id));
    assert!(!restored_surface_ids.contains(&live_dock_surface_id));
}

#[test]
fn guardrail_existing_normalizer_keeps_domain_specific_identity_semantics() {
    let mut restored = snapshot_fixture(0x700, 1);
    let old_window_id = restored.windows[0].window_id.clone();
    let old_workspace_id = restored.windows[0].tab_manager.workspaces[0]
        .workspace_id
        .clone();
    let old_pane_id = pane_id(&restored.windows[0]).to_string();
    let old_surface_id = surface_id(&restored.windows[0]).to_string();

    assert!(remint_noncanonical_identities(&mut restored));

    assert_eq!(restored.windows[0].window_id, old_window_id);
    assert_ne!(
        restored.windows[0].tab_manager.workspaces[0].workspace_id,
        old_workspace_id
    );
    assert_ne!(pane_id(&restored.windows[0]), old_pane_id);
    assert_eq!(surface_id(&restored.windows[0]), old_surface_id);
}

#[test]
fn restore_returns_aliases_that_remap_closed_workspace_history() {
    let current = snapshot_fixture(0x800, 1);
    let old_window_id = current.windows[0].window_id.clone();
    let previous = snapshot_fixture(0x800, 1);
    let old_workspace_id = previous.windows[0].tab_manager.workspaces[0]
        .workspace_id
        .clone();
    let history = Mutex::new(vec![ClosedWorkspaceSnapshot {
        window_id: old_window_id.clone(),
        workspace: previous.windows[0].tab_manager.workspaces[0].clone(),
        original_index: 0,
    }]);
    let mut publication = RecordingPublication::default();

    let (result, _, _) =
        restore_with_current_production(current, Some(previous), 50, &mut publication);
    result.expect("valid previous snapshot");
    let history = history.lock().unwrap();

    assert_ne!(history[0].window_id, old_window_id);
    assert_ne!(history[0].workspace.workspace_id, old_workspace_id);
}

#[test]
fn additive_restore_never_regresses_the_live_panel_allocator() {
    let current = initial_snapshot("surface-80");
    let previous = snapshot_fixture(0x900, 1);
    let mut publication = RecordingPublication::default();

    let (result, _, next_panel) =
        restore_with_current_production(current, Some(previous), 81, &mut publication);
    result.expect("valid previous snapshot");

    assert!(next_panel >= 81, "allocator regressed to {next_panel}");
}

#[test]
fn wrong_schema_and_empty_previous_are_no_snapshot_noops() {
    let current = snapshot_fixture(0xa00, 1);
    let mut wrong_schema = snapshot_fixture(0xb00, 1);
    wrong_schema.version = SESSION_SNAPSHOT_SCHEMA_VERSION + 1;
    let empty = snapshot(Vec::new());

    for (name, previous) in [("wrong-schema", wrong_schema), ("empty", empty)] {
        let mut publication = RecordingPublication::default();
        let (result, authoritative, next_panel) =
            restore_with_current_production(current.clone(), Some(previous), 70, &mut publication);

        assert!(
            result.expect(name) == current,
            "{name}: returned state changed"
        );
        assert!(authoritative == current, "{name}: authority changed");
        assert_eq!(next_panel, 70, "{name}");
        assert!(publication.calls.is_empty(), "{name}");
    }
}

#[test]
fn guardrail_missing_previous_keeps_every_authority_unchanged() {
    let current = snapshot_fixture(0xc00, 1);
    let mut publication = RecordingPublication::default();

    let (result, authoritative, next_panel) =
        restore_with_current_production(current.clone(), None, 70, &mut publication);

    assert_eq!(result.unwrap(), current);
    assert!(
        authoritative == current,
        "failed publication changed authority"
    );
    assert_eq!(next_panel, 70);
    assert!(publication.calls.is_empty());
}

#[test]
fn manual_restore_does_not_rewrite_current_or_previous_files() {
    let current = snapshot_fixture(0xd00, 1);
    let previous = snapshot_fixture(0xe00, 1);
    let mut publication = RecordingPublication::default();

    let (result, _, _) =
        restore_with_current_production(current, Some(previous), 70, &mut publication);
    result.expect("valid previous snapshot");

    assert!(publication.persisted.is_empty());
    assert_eq!(publication.calls, ["baseline", "emit"]);
}

#[test]
fn publication_failure_rolls_back_authority_baseline_events_and_counter() {
    let current = snapshot_fixture(0xf00, 1);
    let previous = snapshot_fixture(0x1000, 1);
    let mut publication = RecordingPublication {
        emit_error: Some("injected emit failure".to_string()),
        ..Default::default()
    };

    let (result, authoritative, next_panel) =
        restore_with_current_production(current.clone(), Some(previous), 70, &mut publication);

    assert_eq!(result, Err("injected emit failure".to_string()));
    assert!(
        authoritative == current,
        "failed publication changed authority"
    );
    assert_eq!(next_panel, 70);
    assert!(publication.baseline.is_none());
    assert!(publication.emitted.is_empty());
}

#[derive(Clone, Copy)]
enum InjectedWindowFault {
    BuildSecond,
    ShowSecond,
}

#[derive(Default)]
struct RecordingWindowEffects {
    calls: Vec<String>,
    fault: Option<InjectedWindowFault>,
}

fn exercise_current_restore_with_window_probe(
    current: AppSessionSnapshot,
    previous: AppSessionSnapshot,
    publication: &mut RecordingPublication,
    effects: &mut RecordingWindowEffects,
) -> (Result<AppSessionSnapshot, String>, AppSessionSnapshot, u64) {
    let observed_before = effects.calls.len();
    let requested_fault = effects.fault;
    let result = restore_with_current_production(current, Some(previous), 70, publication);
    assert_eq!(effects.calls.len(), observed_before);
    let _ = requested_fault;
    result
}

#[test]
fn batch_builds_all_windows_hidden_then_shows_in_order_without_focus() {
    let current = snapshot_fixture(0x1100, 1);
    let previous = snapshot_fixture(0x1200, 2);
    let restored_ids = previous
        .windows
        .iter()
        .map(|window| window.window_id.clone().expect("window id"))
        .collect::<Vec<_>>();
    let mut publication = RecordingPublication::default();
    let mut effects = RecordingWindowEffects::default();

    let (result, _, _) = exercise_current_restore_with_window_probe(
        current,
        previous,
        &mut publication,
        &mut effects,
    );
    result.expect("valid previous snapshot");

    assert_eq!(
        effects.calls,
        [
            format!("build-hidden:{}", restored_ids[0]),
            format!("build-hidden:{}", restored_ids[1]),
            format!("show-unfocused:{}", restored_ids[0]),
            format!("show-unfocused:{}", restored_ids[1]),
        ]
    );
}

#[test]
fn batch_build_or_show_failure_closes_staged_windows_and_leaks_nothing() {
    for fault in [
        InjectedWindowFault::BuildSecond,
        InjectedWindowFault::ShowSecond,
    ] {
        let current = snapshot_fixture(0x1300, 1);
        let previous = snapshot_fixture(0x1400, 2);
        let mut publication = RecordingPublication::default();
        let mut effects = RecordingWindowEffects {
            calls: Vec::new(),
            fault: Some(fault),
        };

        let (result, authoritative, next_panel) = exercise_current_restore_with_window_probe(
            current.clone(),
            previous,
            &mut publication,
            &mut effects,
        );

        assert!(result.is_err());
        assert_eq!(authoritative, current);
        assert_eq!(next_panel, 70);
        assert!(publication.calls.is_empty());
        assert!(effects.calls.iter().any(|call| call.starts_with("close:")));
    }
}

#[test]
fn product_activates_first_restored_window_but_control_never_activates() {
    for should_activate in [false, true] {
        let current = snapshot_fixture(0x1500, 1);
        let previous = snapshot_fixture(0x1600, 2);
        let first_restored = previous.windows[0].window_id.clone().expect("window id");
        let mut publication = RecordingPublication::default();
        let mut effects = RecordingWindowEffects::default();

        let (result, _, _) = exercise_current_restore_with_window_probe(
            current,
            previous,
            &mut publication,
            &mut effects,
        );
        result.expect("valid previous snapshot");

        let focus_calls = effects
            .calls
            .iter()
            .filter(|call| call.starts_with("focus:"))
            .cloned()
            .collect::<Vec<_>>();
        let expected = should_activate
            .then(|| vec![format!("focus:{first_restored}")])
            .unwrap_or_default();
        assert_eq!(focus_calls, expected, "should_activate={should_activate}");
    }
}
