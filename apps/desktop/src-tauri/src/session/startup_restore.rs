use super::*;

use std::sync::atomic::Ordering;

fn running_under_automated_tests(environment: &HashMap<String, String>) -> bool {
    environment.get("CMUX_UI_TEST_MODE").map(String::as_str) == Some("1")
        || environment
            .keys()
            .any(|key| key.starts_with("CMUX_UI_TEST_"))
        || [
            "XCTestConfigurationFilePath",
            "XCTestBundlePath",
            "XCTestSessionIdentifier",
            "XCInjectBundle",
            "XCInjectBundleInto",
        ]
        .iter()
        .any(|key| environment.contains_key(*key))
        || environment
            .get("DYLD_INSERT_LIBRARIES")
            .is_some_and(|value| value.contains("libXCTest"))
}

fn should_attempt_startup_restore(
    arguments: &[String],
    environment: &HashMap<String, String>,
) -> bool {
    if environment
        .get("CMUX_DISABLE_SESSION_RESTORE")
        .is_some_and(|value| value == "1")
        || running_under_automated_tests(environment)
    {
        return false;
    }
    arguments
        .iter()
        .skip(1)
        .all(|argument| argument.starts_with("-psn_"))
}

fn prepare_startup_snapshot(mut snapshot: AppSessionSnapshot) -> Option<AppSessionSnapshot> {
    if snapshot.version != SESSION_SNAPSHOT_SCHEMA_VERSION || snapshot.windows.is_empty() {
        return None;
    }
    drop_nonrestorable_remote_mirrors(&mut snapshot);
    prune_crash_diagnostic_workspaces(&mut snapshot);
    snapshot.windows.truncate(MAX_MANUALLY_RESTORED_WINDOWS);
    if snapshot.windows.is_empty() || !remint_noncanonical_identities(&mut snapshot) {
        return None;
    }
    Some(snapshot)
}

fn close_built_windows(app: &AppHandle, window_ids: &[String]) {
    for window_id in window_ids.iter().rev() {
        if let Err(error) = crate::window::close_restored_window(app, window_id) {
            eprintln!("[session] failed to close startup-restore window {window_id}: {error}");
        }
    }
}

fn install_startup_snapshot(
    app: &AppHandle,
    state: &SessionState,
    snapshot: AppSessionSnapshot,
) -> bool {
    let additional_window_ids = snapshot
        .windows
        .iter()
        .skip(1)
        .filter_map(|window| window.window_id.clone())
        .collect::<Vec<_>>();
    let mut built = Vec::with_capacity(additional_window_ids.len());
    for window_id in &additional_window_ids {
        if let Err(error) = crate::window::build_hidden_restored_window(app, window_id) {
            eprintln!("[session] failed to build startup-restore window {window_id}: {error}");
            close_built_windows(app, &built);
            return false;
        }
        built.push(window_id.clone());
    }

    let previous = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        std::mem::replace(&mut *guard, snapshot.clone())
    };
    state
        .next_panel
        .store(next_panel_counter(&snapshot), Ordering::Relaxed);

    for window_id in &built {
        if let Err(error) = crate::window::show_restored_window_unfocused(app, window_id) {
            eprintln!("[session] failed to show startup-restore window {window_id}: {error}");
            close_built_windows(app, &built);
            let previous_next_panel = next_panel_counter(&previous);
            *state
                .snapshot
                .lock()
                .expect("session snapshot mutex poisoned") = previous;
            state
                .next_panel
                .store(previous_next_panel, Ordering::Relaxed);
            return false;
        }
    }
    if let Some(window_id) = snapshot
        .windows
        .last()
        .and_then(|window| window.window_id.as_deref())
    {
        if let Some(active) = app.try_state::<crate::control_socket::ControlActiveWindowState>() {
            active.set_startup_fallback(window_id);
        }
    }
    true
}

/// Preserve the last launch for manual reopen, restore a normal launch when
/// canonical policy allows it, then publish the resulting live snapshot.
pub fn bootstrap_session_persistence(app: &AppHandle, state: State<'_, SessionState>) {
    let environment = std::env::vars().collect::<HashMap<_, _>>();
    let arguments = std::env::args().collect::<Vec<_>>();
    let should_restore = should_attempt_startup_restore(&arguments, &environment);

    let candidates = session_snapshot_paths(app).map(|(current, previous)| {
        let current_snapshot = load_snapshot_file(&current);
        let previous_snapshot = load_snapshot_file(&previous);
        if current.exists() {
            if let Some(parent) = previous.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::copy(&current, &previous);
        }
        (current_snapshot, previous_snapshot)
    });
    if should_restore {
        if let Some(snapshot) = candidates.and_then(|(current, previous)| {
            current
                .and_then(prepare_startup_snapshot)
                .or_else(|| previous.and_then(prepare_startup_snapshot))
        }) {
            install_startup_snapshot(app, state.inner(), snapshot);
        }
    }

    let snapshot = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        ensure_workspace_ids(&mut guard);
        ensure_pane_ids(&mut guard);
        guard.clone()
    };
    *state
        .workspace_focus_history
        .lock()
        .expect("workspace focus history mutex poisoned") =
        workspace_focus_history_for_snapshot(&snapshot);
    let _ = persist_current_snapshot(app, &snapshot);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(extra: &[&str]) -> Vec<String> {
        std::iter::once("cmux".to_string())
            .chain(extra.iter().map(|value| value.to_string()))
            .collect()
    }

    #[test]
    fn startup_restore_policy_matches_canonical_launch_intent_rules() {
        assert!(should_attempt_startup_restore(&args(&[]), &HashMap::new()));
        assert!(should_attempt_startup_restore(
            &args(&["-psn_0_12345"]),
            &HashMap::new()
        ));
        assert!(!should_attempt_startup_restore(
            &args(&["C:\\repo"]),
            &HashMap::new()
        ));

        for (key, value) in [
            ("CMUX_DISABLE_SESSION_RESTORE", "1"),
            ("CMUX_UI_TEST_MODE", "1"),
            ("CMUX_UI_TEST_SOCKET", "pipe"),
            ("XCTestBundlePath", "tests"),
            ("DYLD_INSERT_LIBRARIES", "/tmp/libXCTest.dylib"),
        ] {
            let environment = HashMap::from([(key.to_string(), value.to_string())]);
            assert!(!should_attempt_startup_restore(&args(&[]), &environment));
        }
    }

    #[test]
    fn startup_snapshot_rejects_wrong_schema_and_caps_window_count() {
        let mut wrong_schema = initial_snapshot("surface-1");
        wrong_schema.version += 1;
        assert!(prepare_startup_snapshot(wrong_schema).is_none());

        let mut many = initial_snapshot("surface-1");
        let template = many.windows[0].clone();
        many.windows = (0..20).map(|_| template.clone()).collect();
        let prepared = prepare_startup_snapshot(many).expect("prepare valid snapshot");
        assert_eq!(prepared.windows.len(), MAX_MANUALLY_RESTORED_WINDOWS);
    }
}
