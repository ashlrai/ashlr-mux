use super::*;

pub(super) fn handle_pane_surface_lifecycle_request(
    app: &AppHandle,
    method: &str,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    handle_pane_surface_lifecycle_request_with_terminal_policy(
        app,
        method,
        params,
        TerminalCreateRuntimePolicy::Eager,
    )
}

pub(super) fn handle_pane_surface_lifecycle_request_with_terminal_policy(
    app: &AppHandle,
    method: &str,
    params: &serde_json::Map<String, Value>,
    terminal_create_runtime_policy: TerminalCreateRuntimePolicy,
) -> ControlCallResult {
    let current = snapshot(app);
    let active_window_id = control_active_window_id(app);
    let mut transition = pane_surface_lifecycle::dispatch_lifecycle_request(
        &current,
        method,
        params,
        &pane_surface_lifecycle::LifecycleDispatchContext {
            browser_enabled: app.try_state::<BrowserWebviewState>().is_some(),
            dock_available: app
                .try_state::<crate::right_sidebar::RightSidebarState>()
                .is_some_and(|state| state.beta_settings().dock_enabled),
            active_window_id,
        },
    );
    // R5: forget closed/respawned entities BEFORE decoration so the response
    // echo mints fresh refs like canonical.
    forget_recreated_lifecycle_handles(app, method, &current, &transition);
    if let Some(decorated) = decorate_lifecycle_result_refs(app, method, &mut transition.result) {
        for event in &mut transition.events {
            if let Some(result) = event.payload.get_mut("result") {
                *result = decorated.clone();
            }
        }
    }
    if !transition.changed && transition.effects.is_empty() {
        return transition.result;
    }
    let external_url = transition.effects.iter().find_map(|effect| match effect {
        pane_surface_lifecycle::LifecycleEffect::ExternalBrowserOpen { url, .. } => {
            Some(url.clone())
        }
        _ => None,
    });
    let lifecycle_failure = transition.effects.iter().find_map(|effect| match effect {
        pane_surface_lifecycle::LifecycleEffect::DockCreate {
            failure_code,
            failure_message,
            ..
        }
        | pane_surface_lifecycle::LifecycleEffect::RuntimeTeardown {
            failure_code,
            failure_message,
            ..
        }
        | pane_surface_lifecycle::LifecycleEffect::TerminalCreate {
            failure_code,
            failure_message,
            ..
        }
        | pane_surface_lifecycle::LifecycleEffect::BrowserAttach {
            failure_code,
            failure_message,
            ..
        }
        | pane_surface_lifecycle::LifecycleEffect::BrowserReload {
            failure_code,
            failure_message,
            ..
        }
        | pane_surface_lifecycle::LifecycleEffect::ExternalBrowserOpen {
            failure_code,
            failure_message,
            ..
        }
        | pane_surface_lifecycle::LifecycleEffect::RemoteCreate {
            failure_code,
            failure_message,
            ..
        }
        | pane_surface_lifecycle::LifecycleEffect::RemoteWindowClose {
            failure_code,
            failure_message,
            ..
        } => Some(((*failure_code).to_string(), (*failure_message).to_string())),
        _ => None,
    });
    let completion_events = transition.events.clone();
    let previous = current.clone();
    let mut target = current;
    let mut executor = ProductionLifecycleExecutor {
        app,
        terminal_create_runtime_policy,
        candidate: None,
        previous: Some(previous),
        staged: Vec::new(),
        staged_terminals: Vec::new(),
        staged_remote_creations: Vec::new(),
        deferred_remote_reconciliations: Vec::new(),
        deferred_remote_departures: Vec::new(),
        staged_browsers: Vec::new(),
        dock_journal: DockCommitJournal::default(),
    };
    let result =
        pane_surface_lifecycle::commit_lifecycle_transition(&mut target, transition, &mut executor)
            .unwrap_or_else(|message| {
                if message == "Failed to open URL externally" {
                    ControlCallResult::Err {
                        code: "external_open_failed".into(),
                        message,
                        data: external_url
                            .and_then(|url| JsonValue::try_from(json!({"url":url})).ok()),
                    }
                } else if message.contains("Lifecycle rollback failed:") {
                    ControlCallResult::Err {
                        code: "internal_error".into(),
                        message,
                        data: None,
                    }
                } else {
                    let (code, mapped_message) = lifecycle_failure
                        .clone()
                        .unwrap_or_else(|| ("internal_error".into(), message.clone()));
                    ControlCallResult::Err {
                        code,
                        message: mapped_message,
                        data: None,
                    }
                }
            });
    if matches!(result, ControlCallResult::Ok(_)) {
        // D8b: canonical emits ONLY the workspace.lifecycle / socket.v2
        // completion frames for socket commands — no derived session.model
        // extras (session.changed, pane.focused) on this path (capture frame
        // lists for all four events cases).
        for completion in completion_events {
            record_event(
                app,
                completion.name,
                completion.category,
                completion.source,
                completion.window_id,
                completion.workspace_id,
                completion.pane_id,
                completion.surface_id,
                completion.payload,
            );
        }
        executor.flush_deferred_remote_reconciliations();
        executor.flush_deferred_remote_departures();
    }
    result
}

pub(super) fn handle_workspace_action_request(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
    event_params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let current = snapshot(app);
    let palette = crate::config::current_workspace_palette_snapshot();
    let active_window_id = control_active_window_id(app);
    let crate::workspace_action::WorkspaceActionPlan {
        result,
        mutation,
        window_index,
        workspace_id,
    } = crate::workspace_action::plan_workspace_action_with_active_window(
        &current,
        params,
        &palette,
        active_window_id.as_deref(),
    );
    let ControlCallResult::Ok(raw_payload) = result else {
        return result;
    };

    match &mutation {
        crate::workspace_action::WorkspaceActionMutation::MarkUnread {
            workspace_id,
            unread,
            ..
        } => {
            let session = app.state::<SessionState>();
            let notifications = app.state::<crate::notifications::NotificationCommandState>();
            let mut effects = None;
            let transaction =
                crate::notifications::with_notification_store(notifications.inner(), |store| {
                    apply_workspace_action_for_control_with_post_commit(
                        app,
                        &session,
                        &mutation,
                        || {
                            effects = Some(crate::notifications::set_workspace_unread_in_store(
                                store,
                                workspace_id,
                                *unread,
                            ));
                        },
                    )
                });
            let transaction = match transaction {
                Ok(transaction) => transaction,
                Err(message) => {
                    return ControlCallResult::Err {
                        code: "notification_store_failed".to_string(),
                        message,
                        data: None,
                    };
                }
            };
            if let Err(message) = transaction {
                return ControlCallResult::Err {
                    code: "internal".to_string(),
                    message,
                    data: None,
                };
            }
            if let Some(effects) = effects {
                publish_notification_removal_effects(
                    app,
                    &effects,
                    "notification.read",
                    Some(workspace_id),
                );
            }
        }
        crate::workspace_action::WorkspaceActionMutation::None => {}
        _ => {
            let state = app.state::<SessionState>();
            if let Err(message) = apply_workspace_action_for_control(app, &state, &mutation) {
                return ControlCallResult::Err {
                    code: "internal".to_string(),
                    message,
                    data: None,
                };
            }
        }
    }

    let mut payload = Value::from(raw_payload);
    if let (Some(window_index), Some(workspace_id)) = (window_index, workspace_id.as_deref()) {
        payload["workspace_ref"] = json!(control_handle_ref(app, "workspace", workspace_id));
        let window_id = current
            .windows
            .get(window_index)
            .and_then(|window| window.window_id.as_deref());
        payload["window_ref"] = window_id
            .map(|window_id| json!(control_handle_ref(app, "window", window_id)))
            .unwrap_or(Value::Null);
    }
    let completion = crate::workspace_action::workspace_action_completion(event_params, &payload);
    record_event(
        app,
        "workspace.action",
        "workspace",
        "socket.v2",
        completion.window_id,
        completion.workspace_id,
        None,
        None,
        completion.payload,
    );
    ok(payload)
}

/// Canonical quit-confirmation mode key: `app.confirmQuit`, default `always`
/// (QuitConfirmationStore, Packages/macOS/CmuxSettings/Sources/CmuxSettings/
/// Stores/QuitConfirmationStore.swift at pinned e1825d40d).
pub(crate) const CONFIRM_QUIT_SETTING_KEY: &str = "app.confirmQuit";
pub(super) const WINDOW_QUIT_CONFIRMATION_EVENT: &str = "cmux://window-quit-confirmation";

/// Whether the last-window close routes into the confirmation dialog.
///
/// Canonical `QuitConfirmationStore.shouldShowConfirmation`
/// (handleQuitShortcutWarning, AppDelegate.swift:12831-12856): mode `always`
/// (the default when the key is absent/unrecognized) confirms, `never`
/// terminates immediately. `dirtyOnly` degrades to `always` on this port
/// until dirty-workspace tracking exists (canonical consults
/// `hasDirtyWorkspaces`); the dev-build and in-session-confirmed skips are
/// terminate-flow concerns outside this socket path.
pub(super) fn window_quit_confirmation_required(
    settings: Option<&crate::app_settings::SettingsStore>,
) -> bool {
    settings
        .and_then(|store| store.get_string(CONFIRM_QUIT_SETTING_KEY))
        .is_none_or(|mode| mode != "never")
}

/// The shared flat settings file (same rooting as right_sidebar.rs /
/// agent_session.rs: `app_data_dir()/cmux/settings.json`).
pub(super) fn control_settings_store(
    app: &AppHandle,
) -> Option<crate::app_settings::SettingsStore> {
    app.path()
        .app_data_dir()
        .ok()
        .map(|dir| crate::app_settings::SettingsStore::new(dir.join("cmux").join("settings.json")))
}

/// The platform-equivalent of canonical QuitConfirmationAlertPresenter
/// (Sources/QuitConfirmationAlertPresenter.swift:23-34 at pinned e1825d40d):
/// a warning alert "Quit cmux?" / "This will close all windows and
/// workspaces." with Quit/Cancel. Confirm terminates (NSApp.terminate
/// parity); cancel is the veto. Non-blocking, mirroring canonical's async
/// sheet — the reply already went out ("performClose invoked"). The
/// suppression checkbox ("Don't warn again for Cmd+Q") has no Tauri dialog
/// equivalent; users set `app.confirmQuit` to `never` instead. Live-verify
/// only: canonical bypasses the alert under XCTest.
pub(super) fn present_quit_confirmation_dialog(app: &AppHandle) {
    use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
    let exit_app = app.clone();
    app.dialog()
        .message("This will close all windows and workspaces.")
        .title("Quit cmux?")
        .kind(MessageDialogKind::Warning)
        .buttons(MessageDialogButtons::OkCancelCustom(
            "Quit".into(),
            "Cancel".into(),
        ))
        .show(move |confirmed| {
            if confirmed {
                exit_app.exit(0);
            }
        });
}

/// Canonical activeTabManager pointer for socket routing: SetActiveWindow
/// effects repoint it (socket create/close,
/// TerminalControllerControlCommandContext.swift:71-76) and key-window
/// transitions repoint it (CmuxLifecycleEventPublishing.swift:258-268) via
/// the webview Focused listener. Selector-less routing prefers this pointer;
/// the focused webview is only the pre-first-write fallback.
#[derive(Default)]
pub struct ControlActiveWindowState {
    inner: Mutex<ControlActiveWindow>,
}

#[derive(Default)]
struct ControlActiveWindow {
    current: Option<String>,
    key: Option<String>,
    startup_fallback: Option<String>,
}

impl ControlActiveWindowState {
    pub(crate) fn set(&self, window_id: &str) {
        self.inner
            .lock()
            .expect("active window pointer mutex poisoned")
            .current = Some(window_id.to_owned());
    }

    pub(crate) fn set_key(&self, window_id: &str) {
        let mut active = self
            .inner
            .lock()
            .expect("active window pointer mutex poisoned");
        active.current = Some(window_id.to_owned());
        active.key = Some(window_id.to_owned());
    }

    pub(super) fn key(&self) -> Option<String> {
        self.inner
            .lock()
            .expect("active window pointer mutex poisoned")
            .key
            .clone()
    }

    #[cfg(test)]
    pub(crate) fn get(&self) -> Option<String> {
        self.inner
            .lock()
            .expect("active window pointer mutex poisoned")
            .current
            .clone()
    }

    pub(crate) fn set_startup_fallback(&self, window_id: &str) {
        self.inner
            .lock()
            .expect("active window pointer mutex poisoned")
            .startup_fallback = Some(window_id.to_owned());
    }

    /// Return the authoritative pointer without consulting native window
    /// state once startup focus resolution has completed.
    pub(super) fn resolved_current(&self) -> Option<String> {
        let active = self
            .inner
            .lock()
            .expect("active window pointer mutex poisoned");
        if active.startup_fallback.is_none() {
            active.current.clone()
        } else {
            None
        }
    }

    pub(super) fn resolve_startup(&self, focused_window_id: Option<String>) -> Option<String> {
        let mut active = self
            .inner
            .lock()
            .expect("active window pointer mutex poisoned");
        if let Some(startup_fallback) = active.startup_fallback.take() {
            active.current = focused_window_id.or(Some(startup_fallback));
            active.key = active.current.clone();
        }
        active.current.clone()
    }
}

/// Pointer precedence: the stored pointer wins (canonical setActiveTabManager
/// overrides the caller default until the next key transition rewrites it);
/// the focused webview is only the fallback before the first write.
pub(super) fn control_active_window_from(
    stored: Option<String>,
    focused_webview: Option<String>,
) -> Option<String> {
    stored.or(focused_webview)
}

/// The active window id used by selector-less routing.
pub(super) fn control_active_window_id(app: &AppHandle) -> Option<String> {
    let active_state = app.try_state::<ControlActiveWindowState>();
    if let Some(stored) = active_state
        .as_ref()
        .and_then(|state| state.resolved_current())
    {
        return Some(stored);
    }
    let snapshot = snapshot(app);
    let focused = app.webview_windows().iter().find_map(|(label, window)| {
        (window.is_focused().ok() == Some(true)).then(|| label.clone())
    });
    let focused = focused.and_then(|label| session_window_id_for_label(&snapshot, &label));
    let stored = active_state.and_then(|state| state.resolve_startup(focused.clone()));
    control_active_window_from(stored, focused)
}

/// Webview focus listener hook: a key-window transition repoints the active
/// pointer (canonical CmuxLifecycleEventPublishing.swift:258-268).
pub(crate) fn note_window_focused(app: &AppHandle, label: &str) {
    let Some(state) = app.try_state::<ControlActiveWindowState>() else {
        return;
    };
    let window_id =
        session_window_id_for_label(&snapshot(app), label).unwrap_or_else(|| label.to_owned());
    state.set_key(&window_id);
}

/// The session window presented by a webview label: labels match session ids
/// directly except "main", which hosts the first session window (the
/// window.list mapping).
pub(super) fn session_window_id_for_label(
    snapshot: &AppSessionSnapshot,
    label: &str,
) -> Option<String> {
    snapshot
        .windows
        .iter()
        .find(|window| window.window_id.as_deref() == Some(label))
        .and_then(|window| window.window_id.clone())
        .or_else(|| {
            (label == "main")
                .then(|| {
                    snapshot
                        .windows
                        .first()
                        .and_then(|window| window.window_id.clone())
                })
                .flatten()
        })
}

/// Native window utilities index webview labels ("main" is represented as
/// "window-1" by cmux_core::window_display::ordered_window_identities), while
/// public window rows expose the owning session UUID. Map a native identity
/// id/label selector onto the session window id so the pure transition layer
/// resolves it.
pub(super) fn normalize_window_identity_selector(
    app: &AppHandle,
    snapshot: &AppSessionSnapshot,
    params: &mut serde_json::Map<String, Value>,
) {
    let Some(selector) = params.get("window_id").and_then(Value::as_str) else {
        return;
    };
    if snapshot
        .windows
        .iter()
        .any(|window| window.window_id.as_deref() == Some(selector))
    {
        return; // already a session id
    }
    let identities =
        cmux_core::window_display::ordered_window_identities(app.webview_windows().keys().cloned());
    let Some(index) = cmux_core::window_display::resolve_window_selector(&identities, selector)
    else {
        return;
    };
    if let Some(id) = session_window_id_for_label(snapshot, identities[index].label.as_str()) {
        params.insert("window_id".into(), json!(id));
    }
}

/// The webview label presenting a session window: labels match session ids
/// directly except the first session window, which the "main" webview hosts.
pub(super) fn webview_label_for_session_window(
    app: &AppHandle,
    snapshot: &AppSessionSnapshot,
    window_id: &str,
) -> String {
    if app.webview_windows().contains_key(window_id) {
        return window_id.to_owned();
    }
    if snapshot
        .windows
        .first()
        .and_then(|window| window.window_id.as_deref())
        == Some(window_id)
    {
        return "main".to_owned();
    }
    window_id.to_owned()
}

pub(super) fn handle_window_lifecycle_request(
    app: &AppHandle,
    method: &str,
    mut params: serde_json::Map<String, Value>,
) -> ControlCallResult {
    let current = snapshot(app);
    normalize_window_identity_selector(app, &current, &mut params);
    let active_window_id = control_active_window_id(app);
    let key_window_id = app
        .try_state::<ControlActiveWindowState>()
        .and_then(|state| state.key());
    let context = window_lifecycle::WindowLifecycleContext {
        active_window_id,
        key_window_id,
        quit_confirmation_required: window_quit_confirmation_required(
            control_settings_store(app).as_ref(),
        ),
        now_epoch_seconds: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_secs_f64())
            .unwrap_or(0.0),
        // Canonical allocates a UUID for every socket-created window. UUIDs
        // are valid Tauri labels, so the create effect can use the same value
        // for the public session identity and the native auxiliary window.
        new_window_id: None,
        new_surface_id: None,
    };
    let mut transition =
        window_lifecycle::dispatch_window_lifecycle_request(&current, method, &params, &context);
    decorate_lifecycle_result_refs(app, method, &mut transition.result);
    if matches!(transition.result, ControlCallResult::Err { .. }) {
        return transition.result;
    }
    // window.create / window.close mutate the session model through the
    // existing registration seams (register/unregister_window_for_control),
    // driven by the effects below — committing the transition snapshot too
    // would double-apply. Resume mutations have no such seam: commit here.
    if transition.changed && method.starts_with("surface.resume.") {
        let state = app.state::<SessionState>();
        if let Err(message) = commit_lifecycle_snapshot_for_control_if_current(
            app,
            state.inner(),
            &current,
            &transition.snapshot,
            false,
        ) {
            return ControlCallResult::Err {
                code: "internal_error".into(),
                message,
                data: None,
            };
        }
    }
    for effect in &transition.effects {
        if let Err((code, message)) =
            apply_window_lifecycle_effect(app, &current, &transition.snapshot, effect)
        {
            return ControlCallResult::Err {
                code: code.into(),
                message,
                data: None,
            };
        }
    }
    for event in transition.events {
        record_event(
            app,
            event.name,
            event.category,
            event.source,
            event.window_id,
            event.workspace_id,
            event.pane_id,
            event.surface_id,
            event.payload,
        );
    }
    transition.result
}

pub(super) fn apply_window_lifecycle_effect(
    app: &AppHandle,
    current: &AppSessionSnapshot,
    next: &AppSessionSnapshot,
    effect: &window_lifecycle::WindowLifecycleEffect,
) -> Result<(), (&'static str, String)> {
    use window_lifecycle::WindowLifecycleEffect as Effect;
    match effect {
        Effect::WindowCreate {
            window_id,
            failure_code,
            failure_message,
            ..
        } => {
            let Some(window) = next
                .windows
                .iter()
                .find(|window| window.window_id.as_deref() == Some(window_id))
            else {
                return Err((
                    "internal_error",
                    "Created window snapshot is missing".into(),
                ));
            };
            crate::window::create_socket_window(app, window).map_err(|error| {
                // The wire message is byte-frozen ("Failed to create window");
                // keep the detail on stderr only.
                eprintln!("[control] window.create failed: {error}");
                (*failure_code, (*failure_message).to_string())
            })
        }
        Effect::WindowCloseCommit { window_id } => {
            let label = webview_label_for_session_window(app, current, window_id);
            let active_state = app.try_state::<ControlActiveWindowState>();
            let was_key = active_state
                .as_ref()
                .and_then(|state| state.key())
                .as_deref()
                == Some(window_id);
            crate::window::close_socket_window(app, &label)
                .map_err(|error| ("internal_error", error))?;
            if was_key {
                if let (Some(state), Some(next_window_id)) = (
                    active_state,
                    next.windows
                        .first()
                        .and_then(|window| window.window_id.as_deref()),
                ) {
                    state.set_key(next_window_id);
                }
            }
            Ok(())
        }
        Effect::WindowFocus { window_id } => {
            // Best-effort platform focus: canonical focus() returns true on
            // every path, so focus failures never fail the RPC. Real Win32
            // foregrounding is a platform_equivalent verified live.
            let label = webview_label_for_session_window(app, current, window_id);
            if let Err(error) = crate::window::focus_control_window(app, &label) {
                eprintln!("[control] window.focus: {error}");
            }
            if let Some(state) = app.try_state::<ControlActiveWindowState>() {
                state.set_key(window_id);
            }
            Ok(())
        }
        Effect::QuitConfirmation { window_id } => {
            // Event kept for the web layer/tests; the real consumer is the
            // native dialog below.
            let _ = app.emit(
                WINDOW_QUIT_CONFIRMATION_EVENT,
                json!({ "window_id": window_id }),
            );
            present_quit_confirmation_dialog(app);
            Ok(())
        }
        Effect::AppTerminate { .. } => {
            app.exit(0);
            Ok(())
        }
        Effect::TerminalRefresh { surface_id, reason } => {
            let _ = app.emit(
                SURFACE_REFRESH_EVENT,
                json!({ "refresh": true, "surface_id": surface_id, "reason": reason }),
            );
            Ok(())
        }
        // Canonical defensive setActiveTabManager parity: repoint the
        // selector-less routing pointer
        // (TerminalControllerControlCommandContext.swift:71-76).
        Effect::SetActiveWindow { window_id } => {
            if let Some(state) = app.try_state::<ControlActiveWindowState>() {
                state.set(window_id);
            }
            Ok(())
        }
        // The session commit paths persist; closed-window history, per-window
        // geometry persistence, remote detach, and the resume approval store
        // are platform-equivalence/deferred-subsystem candidates pinned by
        // the transition tests until their subsystems land.
        // Canonical unregisterMainWindow notification clearing
        // (AppDelegate.swift:16274-16280). Best-effort like the rest of the
        // teardown: a poisoned store must not fail an already-closed window.
        Effect::ClearWindowNotifications {
            window_id,
            workspace_ids,
        } => {
            if let Some(state) = app.try_state::<crate::notifications::NotificationCommandState>() {
                if let Err(error) = crate::notifications::notification_clear_window_for_control(
                    state.inner(),
                    window_id,
                    workspace_ids,
                ) {
                    eprintln!("[control] window.close notification clear: {error}");
                }
            }
            Ok(())
        }
        Effect::RecordClosedWindowHistory { .. }
        | Effect::PersistWindowGeometry { .. }
        | Effect::RemoteWorkspaceDetach { .. }
        | Effect::ResumeApprovalPrompt { .. }
        | Effect::PersistSession => Ok(()),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RuntimeArrivalCommitOutcome {
    Committed,
    DuplicateOrStale,
    SourceMissing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RuntimeDepartureCommitOutcome {
    Committed,
    DuplicateOrStale,
}

pub(super) fn commit_runtime_arrival_for_control(
    app: &AppHandle,
    arrival: pane_surface_lifecycle::RuntimeArrival,
) -> Result<RuntimeArrivalCommitOutcome, String> {
    let state = app.state::<SessionState>();
    let _control_guard = state.lock_control_mutation()?;
    let current = current_session_snapshot(&state);
    let model = cmux_core::surface_lifecycle::SurfaceLifecycleModel::from_app_session(&current)
        .map_err(|error| error.to_string())?;
    if arrival.generation == 0 || model.surface(&arrival.surface_id).is_some() {
        return Ok(RuntimeArrivalCommitOutcome::DuplicateOrStale);
    }
    let scope_exists =
        current.windows.iter().any(|window| {
            window.window_id.as_deref() == Some(&arrival.window_id)
                && window.tab_manager.workspaces.iter().any(|workspace| {
                    workspace.workspace_id.as_deref() == Some(&arrival.workspace_id)
                })
        });
    if !scope_exists {
        return Ok(RuntimeArrivalCommitOutcome::SourceMissing);
    }
    if !arrival.creates_pane {
        let source_matches = arrival.anchor_surface_id.as_ref().is_some_and(|source| {
            model.owner_of_surface(source).is_some_and(|owner| {
                owner.window_id == arrival.window_id
                    && owner.workspace_id == arrival.workspace_id
                    && owner.pane_id == arrival.pane_id
            })
        });
        if !source_matches {
            return Ok(RuntimeArrivalCommitOutcome::SourceMissing);
        }
    } else if let Some(source) = &arrival.anchor_surface_id {
        let source_matches = model.owner_of_surface(source).is_some_and(|owner| {
            owner.window_id == arrival.window_id
                && owner.workspace_id == arrival.workspace_id
                && arrival.source_pane_id.as_deref() == Some(owner.pane_id.as_str())
        });
        if !source_matches {
            return Ok(RuntimeArrivalCommitOutcome::SourceMissing);
        }
    }
    let reconciled = pane_surface_lifecycle::reconcile_runtime_arrival(&current, arrival.clone());
    if reconciled.snapshot == current {
        return Ok(RuntimeArrivalCommitOutcome::DuplicateOrStale);
    }
    commit_lifecycle_snapshot_for_control(app, state.inner(), &reconciled.snapshot, false)?;
    record_session_changed_event_suppressing(
        app,
        &reconciled.snapshot,
        &HashSet::from(["pane.created", "surface.created"]),
    );
    let (emit_pane_created, origin) = runtime_arrival_event_semantics(&arrival);
    if emit_pane_created {
        record_event(
            app,
            "pane.created",
            "pane",
            "workspace.lifecycle",
            Some(arrival.window_id.clone()),
            Some(arrival.workspace_id.clone()),
            Some(arrival.pane_id.clone()),
            Some(arrival.surface_id.clone()),
            json!({"pane_id":arrival.pane_id,"source_pane_id":arrival.source_pane_id,"orientation":arrival.split_orientation,"surface_id":arrival.surface_id,"origin":"terminal_split"}),
        );
    }
    record_event(
        app,
        "surface.created",
        "surface",
        "workspace.lifecycle",
        Some(arrival.window_id),
        Some(arrival.workspace_id),
        Some(arrival.pane_id.clone()),
        Some(arrival.surface_id.clone()),
        json!({"surface_id":arrival.surface_id,"pane_id":arrival.pane_id,"kind":"terminal","origin":origin,"focused":arrival.focused}),
    );
    Ok(RuntimeArrivalCommitOutcome::Committed)
}

pub(super) fn commit_runtime_departure_for_control(
    app: &AppHandle,
    departure: pane_surface_lifecycle::RuntimeDeparture,
) -> Result<RuntimeDepartureCommitOutcome, String> {
    let state = app.state::<SessionState>();
    let _control_guard = state.lock_control_mutation()?;
    let current = current_session_snapshot(&state);
    let model = cmux_core::surface_lifecycle::SurfaceLifecycleModel::from_app_session(&current)
        .map_err(|error| error.to_string())?;
    let Some(record) = model.surface(&departure.surface_id) else {
        return Ok(RuntimeDepartureCommitOutcome::DuplicateOrStale);
    };
    let owner_matches = model
        .owner_of_surface(&departure.surface_id)
        .is_some_and(|owner| {
            owner.window_id == departure.window_id
                && owner.workspace_id == departure.workspace_id
                && owner.pane_id == departure.pane_id
        });
    if record.generation != departure.generation || !owner_matches {
        return Ok(RuntimeDepartureCommitOutcome::DuplicateOrStale);
    }
    let reconciled = pane_surface_lifecycle::reconcile_runtime_departure(&current, &departure);
    if reconciled.snapshot == current {
        return Err("remote departure reconciliation made no progress".into());
    }
    commit_lifecycle_snapshot_for_control(app, state.inner(), &reconciled.snapshot, false)?;
    record_session_changed_event_suppressing(
        app,
        &reconciled.snapshot,
        &HashSet::from(["surface.closed"]),
    );
    record_event(
        app,
        "surface.closed",
        "surface",
        "workspace.lifecycle",
        Some(departure.window_id),
        Some(departure.workspace_id),
        Some(departure.pane_id),
        Some(departure.surface_id.clone()),
        json!({"surface_id":departure.surface_id,"origin":"remote_window_close"}),
    );
    Ok(RuntimeDepartureCommitOutcome::Committed)
}

pub(super) fn runtime_arrival_event_semantics(
    arrival: &pane_surface_lifecycle::RuntimeArrival,
) -> (bool, &'static str) {
    if arrival.creates_pane {
        (true, "terminal_split")
    } else {
        (false, "terminal_tab")
    }
}

pub(super) const LIFECYCLE_ID_REF_FIELDS: [(&str, &str, &str); 10] = [
    ("window_id", "window_ref", "window"),
    ("source_window_id", "source_window_ref", "window"),
    ("workspace_id", "workspace_ref", "workspace"),
    ("source_workspace_id", "source_workspace_ref", "workspace"),
    ("created_workspace_id", "created_workspace_ref", "workspace"),
    ("pane_id", "pane_ref", "pane"),
    ("surface_id", "surface_ref", "surface"),
    ("created_surface_id", "created_surface_ref", "surface"),
    ("tab_id", "tab_ref", "surface"),
    ("created_tab_id", "created_tab_ref", "surface"),
];

/// R5 (round-5 adjudication, reverting round 3's pane half): a successful
/// surface.close unregisters the closed SURFACE ref always, and its pane's
/// ref only when the pane left the tree — pane:2 survives across all three
/// canonical datasets after a surface close on that pane. Respawn forgets
/// NOTHING: the canonical respawn echo reuses the surface's pre-existing
/// ref.
pub(super) fn forget_recreated_lifecycle_handles(
    app: &AppHandle,
    method: &str,
    before: &AppSessionSnapshot,
    transition: &pane_surface_lifecycle::LifecycleTransition,
) {
    if method != "surface.close" {
        return;
    }
    let ControlCallResult::Ok(payload) = &transition.result else {
        return;
    };
    let payload = Value::from(payload.clone());
    let Some(surface_id) = payload.get("surface_id").and_then(Value::as_str) else {
        return;
    };
    let pane_id = cmux_core::surface_lifecycle::SurfaceLifecycleModel::from_app_session(before)
        .ok()
        .and_then(|model| {
            model
                .owner_of_surface(surface_id)
                .map(|owner| owner.pane_id.clone())
        });
    let state = app.state::<ControlHandleRegistryState>();
    let mut registry = state.inner.lock().expect("handle registry mutex poisoned");
    registry.forget("surface", surface_id);
    if let Some(pane_id) = pane_id {
        let pane_survives = cmux_core::surface_lifecycle::SurfaceLifecycleModel::from_app_session(
            &transition.snapshot,
        )
        .ok()
        .is_some_and(|model| model.pane(&pane_id).is_some());
        if !pane_survives {
            registry.forget("pane", &pane_id);
        }
    }
}

pub(super) fn decorate_lifecycle_result_refs(
    app: &AppHandle,
    method: &str,
    result: &mut ControlCallResult,
) -> Option<Value> {
    decorate_lifecycle_result_refs_with(method, result, &mut |kind, id| {
        control_handle_ref(app, kind, id)
    })
}

/// Canonical error payloads carry plain ids without refs, with two exceptions:
/// the tab.action/surface.action Tab-not-found data
/// (ControlCommandCoordinator+SystemTabAction.swift:39-48) and
/// surface.report_pwd's not_found requested-identity block
/// (ControlCommandCoordinator+Surface3.swift:240-251,365-375). Success
/// payloads are always decorated.
pub(super) fn error_data_ref_decoration_is_canonical(
    method: &str,
    code: &str,
    message: &str,
) -> bool {
    (matches!(method, "surface.action" | "tab.action") && message == "Tab not found")
        || (method == "surface.report_pwd" && code == "not_found")
        // QUIRK: window.focus/window.close not_found data MINTS a window_ref
        // for the nonexistent id (ControlCommandCoordinator+Window.swift:
        // 129-135,155-161; ref() mints for ANY uuid).
        || (matches!(method, "window.close" | "window.focus") && message == "Window not found")
}

pub(super) fn decorate_lifecycle_result_refs_with(
    method: &str,
    result: &mut ControlCallResult,
    mint: &mut impl FnMut(&'static str, &str) -> String,
) -> Option<Value> {
    let success = matches!(result, ControlCallResult::Ok(_));
    let payload = match result {
        ControlCallResult::Ok(payload) => payload,
        ControlCallResult::Err {
            code,
            message,
            data: Some(data),
        } if error_data_ref_decoration_is_canonical(method, code, message) => data,
        ControlCallResult::Err { .. } => return None,
    };
    let mut value = Value::from(payload.clone());
    decorate_lifecycle_value_refs(&mut value, mint);
    if let Ok(decorated) = JsonValue::try_from(value) {
        *payload = decorated;
    }
    success.then(|| Value::from(payload.clone()))
}

pub(super) fn decorate_lifecycle_value_refs(
    value: &mut Value,
    mint: &mut impl FnMut(&'static str, &str) -> String,
) {
    fn decorate(
        value: &mut Value,
        row_is_surface: bool,
        mint: &mut impl FnMut(&'static str, &str) -> String,
    ) {
        match value {
            Value::Array(values) => {
                for value in values {
                    decorate(value, true, mint);
                }
            }
            Value::Object(object) => {
                for (id_key, ref_key, kind) in LIFECYCLE_ID_REF_FIELDS {
                    if let Some(id) = object.get(id_key) {
                        let reference = id.as_str().map(|id| {
                            let reference = mint(kind, id);
                            if matches!(ref_key, "tab_ref" | "created_tab_ref") {
                                tab_ref_from_surface_ref(&reference)
                            } else {
                                reference
                            }
                        });
                        object.entry(ref_key).or_insert_with(|| json!(reference));
                    }
                }
                if row_is_surface {
                    if let Some(id) = object.get("id").and_then(Value::as_str).map(str::to_owned) {
                        object
                            .entry("ref")
                            .or_insert_with(|| json!(mint("surface", &id)));
                    }
                }
                for child in object.values_mut() {
                    decorate(child, false, mint);
                }
            }
            _ => {}
        }
    }
    decorate(value, false, mint);
}

pub(super) fn resolve_request_handle_refs(
    app: &AppHandle,
    params: &mut serde_json::Map<String, Value>,
) {
    for (key, kind) in [
        ("window_id", "window"),
        ("group_id", "workspace_group"),
        ("before_group_id", "workspace_group"),
        ("after_group_id", "workspace_group"),
        ("workspace_id", "workspace"),
        ("group_reference_workspace_id", "workspace"),
        ("reference_workspace_id", "workspace"),
        ("surface_id", "surface"),
        ("terminal_id", "surface"),
        ("tab_id", "surface"),
        // Round 5 item 6: anchor refs resolve like every other surface
        // selector before the uuid-counting anchor validation.
        ("before_surface_id", "surface"),
        ("after_surface_id", "surface"),
        ("pane_id", "pane"),
    ] {
        let Some(reference) = params.get(key).and_then(Value::as_str) else {
            continue;
        };
        let normalized = (key == "tab_id")
            .then(|| surface_ref_from_tab_ref(reference))
            .flatten();
        if let Some(id) =
            resolve_control_handle_ref(app, kind, normalized.as_deref().unwrap_or(reference))
        {
            params.insert(key.to_string(), json!(id));
        }
    }
}

pub(super) fn tab_ref_from_surface_ref(reference: &str) -> String {
    reference
        .strip_prefix("surface:")
        .map_or_else(|| reference.to_string(), |suffix| format!("tab:{suffix}"))
}

pub(super) fn surface_ref_from_tab_ref(reference: &str) -> Option<String> {
    reference
        .strip_prefix("tab:")
        .map(|suffix| format!("surface:{suffix}"))
}
