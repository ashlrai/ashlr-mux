#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct GlobalHotkeyStatus {
    pub platform: &'static str,
    pub backend: &'static str,
    pub available: bool,
    pub registered: bool,
    pub reason: Option<&'static str>,
}

#[tauri::command]
pub fn global_hotkey_status() -> GlobalHotkeyStatus {
    global_hotkey_status_snapshot()
}

fn global_hotkey_status_snapshot() -> GlobalHotkeyStatus {
    GlobalHotkeyStatus {
        platform: std::env::consts::OS,
        backend: "none",
        available: false,
        registered: false,
        reason: Some(
            "No native system-wide shortcut registrar is compiled into this desktop build.",
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::global_hotkey_status_snapshot;

    #[test]
    fn global_hotkey_status_reports_the_platform() {
        let status = global_hotkey_status_snapshot();

        assert_eq!(status.platform, std::env::consts::OS);
    }

    #[test]
    fn global_hotkey_status_is_explicit_when_no_registrar_is_configured() {
        let status = global_hotkey_status_snapshot();

        assert_eq!(status.backend, "none");
        assert!(!status.available);
        assert!(!status.registered);
        assert!(status.reason.is_some());
    }
}
