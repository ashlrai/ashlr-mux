use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::app_settings::SettingsStore;

pub const RIGHT_SIDEBAR_CHANGED_EVENT: &str = "cmux://right-sidebar-changed";
pub const FEED_ENABLED_KEY: &str = "rightSidebar.beta.feed.enabled";
pub const DOCK_ENABLED_KEY: &str = "rightSidebar.beta.dock.enabled";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RightSidebarSnapshot {
    pub visible: bool,
    pub mode: String,
}

impl Default for RightSidebarSnapshot {
    fn default() -> Self {
        Self {
            visible: false,
            mode: "files".to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RightSidebarChanged {
    pub visible: bool,
    pub mode: String,
    pub focus: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RightSidebarControlOutcome {
    State(RightSidebarSnapshot),
    Changed(RightSidebarChanged),
}

pub struct RightSidebarState {
    inner: Mutex<RightSidebarInner>,
}

struct RightSidebarInner {
    snapshot: RightSidebarSnapshot,
    feed_enabled: bool,
    dock_enabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct RightSidebarBetaSettings {
    pub feed_enabled: bool,
    pub dock_enabled: bool,
}

impl Default for RightSidebarState {
    fn default() -> Self {
        Self {
            inner: Mutex::new(RightSidebarInner {
                snapshot: RightSidebarSnapshot::default(),
                feed_enabled: false,
                dock_enabled: false,
            }),
        }
    }
}

impl RightSidebarState {
    pub(crate) fn reveal_dock_for_control(&self) -> Result<Option<RightSidebarChanged>, String> {
        let before = self.snapshot();
        match self.apply_control("set", Some("dock"), true)? {
            RightSidebarControlOutcome::Changed(_change)
                if before.visible && before.mode == "dock" =>
            {
                Ok(None)
            }
            RightSidebarControlOutcome::Changed(change) => Ok(Some(change)),
            RightSidebarControlOutcome::State(_) => Ok(None),
        }
    }

    pub fn update_from_ui(
        &self,
        visible: bool,
        mode: &str,
    ) -> Result<RightSidebarSnapshot, String> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| "Right sidebar state is unavailable".to_string())?;
        let mode = available_mode(mode, inner.feed_enabled, inner.dock_enabled)?;
        inner.snapshot.visible = visible;
        inner.snapshot.mode = mode.to_string();
        Ok(inner.snapshot.clone())
    }

    pub fn apply_control(
        &self,
        action: &str,
        mode: Option<&str>,
        focus: bool,
    ) -> Result<RightSidebarControlOutcome, String> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| "Right sidebar state is unavailable".to_string())?;

        match action {
            "mode" => return Ok(RightSidebarControlOutcome::State(inner.snapshot.clone())),
            "toggle" => inner.snapshot.visible = !inner.snapshot.visible,
            "show" => inner.snapshot.visible = true,
            "hide" => inner.snapshot.visible = false,
            "focus" => inner.snapshot.visible = true,
            "set" => {
                inner.snapshot.visible = true;
                inner.snapshot.mode = available_mode(
                    mode.ok_or_else(|| "right_sidebar set requires mode".to_string())?,
                    inner.feed_enabled,
                    inner.dock_enabled,
                )?
                .to_string();
            }
            _ => return Err(format!("Unknown right sidebar command '{action}'")),
        }

        Ok(RightSidebarControlOutcome::Changed(RightSidebarChanged {
            visible: inner.snapshot.visible,
            mode: inner.snapshot.mode.clone(),
            focus: action == "focus" || (action == "set" && focus),
        }))
    }

    pub fn beta_settings(&self) -> RightSidebarBetaSettings {
        let inner = self
            .inner
            .lock()
            .expect("right sidebar state mutex poisoned");
        RightSidebarBetaSettings {
            feed_enabled: inner.feed_enabled,
            dock_enabled: inner.dock_enabled,
        }
    }

    pub fn snapshot(&self) -> RightSidebarSnapshot {
        self.inner
            .lock()
            .expect("right sidebar state mutex poisoned")
            .snapshot
            .clone()
    }

    pub fn set_beta_settings(
        &self,
        feed_enabled: bool,
        dock_enabled: bool,
    ) -> RightSidebarSnapshot {
        let mut inner = self
            .inner
            .lock()
            .expect("right sidebar state mutex poisoned");
        inner.feed_enabled = feed_enabled;
        inner.dock_enabled = dock_enabled;
        if (inner.snapshot.mode == "feed" && !feed_enabled)
            || (inner.snapshot.mode == "dock" && !dock_enabled)
        {
            inner.snapshot.mode = "files".to_string();
        }
        inner.snapshot.clone()
    }
}

fn available_mode(
    mode: &str,
    feed_enabled: bool,
    dock_enabled: bool,
) -> Result<&'static str, String> {
    match mode.trim().to_ascii_lowercase().as_str() {
        "files" => Ok("files"),
        "find" => Ok("find"),
        "vault" | "sessions" => Ok("sessions"),
        "feed" if feed_enabled => Ok("feed"),
        "dock" if dock_enabled => Ok("dock"),
        "feed" | "dock" => Err(format!(
            "Right sidebar mode '{}' is not available",
            mode.trim().to_ascii_lowercase()
        )),
        _ => Err(format!("Unknown right sidebar mode '{}'", mode.trim())),
    }
}

#[tauri::command]
pub fn right_sidebar_update_state(
    visible: bool,
    mode: String,
    state: State<'_, RightSidebarState>,
) -> Result<RightSidebarSnapshot, String> {
    state.update_from_ui(visible, &mode)
}

pub fn bootstrap_beta_settings(app: &AppHandle, state: &RightSidebarState) {
    let Some(store) = settings_store(app) else {
        return;
    };
    state.set_beta_settings(
        store.get_bool(FEED_ENABLED_KEY).unwrap_or(false),
        store.get_bool(DOCK_ENABLED_KEY).unwrap_or(false),
    );
}

#[tauri::command]
pub fn right_sidebar_beta_settings(
    state: State<'_, RightSidebarState>,
) -> RightSidebarBetaSettings {
    state.beta_settings()
}

#[tauri::command]
pub fn right_sidebar_set_beta_feature(
    app: AppHandle,
    feature: String,
    enabled: bool,
    state: State<'_, RightSidebarState>,
) -> Result<RightSidebarBetaSettings, String> {
    let mut settings = state.beta_settings();
    let key = match feature.as_str() {
        "feed" => {
            settings.feed_enabled = enabled;
            FEED_ENABLED_KEY
        }
        "dock" => {
            settings.dock_enabled = enabled;
            DOCK_ENABLED_KEY
        }
        _ => return Err(format!("Unknown right sidebar beta feature '{feature}'")),
    };
    let previous = state.snapshot();
    let snapshot = state.set_beta_settings(settings.feed_enabled, settings.dock_enabled);
    if let Some(store) = settings_store(&app) {
        store.set_bool(key, enabled);
    }
    if previous != snapshot {
        app.emit(
            RIGHT_SIDEBAR_CHANGED_EVENT,
            RightSidebarChanged {
                visible: snapshot.visible,
                mode: snapshot.mode,
                focus: false,
            },
        )
        .map_err(|error| error.to_string())?;
    }
    Ok(settings)
}

fn settings_store(app: &AppHandle) -> Option<SettingsStore> {
    app.path()
        .app_data_dir()
        .ok()
        .map(|dir| SettingsStore::new(dir.join("cmux").join("settings.json")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_actions_update_and_query_authoritative_state() {
        let state = RightSidebarState::default();

        assert_eq!(
            state.apply_control("show", None, false).unwrap(),
            RightSidebarControlOutcome::Changed(RightSidebarChanged {
                visible: true,
                mode: "files".to_string(),
                focus: false,
            })
        );
        assert_eq!(
            state.apply_control("set", Some("vault"), false).unwrap(),
            RightSidebarControlOutcome::Changed(RightSidebarChanged {
                visible: true,
                mode: "sessions".to_string(),
                focus: false,
            })
        );
        assert_eq!(
            state.apply_control("mode", None, false).unwrap(),
            RightSidebarControlOutcome::State(RightSidebarSnapshot {
                visible: true,
                mode: "sessions".to_string(),
            })
        );
        assert_eq!(
            state.apply_control("toggle", None, false).unwrap(),
            RightSidebarControlOutcome::Changed(RightSidebarChanged {
                visible: false,
                mode: "sessions".to_string(),
                focus: false,
            })
        );
    }

    #[test]
    fn focus_reveals_the_current_mode_and_set_focus_is_explicit() {
        let state = RightSidebarState::default();
        assert_eq!(
            state.apply_control("focus", None, false).unwrap(),
            RightSidebarControlOutcome::Changed(RightSidebarChanged {
                visible: true,
                mode: "files".to_string(),
                focus: true,
            })
        );
        assert_eq!(
            state.apply_control("set", Some("find"), true).unwrap(),
            RightSidebarControlOutcome::Changed(RightSidebarChanged {
                visible: true,
                mode: "find".to_string(),
                focus: true,
            })
        );
    }

    #[test]
    fn dock_reveal_helper_selects_and_focuses_the_enabled_dock_mode() {
        let state = RightSidebarState::default();
        state.set_beta_settings(false, true);
        assert_eq!(
            state.reveal_dock_for_control().unwrap(),
            Some(RightSidebarChanged {
                visible: true,
                mode: "dock".to_string(),
                focus: true,
            })
        );
        assert_eq!(state.reveal_dock_for_control().unwrap(), None);
        assert_eq!(
            state.snapshot(),
            RightSidebarSnapshot {
                visible: true,
                mode: "dock".to_string(),
            }
        );
    }

    #[test]
    fn ui_updates_are_queryable_and_unavailable_beta_modes_are_rejected() {
        let state = RightSidebarState::default();
        assert_eq!(
            state.update_from_ui(true, "sessions").unwrap(),
            RightSidebarSnapshot {
                visible: true,
                mode: "sessions".to_string(),
            }
        );
        assert_eq!(
            state.apply_control("mode", None, false).unwrap(),
            RightSidebarControlOutcome::State(RightSidebarSnapshot {
                visible: true,
                mode: "sessions".to_string(),
            })
        );
        assert_eq!(
            state.apply_control("set", Some("feed"), true).unwrap_err(),
            "Right sidebar mode 'feed' is not available"
        );
        state.set_beta_settings(true, false);
        assert_eq!(
            state.apply_control("set", Some("feed"), true).unwrap(),
            RightSidebarControlOutcome::Changed(RightSidebarChanged {
                visible: true,
                mode: "feed".to_string(),
                focus: true,
            })
        );
        assert_eq!(
            state.apply_control("set", Some("dock"), true).unwrap_err(),
            "Right sidebar mode 'dock' is not available"
        );
    }

    #[test]
    fn disabling_the_active_beta_mode_falls_back_to_files() {
        let state = RightSidebarState::default();
        state.set_beta_settings(true, false);
        state.apply_control("set", Some("feed"), false).unwrap();

        assert_eq!(
            state.set_beta_settings(false, false),
            RightSidebarSnapshot {
                visible: true,
                mode: "files".to_string(),
            }
        );
    }
}
