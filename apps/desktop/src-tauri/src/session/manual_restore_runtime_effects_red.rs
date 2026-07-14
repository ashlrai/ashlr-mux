//! Production-connected RED contract for manual-restore runtime effects.

use super::*;
use std::sync::{Arc, Mutex as StdMutex};

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

fn ledger_entries(ledger: &Arc<StdMutex<Vec<String>>>) -> Vec<String> {
    ledger.lock().expect("ledger mutex poisoned").clone()
}

struct OrderedPublication {
    ledger: Arc<StdMutex<Vec<String>>>,
    emit_error: Option<String>,
}

impl SnapshotPublicationOperations for OrderedPublication {
    fn persist(&mut self, _candidate: &AppSessionSnapshot) -> Result<(), String> {
        self.ledger.lock().unwrap().push("persist".into());
        Ok(())
    }

    fn update_event_baseline(&mut self, _candidate: &AppSessionSnapshot) {
        self.ledger.lock().unwrap().push("baseline".into());
    }

    fn emit(&mut self, _candidate: &AppSessionSnapshot) -> Result<(), String> {
        self.ledger.lock().unwrap().push("snapshot-emit".into());
        self.emit_error.take().map_or(Ok(()), Err)
    }
}

struct RecordingEffects {
    ledger: Arc<StdMutex<Vec<String>>>,
    fail_build: Option<String>,
    fail_show: Option<String>,
    fail_close: Option<String>,
    fail_activate: bool,
}

impl RecordingEffects {
    fn push(&self, entry: String) {
        self.ledger.lock().unwrap().push(entry);
    }
}

impl ManualRestoreEffects for RecordingEffects {
    fn build_hidden(&mut self, window: &SessionWindowSnapshot) -> Result<(), String> {
        let window_id = window.window_id.as_deref().expect("restored window id");
        self.push(format!("build-hidden:{window_id}"));
        if self.fail_build.as_deref() == Some(window_id) {
            Err(format!("build failed for {window_id}"))
        } else {
            Ok(())
        }
    }

    fn show_unfocused(&mut self, window_id: &str) -> Result<(), String> {
        self.push(format!("show-unfocused:{window_id}"));
        if self.fail_show.as_deref() == Some(window_id) {
            Err(format!("show failed for {window_id}"))
        } else {
            Ok(())
        }
    }

    fn close(&mut self, window_id: &str) -> Result<(), String> {
        self.push(format!("close:{window_id}"));
        if self.fail_close.as_deref() == Some(window_id) {
            Err(format!("close failed for {window_id}"))
        } else {
            Ok(())
        }
    }

    fn activate(&mut self, window_id: &str) -> Result<(), String> {
        self.push(format!("activate:{window_id}"));
        if self.fail_activate {
            Err(format!("activation failed for {window_id}"))
        } else {
            Ok(())
        }
    }

    fn record_window_created(&mut self, window: &SessionWindowSnapshot) {
        self.push(format!(
            "window.created:{}",
            window.window_id.as_deref().expect("restored window id")
        ));
    }
}

fn harness(
    current: AppSessionSnapshot,
    emit_error: Option<String>,
) -> (
    GatedSnapshot,
    AtomicU64,
    OrderedPublication,
    RecordingEffects,
    Arc<StdMutex<Vec<String>>>,
) {
    let ledger = Arc::new(StdMutex::new(Vec::new()));
    (
        GatedSnapshot::new(current),
        AtomicU64::new(2),
        OrderedPublication {
            ledger: Arc::clone(&ledger),
            emit_error,
        },
        RecordingEffects {
            ledger: Arc::clone(&ledger),
            fail_build: None,
            fail_show: None,
            fail_close: None,
            fail_activate: false,
        },
        ledger,
    )
}

#[test]
fn control_restore_builds_and_shows_sequentially_then_publishes_exact_created_events() {
    let first = window_fixture(0x10);
    let second = window_fixture(0x20);
    let first_id = first.window_id.clone().unwrap();
    let second_id = second.window_id.clone().unwrap();
    let previous = snapshot(vec![first, second]);
    let current = snapshot(vec![window_fixture(1)]);
    let (authority, next_panel, mut publication, mut effects, ledger) = harness(current, None);

    let outcome = restore_previous_launch_transaction_with_effects(
        &authority,
        &next_panel,
        &mut publication,
        &mut effects,
        ManualRestoreRoute::Control,
        || Some(previous),
    )
    .expect("restore succeeds");

    assert!(outcome.restored);
    assert_eq!(
        ledger_entries(&ledger),
        [
            format!("build-hidden:{first_id}"),
            format!("show-unfocused:{first_id}"),
            format!("build-hidden:{second_id}"),
            format!("show-unfocused:{second_id}"),
            "baseline".into(),
            "snapshot-emit".into(),
            format!("window.created:{first_id}"),
            format!("window.created:{second_id}"),
        ]
    );
}

#[test]
fn product_restore_activates_only_the_first_restored_window_after_created_events() {
    let first = window_fixture(0x30);
    let second = window_fixture(0x40);
    let first_id = first.window_id.clone().unwrap();
    let second_id = second.window_id.clone().unwrap();
    let previous = snapshot(vec![first, second]);
    let current = snapshot(vec![window_fixture(2)]);
    let (authority, next_panel, mut publication, mut effects, ledger) = harness(current, None);

    restore_previous_launch_transaction_with_effects(
        &authority,
        &next_panel,
        &mut publication,
        &mut effects,
        ManualRestoreRoute::Product,
        || Some(previous),
    )
    .expect("restore succeeds");

    assert_eq!(
        ledger_entries(&ledger),
        [
            format!("build-hidden:{first_id}"),
            format!("show-unfocused:{first_id}"),
            format!("build-hidden:{second_id}"),
            format!("show-unfocused:{second_id}"),
            "baseline".into(),
            "snapshot-emit".into(),
            format!("window.created:{first_id}"),
            format!("window.created:{second_id}"),
            format!("activate:{first_id}"),
        ]
    );
}

#[test]
fn product_activation_failure_does_not_fail_an_otherwise_completed_restore() {
    let restored = window_fixture(0x45);
    let restored_id = restored.window_id.clone().unwrap();
    let previous = snapshot(vec![restored]);
    let current = snapshot(vec![window_fixture(0x44)]);
    let (authority, next_panel, mut publication, mut effects, ledger) = harness(current, None);
    effects.fail_activate = true;

    let outcome = restore_previous_launch_transaction_with_effects(
        &authority,
        &next_panel,
        &mut publication,
        &mut effects,
        ManualRestoreRoute::Product,
        || Some(previous),
    )
    .expect("canonical activation is best-effort");

    assert!(outcome.restored);
    assert_eq!(
        ledger_entries(&ledger).last(),
        Some(&format!("activate:{restored_id}"))
    );
}

#[test]
fn second_build_failure_closes_the_first_window_and_preserves_all_authority() {
    let first = window_fixture(0x50);
    let second = window_fixture(0x60);
    let first_id = first.window_id.clone().unwrap();
    let second_id = second.window_id.clone().unwrap();
    let previous = snapshot(vec![first, second]);
    let current = snapshot(vec![window_fixture(3)]);
    let (authority, next_panel, mut publication, mut effects, ledger) =
        harness(current.clone(), None);
    effects.fail_build = Some(second_id.clone());

    assert_eq!(
        restore_previous_launch_transaction_with_effects(
            &authority,
            &next_panel,
            &mut publication,
            &mut effects,
            ManualRestoreRoute::Control,
            || Some(previous),
        ),
        Err(format!("build failed for {second_id}"))
    );
    assert_eq!(*authority.lock().unwrap(), current);
    assert_eq!(next_panel.load(Ordering::Relaxed), 2);
    assert_eq!(
        ledger_entries(&ledger),
        [
            format!("build-hidden:{first_id}"),
            format!("show-unfocused:{first_id}"),
            format!("build-hidden:{second_id}"),
            format!("close:{first_id}"),
        ]
    );
}

#[test]
fn show_failure_closes_every_built_window_in_reverse_order() {
    let first = window_fixture(0x70);
    let second = window_fixture(0x80);
    let first_id = first.window_id.clone().unwrap();
    let second_id = second.window_id.clone().unwrap();
    let previous = snapshot(vec![first, second]);
    let current = snapshot(vec![window_fixture(4)]);
    let (authority, next_panel, mut publication, mut effects, ledger) =
        harness(current.clone(), None);
    effects.fail_show = Some(second_id.clone());

    assert!(restore_previous_launch_transaction_with_effects(
        &authority,
        &next_panel,
        &mut publication,
        &mut effects,
        ManualRestoreRoute::Control,
        || Some(previous),
    )
    .is_err());
    assert_eq!(*authority.lock().unwrap(), current);
    assert_eq!(
        ledger_entries(&ledger)[4..],
        [format!("close:{second_id}"), format!("close:{first_id}")]
    );
}

#[test]
fn publication_failure_rolls_back_model_and_runtime_and_reports_cleanup_failure() {
    let first = window_fixture(0x90);
    let second = window_fixture(0xa0);
    let first_id = first.window_id.clone().unwrap();
    let second_id = second.window_id.clone().unwrap();
    let previous = snapshot(vec![first, second]);
    let current = snapshot(vec![window_fixture(5)]);
    let (authority, next_panel, mut publication, mut effects, ledger) =
        harness(current.clone(), Some("publication failed".into()));
    effects.fail_close = Some(first_id.clone());

    let error = restore_previous_launch_transaction_with_effects(
        &authority,
        &next_panel,
        &mut publication,
        &mut effects,
        ManualRestoreRoute::Control,
        || Some(previous),
    )
    .expect_err("publication failure is returned");

    assert_eq!(
        error,
        format!("publication failed; compensation failed: close failed for {first_id}")
    );
    assert_eq!(*authority.lock().unwrap(), current);
    assert_eq!(next_panel.load(Ordering::Relaxed), 2);
    let entries = ledger_entries(&ledger);
    assert_eq!(entries[entries.len() - 2], format!("close:{second_id}"));
    assert_eq!(entries[entries.len() - 1], format!("close:{first_id}"));
    assert!(!entries
        .iter()
        .any(|entry| entry.starts_with("window.created:")));
    assert!(!entries.iter().any(|entry| entry.starts_with("activate:")));
}

#[test]
fn unavailable_restore_has_no_runtime_or_publication_effects() {
    let current = snapshot(vec![window_fixture(6)]);
    let (authority, next_panel, mut publication, mut effects, ledger) =
        harness(current.clone(), None);

    let outcome = restore_previous_launch_transaction_with_effects(
        &authority,
        &next_panel,
        &mut publication,
        &mut effects,
        ManualRestoreRoute::Product,
        || None,
    )
    .expect("missing previous snapshot is a no-op");

    assert!(!outcome.restored);
    assert_eq!(outcome.snapshot, current);
    assert!(ledger_entries(&ledger).is_empty());
}

fn function_source<'a>(source: &'a str, signature: &str) -> &'a str {
    let start = source.find(signature).expect("function signature");
    let tail = &source[start..];
    let body_start = tail.find('{').expect("function body");
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
    panic!("unterminated function")
}

#[test]
fn manual_event_suppression_does_not_change_shared_deferred_reseed_publications() {
    let source = include_str!("../session.rs");
    let shared = function_source(source, "fn with_deferred_next_panel_reseed(");
    assert!(shared.contains("derived_events: DerivedEventPolicy::Record"));

    let manual = function_source(source, "fn for_manual_restore(");
    assert!(manual.contains("derived_events: DerivedEventPolicy::Suppress"));
    assert!(manual.contains("reseed_next_panel: false"));

    let route = function_source(source, "fn restore_previous_launch_for_route(");
    assert!(route.contains("ProductionSnapshotPublicationOperations::for_manual_restore("));
    assert!(!route.contains("with_deferred_next_panel_reseed("));
}
