//! Native window-chrome bridge for the desktop shell.
//!
//! Exposes the current window's chrome state (maximized + resolved native
//! title) and a minimal command surface for caption buttons. The frontend title
//! bar consumes this so its presentation stays in sync with the actual Tauri
//! window, including windows created at runtime from the command palette.

use std::sync::atomic::{AtomicU64, Ordering};

use cmux_core::window_display::{
    centered_window_geometry, matching_monitor_index, ordered_window_identities,
    resolve_window_selector, WindowControlIdentity,
};
use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, WebviewWindow, Window, WindowEvent,
};

const MAIN_WINDOW_LABEL: &str = "main";
const AUX_WINDOW_LABEL_PREFIX: &str = "window-";
const WINDOW_STATE_CHANGED_EVENT: &str = "cmux://window-state-changed";
static NEXT_WINDOW_NUMBER: AtomicU64 = AtomicU64::new(2);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowDisplayInfo {
    pub name: String,
    pub index: usize,
    pub is_main: bool,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowDisplayMoveResult {
    pub display: String,
    pub moved: Vec<WindowControlIdentity>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowControlSummary {
    pub identity: WindowControlIdentity,
    pub is_key: bool,
    pub is_visible: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WindowDisplayMoveError {
    WindowNotFound(String),
    DisplayNotFound {
        requested: String,
        available: Vec<String>,
    },
    Internal(String),
}

pub fn available_displays(app: &AppHandle) -> Result<Vec<WindowDisplayInfo>, String> {
    let monitors = app
        .available_monitors()
        .map_err(|error| error.to_string())?;
    let primary = app.primary_monitor().map_err(|error| error.to_string())?;
    Ok(monitors
        .iter()
        .enumerate()
        .map(|(index, monitor)| {
            let position = monitor.position();
            let size = monitor.size();
            let is_main = primary.as_ref().is_some_and(|primary| {
                primary.position() == position
                    && primary.size() == size
                    && primary.name() == monitor.name()
            });
            WindowDisplayInfo {
                name: monitor
                    .name()
                    .cloned()
                    .unwrap_or_else(|| "(unknown)".into()),
                index,
                is_main,
                x: position.x,
                y: position.y,
                width: size.width,
                height: size.height,
            }
        })
        .collect())
}

pub fn move_control_windows_to_display(
    app: &AppHandle,
    query: &str,
    selector: Option<&str>,
) -> Result<WindowDisplayMoveResult, WindowDisplayMoveError> {
    let mut windows = ordered_control_windows(app);
    if let Some(selector) = selector {
        let identities: Vec<_> = windows
            .iter()
            .map(|(identity, _)| identity.clone())
            .collect();
        let Some(index) = resolve_window_selector(&identities, selector) else {
            return Err(WindowDisplayMoveError::WindowNotFound(selector.to_string()));
        };
        windows = vec![windows.remove(index)];
    }
    let displays = available_displays(app).map_err(WindowDisplayMoveError::Internal)?;
    let names: Vec<_> = displays
        .iter()
        .map(|display| Some(display.name.clone()))
        .collect();
    let Some(display_index) = matching_monitor_index(&names, query) else {
        return Err(WindowDisplayMoveError::DisplayNotFound {
            requested: query.to_string(),
            available: displays.into_iter().map(|display| display.name).collect(),
        });
    };
    let resolved_display = displays[display_index].name.clone();
    let mut moved = Vec::with_capacity(windows.len());
    for (identity, window) in windows {
        move_window_to_display(&window, &resolved_display)
            .map_err(WindowDisplayMoveError::Internal)?;
        moved.push(identity);
    }
    Ok(WindowDisplayMoveResult {
        display: resolved_display,
        moved,
    })
}

pub fn control_window_summaries(app: &AppHandle) -> Vec<WindowControlSummary> {
    ordered_control_windows(app)
        .into_iter()
        .map(|(identity, window)| WindowControlSummary {
            identity,
            is_key: window.is_focused().unwrap_or(false),
            is_visible: window.is_visible().unwrap_or(false),
        })
        .collect()
}

pub fn current_control_window(
    app: &AppHandle,
    selector: Option<&str>,
) -> Option<WindowControlIdentity> {
    let summaries = control_window_summaries(app);
    if let Some(selector) = selector {
        let identities: Vec<_> = summaries
            .iter()
            .map(|summary| summary.identity.clone())
            .collect();
        return resolve_window_selector(&identities, selector)
            .map(|index| summaries[index].identity.clone());
    }
    summaries
        .iter()
        .find(|summary| summary.is_key)
        .or_else(|| summaries.first())
        .map(|summary| summary.identity.clone())
}

/// Focus a scriptable window without activating another application. The
/// caller supplies the same UUID/ref/label selector accepted by window.list.
pub fn focus_control_window(app: &AppHandle, selector: &str) -> Result<(), String> {
    let windows = ordered_control_windows(app);
    let identities: Vec<_> = windows
        .iter()
        .map(|(identity, _)| identity.clone())
        .collect();
    let index = resolve_window_selector(&identities, selector)
        .ok_or_else(|| format!("Window not found: {selector}"))?;
    let window = &windows[index].1;
    window.show().map_err(|error| error.to_string())?;
    window.set_focus().map_err(|error| error.to_string())
}

fn ordered_control_windows(app: &AppHandle) -> Vec<(WindowControlIdentity, Window)> {
    let mut windows = app.windows();
    ordered_window_identities(windows.keys().cloned())
        .into_iter()
        .filter_map(|identity| {
            windows
                .remove(&identity.label)
                .map(|window| (identity, window))
        })
        .collect()
}

fn move_window_to_display(window: &Window, query: &str) -> Result<(), String> {
    let monitors = window
        .available_monitors()
        .map_err(|error| error.to_string())?;
    let names: Vec<_> = monitors
        .iter()
        .map(|monitor| monitor.name().cloned())
        .collect();
    let index = matching_monitor_index(&names, query)
        .ok_or_else(|| format!("Display not found: {query}"))?;
    let monitor = &monitors[index];
    let work_area = monitor.work_area();
    let window_size = window.outer_size().map_err(|error| error.to_string())?;
    let (position, size) = centered_window_geometry(
        (work_area.position.x, work_area.position.y),
        (work_area.size.width, work_area.size.height),
        (window_size.width, window_size.height),
    );
    if size != (window_size.width, window_size.height) {
        window
            .set_size(PhysicalSize::new(size.0, size.1))
            .map_err(|error| error.to_string())?;
    }
    window
        .set_position(PhysicalPosition::new(position.0, position.1))
        .map_err(|error| error.to_string())
}

#[cfg(debug_assertions)]
fn apply_default_display(window: &WebviewWindow) -> Result<(), String> {
    let Some(config_path) = cmux_config::config_path() else {
        return Ok(());
    };
    let Some(query) = cmux_config::dev_window_display_at(&config_path)
        .ok()
        .flatten()
    else {
        return Ok(());
    };
    move_window_to_display(&window.as_ref().window(), &query).or_else(|error| {
        if error.starts_with("Display not found:") {
            Ok(())
        } else {
            Err(error)
        }
    })
}

#[cfg(not(debug_assertions))]
fn apply_default_display(_window: &WebviewWindow) -> Result<(), String> {
    Ok(())
}

pub fn apply_default_display_to_existing_windows(app: &AppHandle) {
    for window in app.webview_windows().into_values() {
        if let Err(error) = apply_default_display(&window) {
            eprintln!("[window] failed to apply default display: {error}");
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowStateSnapshot {
    pub is_maximized: bool,
    pub title: String,
}

fn read_window_state(window: &WebviewWindow) -> Result<WindowStateSnapshot, String> {
    Ok(WindowStateSnapshot {
        is_maximized: window.is_maximized().map_err(|error| error.to_string())?,
        title: window.title().map_err(|error| error.to_string())?,
    })
}

fn next_window_label(app: &AppHandle) -> String {
    loop {
        let number = NEXT_WINDOW_NUMBER.fetch_add(1, Ordering::Relaxed);
        let label = format!("{AUX_WINDOW_LABEL_PREFIX}{number}");
        if !app.webview_windows().contains_key(&label) {
            return label;
        }
    }
}

fn cloned_main_window_config(
    app: &AppHandle,
    label: String,
) -> Result<tauri::utils::config::WindowConfig, String> {
    let mut config = app
        .config()
        .app
        .windows
        .iter()
        .find(|config| config.label == MAIN_WINDOW_LABEL)
        .cloned()
        .or_else(|| app.config().app.windows.first().cloned())
        .ok_or_else(|| "missing base window config".to_owned())?;
    config.label = label;
    Ok(config)
}

fn append_compensation_failures(primary: String, failures: Vec<String>) -> String {
    if failures.is_empty() {
        primary
    } else {
        format!("{primary}; compensation failed: {}", failures.join("; "))
    }
}

fn close_main_window(window: &WebviewWindow) -> Result<(), String> {
    window.close().map_err(|error| error.to_string())
}

pub fn emit_window_state(window: &WebviewWindow) -> Result<WindowStateSnapshot, String> {
    let snapshot = read_window_state(&window)?;
    window
        .emit(WINDOW_STATE_CHANGED_EVENT, snapshot.clone())
        .map_err(|error| error.to_string())?;
    Ok(snapshot)
}

pub fn emit_window_states(app: &AppHandle) {
    for window in app.webview_windows().into_values() {
        let _ = emit_window_state(&window);
    }
}

pub fn install_window_state_listener(window: &WebviewWindow) {
    let listener_window = window.clone();
    window.clone().on_window_event(move |event| {
        if matches!(
            event,
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. }
        ) {
            let _ = emit_window_state(&listener_window);
        }
        // Key-window transitions repoint the control socket's active-window
        // pointer (canonical CmuxLifecycleEventPublishing.swift:258-268).
        if matches!(event, WindowEvent::Focused(true)) {
            crate::control_socket::note_window_focused(
                listener_window.app_handle(),
                listener_window.label(),
            );
        }
    });
}

pub fn install_window_state_listeners(app: &AppHandle) {
    for window in app.webview_windows().into_values() {
        install_window_state_listener(&window);
    }
}

#[tauri::command]
pub fn window_state(window: WebviewWindow) -> Result<WindowStateSnapshot, String> {
    read_window_state(&window)
}

#[tauri::command]
pub fn window_minimize(window: WebviewWindow) -> Result<WindowStateSnapshot, String> {
    window.minimize().map_err(|error| error.to_string())?;
    emit_window_state(&window)
}

#[tauri::command]
pub fn window_toggle_maximize(window: WebviewWindow) -> Result<WindowStateSnapshot, String> {
    if window.is_maximized().map_err(|error| error.to_string())? {
        window.unmaximize().map_err(|error| error.to_string())?;
    } else {
        window.maximize().map_err(|error| error.to_string())?;
    }
    emit_window_state(&window)
}

#[tauri::command]
pub fn window_toggle_fullscreen(window: WebviewWindow) -> Result<WindowStateSnapshot, String> {
    let next_fullscreen = !window.is_fullscreen().map_err(|error| error.to_string())?;
    window
        .set_fullscreen(next_fullscreen)
        .map_err(|error| error.to_string())?;
    emit_window_state(&window)
}

#[tauri::command]
pub async fn window_new(app: AppHandle, window: WebviewWindow) -> Result<String, String> {
    let label = next_window_label(&app);
    create_window_for_label(&app, &label, window.title().ok().as_deref(), true, None)?;
    Ok(label)
}

/// Create an auxiliary window on behalf of the control socket with a
/// UUID label allocated by the lifecycle transition, WITHOUT focusing it —
/// the Windows mapping of
/// canonical orderFront-only socket window creation (window.create is not
/// focus-intent: AppDelegate.swift:8862-8868 at pinned e1825d40d).
pub(crate) fn create_socket_window(
    app: &AppHandle,
    snapshot: &cmux_core::session::SessionWindowSnapshot,
) -> Result<(), String> {
    let label = snapshot
        .window_id
        .as_deref()
        .ok_or_else(|| "Socket window is missing its identity".to_string())?;
    create_window_for_label(app, label, None, false, Some(snapshot))
}

/// Close a window on behalf of the control socket, keyed by webview label.
/// A model-only window (no webview) is still removed from the session model
/// so v2 window.close stays authoritative over the snapshot.
pub(crate) fn close_socket_window(app: &AppHandle, label: &str) -> Result<(), String> {
    let Some(window) = app.get_webview_window(label) else {
        let state = app.state::<crate::session::SessionState>();
        return crate::session::unregister_window_for_control_suppressing_events(
            app,
            state.inner(),
            label,
        )
        .map(|_| ());
    };
    close_webview_window(window, true)
}

fn close_webview_window(
    window: WebviewWindow,
    suppress_derived_events: bool,
) -> Result<(), String> {
    let label = window.label().to_string();
    let app = window.app_handle().clone();
    if label == MAIN_WINDOW_LABEL {
        return close_main_window(&window);
    }
    window.hide().map_err(|error| error.to_string())?;
    let state = app.state::<crate::session::SessionState>();
    let unregistration = if suppress_derived_events {
        crate::session::unregister_window_for_control_suppressing_events(
            &app,
            state.inner(),
            &label,
        )
    } else {
        crate::session::unregister_window_for_control(&app, state.inner(), &label)
    };
    let outcome = match unregistration {
        Ok(outcome) => outcome,
        Err(message) => {
            let failures = window
                .show()
                .err()
                .map(|error| vec![error.to_string()])
                .unwrap_or_default();
            return Err(append_compensation_failures(message, failures));
        }
    };
    let crate::session::UnregisterWindowOutcome::Removed { lease, .. } = outcome else {
        return window.close().map_err(|error| {
            let failures = window
                .show()
                .err()
                .map(|show_error| vec![show_error.to_string()])
                .unwrap_or_default();
            append_compensation_failures(error.to_string(), failures)
        });
    };
    if let Err(error) = window.close() {
        let mut failures = Vec::new();
        if let Err(error) =
            crate::session::restore_removed_window_for_control(&app, state.inner(), &lease)
        {
            failures.push(error);
        }
        if let Err(error) = window.show() {
            failures.push(error.to_string());
        }
        return Err(append_compensation_failures(error.to_string(), failures));
    }
    Ok(())
}

fn create_window_for_label(
    app: &AppHandle,
    label: &str,
    title: Option<&str>,
    focused: bool,
    prepared_snapshot: Option<&cmux_core::session::SessionWindowSnapshot>,
) -> Result<(), String> {
    let new_window = build_hidden_window(app, label, focused)?;
    if let Some(title) = title {
        let _ = new_window.set_title(title);
    }
    let state = app.state::<crate::session::SessionState>();
    let registration = match prepared_snapshot {
        Some(snapshot) => {
            crate::session::register_prepared_window_for_control(app, state.inner(), snapshot)
        }
        None => crate::session::register_window_for_control(app, state.inner(), label),
    };
    if let Err(message) = registration {
        let failures = new_window
            .close()
            .err()
            .map(|error| vec![error.to_string()])
            .unwrap_or_default();
        return Err(append_compensation_failures(message, failures));
    }
    install_window_state_listener(&new_window);
    if let Err(error) = new_window.show() {
        let mut failures = Vec::new();
        let unregistration = if prepared_snapshot.is_some() {
            crate::session::unregister_window_for_control_suppressing_events(
                app,
                state.inner(),
                label,
            )
        } else {
            crate::session::unregister_window_for_control(app, state.inner(), label)
        };
        match unregistration {
            Ok(crate::session::UnregisterWindowOutcome::Removed { .. }) => {}
            Ok(crate::session::UnregisterWindowOutcome::Unchanged(_)) => {
                failures.push(format!("registered window model {label} was not removed"))
            }
            Err(error) => failures.push(error),
        }
        if let Err(error) = new_window.close() {
            failures.push(error.to_string());
        }
        return Err(append_compensation_failures(error.to_string(), failures));
    }
    let _ = emit_window_state(&new_window);
    Ok(())
}

fn build_hidden_window(
    app: &AppHandle,
    label: &str,
    focused: bool,
) -> Result<WebviewWindow, String> {
    let mut config = cloned_main_window_config(app, label.to_owned())?;
    config.visible = false;
    let new_window = tauri::WebviewWindowBuilder::from_config(app, &config)
        .map_err(|error| error.to_string())?
        .focused(focused)
        .build()
        .map_err(|error| error.to_string())?;
    if let Err(error) = apply_default_display(&new_window) {
        eprintln!("[window] failed to apply default display: {error}");
    }
    Ok(new_window)
}

pub(crate) fn build_hidden_restored_window(app: &AppHandle, label: &str) -> Result<(), String> {
    let window = build_hidden_window(app, label, false)?;
    install_window_state_listener(&window);
    Ok(())
}

fn restored_window(app: &AppHandle, label: &str) -> Result<WebviewWindow, String> {
    app.get_webview_window(label)
        .ok_or_else(|| format!("Restored window {label} is unavailable"))
}

pub(crate) fn show_restored_window_unfocused(app: &AppHandle, label: &str) -> Result<(), String> {
    restored_window(app, label)?
        .show()
        .map_err(|error| error.to_string())
}

pub(crate) fn close_restored_window(app: &AppHandle, label: &str) -> Result<(), String> {
    restored_window(app, label)?
        .close()
        .map_err(|error| error.to_string())
}

pub(crate) fn activate_restored_window(app: &AppHandle, label: &str) -> Result<(), String> {
    restored_window(app, label)?
        .set_focus()
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn window_close(window: WebviewWindow) -> Result<(), String> {
    close_webview_window(window, false)
}

#[cfg(target_os = "windows")]
fn task_manager_command() -> (&'static str, &'static [&'static str]) {
    ("taskmgr.exe", &[])
}

#[cfg(target_os = "macos")]
fn task_manager_command() -> (&'static str, &'static [&'static str]) {
    ("open", &["-a", "Activity Monitor"])
}

#[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
fn task_manager_command() -> (&'static str, &'static [&'static str]) {
    ("xdg-open", &["/"])
}

#[tauri::command]
pub fn window_open_task_manager() -> Result<(), String> {
    let (program, args) = task_manager_command();
    std::process::Command::new(program)
        .args(args)
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("failed to open task manager: {error}"))
}

#[cfg(test)]
mod tests {
    use super::{
        capture_windows_hidden_for_value, task_manager_command, WindowStateSnapshot,
        AUX_WINDOW_LABEL_PREFIX,
    };
    use std::ffi::OsStr;

    #[test]
    fn capture_headless_mode_requires_an_explicit_one() {
        assert!(!capture_windows_hidden_for_value(None));
        assert!(!capture_windows_hidden_for_value(Some(OsStr::new("0"))));
        assert!(capture_windows_hidden_for_value(Some(OsStr::new("1"))));
    }

    #[test]
    fn configured_main_window_starts_hidden_until_setup() {
        let config: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).expect("valid Tauri config");
        assert_eq!(config["app"]["windows"][0]["visible"], false);
    }

    #[test]
    fn window_state_snapshot_serializes_with_camel_case_keys() {
        let value = serde_json::to_value(WindowStateSnapshot {
            is_maximized: true,
            title: "Workspace".to_owned(),
        })
        .expect("window state serializes");
        assert_eq!(
            value,
            serde_json::json!({
                "isMaximized": true,
                "title": "Workspace",
            })
        );
    }

    #[test]
    fn auxiliary_window_label_prefix_stays_stable() {
        assert_eq!(AUX_WINDOW_LABEL_PREFIX, "window-");
    }

    #[test]
    fn task_manager_command_is_platform_specific() {
        let (program, _args) = task_manager_command();
        #[cfg(target_os = "windows")]
        assert_eq!(program, "taskmgr.exe");
        #[cfg(target_os = "macos")]
        assert_eq!(program, "open");
        #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
        assert_eq!(program, "xdg-open");
    }
}
