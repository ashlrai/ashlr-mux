//! Durable model and OS ordering for auxiliary-window lifecycle operations.

use super::*;

#[derive(Debug, Clone, PartialEq)]
enum RegisterWindowOutcomeRed {
    Unchanged(AppSessionSnapshot),
    Registered(AppSessionSnapshot),
}

#[derive(Debug, Clone, PartialEq)]
struct RemovedWindowLeaseRed {
    before: AppSessionSnapshot,
    removed: SessionWindowSnapshot,
}

#[derive(Debug, Clone, PartialEq)]
enum UnregisterWindowOutcomeRed {
    Unchanged(AppSessionSnapshot),
    Removed {
        snapshot: AppSessionSnapshot,
        lease: RemovedWindowLeaseRed,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum MoveWindowErrorRed {
    WorkspaceNotFound,
    Publication(String),
}

struct RecordingPublication {
    calls: Vec<&'static str>,
    persist_error: Option<String>,
    baseline: AppSessionSnapshot,
    events: Vec<AppSessionSnapshot>,
}

impl RecordingPublication {
    fn new(snapshot: &AppSessionSnapshot) -> Self {
        Self {
            calls: Vec::new(),
            persist_error: None,
            baseline: snapshot.clone(),
            events: Vec::new(),
        }
    }
}

impl SnapshotPublicationOperations for RecordingPublication {
    fn persist(&mut self, _candidate: &AppSessionSnapshot) -> Result<(), String> {
        self.calls.push("persist");
        self.persist_error.take().map_or(Ok(()), Err)
    }

    fn update_event_baseline(&mut self, candidate: &AppSessionSnapshot) {
        self.calls.push("baseline");
        self.baseline = candidate.clone();
    }

    fn emit(&mut self, candidate: &AppSessionSnapshot) -> Result<(), String> {
        self.calls.push("emit");
        self.events.push(candidate.clone());
        Ok(())
    }
}

fn current(authority: &GatedSnapshot) -> AppSessionSnapshot {
    authority.lock().unwrap().clone()
}

fn aux_window(window_id: &str, panel_id: &str) -> SessionWindowSnapshot {
    SessionWindowSnapshot {
        window_id: Some(window_id.to_string()),
        selected_workspace_id: None,
        dock: None,
        tab_manager: SessionTabManagerSnapshot {
            selected_workspace_index: Some(0),
            workspaces: vec![session_ops::fresh_terminal_workspace(panel_id)],
            workspace_groups: None,
        },
    }
}

fn register_window_red(
    authority: &GatedSnapshot,
    next_panel: &AtomicU64,
    publication: &mut impl SnapshotPublicationOperations,
    window_id: &str,
) -> Result<RegisterWindowOutcomeRed, String> {
    let _gate = authority.lock_gate();
    let before = current(authority);
    if window_id.trim().is_empty()
        || before
            .windows
            .iter()
            .any(|window| window.window_id.as_deref() == Some(window_id))
    {
        return Ok(RegisterWindowOutcomeRed::Unchanged(before));
    }
    let panel_id = format!("surface-{}", next_panel.load(Ordering::Relaxed));
    let mut candidate = before.clone();
    candidate.windows.push(aux_window(window_id, &panel_id));
    ensure_workspace_ids(&mut candidate);
    ensure_pane_ids(&mut candidate);
    let committed =
        publish_snapshot_transaction(authority, Some(&before), &candidate, publication)?;
    next_panel.fetch_add(1, Ordering::Relaxed);
    Ok(RegisterWindowOutcomeRed::Registered(committed))
}

fn unregister_window_red(
    authority: &GatedSnapshot,
    publication: &mut impl SnapshotPublicationOperations,
    window_id: &str,
) -> Result<UnregisterWindowOutcomeRed, String> {
    let _gate = authority.lock_gate();
    let before = current(authority);
    let Some(index) = before
        .windows
        .iter()
        .position(|window| window.window_id.as_deref() == Some(window_id))
        .filter(|index| *index != 0)
    else {
        return Ok(UnregisterWindowOutcomeRed::Unchanged(before));
    };
    let mut candidate = before.clone();
    let removed = candidate.windows.remove(index);
    let committed =
        publish_snapshot_transaction(authority, Some(&before), &candidate, publication)?;
    Ok(UnregisterWindowOutcomeRed::Removed {
        snapshot: committed,
        lease: RemovedWindowLeaseRed { before, removed },
    })
}

fn restore_removed_window_red(
    authority: &GatedSnapshot,
    publication: &mut impl SnapshotPublicationOperations,
    lease: &RemovedWindowLeaseRed,
) -> Result<AppSessionSnapshot, String> {
    let removed = current(authority);
    publish_snapshot_transaction(authority, Some(&removed), &lease.before, publication)
}

fn move_workspace_red(
    authority: &GatedSnapshot,
    next_panel: &AtomicU64,
    publication: &mut impl SnapshotPublicationOperations,
    workspace_id: &str,
    target_window_id: &str,
    focus: bool,
) -> Result<AppSessionSnapshot, MoveWindowErrorRed> {
    let _gate = authority.lock_gate();
    let before = current(authority);
    if !before.windows.iter().any(|window| {
        window
            .tab_manager
            .workspaces
            .iter()
            .any(|workspace| workspace.workspace_id.as_deref() == Some(workspace_id))
    }) {
        return Err(MoveWindowErrorRed::WorkspaceNotFound);
    }
    let mut candidate = before.clone();
    let mut used = 0u64;
    if !candidate
        .windows
        .iter()
        .any(|window| window.window_id.as_deref() == Some(target_window_id))
    {
        let panel_id = format!("surface-{}", next_panel.load(Ordering::Relaxed) + used);
        used += 1;
        candidate
            .windows
            .push(aux_window(target_window_id, &panel_id));
    }
    let bootstrap_panel_id = format!("surface-{}", next_panel.load(Ordering::Relaxed) + used);
    used += 1;
    session_ops::move_workspace_to_window(
        &mut candidate,
        workspace_id,
        target_window_id,
        session_ops::fresh_terminal_workspace(&bootstrap_panel_id),
        focus,
    )
    .map_err(|error| match error {
        session_ops::MoveWorkspaceToWindowError::WorkspaceNotFound => {
            MoveWindowErrorRed::WorkspaceNotFound
        }
        session_ops::MoveWorkspaceToWindowError::WindowNotFound => unreachable!("registered"),
    })?;
    ensure_workspace_ids(&mut candidate);
    ensure_pane_ids(&mut candidate);
    let committed = publish_snapshot_transaction(authority, Some(&before), &candidate, publication)
        .map_err(MoveWindowErrorRed::Publication)?;
    next_panel.fetch_add(used, Ordering::Relaxed);
    Ok(committed)
}

fn initial() -> AppSessionSnapshot {
    let mut snapshot = initial_snapshot("surface-1");
    snapshot.windows[0].window_id = Some("main".into());
    ensure_workspace_ids(&mut snapshot);
    ensure_pane_ids(&mut snapshot);
    snapshot
}

#[test]
fn register_unregister_exact_outcomes_and_missing_or_main_noops() {
    let before = initial();
    let authority = GatedSnapshot::new(before.clone());
    let next_panel = AtomicU64::new(2);
    let mut publication = RecordingPublication::new(&before);

    for window_id in ["", "main"] {
        assert_eq!(
            register_window_red(&authority, &next_panel, &mut publication, window_id).unwrap(),
            RegisterWindowOutcomeRed::Unchanged(before.clone())
        );
        assert!(publication.calls.is_empty());
    }
    for window_id in ["missing", "main"] {
        assert_eq!(
            unregister_window_red(&authority, &mut publication, window_id).unwrap(),
            UnregisterWindowOutcomeRed::Unchanged(before.clone())
        );
        assert!(publication.calls.is_empty());
    }

    let RegisterWindowOutcomeRed::Registered(registered) =
        register_window_red(&authority, &next_panel, &mut publication, "aux").unwrap()
    else {
        panic!("registered")
    };
    assert_eq!(publication.calls, ["persist", "baseline", "emit"]);
    assert_eq!(next_panel.load(Ordering::Relaxed), 3);
    publication.calls.clear();
    let UnregisterWindowOutcomeRed::Removed { snapshot, lease } =
        unregister_window_red(&authority, &mut publication, "aux").unwrap()
    else {
        panic!("removed")
    };
    assert_eq!(lease.before, registered);
    assert_eq!(lease.removed.window_id.as_deref(), Some("aux"));
    assert_eq!(snapshot, before);
    assert_eq!(publication.calls, ["persist", "baseline", "emit"]);
}

#[test]
fn production_missing_main_repairs_first_window_without_allocating() {
    let mut before = initial();
    before.windows[0].window_id = None;
    let authority = GatedSnapshot::new(before.clone());
    let next_panel = AtomicU64::new(8);
    let mut publication = RecordingPublication::new(&before);

    let RegisterWindowOutcome::Registered(committed) =
        transact_register_window(&authority, &next_panel, &mut publication, "main").unwrap()
    else {
        panic!("main repaired")
    };
    assert_eq!(committed.windows[0].window_id.as_deref(), Some("main"));
    assert_eq!(publication.calls, ["persist", "baseline", "emit"]);
    assert_eq!(next_panel.load(Ordering::Relaxed), 8);
}

#[test]
fn production_removed_window_restore_preserves_intervening_session_changes() {
    let mut before = initial();
    before.windows.push(aux_window("aux", "surface-2"));
    ensure_workspace_ids(&mut before);
    ensure_pane_ids(&mut before);
    let authority = GatedSnapshot::new(before.clone());
    let mut removal_publication = RecordingPublication::new(&before);
    let UnregisterWindowOutcome::Removed { snapshot, lease } =
        transact_unregister_window(&authority, &mut removal_publication, "aux").unwrap()
    else {
        panic!("removed")
    };

    let mut intervening = snapshot.clone();
    intervening.windows[0].tab_manager.workspaces[0].custom_title = Some("kept".into());
    let mut intervening_publication = RecordingPublication::new(&snapshot);
    publish_snapshot_transaction(
        &authority,
        Some(&snapshot),
        &intervening,
        &mut intervening_publication,
    )
    .unwrap();

    let mut restore_publication = RecordingPublication::new(&intervening);
    let restored =
        transact_restore_removed_window(&authority, &mut restore_publication, &lease).unwrap();
    assert_eq!(
        restored.windows[0].tab_manager.workspaces[0]
            .custom_title
            .as_deref(),
        Some("kept")
    );
    assert_eq!(restored.windows[1], before.windows[1]);
}

#[test]
fn register_unregister_and_move_persist_failures_preserve_all_authority() {
    for operation in ["register", "unregister", "move"] {
        let mut before = initial();
        if operation == "unregister" {
            before.windows.push(aux_window("aux", "surface-2"));
        }
        let workspace_id = before.windows[0].tab_manager.workspaces[0]
            .workspace_id
            .clone()
            .unwrap();
        let authority = GatedSnapshot::new(before.clone());
        let next_panel = AtomicU64::new(9);
        let mut publication = RecordingPublication::new(&before);
        publication.persist_error = Some(format!("injected {operation} failure"));
        let failed = match operation {
            "register" => {
                register_window_red(&authority, &next_panel, &mut publication, "aux").is_err()
            }
            "unregister" => unregister_window_red(&authority, &mut publication, "aux").is_err(),
            _ => move_workspace_red(
                &authority,
                &next_panel,
                &mut publication,
                &workspace_id,
                "aux",
                true,
            )
            .is_err(),
        };
        assert!(failed, "{operation}");
        assert_eq!(publication.calls, ["persist"], "{operation}");
        assert_eq!(*authority.lock().unwrap(), before, "{operation}");
        assert_eq!(publication.baseline, before, "{operation}");
        assert!(publication.events.is_empty(), "{operation}");
        assert_eq!(next_panel.load(Ordering::Relaxed), 9, "{operation}");
    }
}

#[test]
fn missing_target_registration_and_move_share_one_publication_and_concurrent_ids() {
    let before = initial();
    let workspace_id = before.windows[0].tab_manager.workspaces[0]
        .workspace_id
        .clone()
        .unwrap();
    let authority = GatedSnapshot::new(before.clone());
    let next_panel = AtomicU64::new(2);
    let mut publication = RecordingPublication::new(&before);
    assert_eq!(
        move_workspace_red(
            &authority,
            &next_panel,
            &mut publication,
            "missing-workspace",
            "aux",
            true,
        ),
        Err(MoveWindowErrorRed::WorkspaceNotFound)
    );
    assert!(publication.calls.is_empty());
    assert_eq!(next_panel.load(Ordering::Relaxed), 2);
    let moved = move_workspace_red(
        &authority,
        &next_panel,
        &mut publication,
        &workspace_id,
        "aux",
        true,
    )
    .unwrap();
    assert_eq!(publication.calls, ["persist", "baseline", "emit"]);
    assert_eq!(publication.events, [moved.clone()]);
    assert_eq!(next_panel.load(Ordering::Relaxed), 4);
    assert!(moved
        .windows
        .iter()
        .find(|window| window.window_id.as_deref() == Some("aux"))
        .unwrap()
        .tab_manager
        .workspaces
        .iter()
        .any(|workspace| workspace.workspace_id.as_deref() == Some(workspace_id.as_str())));

    let authority = Arc::new(GatedSnapshot::new(before.clone()));
    let next_panel = Arc::new(AtomicU64::new(2));
    std::thread::scope(|scope| {
        for label in ["aux-a", "aux-b"] {
            let authority = Arc::clone(&authority);
            let next_panel = Arc::clone(&next_panel);
            let baseline = before.clone();
            scope.spawn(move || {
                let mut publication = RecordingPublication::new(&baseline);
                register_window_red(&authority, &next_panel, &mut publication, label).unwrap();
            });
        }
    });
    assert_eq!(next_panel.load(Ordering::Relaxed), 4);
    let snapshot = authority.lock().unwrap();
    let mut panel_ids = snapshot
        .windows
        .iter()
        .skip(1)
        .flat_map(|window| &window.tab_manager.workspaces)
        .filter_map(|workspace| workspace.layout.as_ref())
        .flat_map(|layout| match layout {
            SessionWorkspaceLayoutSnapshot::Pane(pane) => pane.panel_ids.clone(),
            SessionWorkspaceLayoutSnapshot::Split(_) => Vec::new(),
        })
        .collect::<Vec<_>>();
    panel_ids.sort();
    assert_eq!(panel_ids, ["surface-2", "surface-3"]);
}

trait WindowEffectsRed {
    fn build_hidden(&mut self, label: &str) -> Result<(), String>;
    fn show(&mut self, label: &str);
    fn hide(&mut self, label: &str);
    fn close(&mut self, label: &str) -> Result<(), String>;
}

#[derive(Default)]
struct RecordingWindowEffects {
    calls: Vec<String>,
    close_error: Option<String>,
}

impl WindowEffectsRed for RecordingWindowEffects {
    fn build_hidden(&mut self, label: &str) -> Result<(), String> {
        self.calls.push(format!("build-hidden:{label}"));
        Ok(())
    }
    fn show(&mut self, label: &str) {
        self.calls.push(format!("show:{label}"));
    }
    fn hide(&mut self, label: &str) {
        self.calls.push(format!("hide:{label}"));
    }
    fn close(&mut self, label: &str) -> Result<(), String> {
        self.calls.push(format!("close:{label}"));
        self.close_error.take().map_or(Ok(()), Err)
    }
}

fn window_new_red(
    authority: &GatedSnapshot,
    next_panel: &AtomicU64,
    publication: &mut impl SnapshotPublicationOperations,
    effects: &mut impl WindowEffectsRed,
    label: &str,
) -> Result<(), String> {
    effects.build_hidden(label)?;
    match register_window_red(authority, next_panel, publication, label) {
        Ok(_) => {
            effects.show(label);
            Ok(())
        }
        Err(message) => {
            let _ = effects.close(label);
            Err(message)
        }
    }
}

fn window_close_red(
    authority: &GatedSnapshot,
    publication: &mut impl SnapshotPublicationOperations,
    effects: &mut impl WindowEffectsRed,
    label: &str,
) -> Result<(), String> {
    if label == "main" {
        return effects.close(label);
    }
    effects.hide(label);
    let outcome = match unregister_window_red(authority, publication, label) {
        Ok(outcome) => outcome,
        Err(message) => {
            effects.show(label);
            return Err(message);
        }
    };
    let UnregisterWindowOutcomeRed::Removed { lease, .. } = outcome else {
        effects.show(label);
        return Ok(());
    };
    if let Err(message) = effects.close(label) {
        restore_removed_window_red(authority, publication, &lease)?;
        effects.show(label);
        return Err(message);
    }
    Ok(())
}

#[test]
fn window_effects_order_and_fault_compensation_preserve_os_and_model() {
    let before = initial();
    let authority = GatedSnapshot::new(before.clone());
    let next_panel = AtomicU64::new(2);
    let mut publication = RecordingPublication::new(&before);
    let mut effects = RecordingWindowEffects::default();
    window_new_red(
        &authority,
        &next_panel,
        &mut publication,
        &mut effects,
        "aux",
    )
    .unwrap();
    assert_eq!(effects.calls, ["build-hidden:aux", "show:aux"]);
    assert_eq!(publication.calls, ["persist", "baseline", "emit"]);
    let with_aux = authority.lock().unwrap().clone();
    publication.calls.clear();
    effects.calls.clear();
    window_close_red(&authority, &mut publication, &mut effects, "aux").unwrap();
    assert_eq!(effects.calls, ["hide:aux", "close:aux"]);
    assert_eq!(*authority.lock().unwrap(), before);

    let authority = GatedSnapshot::new(before.clone());
    let next_panel = AtomicU64::new(2);
    let mut publication = RecordingPublication::new(&before);
    publication.persist_error = Some("new persist failed".into());
    let mut effects = RecordingWindowEffects::default();
    assert!(window_new_red(
        &authority,
        &next_panel,
        &mut publication,
        &mut effects,
        "aux"
    )
    .is_err());
    assert_eq!(effects.calls, ["build-hidden:aux", "close:aux"]);
    assert_eq!(*authority.lock().unwrap(), before);

    let authority = GatedSnapshot::new(with_aux.clone());
    let mut publication = RecordingPublication::new(&with_aux);
    publication.persist_error = Some("close persist failed".into());
    let mut effects = RecordingWindowEffects::default();
    assert!(window_close_red(&authority, &mut publication, &mut effects, "aux").is_err());
    assert_eq!(effects.calls, ["hide:aux", "show:aux"]);
    assert_eq!(*authority.lock().unwrap(), with_aux);

    let authority = GatedSnapshot::new(with_aux.clone());
    let mut publication = RecordingPublication::new(&with_aux);
    let mut effects = RecordingWindowEffects {
        calls: Vec::new(),
        close_error: Some("os close failed".into()),
    };
    assert_eq!(
        window_close_red(&authority, &mut publication, &mut effects, "aux"),
        Err("os close failed".into())
    );
    assert_eq!(effects.calls, ["hide:aux", "close:aux", "show:aux"]);
    assert_eq!(*authority.lock().unwrap(), with_aux);
    assert_eq!(
        publication.calls,
        ["persist", "baseline", "emit", "persist", "baseline", "emit"]
    );
}

fn function_source<'a>(source: &'a str, signature: &str) -> &'a str {
    let start = source
        .find(signature)
        .unwrap_or_else(|| panic!("missing {signature}"));
    let tail = &source[start..];
    let body_start = tail.find('{').unwrap();
    let mut depth = 0;
    for (offset, character) in tail[body_start..].char_indices() {
        match character {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return &tail[..body_start + offset + 1];
                }
            }
            _ => {}
        }
    }
    panic!("unterminated {signature}")
}

#[test]
fn production_helpers_window_commands_and_socket_use_fallible_compensating_paths() {
    let session = include_str!("../session.rs");
    for (signature, result) in [
        (
            "pub(crate) fn register_window_for_control(",
            "Result<RegisterWindowOutcome, String>",
        ),
        (
            "pub(crate) fn unregister_window_for_control(",
            "Result<UnregisterWindowOutcome, String>",
        ),
        (
            "pub(crate) fn move_workspace_to_window_for_control(",
            "Result<AppSessionSnapshot, MoveWorkspaceToWindowControlError>",
        ),
    ] {
        let body = function_source(session, signature);
        assert!(body.contains(result), "{signature}");
        assert!(body.contains("transact_"), "{signature}");
        assert!(!body.contains("snapshot.lock()"), "{signature}");
        assert!(!body.contains("notify_session_changed("), "{signature}");
        assert!(!body.contains("next_panel.fetch_add"), "{signature}");
    }
    let mover = function_source(
        session,
        "pub(crate) fn move_workspace_to_window_for_control(",
    );
    assert!(mover.contains("register") || mover.contains("ensure_window"));

    let window = include_str!("../window.rs");
    let new = function_source(window, "pub async fn window_new(");
    assert!(new.contains("Result<String, String>"));
    // The build/register/show fault-compensation flow moved into the shared
    // create_window_for_label (also used by the control socket's
    // window.create); window_new must still delegate to it.
    assert!(new.contains("create_window_for_label("));
    let create = function_source(window, "fn create_window_for_label(");
    assert!(create.contains("register_window_for_control("));
    assert!(create.contains("build_hidden_window("));
    assert!(create.contains("show("));
    assert!(create.contains("close("));
    let build = function_source(window, "fn build_hidden_window(");
    assert!(build.contains("visible = false"));
    assert!(build.contains("build()"));
    assert!(
        create.find("build_hidden_window(").unwrap()
            < create.find("register_window_for_control(").unwrap()
    );
    assert!(create.find("register_window_for_control(").unwrap() < create.rfind("show(").unwrap());

    let close = function_source(window, "pub fn window_close(");
    assert!(close.contains("Result<(), String>"));
    assert!(close.contains("hide("));
    assert!(close.contains("unregister_window_for_control("));
    assert!(close.contains("restore") || close.contains("rollback"));
    assert!(close.contains("show("));
    assert!(close.find("hide(").unwrap() < close.find("unregister_window_for_control(").unwrap());
    assert!(close.find("unregister_window_for_control(").unwrap() < close.find(".close(").unwrap());

    let socket = include_str!("../control_socket.rs");
    let route = function_source(socket, "fn workspace_move_to_window(");
    assert_eq!(
        route
            .matches("move_workspace_to_window_for_control(")
            .count(),
        1
    );
    assert!(!route.contains("register_window_for_control("));
    assert!(!route.contains("set_focus("));
    assert!(route.contains("MoveWorkspaceToWindowControlError::NotFound"));
    assert!(route.contains("MoveWorkspaceToWindowControlError::Publication"));
    assert!(route.contains("\"not_found\""));
    assert!(route.contains("\"internal\""));
}
