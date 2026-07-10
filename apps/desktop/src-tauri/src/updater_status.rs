#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct UpdaterStatus {
    pub platform: &'static str,
    pub current_version: &'static str,
    pub backend: &'static str,
    pub configured: bool,
    pub check_supported: bool,
    pub install_supported: bool,
    pub reason: Option<&'static str>,
}

#[tauri::command]
pub fn updater_status() -> UpdaterStatus {
    updater_status_snapshot()
}

fn updater_status_snapshot() -> UpdaterStatus {
    UpdaterStatus {
        platform: std::env::consts::OS,
        current_version: env!("CARGO_PKG_VERSION"),
        backend: "none",
        configured: false,
        check_supported: false,
        install_supported: false,
        reason: Some(
            "No native updater plugin or Windows installer update endpoint is configured in this build.",
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::updater_status_snapshot;

    #[test]
    fn updater_status_reports_the_package_version() {
        let status = updater_status_snapshot();

        assert_eq!(status.current_version, env!("CARGO_PKG_VERSION"));
        assert_eq!(status.platform, std::env::consts::OS);
    }

    #[test]
    fn updater_status_is_explicit_when_no_backend_is_configured() {
        let status = updater_status_snapshot();

        assert_eq!(status.backend, "none");
        assert!(!status.configured);
        assert!(!status.check_supported);
        assert!(!status.install_supported);
        assert!(status.reason.is_some());
    }
}
