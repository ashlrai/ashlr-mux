//! `cmux.json` load/save commands (backlog E1/E2).
//!
//! The load path returns the RAW JSON tree so unmodeled sections survive a
//! round-trip untouched (the typed [`cmux_config::Config`] keeps unknown keys
//! in `extra`, but the raw tree sidesteps re-serialization entirely — the E2
//! contract). Writes are dotted-JSONPath set/remove applied onto that raw tree
//! via the golden-pinned [`cmux_config::json_path::JsonPath`] (the port of the
//! canonical `JSONPath.assign/remove` the macOS settings UI uses), then the
//! whole document is rewritten pretty-printed and `cmux://config-changed` is
//! broadcast with the new tree so every mounted webview reconciles.
//!
//! A missing `cmux.json` loads as `{}` (canonical: defaults apply when the
//! file is absent); a malformed one is an explicit error, never silently
//! replaced (a save onto a malformed file would destroy user data).

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use serde_json::Value;
use tauri::{AppHandle, Emitter};

use cmux_config::json_path::JsonPath;

/// Broadcast after every successful config write, payload = the new raw tree.
pub const CONFIG_CHANGED_EVENT: &str = "cmux://config-changed";

/// The global `cmux.json` path (`%APPDATA%\cmux\cmux.json` on Windows).
fn global_config_path() -> Result<PathBuf, String> {
    cmux_config::config_path().ok_or_else(|| "no user config directory".to_string())
}

/// Read the raw config tree: `{}` when the file is absent, an error when it
/// exists but is unreadable or not valid JSON.
fn load_raw(path: &Path) -> Result<Value, String> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            return Ok(Value::Object(serde_json::Map::new()));
        }
        Err(error) => return Err(format!("cannot read {}: {error}", path.display())),
    };
    serde_json::from_str(&text).map_err(|error| {
        format!("{} is not valid JSON: {error}", path.display())
    })
}

/// Persist the raw tree pretty-printed (trailing newline, like the canonical
/// writer), creating `…/cmux/` on first write.
fn write_raw(path: &Path, root: &Value) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
    }
    let mut text = serde_json::to_string_pretty(root)
        .map_err(|error| format!("cannot encode config: {error}"))?;
    text.push('\n');
    fs::write(path, text).map_err(|error| format!("cannot write {}: {error}", path.display()))
}

/// Set `dotted` to `value` in the document at `path` and return the new tree.
fn set_in_file(path: &Path, dotted: &str, value: Value) -> Result<Value, String> {
    let json_path = JsonPath::parse(dotted).map_err(|error| error.to_string())?;
    let mut root = load_raw(path)?;
    json_path.assign(&mut root, value);
    write_raw(path, &root)?;
    Ok(root)
}

/// Remove `dotted` from the document at `path` and return the new tree.
fn remove_in_file(path: &Path, dotted: &str) -> Result<Value, String> {
    let json_path = JsonPath::parse(dotted).map_err(|error| error.to_string())?;
    let mut root = load_raw(path)?;
    json_path.remove(&mut root);
    write_raw(path, &root)?;
    Ok(root)
}

fn emit_config_changed(app: &AppHandle, root: &Value) {
    let _ = app.emit(CONFIG_CHANGED_EVENT, root);
}

/// Load the raw `cmux.json` tree (`{}` when the file does not exist).
#[tauri::command]
pub fn config_load() -> Result<Value, String> {
    load_raw(&global_config_path()?)
}

/// Set the dotted JSON path `path` to `value` in `cmux.json`, write the file,
/// broadcast `cmux://config-changed`, and return the new tree.
#[tauri::command]
pub fn config_set(app: AppHandle, path: String, value: Value) -> Result<Value, String> {
    let root = set_in_file(&global_config_path()?, &path, value)?;
    emit_config_changed(&app, &root);
    Ok(root)
}

/// Remove the dotted JSON path `path` from `cmux.json`, write the file,
/// broadcast `cmux://config-changed`, and return the new tree.
#[tauri::command]
pub fn config_remove(app: AppHandle, path: String) -> Result<Value, String> {
    let root = remove_in_file(&global_config_path()?, &path)?;
    emit_config_changed(&app, &root);
    Ok(root)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// A unique temp file path per test (no tempfile dep; std-only).
    fn temp_config(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "cmux-config-test-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        dir.join("cmux").join("cmux.json")
    }

    #[test]
    fn missing_file_loads_as_empty_object() {
        let path = temp_config("missing");
        assert_eq!(load_raw(&path).unwrap(), json!({}));
    }

    #[test]
    fn malformed_file_is_an_explicit_error_not_a_silent_reset() {
        let path = temp_config("malformed");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "{ not json").unwrap();
        let error = load_raw(&path).unwrap_err();
        assert!(error.contains("not valid JSON"), "{error}");
        // And a set onto it must refuse too (never clobber user data).
        assert!(set_in_file(&path, "a.b", json!(1)).is_err());
    }

    #[test]
    fn set_creates_intermediate_objects_and_round_trips() {
        let path = temp_config("set");
        let tree = set_in_file(&path, "terminal.fontSize", json!(14)).unwrap();
        assert_eq!(tree, json!({ "terminal": { "fontSize": 14 } }));
        // The write landed and re-loads identically.
        assert_eq!(load_raw(&path).unwrap(), tree);
        // A second set PRESERVES unmodeled/unrelated content (raw-tree edit).
        let tree = set_in_file(&path, "sidebar.width", json!(240)).unwrap();
        assert_eq!(
            tree,
            json!({ "terminal": { "fontSize": 14 }, "sidebar": { "width": 240 } })
        );
    }

    #[test]
    fn set_preserves_unknown_sections_verbatim() {
        let path = temp_config("preserve");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            &path,
            r#"{ "futureFeature": { "nested": [1, 2, 3] }, "terminal": {} }"#,
        )
        .unwrap();
        let tree = set_in_file(&path, "terminal.fontSize", json!(12)).unwrap();
        assert_eq!(tree["futureFeature"], json!({ "nested": [1, 2, 3] }));
    }

    #[test]
    fn remove_deletes_the_leaf_pruning_emptied_parents_and_is_idempotent() {
        let path = temp_config("remove");
        set_in_file(&path, "a.b", json!(true)).unwrap();
        set_in_file(&path, "keep", json!(1)).unwrap();
        // Canonical removeAtPath prunes parents emptied by the removal
        // (JSONPath.swift:136 contract) — "a" disappears with its last leaf.
        let tree = remove_in_file(&path, "a.b").unwrap();
        assert_eq!(tree, json!({ "keep": 1 }));
        // Removing an absent path is a clean no-op, not an error.
        let tree = remove_in_file(&path, "a.b").unwrap();
        assert_eq!(tree, json!({ "keep": 1 }));
    }

    #[test]
    fn empty_or_malformed_dotted_paths_are_rejected() {
        let path = temp_config("badpath");
        assert!(set_in_file(&path, "", json!(1)).is_err());
        assert!(set_in_file(&path, "a..b", json!(1)).is_err());
    }

    #[test]
    fn written_file_is_pretty_printed_with_trailing_newline() {
        let path = temp_config("pretty");
        set_in_file(&path, "a", json!(1)).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.ends_with('\n'));
        assert!(text.contains("\n  \"a\": 1"));
    }
}
