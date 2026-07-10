//! Minimal file-backed JSON settings store — the port's settings-persistence
//! foundation.
//!
//! Canonical macOS cmux persists per-panel state (e.g. the agent-session
//! provider) inside the session-restore snapshot and other preferences in
//! UserDefaults; the port has neither surface yet, so host-side settings that
//! must survive a restart land in one flat JSON object at
//! `app_data_dir()/cmux/settings.json` (the same rooting convention as the
//! diff-comment store, `diff.rs`). Keys are flat dotted strings
//! (`"agentSession.selectedProviderId"`); the store preserves keys it does not
//! own, so future settings (appearance, …) can join the same file.
//!
//! Everything is best-effort: reads of a missing/corrupt file yield `None`, and
//! write failures are swallowed — a broken disk must never take down a feature
//! that also works in-memory for the session.

use std::path::PathBuf;

use serde_json::{Map, Value};

/// The composer's last-selected agent provider (raw `ProviderId` wire string).
pub const SELECTED_PROVIDER_KEY: &str = "agentSession.selectedProviderId";

/// A flat string-keyed JSON settings file with atomic-ish replace-on-write.
pub struct SettingsStore {
    path: PathBuf,
}

impl SettingsStore {
    /// A store over `path` (nothing is read or created until first use).
    /// Production callers root it at `app.path().app_data_dir()?/cmux/settings.json`.
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    /// Read one string value. Missing file, unreadable file, or non-object /
    /// corrupt JSON all yield `None` (best-effort, never errors).
    pub fn get_string(&self, key: &str) -> Option<String> {
        self.load()
            .get(key)
            .and_then(Value::as_str)
            .map(str::to_string)
    }

    pub fn get_bool(&self, key: &str) -> Option<bool> {
        self.load().get(key).and_then(Value::as_bool)
    }

    /// Set one string value, preserving every other key in the file.
    ///
    /// Read-modify-write of the whole object: load the existing map (or start
    /// `{}` if missing/corrupt), set the key, then write a temp file in the same
    /// directory and rename it over the target — a crash mid-write never leaves
    /// a truncated `settings.json`. IO failures are swallowed.
    pub fn set_string(&self, key: &str, value: &str) {
        let mut map = self.load();
        map.insert(key.to_string(), Value::String(value.to_string()));
        self.save(map);
    }

    pub fn set_bool(&self, key: &str, value: bool) {
        let mut map = self.load();
        map.insert(key.to_string(), Value::Bool(value));
        self.save(map);
    }

    fn save(&self, map: Map<String, Value>) {
        let Ok(bytes) = serde_json::to_vec_pretty(&Value::Object(map)) else {
            return;
        };
        let Some(parent) = self.path.parent() else {
            return;
        };
        if std::fs::create_dir_all(parent).is_err() {
            return;
        }
        // Same-directory temp file so the rename is a same-volume replace
        // (std::fs::rename replaces an existing destination on Windows).
        let tmp = self.path.with_extension("json.tmp");
        if std::fs::write(&tmp, bytes).is_ok() {
            let _ = std::fs::rename(&tmp, &self.path);
        }
    }

    /// Parse the file as a JSON object; anything short of that is an empty map.
    fn load(&self) -> Map<String, Value> {
        std::fs::read(&self.path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
            .and_then(|value| match value {
                Value::Object(map) => Some(map),
                _ => None,
            })
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store_in(dir: &std::path::Path) -> SettingsStore {
        SettingsStore::new(dir.join("settings.json"))
    }

    #[test]
    fn round_trips_a_string_value() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_in(dir.path());
        store.set_string(SELECTED_PROVIDER_KEY, "opencode");
        assert_eq!(
            store.get_string(SELECTED_PROVIDER_KEY),
            Some("opencode".to_string())
        );
        // A fresh store over the same path (simulated restart) reads it back.
        let reopened = store_in(dir.path());
        assert_eq!(
            reopened.get_string(SELECTED_PROVIDER_KEY),
            Some("opencode".to_string())
        );
    }

    #[test]
    fn missing_file_reads_none() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(store_in(dir.path()).get_string("anything"), None);
    }

    #[test]
    fn corrupt_file_reads_none_and_next_set_recovers() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(&path, b"{not json").unwrap();
        let store = SettingsStore::new(path.clone());
        assert_eq!(store.get_string("k"), None);
        // A non-object top level is equally "corrupt" for our purposes.
        std::fs::write(&path, b"[1,2,3]").unwrap();
        assert_eq!(store.get_string("k"), None);

        store.set_string("k", "v");
        assert_eq!(store.get_string("k"), Some("v".to_string()));
    }

    #[test]
    fn set_preserves_unrelated_keys() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_in(dir.path());
        store.set_string("appearance.mode", "dark");
        store.set_string(SELECTED_PROVIDER_KEY, "claude");
        store.set_string(SELECTED_PROVIDER_KEY, "codex");
        assert_eq!(
            store.get_string("appearance.mode"),
            Some("dark".to_string())
        );
        assert_eq!(
            store.get_string(SELECTED_PROVIDER_KEY),
            Some("codex".to_string())
        );
    }

    #[test]
    fn nested_directory_is_auto_created() {
        let dir = tempfile::tempdir().unwrap();
        let store = SettingsStore::new(dir.path().join("cmux").join("deep").join("settings.json"));
        store.set_string("k", "v");
        assert_eq!(store.get_string("k"), Some("v".to_string()));
    }

    #[test]
    fn non_string_value_reads_none() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(&path, br#"{"k": 7}"#).unwrap();
        assert_eq!(SettingsStore::new(path).get_string("k"), None);
    }

    #[test]
    fn round_trips_boolean_preferences_without_disturbing_strings() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_in(dir.path());
        store.set_string(SELECTED_PROVIDER_KEY, "codex");
        store.set_bool("rightSidebar.beta.feed.enabled", true);
        assert_eq!(store.get_bool("rightSidebar.beta.feed.enabled"), Some(true));
        assert_eq!(
            store.get_string(SELECTED_PROVIDER_KEY),
            Some("codex".to_string())
        );
        store.set_bool("rightSidebar.beta.feed.enabled", false);
        assert_eq!(
            store.get_bool("rightSidebar.beta.feed.enabled"),
            Some(false)
        );
    }
}
