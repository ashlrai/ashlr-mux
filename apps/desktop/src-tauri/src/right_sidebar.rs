use std::sync::Mutex;

use serde::Serialize;
use tauri::State;

pub const RIGHT_SIDEBAR_CHANGED_EVENT: &str = "cmux://right-sidebar-changed";

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
    inner: Mutex<RightSidebarSnapshot>,
}

impl Default for RightSidebarState {
    fn default() -> Self {
        Self {
            inner: Mutex::new(RightSidebarSnapshot::default()),
        }
    }
}

impl RightSidebarState {
    pub fn update_from_ui(
        &self,
        visible: bool,
        mode: &str,
    ) -> Result<RightSidebarSnapshot, String> {
        let mode = available_mode(mode)?;
        let mut snapshot = self
            .inner
            .lock()
            .map_err(|_| "Right sidebar state is unavailable".to_string())?;
        snapshot.visible = visible;
        snapshot.mode = mode.to_string();
        Ok(snapshot.clone())
    }

    pub fn apply_control(
        &self,
        action: &str,
        mode: Option<&str>,
        focus: bool,
    ) -> Result<RightSidebarControlOutcome, String> {
        let mut snapshot = self
            .inner
            .lock()
            .map_err(|_| "Right sidebar state is unavailable".to_string())?;

        match action {
            "mode" => return Ok(RightSidebarControlOutcome::State(snapshot.clone())),
            "toggle" => snapshot.visible = !snapshot.visible,
            "show" => snapshot.visible = true,
            "hide" => snapshot.visible = false,
            "focus" => snapshot.visible = true,
            "set" => {
                snapshot.visible = true;
                snapshot.mode = available_mode(
                    mode.ok_or_else(|| "right_sidebar set requires mode".to_string())?,
                )?
                .to_string();
            }
            _ => return Err(format!("Unknown right sidebar command '{action}'")),
        }

        Ok(RightSidebarControlOutcome::Changed(RightSidebarChanged {
            visible: snapshot.visible,
            mode: snapshot.mode.clone(),
            focus: action == "focus" || (action == "set" && focus),
        }))
    }
}

fn available_mode(mode: &str) -> Result<&'static str, String> {
    match mode.trim().to_ascii_lowercase().as_str() {
        "files" => Ok("files"),
        "find" => Ok("find"),
        "vault" | "sessions" => Ok("sessions"),
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
        assert_eq!(
            state.apply_control("set", Some("dock"), true).unwrap_err(),
            "Right sidebar mode 'dock' is not available"
        );
    }
}
