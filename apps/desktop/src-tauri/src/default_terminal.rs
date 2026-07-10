//! Windows default-terminal registration.
//!
//! macOS cmux registers for `ssh://` plus executable/script document types. The
//! Windows port's safe first-class equivalent is the current-user `ssh://`
//! protocol handler. The palette row is driven by this status and the action
//! registers the desktop executable as that handler.

use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
pub struct DefaultTerminalStatus {
    pub is_default: bool,
    pub command: String,
}

#[tauri::command]
pub fn default_terminal_status() -> DefaultTerminalStatus {
    let command = default_terminal_command();
    DefaultTerminalStatus {
        is_default: current_ssh_command().is_some_and(|current| {
            normalize_command_for_compare(&current) == normalize_command_for_compare(&command)
        }),
        command,
    }
}

#[tauri::command]
pub fn make_default_terminal() -> Result<DefaultTerminalStatus, String> {
    register_ssh_handler(&current_exe_path()?)?;
    Ok(default_terminal_status())
}

fn default_terminal_command() -> String {
    current_exe_path()
        .map(|path| ssh_handler_command(&path))
        .unwrap_or_default()
}

fn current_exe_path() -> Result<PathBuf, String> {
    std::env::current_exe().map_err(|error| format!("Could not resolve cmux executable: {error}"))
}

fn ssh_handler_command(executable: &Path) -> String {
    format!("\"{}\" \"%1\"", executable.display())
}

#[cfg(windows)]
fn register_ssh_handler(executable: &Path) -> Result<(), String> {
    let command = ssh_handler_command(executable);
    run_reg(&[
        "add",
        r"HKCU\Software\Classes\ssh",
        "/ve",
        "/d",
        "URL:SSH Protocol",
        "/f",
    ])?;
    run_reg(&[
        "add",
        r"HKCU\Software\Classes\ssh",
        "/v",
        "URL Protocol",
        "/t",
        "REG_SZ",
        "/d",
        "",
        "/f",
    ])?;
    run_reg(&[
        "add",
        r"HKCU\Software\Classes\ssh\shell\open\command",
        "/ve",
        "/d",
        &command,
        "/f",
    ])
}

#[cfg(not(windows))]
fn register_ssh_handler(_executable: &Path) -> Result<(), String> {
    Ok(())
}

#[cfg(windows)]
fn current_ssh_command() -> Option<String> {
    let output = Command::new("reg")
        .args([
            "query",
            r"HKCU\Software\Classes\ssh\shell\open\command",
            "/ve",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    parse_reg_default_value(&String::from_utf8_lossy(&output.stdout))
}

#[cfg(not(windows))]
fn current_ssh_command() -> Option<String> {
    None
}

#[cfg(windows)]
fn run_reg(args: &[&str]) -> Result<(), String> {
    let output = Command::new("reg")
        .args(args)
        .output()
        .map_err(|error| format!("Could not update Windows protocol handlers: {error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(if stderr.is_empty() {
            "Could not update Windows protocol handlers".to_string()
        } else {
            format!("Could not update Windows protocol handlers: {stderr}")
        })
    }
}

fn parse_reg_default_value(output: &str) -> Option<String> {
    output.lines().find_map(|line| {
        let trimmed = line.trim();
        if !trimmed.starts_with("(Default)") && !trimmed.starts_with("<NO NAME>") {
            return None;
        }
        trimmed
            .split_once("REG_SZ")
            .map(|(_, value)| value.trim().to_string())
            .filter(|value| !value.is_empty())
    })
}

fn normalize_command_for_compare(command: &str) -> String {
    command
        .trim()
        .trim_matches('"')
        .replace('/', "\\")
        .to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ssh_handler_command_quotes_executable_and_url_argument() {
        assert_eq!(
            ssh_handler_command(Path::new(r"C:\Program Files\cmux\cmux.exe")),
            r#""C:\Program Files\cmux\cmux.exe" "%1""#
        );
    }

    #[test]
    fn parse_reg_default_value_reads_windows_reg_query_output() {
        let output = r#"
HKEY_CURRENT_USER\Software\Classes\ssh\shell\open\command
    (Default)    REG_SZ    "C:\Program Files\cmux\cmux.exe" "%1"
"#;
        assert_eq!(
            parse_reg_default_value(output),
            Some(r#""C:\Program Files\cmux\cmux.exe" "%1""#.to_string())
        );
    }

    #[test]
    fn normalize_command_for_compare_is_case_and_slash_insensitive() {
        assert_eq!(
            normalize_command_for_compare(r#""C:/Program Files/cmux/cmux.exe" "%1""#),
            normalize_command_for_compare(r#""c:\program files\cmux\CMUX.EXE" "%1""#)
        );
    }
}
