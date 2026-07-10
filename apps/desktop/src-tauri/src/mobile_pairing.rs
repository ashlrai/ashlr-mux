#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct MobilePairingStatus {
    pub platform: &'static str,
    pub backend: &'static str,
    pub available: bool,
    pub running: bool,
    pub pairing_supported: bool,
    pub advertised_endpoint: Option<String>,
    pub reason: Option<&'static str>,
}

#[tauri::command]
pub fn mobile_pairing_status() -> MobilePairingStatus {
    mobile_pairing_status_snapshot()
}

fn mobile_pairing_status_snapshot() -> MobilePairingStatus {
    MobilePairingStatus {
        platform: std::env::consts::OS,
        backend: "none",
        available: false,
        running: false,
        pairing_supported: false,
        advertised_endpoint: None,
        reason: Some("No native mobile pairing listener is compiled into this desktop build."),
    }
}

#[cfg(test)]
mod tests {
    use super::mobile_pairing_status_snapshot;

    #[test]
    fn mobile_pairing_status_reports_the_platform() {
        let status = mobile_pairing_status_snapshot();

        assert_eq!(status.platform, std::env::consts::OS);
    }

    #[test]
    fn mobile_pairing_status_is_explicit_when_no_listener_is_configured() {
        let status = mobile_pairing_status_snapshot();

        assert_eq!(status.backend, "none");
        assert!(!status.available);
        assert!(!status.running);
        assert!(!status.pairing_supported);
        assert_eq!(status.advertised_endpoint, None);
        assert!(status.reason.is_some());
    }
}
