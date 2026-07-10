use std::path::PathBuf;

use serde::Serialize;

const SETTINGS_OPEN_CAPTURE_PATH_ENV: &str = "CMUX_UI_TEST_SETTINGS_OPEN_CAPTURE_PATH";

#[derive(Debug, Serialize)]
struct SettingsOpenCapture {
    opened: bool,
    target: Option<String>,
    used_open_window_override: bool,
}

#[tauri::command]
pub fn settings_open_capture(target: Option<String>) -> Result<(), String> {
    settings_open_capture_inner(
        target,
        std::env::var_os(SETTINGS_OPEN_CAPTURE_PATH_ENV).map(PathBuf::from),
    )
}

fn settings_open_capture_inner(
    target: Option<String>,
    capture_path: Option<PathBuf>,
) -> Result<(), String> {
    let Some(path) = capture_path else {
        return Ok(());
    };
    let payload = SettingsOpenCapture {
        opened: true,
        target,
        used_open_window_override: true,
    };
    let bytes = serde_json::to_vec_pretty(&payload)
        .map_err(|error| format!("failed to encode settings-open capture: {error}"))?;
    std::fs::write(&path, bytes).map_err(|error| {
        format!(
            "failed to write settings-open capture {}: {error}",
            path.display()
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_open_capture_writes_ui_test_contract() {
        let temp = tempfile::tempdir().expect("tempdir");
        let capture = temp.path().join("settings-open.json");

        settings_open_capture_inner(Some("browserImport".to_string()), Some(capture.clone()))
            .expect("capture written");

        let captured = std::fs::read_to_string(capture).expect("capture file");
        assert!(captured.contains(r#""opened": true"#));
        assert!(captured.contains(r#""target": "browserImport""#));
        assert!(captured.contains(r#""used_open_window_override": true"#));
    }

    #[test]
    fn settings_open_capture_is_noop_without_capture_path() {
        settings_open_capture_inner(Some("browserImport".to_string()), None).expect("noop");
    }
}
