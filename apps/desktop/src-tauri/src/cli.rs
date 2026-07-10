//! User PATH integration for the bundled `cmux` CLI sidecar.
//!
//! The desktop bundle carries the real Rust CLI as an external binary. The
//! palette's install/uninstall rows manage a small `cmux.cmd` shim in a
//! cmux-owned user bin directory and add/remove that directory from the user's
//! PATH. Uninstall only removes this managed shim.

use std::path::{Path, PathBuf};
use std::process::Command;

use tauri::{AppHandle, Manager};

const CLI_EXE_NAME: &str = "cmux-x86_64-pc-windows-msvc.exe";
const SHIM_NAME: &str = "cmux.cmd";

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
pub struct CliInstallStatus {
    pub installed_in_path: bool,
    pub shim_path: String,
    pub shim_directory: String,
    pub bundled_cli_path: Option<String>,
}

#[tauri::command]
pub fn cli_install_status(app: AppHandle) -> CliInstallStatus {
    cli_install_status_inner(&app)
}

#[tauri::command]
pub fn install_cli(app: AppHandle) -> Result<CliInstallStatus, String> {
    let target = bundled_cli_path(&app)
        .ok_or_else(|| "Bundled cmux CLI executable was not found".to_string())?;
    let shim = cli_shim_path(&app)?;
    let Some(shim_dir) = shim.parent() else {
        return Err("Could not resolve cmux CLI shim directory".to_string());
    };

    std::fs::create_dir_all(shim_dir)
        .map_err(|error| format!("Could not create CLI shim directory: {error}"))?;
    std::fs::write(&shim, cli_shim_contents(&target))
        .map_err(|error| format!("Could not write CLI shim: {error}"))?;
    add_directory_to_user_path(shim_dir)?;
    add_directory_to_current_process_path(shim_dir);
    Ok(cli_install_status_inner(&app))
}

#[tauri::command]
pub fn uninstall_cli(app: AppHandle) -> Result<CliInstallStatus, String> {
    let shim = cli_shim_path(&app)?;
    if shim.exists() {
        std::fs::remove_file(&shim)
            .map_err(|error| format!("Could not remove CLI shim: {error}"))?;
    }
    if let Some(shim_dir) = shim.parent() {
        remove_directory_from_user_path(shim_dir)?;
        remove_directory_from_current_process_path(shim_dir);
    }
    Ok(cli_install_status_inner(&app))
}

fn cli_install_status_inner(app: &AppHandle) -> CliInstallStatus {
    let shim = cli_shim_path(app).unwrap_or_else(|_| fallback_cli_shim_path());
    let shim_dir = shim
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(fallback_cli_bin_dir);
    let installed_in_path = shim.is_file() && path_contains_directory(&current_path(), &shim_dir);
    CliInstallStatus {
        installed_in_path,
        shim_path: shim.to_string_lossy().into_owned(),
        shim_directory: shim_dir.to_string_lossy().into_owned(),
        bundled_cli_path: bundled_cli_path(app).map(|path| path.to_string_lossy().into_owned()),
    }
}

fn cli_shim_path(app: &AppHandle) -> Result<PathBuf, String> {
    let base = app
        .path()
        .app_local_data_dir()
        .map_err(|error| format!("Could not resolve local app data directory: {error}"))?;
    Ok(base.join("bin").join(SHIM_NAME))
}

fn fallback_cli_shim_path() -> PathBuf {
    fallback_cli_bin_dir().join(SHIM_NAME)
}

fn fallback_cli_bin_dir() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("cmux")
        .join("bin")
}

pub(crate) fn bundled_cli_path(app: &AppHandle) -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(resource_dir) = app.path().resource_dir() {
        candidates.push(resource_dir.join("binaries").join(CLI_EXE_NAME));
        candidates.push(resource_dir.join(CLI_EXE_NAME));
    }
    if let Ok(current_exe) = std::env::current_exe() {
        if let Some(exe_dir) = current_exe.parent() {
            candidates.push(exe_dir.join(CLI_EXE_NAME));
            candidates.push(exe_dir.join("binaries").join(CLI_EXE_NAME));
            candidates.push(exe_dir.join("cmux.exe"));
        }
    }
    candidates.push(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("binaries")
            .join(CLI_EXE_NAME),
    );
    candidates.into_iter().find(|path| path.is_file())
}

fn cli_shim_contents(target: &Path) -> String {
    format!(
        "@echo off\r\n\"{}\" %*\r\nexit /b %ERRORLEVEL%\r\n",
        target.display()
    )
}

fn current_path() -> String {
    std::env::var("PATH").unwrap_or_default()
}

fn path_contains_directory(path_value: &str, directory: &Path) -> bool {
    let directory = normalize_path_for_compare(directory);
    split_path_value(path_value)
        .iter()
        .any(|entry| normalize_path_for_compare(Path::new(entry)) == directory)
}

fn append_directory_to_path_value(path_value: &str, directory: &Path) -> String {
    if path_contains_directory(path_value, directory) {
        return path_value.to_string();
    }
    let directory = directory.to_string_lossy();
    if path_value.trim().is_empty() {
        directory.into_owned()
    } else {
        format!("{};{}", path_value.trim_end_matches(';'), directory)
    }
}

fn remove_directory_from_path_value(path_value: &str, directory: &Path) -> String {
    let directory = normalize_path_for_compare(directory);
    split_path_value(path_value)
        .into_iter()
        .filter(|entry| normalize_path_for_compare(Path::new(entry)) != directory)
        .collect::<Vec<_>>()
        .join(";")
}

fn split_path_value(path_value: &str) -> Vec<String> {
    path_value
        .split(';')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .map(str::to_string)
        .collect()
}

fn normalize_path_for_compare(path: &Path) -> String {
    path.to_string_lossy()
        .trim_end_matches(['\\', '/'])
        .to_ascii_lowercase()
}

fn add_directory_to_current_process_path(directory: &Path) {
    let next = append_directory_to_path_value(&current_path(), directory);
    std::env::set_var("PATH", next);
}

fn remove_directory_from_current_process_path(directory: &Path) {
    let next = remove_directory_from_path_value(&current_path(), directory);
    std::env::set_var("PATH", next);
}

#[cfg(windows)]
fn add_directory_to_user_path(directory: &Path) -> Result<(), String> {
    mutate_user_path_with_powershell(
        r#"
$dir = $args[0]
$current = [Environment]::GetEnvironmentVariable('Path', 'User')
if ([string]::IsNullOrWhiteSpace($current)) {
  $next = $dir
} elseif (($current -split ';' | ForEach-Object { $_.TrimEnd('\', '/') }) -notcontains $dir.TrimEnd('\', '/')) {
  $next = $current.TrimEnd(';') + ';' + $dir
} else {
  $next = $current
}
[Environment]::SetEnvironmentVariable('Path', $next, 'User')
"#,
        directory,
    )
}

#[cfg(not(windows))]
fn add_directory_to_user_path(_directory: &Path) -> Result<(), String> {
    Ok(())
}

#[cfg(windows)]
fn remove_directory_from_user_path(directory: &Path) -> Result<(), String> {
    mutate_user_path_with_powershell(
        r#"
$dir = $args[0].TrimEnd('\', '/')
$current = [Environment]::GetEnvironmentVariable('Path', 'User')
if ([string]::IsNullOrWhiteSpace($current)) { exit 0 }
$next = (($current -split ';') | Where-Object { $_.Trim() -ne '' -and $_.TrimEnd('\', '/') -ne $dir }) -join ';'
[Environment]::SetEnvironmentVariable('Path', $next, 'User')
"#,
        directory,
    )
}

#[cfg(not(windows))]
fn remove_directory_from_user_path(_directory: &Path) -> Result<(), String> {
    Ok(())
}

#[cfg(windows)]
fn mutate_user_path_with_powershell(script: &str, directory: &Path) -> Result<(), String> {
    let output = Command::new("powershell.exe")
        .arg("-NoProfile")
        .arg("-NonInteractive")
        .arg("-ExecutionPolicy")
        .arg("Bypass")
        .arg("-Command")
        .arg(script)
        .arg(directory)
        .output()
        .map_err(|error| format!("Could not update user PATH: {error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(if stderr.is_empty() {
            "Could not update user PATH".to_string()
        } else {
            format!("Could not update user PATH: {stderr}")
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_helpers_append_and_remove_without_duplicates() {
        let dir = Path::new("C:/Users/A/AppData/Local/cmux/bin");
        let path = "C:/Windows/System32;C:/Tools";
        let appended = append_directory_to_path_value(path, dir);
        assert_eq!(
            appended,
            "C:/Windows/System32;C:/Tools;C:/Users/A/AppData/Local/cmux/bin"
        );
        assert_eq!(append_directory_to_path_value(&appended, dir), appended);
        assert_eq!(
            remove_directory_from_path_value(&appended, dir),
            "C:/Windows/System32;C:/Tools"
        );
    }

    #[test]
    fn path_contains_directory_is_case_insensitive_and_trims_slashes() {
        assert!(path_contains_directory(
            "C:/TOOLS;C:/Users/A/AppData/Local/cmux/bin/",
            Path::new("c:/users/a/appdata/local/cmux/bin")
        ));
    }

    #[test]
    fn cli_shim_forwards_all_arguments_to_the_bundled_cli() {
        let contents = cli_shim_contents(Path::new("C:/Program Files/cmux/cmux.exe"));
        assert!(contents.contains("\"C:/Program Files/cmux/cmux.exe\" %*"));
        assert!(contents.contains("exit /b %ERRORLEVEL%"));
    }
}
