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
    AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, WebviewWindow, WindowEvent,
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

fn ordered_control_windows(app: &AppHandle) -> Vec<(WindowControlIdentity, WebviewWindow)> {
    let mut windows = app.webview_windows();
    ordered_window_identities(windows.keys().cloned())
        .into_iter()
        .filter_map(|identity| {
            windows
                .remove(&identity.label)
                .map(|window| (identity, window))
        })
        .collect()
}

fn move_window_to_display(window: &WebviewWindow, query: &str) -> Result<(), String> {
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
    move_window_to_display(window, &query).or_else(|error| {
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
    let config = cloned_main_window_config(&app, label.clone())?;
    let new_window = tauri::WebviewWindowBuilder::from_config(&app, &config)
        .map_err(|error| error.to_string())?
        .build()
        .map_err(|error| error.to_string())?;
    if let Err(error) = apply_default_display(&new_window) {
        eprintln!("[window] failed to apply default display: {error}");
    }
    if let Ok(title) = window.title() {
        let _ = new_window.set_title(&title);
    }
    install_window_state_listener(&new_window);
    let _ = emit_window_state(&new_window);
    Ok(label)
}

#[tauri::command]
pub fn window_close(window: WebviewWindow) -> Result<(), String> {
    window.close().map_err(|error| error.to_string())
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
    use super::{task_manager_command, WindowStateSnapshot, AUX_WINDOW_LABEL_PREFIX};

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
