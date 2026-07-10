//! `cmux.json` load/save bridge for the desktop web shell.
//!
//! The web Settings UI edits typed dotted-key leaves across the rendered config
//! sections (app, automation, browser, terminal, markdown, canvas, file editor,
//! file explorer, diff viewer, workspace colors, sidebar, sidebar appearance,
//! notifications, and escaped shortcut binding keys). To preserve unknown keys and future nested
//! sections, this layer keeps the RAW JSON tree and applies per-leaf patches via
//! `cmux_config::JsonPath`, then validates the result through the typed
//! `cmux-config` decoder before writing it back.
//!
//! E5 (file watching / live external reload) is intentionally separate. This
//! module caches the last raw tree in-process purely so repeated saves preserve
//! unknown nested keys without re-reading between every toggle.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

use cmux_config::{
    ghostty_config_path, AppConfig, AutomationConfig, BrowserConfig, CanvasConfig, Config,
    DiffViewerConfig, FileEditorConfig, FileExplorerConfig, JsonPath, MarkdownConfig,
    NotificationsConfig, ShortcutsConfig, SidebarAppearanceConfig, SidebarConfig, TerminalConfig,
    WorkspaceColorsConfig,
};
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager, State};

const CONFIG_CHANGED_EVENT: &str = "cmux://config-changed";

/// In-memory raw `cmux.json` cache. `None` means "lazy-load from disk on first
/// command". External edits are not observed until the next explicit load lane.
#[derive(Default)]
pub struct ConfigState {
    raw: Mutex<Option<Value>>,
    watcher_started: Mutex<bool>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ConfigChangedPayload {
    pub config: Config,
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct RawConfigFile {
    pub path: String,
    pub contents: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ConfigExtensionStatus {
    pub config_path: String,
    pub actions_count: usize,
    pub commands_count: usize,
    pub surface_tab_bar_buttons_count: usize,
    pub ui_configured: bool,
    pub workspace_groups_count: usize,
    pub unknown_top_level_keys: Vec<String>,
    pub beta_feature_keys: Vec<String>,
}

/// Resolve the global `cmux.json` path for the current user.
fn config_file_path() -> Result<PathBuf, String> {
    cmux_config::config_path().ok_or_else(|| "unable to determine config directory".to_owned())
}

/// Read the raw JSON document from `path`, defaulting a missing file to `{}`.
fn load_raw_config(path: &Path) -> Result<Value, String> {
    if !path.exists() {
        return Ok(Value::Object(serde_json::Map::new()));
    }
    let bytes = std::fs::read(path)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    let raw: Value = serde_json::from_slice(&bytes)
        .map_err(|error| format!("failed to parse {}: {error}", path.display()))?;
    if !raw.is_object() {
        return Err(format!(
            "{} must contain a top-level JSON object",
            path.display()
        ));
    }
    Ok(raw)
}

/// Decode the raw JSON tree through the typed config model, then materialize
/// the sections the current Settings UI renders so the pane is always complete.
fn decode_settings_config(raw: &Value) -> Result<Config, String> {
    let json = serde_json::to_string(raw).map_err(|error| error.to_string())?;
    let mut config = cmux_config::decode_config(&json).map_err(|error| error.to_string())?;
    config.app.get_or_insert_with(AppConfig::default);
    config
        .automation
        .get_or_insert_with(AutomationConfig::default);
    config.sidebar.get_or_insert_with(SidebarConfig::default);
    config
        .notifications
        .get_or_insert_with(NotificationsConfig::default);
    config
        .shortcuts
        .get_or_insert_with(ShortcutsConfig::default);
    config.browser.get_or_insert_with(BrowserConfig::default);
    config.terminal.get_or_insert_with(TerminalConfig::default);
    config.markdown.get_or_insert_with(MarkdownConfig::default);
    config.canvas.get_or_insert_with(CanvasConfig::default);
    config
        .file_editor
        .get_or_insert_with(FileEditorConfig::default);
    config
        .file_explorer
        .get_or_insert_with(FileExplorerConfig::default);
    config
        .diff_viewer
        .get_or_insert_with(DiffViewerConfig::default);
    config
        .workspace_colors
        .get_or_insert_with(WorkspaceColorsConfig::default);
    config
        .sidebar_appearance
        .get_or_insert_with(SidebarAppearanceConfig::default);
    Ok(config)
}

/// Best-effort current app language from `cmux.json`.
///
/// Settings materializes `app.language`, but host-only call sites (for example
/// the agent-session `app.context` copy fixture) also need a cheap non-command
/// read. Any filesystem or decode problem falls back to the schema default,
/// matching the Settings UI's "system" default rather than failing the caller.
pub(crate) fn current_app_language() -> String {
    let default_language = AppConfig::default().language;
    let Ok(path) = config_file_path() else {
        return default_language;
    };
    let Ok(raw) = load_raw_config(&path) else {
        return default_language;
    };
    let Ok(config) = decode_settings_config(&raw) else {
        return default_language;
    };
    config
        .app
        .map(|app| app.language)
        .unwrap_or(default_language)
}

/// Best-effort current markdown viewer defaults from `cmux.json`.
pub(crate) fn current_markdown_config() -> MarkdownConfig {
    let default_config = MarkdownConfig::default();
    let Ok(path) = config_file_path() else {
        return default_config;
    };
    let Ok(raw) = load_raw_config(&path) else {
        return default_config;
    };
    let Ok(config) = decode_settings_config(&raw) else {
        return default_config;
    };
    config.markdown.unwrap_or(default_config)
}

fn config_changed_payload(path: &Path, raw: Value) -> Result<ConfigChangedPayload, String> {
    let config = decode_settings_config(&raw)?;
    Ok(ConfigChangedPayload {
        config,
        path: path.to_string_lossy().to_string(),
    })
}

fn reload_config_from_disk(
    state: &ConfigState,
    path: &Path,
) -> Result<ConfigChangedPayload, String> {
    let raw = load_raw_config(path)?;
    let payload = config_changed_payload(path, raw.clone())?;
    *state.raw.lock().expect("config raw mutex poisoned") = Some(raw);
    Ok(payload)
}

fn reload_config_at_path(app: &AppHandle, path: &Path) -> Result<ConfigChangedPayload, String> {
    let state = app.state::<ConfigState>();
    let payload = reload_config_from_disk(&state, path)?;
    app.emit(CONFIG_CHANGED_EVENT, payload.clone())
        .map_err(|error| format!("failed to emit config reload event: {error}"))?;
    Ok(payload)
}

pub(crate) fn reload_config_for_control(app: &AppHandle) -> Result<ConfigChangedPayload, String> {
    let path = config_file_path()?;
    reload_config_at_path(app, &path)
}

fn parse_raw_config_contents(contents: &str) -> Result<Value, String> {
    let raw: Value = serde_json::from_str(contents)
        .map_err(|error| format!("failed to parse edited cmux.json: {error}"))?;
    if !raw.is_object() {
        return Err("cmux.json must contain a top-level JSON object".to_owned());
    }
    Ok(raw)
}

fn config_file_modified(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .ok()
}

pub fn start_config_file_watcher(app: &AppHandle) -> Result<(), String> {
    let path = config_file_path()?;
    let state = app.state::<ConfigState>();
    {
        let mut started = state
            .watcher_started
            .lock()
            .expect("config watcher mutex poisoned");
        if *started {
            return Ok(());
        }
        *started = true;
    }

    let app = app.clone();
    std::thread::Builder::new()
        .name("cmux-config-watch".to_owned())
        .spawn(move || {
            let mut last_modified = config_file_modified(&path);
            loop {
                std::thread::sleep(Duration::from_millis(750));
                let next_modified = config_file_modified(&path);
                if next_modified == last_modified {
                    continue;
                }
                last_modified = next_modified;
                if let Err(error) = reload_config_at_path(&app, &path) {
                    eprintln!("[config] failed to reload changed cmux.json: {error}");
                }
            }
        })
        .map(|_| ())
        .map_err(|error| format!("failed to start config watcher: {error}"))
}

fn config_extension_status_from_raw(path: &Path, raw: &Value) -> ConfigExtensionStatus {
    let actions_count = object_entry_count(raw, "actions");
    let commands_count = array_entry_count(raw, "commands");
    let surface_tab_bar_buttons_count = array_entry_count(raw, "surfaceTabBarButtons")
        + array_entry_count(raw, "surface_tab_bar_buttons");
    let workspace_groups_count = workspace_groups_count(
        raw.get("workspaceGroups")
            .or_else(|| raw.get("workspace_groups")),
    );
    let ui_configured = raw
        .get("ui")
        .is_some_and(|value| value.as_object().is_some_and(|object| !object.is_empty()));
    let unknown_top_level_keys = unknown_top_level_keys(raw);
    let beta_feature_keys = beta_feature_keys(raw, &unknown_top_level_keys);

    ConfigExtensionStatus {
        config_path: path.to_string_lossy().to_string(),
        actions_count,
        commands_count,
        surface_tab_bar_buttons_count,
        ui_configured,
        workspace_groups_count,
        unknown_top_level_keys,
        beta_feature_keys,
    }
}

fn object_entry_count(raw: &Value, key: &str) -> usize {
    raw.get(key)
        .and_then(Value::as_object)
        .map_or(0, serde_json::Map::len)
}

fn array_entry_count(raw: &Value, key: &str) -> usize {
    raw.get(key).and_then(Value::as_array).map_or(0, Vec::len)
}

fn workspace_groups_count(value: Option<&Value>) -> usize {
    value
        .and_then(|value| value.get("byCwd").or_else(|| value.get("by_cwd")))
        .and_then(Value::as_object)
        .map_or(0, serde_json::Map::len)
}

fn unknown_top_level_keys(raw: &Value) -> Vec<String> {
    let Some(object) = raw.as_object() else {
        return Vec::new();
    };
    let mut keys: Vec<String> = object
        .keys()
        .filter(|key| !is_known_top_level_config_key(key))
        .cloned()
        .collect();
    keys.sort();
    keys
}

fn is_known_top_level_config_key(key: &str) -> bool {
    matches!(
        key,
        "$schema"
            | "schema"
            | "schemaVersion"
            | "schema_version"
            | "app"
            | "terminal"
            | "notifications"
            | "sidebar"
            | "workspaceColors"
            | "workspace_colors"
            | "sidebarAppearance"
            | "sidebar_appearance"
            | "automation"
            | "browser"
            | "markdown"
            | "canvas"
            | "fileEditor"
            | "file_editor"
            | "fileExplorer"
            | "file_explorer"
            | "diffViewer"
            | "diff_viewer"
            | "shortcuts"
            | "vault"
            | "workspaceGroups"
            | "workspace_groups"
            | "newWorkspaceCommand"
            | "new_workspace_command"
            | "actions"
            | "ui"
            | "commands"
            | "surfaceTabBarButtons"
            | "surface_tab_bar_buttons"
    )
}

fn beta_feature_keys(raw: &Value, unknown_keys: &[String]) -> Vec<String> {
    let mut keys = Vec::new();
    for key in unknown_keys {
        let normalized = key.to_ascii_lowercase();
        if normalized.contains("beta")
            || normalized.contains("experimental")
            || normalized.contains("feature")
            || normalized.contains("flag")
        {
            keys.push(key.clone());
            if let Some(object) = raw.get(key).and_then(Value::as_object) {
                keys.extend(object.keys().map(|child| format!("{key}.{child}")));
            }
        }
    }
    keys.sort();
    keys.dedup();
    keys
}

/// Apply one JSON-path mutation to the raw config tree.
fn apply_config_mutation(
    raw: &mut Value,
    path: &str,
    value: Option<Value>,
    remove: bool,
) -> Result<(), String> {
    let path = JsonPath::parse(path).map_err(|error| error.to_string())?;
    if remove {
        path.remove(raw);
        return Ok(());
    }
    let value =
        value.ok_or_else(|| "config_save requires a value when remove is false".to_owned())?;
    path.assign(raw, value);
    Ok(())
}

/// Persist the raw JSON tree as pretty-printed `cmux.json`.
fn write_raw_config(path: &Path, raw: &Value) -> Result<(), String> {
    let Some(parent) = path.parent() else {
        return Err(format!(
            "config path {} has no parent directory",
            path.display()
        ));
    };
    std::fs::create_dir_all(parent)
        .map_err(|error| format!("failed to create {}: {error}", parent.display()))?;
    let bytes = serde_json::to_vec_pretty(raw).map_err(|error| error.to_string())?;
    std::fs::write(path, bytes)
        .map_err(|error| format!("failed to write {}: {error}", path.display()))?;
    Ok(())
}

fn ensure_config_file_exists(path: &Path) -> Result<(), String> {
    if path.exists() {
        return Ok(());
    }
    write_raw_config(path, &Value::Object(serde_json::Map::new()))
}

fn ensure_text_file_exists(path: &Path) -> Result<(), String> {
    if path.exists() {
        return Ok(());
    }
    let Some(parent) = path.parent() else {
        return Err(format!(
            "config path {} has no parent directory",
            path.display()
        ));
    };
    std::fs::create_dir_all(parent)
        .map_err(|error| format!("failed to create {}: {error}", parent.display()))?;
    std::fs::write(path, [])
        .map_err(|error| format!("failed to create {}: {error}", path.display()))
}

fn open_path_in_editor(path: &Path) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut command = std::process::Command::new("notepad");
        command.arg(path);
        command
    };

    #[cfg(target_os = "macos")]
    let mut command = {
        let mut command = std::process::Command::new("open");
        command.arg("-e").arg(path);
        command
    };

    #[cfg(all(unix, not(target_os = "macos")))]
    let mut command = {
        let mut command = std::process::Command::new("xdg-open");
        command.arg(path);
        command
    };

    command
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("failed to open {}: {error}", path.display()))
}

fn ghostty_config_file_path() -> Result<PathBuf, String> {
    ghostty_config_path().ok_or_else(|| "unable to determine Ghostty config directory".to_owned())
}

/// Load the user's `cmux.json`, returning a Settings-ready typed config.
#[tauri::command]
pub fn config_load(state: State<'_, ConfigState>) -> Result<Config, String> {
    let path = config_file_path()?;
    let raw = load_raw_config(&path)?;
    let config = decode_settings_config(&raw)?;
    *state.raw.lock().expect("config raw mutex poisoned") = Some(raw);
    Ok(config)
}

/// Summarize config-backed extension and experimental surfaces for Settings.
#[tauri::command]
pub fn config_extension_status(
    state: State<'_, ConfigState>,
) -> Result<ConfigExtensionStatus, String> {
    let path = config_file_path()?;
    let mut cache = state.raw.lock().expect("config raw mutex poisoned");
    let raw = match cache.clone() {
        Some(raw) => raw,
        None => {
            let raw = load_raw_config(&path)?;
            *cache = Some(raw.clone());
            raw
        }
    };
    Ok(config_extension_status_from_raw(&path, &raw))
}

/// Patch one dotted-key leaf in `cmux.json`, preserving unknown keys.
#[tauri::command]
pub fn config_save(
    state: State<'_, ConfigState>,
    path: String,
    value: Option<Value>,
    remove: Option<bool>,
) -> Result<Config, String> {
    let file_path = config_file_path()?;
    let mut cache = state.raw.lock().expect("config raw mutex poisoned");
    let base_raw = cache.clone().unwrap_or(load_raw_config(&file_path)?);
    let mut next_raw = base_raw.clone();
    apply_config_mutation(&mut next_raw, &path, value, remove.unwrap_or(false))?;
    let config = decode_settings_config(&next_raw)?;
    write_raw_config(&file_path, &next_raw)?;
    *cache = Some(next_raw);
    Ok(config)
}

#[tauri::command]
pub fn config_reset(state: State<'_, ConfigState>) -> Result<Config, String> {
    let file_path = config_file_path()?;
    let next_raw = Value::Object(serde_json::Map::new());
    let config = decode_settings_config(&next_raw)?;
    write_raw_config(&file_path, &next_raw)?;
    *state.raw.lock().expect("config raw mutex poisoned") = Some(next_raw);
    Ok(config)
}

#[tauri::command]
pub fn open_cmux_settings_file() -> Result<(), String> {
    let path = config_file_path()?;
    ensure_config_file_exists(&path)?;
    open_path_in_editor(&path)
}

#[tauri::command]
pub fn config_settings_file_path() -> Result<String, String> {
    let path = config_file_path()?;
    ensure_config_file_exists(&path)?;
    Ok(path.to_string_lossy().to_string())
}

#[tauri::command]
pub fn config_read_raw(state: State<'_, ConfigState>) -> Result<RawConfigFile, String> {
    let path = config_file_path()?;
    ensure_config_file_exists(&path)?;
    let contents = std::fs::read_to_string(&path)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    let raw = load_raw_config(&path)?;
    *state.raw.lock().expect("config raw mutex poisoned") = Some(raw);
    Ok(RawConfigFile {
        path: path.to_string_lossy().to_string(),
        contents,
    })
}

#[tauri::command]
pub fn config_write_raw(state: State<'_, ConfigState>, contents: String) -> Result<Config, String> {
    let path = config_file_path()?;
    let raw = parse_raw_config_contents(&contents)?;
    let config = decode_settings_config(&raw)?;
    write_raw_config(&path, &raw)?;
    *state.raw.lock().expect("config raw mutex poisoned") = Some(raw);
    Ok(config)
}

#[tauri::command]
pub fn open_ghostty_settings_file() -> Result<(), String> {
    let path = ghostty_config_file_path()?;
    ensure_text_file_exists(&path)?;
    open_path_in_editor(&path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn decode_settings_config_materializes_the_supported_sections() {
        let config = decode_settings_config(&json!({})).expect("decode");
        assert!(config.app.is_some());
        assert!(config.automation.is_some());
        assert!(config.sidebar.is_some());
        assert!(config.notifications.is_some());
        assert!(config.shortcuts.is_some());
        assert!(config.browser.is_some());
        assert!(config.terminal.is_some());
        assert!(config.markdown.is_some());
        assert!(config.canvas.is_some());
        assert!(config.file_editor.is_some());
        assert!(config.file_explorer.is_some());
        assert!(config.diff_viewer.is_some());
        assert!(config.workspace_colors.is_some());
        assert!(config.sidebar_appearance.is_some());
    }

    #[test]
    fn config_changed_payload_decodes_settings_ready_config() {
        let path = Path::new("C:/cmux/cmux.json");
        let payload = config_changed_payload(path, json!({ "app": { "appearance": "dark" } }))
            .expect("payload");
        assert_eq!(payload.path, "C:/cmux/cmux.json");
        assert_eq!(
            payload.config.app.as_ref().map(|app| app.appearance),
            Some(cmux_config::Appearance::Dark)
        );
        assert!(payload.config.shortcuts.is_some());
    }

    #[test]
    fn reload_config_from_disk_replaces_the_raw_cache() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("cmux.json");
        std::fs::write(&path, r#"{"app":{"appearance":"dark"}}"#).expect("write config");
        let state = ConfigState::default();

        let payload = reload_config_from_disk(&state, &path).expect("reload config");

        assert_eq!(payload.path, path.to_string_lossy());
        assert_eq!(
            *state.raw.lock().expect("raw cache"),
            Some(json!({"app": {"appearance": "dark"}}))
        );
    }

    #[test]
    fn raw_config_editor_parser_requires_a_json_object() {
        assert!(parse_raw_config_contents(r#"{"app":{"appearance":"dark"}}"#).is_ok());
        assert!(parse_raw_config_contents("[]")
            .expect_err("arrays are rejected")
            .contains("top-level JSON object"));
        assert!(parse_raw_config_contents("{")
            .expect_err("invalid JSON is rejected")
            .contains("failed to parse edited cmux.json"));
    }

    #[test]
    fn apply_config_mutation_sets_and_removes_a_leaf() {
        let mut raw = json!({});
        apply_config_mutation(
            &mut raw,
            "app.appearance",
            Some(Value::String("dark".to_owned())),
            false,
        )
        .expect("set");
        assert_eq!(raw, json!({ "app": { "appearance": "dark" } }));

        apply_config_mutation(&mut raw, "app.appearance", None, true).expect("remove");
        assert_eq!(raw, json!({}));
    }

    #[test]
    fn apply_config_mutation_preserves_unknown_nested_keys() {
        let mut raw = json!({
            "sidebar": {
                "mystery": 7
            }
        });
        apply_config_mutation(
            &mut raw,
            "sidebar.showPorts",
            Some(Value::Bool(false)),
            false,
        )
        .expect("set");
        assert_eq!(
            raw,
            json!({
                "sidebar": {
                    "mystery": 7,
                    "showPorts": false
                }
            })
        );
    }

    #[test]
    fn apply_config_mutation_preserves_advanced_config_sections() {
        let mut raw = json!({
            "actions": {
                "ship": {
                    "title": "Ship",
                    "type": "command",
                    "command": "cargo test"
                }
            },
            "commands": [
                { "name": "ship", "command": "cargo test" }
            ],
            "ui": {
                "surfaceTabBar": {
                    "buttons": [ "newTerminal" ]
                }
            },
            "vault": {
                "agents": []
            },
            "workspaceGroups": {
                "newWorkspacePlacement": "top"
            },
            "surfaceTabBarButtons": [ { "command": "ls" } ],
            "newWorkspaceCommand": "ship"
        });

        apply_config_mutation(
            &mut raw,
            "app.appearance",
            Some(Value::String("dark".to_owned())),
            false,
        )
        .expect("set");

        assert_eq!(raw["actions"]["ship"]["command"], json!("cargo test"));
        assert_eq!(raw["commands"][0]["name"], json!("ship"));
        assert_eq!(
            raw["ui"]["surfaceTabBar"]["buttons"][0],
            json!("newTerminal")
        );
        assert_eq!(raw["vault"]["agents"], json!([]));
        assert_eq!(
            raw["workspaceGroups"]["newWorkspacePlacement"],
            json!("top")
        );
        assert_eq!(raw["surfaceTabBarButtons"][0]["command"], json!("ls"));
        assert_eq!(raw["newWorkspaceCommand"], json!("ship"));
        assert_eq!(raw["app"]["appearance"], json!("dark"));
        decode_settings_config(&raw).expect("advanced config still decodes");
    }

    #[test]
    fn config_extension_status_counts_config_backed_extension_surfaces() {
        let raw = json!({
            "actions": {
                "ship": { "title": "Ship" },
                "lint": { "title": "Lint" }
            },
            "commands": [
                { "name": "ship" }
            ],
            "surfaceTabBarButtons": [
                { "id": "ship" },
                { "id": "lint" }
            ],
            "ui": {
                "newWorkspace": { "contextMenu": true }
            },
            "workspaceGroups": {
                "byCwd": {
                    "C:/repo": { "name": "Repo" }
                }
            },
            "betaFeatures": {
                "feed": true,
                "dock": false
            },
            "experimentalThing": true,
            "app": {
                "appearance": "dark"
            }
        });

        let status = config_extension_status_from_raw(Path::new("C:/cmux/cmux.json"), &raw);

        assert_eq!(status.actions_count, 2);
        assert_eq!(status.commands_count, 1);
        assert_eq!(status.surface_tab_bar_buttons_count, 2);
        assert!(status.ui_configured);
        assert_eq!(status.workspace_groups_count, 1);
        assert_eq!(
            status.unknown_top_level_keys,
            vec!["betaFeatures".to_string(), "experimentalThing".to_string()]
        );
        assert_eq!(
            status.beta_feature_keys,
            vec![
                "betaFeatures".to_string(),
                "betaFeatures.dock".to_string(),
                "betaFeatures.feed".to_string(),
                "experimentalThing".to_string()
            ]
        );
    }

    #[test]
    fn apply_config_mutation_uses_backend_wire_section_names() {
        let mut raw = json!({});
        apply_config_mutation(
            &mut raw,
            "sidebarAppearance.tintColor",
            Some(Value::String("#123456".to_owned())),
            false,
        )
        .expect("set sidebar appearance");
        apply_config_mutation(
            &mut raw,
            "fileEditor.wordWrap",
            Some(Value::Bool(true)),
            false,
        )
        .expect("set file editor");
        apply_config_mutation(
            &mut raw,
            "fileExplorer.doubleClickAction",
            Some(Value::String("preferredEditor".to_owned())),
            false,
        )
        .expect("set file explorer");
        apply_config_mutation(
            &mut raw,
            "diffViewer.defaultLayout",
            Some(Value::String("split".to_owned())),
            false,
        )
        .expect("set diff viewer");

        assert_eq!(
            raw,
            json!({
                "sidebarAppearance": { "tintColor": "#123456" },
                "fileEditor": { "wordWrap": true },
                "fileExplorer": { "doubleClickAction": "preferredEditor" },
                "diffViewer": { "defaultLayout": "split" }
            })
        );

        let config = decode_settings_config(&raw).expect("decode");
        assert_eq!(
            config
                .sidebar_appearance
                .as_ref()
                .expect("sidebar appearance")
                .tint_color,
            "#123456"
        );
        assert!(config.file_editor.as_ref().expect("file editor").word_wrap);
        assert_eq!(
            serde_json::to_value(
                config
                    .file_explorer
                    .as_ref()
                    .expect("file explorer")
                    .double_click_action
            )
            .expect("serialize file explorer action"),
            json!("preferredEditor")
        );
        assert_eq!(
            serde_json::to_value(
                config
                    .diff_viewer
                    .as_ref()
                    .expect("diff viewer")
                    .default_layout
            )
            .expect("serialize diff viewer layout"),
            json!("split")
        );
    }

    #[test]
    fn apply_config_mutation_preserves_shortcut_action_ids_as_literal_keys() {
        let mut raw = json!({});
        apply_config_mutation(
            &mut raw,
            r#"shortcuts.bindings["workspace.new"]"#,
            Some(Value::String("cmd+t".to_owned())),
            false,
        )
        .expect("set");
        assert_eq!(
            raw,
            json!({
                "shortcuts": {
                    "bindings": {
                        "workspace.new": "cmd+t"
                    }
                }
            })
        );
    }

    #[test]
    fn write_and_reload_round_trips_the_raw_tree() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("cmux.json");
        let raw = json!({
            "app": { "appearance": "dark" },
            "futureSection": { "kept": true }
        });
        write_raw_config(&path, &raw).expect("write");
        assert_eq!(load_raw_config(&path).expect("read"), raw);
    }

    #[test]
    fn config_reset_raw_shape_is_an_empty_object_with_materialized_defaults() {
        let raw = Value::Object(serde_json::Map::new());
        let config = decode_settings_config(&raw).expect("decode defaults");
        assert_eq!(raw, json!({}));
        assert!(config.app.is_some());
        assert!(config.automation.is_some());
        assert!(config.browser.is_some());
        assert!(config.shortcuts.is_some());
    }

    #[test]
    fn ensure_config_file_exists_creates_an_empty_object() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("cmux.json");
        ensure_config_file_exists(&path).expect("ensure");
        assert_eq!(load_raw_config(&path).expect("read"), json!({}));
    }

    #[test]
    fn ensure_text_file_exists_creates_parent_and_empty_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("ghostty").join("config.ghostty");
        ensure_text_file_exists(&path).expect("ensure");
        assert_eq!(std::fs::read_to_string(&path).expect("read"), "");
    }
}
