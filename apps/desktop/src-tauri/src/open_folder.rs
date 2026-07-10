//! Native folder picker for the command palette's `Open Folder…` action.
//!
//! Returns the chosen directory path as a string, or `None` when the picker is
//! cancelled. The palette owns the follow-up workspace creation step so the web
//! shell can decide whether to keep the current window state or route elsewhere.

use std::ffi::OsString;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{mpsc, Mutex};
use std::time::Duration;

use tauri::{AppHandle, State};

const SERVE_WEB_URL_PREFIX: &str = "Web UI available at ";
const SERVE_WEB_STARTUP_TIMEOUT: Duration = Duration::from_secs(30);

pub struct VSCodeInlineState {
    serve_web: Mutex<Option<ServeWebProcess>>,
}

impl Default for VSCodeInlineState {
    fn default() -> Self {
        Self {
            serve_web: Mutex::new(None),
        }
    }
}

struct ServeWebProcess {
    child: Child,
    url: String,
}

#[tauri::command]
pub fn pick_workspace_folder(app: AppHandle) -> Option<String> {
    use tauri_plugin_dialog::DialogExt;

    app.dialog()
        .file()
        .set_title("Open Folder")
        .blocking_pick_folder()
        .and_then(|entry| entry.into_path().ok())
        .map(|path| path.to_string_lossy().into_owned())
}

#[tauri::command]
pub fn vscode_inline_open_target_available() -> bool {
    find_code_launcher().is_some()
}

#[tauri::command]
pub fn open_folder_in_vscode_inline(
    app: AppHandle,
    state: State<'_, VSCodeInlineState>,
) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;

    let Some(path) = app
        .dialog()
        .file()
        .set_title("Open Folder in VS Code")
        .blocking_pick_folder()
        .and_then(|entry| entry.into_path().ok())
    else {
        return Ok(None);
    };

    let base_url = ensure_serve_web_url(&state)?;
    Ok(Some(vscode_folder_url(&base_url, &path.to_string_lossy())))
}

#[tauri::command]
pub fn vscode_serve_web_stop(state: State<'_, VSCodeInlineState>) -> Result<bool, String> {
    Ok(stop_serve_web(&state))
}

#[tauri::command]
pub fn vscode_serve_web_restart(
    state: State<'_, VSCodeInlineState>,
) -> Result<Option<String>, String> {
    stop_serve_web(&state);
    ensure_serve_web_url(&state).map(Some)
}

fn ensure_serve_web_url(state: &VSCodeInlineState) -> Result<String, String> {
    {
        let mut guard = state
            .serve_web
            .lock()
            .expect("VS Code serve-web mutex poisoned");
        if let Some(process) = guard.as_mut() {
            if process
                .child
                .try_wait()
                .map_err(|error| format!("Could not inspect VS Code serve-web: {error}"))?
                .is_none()
            {
                return Ok(process.url.clone());
            }
        }
        *guard = None;
    }

    let process = launch_serve_web()?;
    let url = process.url.clone();
    let mut guard = state
        .serve_web
        .lock()
        .expect("VS Code serve-web mutex poisoned");
    *guard = Some(process);
    Ok(url)
}

fn stop_serve_web(state: &VSCodeInlineState) -> bool {
    let mut guard = state
        .serve_web
        .lock()
        .expect("VS Code serve-web mutex poisoned");
    let Some(mut process) = guard.take() else {
        return false;
    };
    let _ = process.child.kill();
    let _ = process.child.wait();
    true
}

fn launch_serve_web() -> Result<ServeWebProcess, String> {
    let Some(code) = find_code_launcher() else {
        return Err("VS Code command-line launcher 'code' was not found in PATH".to_string());
    };
    let mut child = Command::new(code)
        .arg("serve-web")
        .arg("--host")
        .arg("127.0.0.1")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("Failed to launch VS Code serve-web: {error}"))?;

    let (tx, rx) = mpsc::channel();
    if let Some(stdout) = child.stdout.take() {
        pipe_serve_web_lines(stdout, tx.clone());
    }
    if let Some(stderr) = child.stderr.take() {
        pipe_serve_web_lines(stderr, tx);
    }

    match rx.recv_timeout(SERVE_WEB_STARTUP_TIMEOUT) {
        Ok(url) => Ok(ServeWebProcess { child, url }),
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            Err("Timed out waiting for VS Code serve-web URL".to_string())
        }
    }
}

fn pipe_serve_web_lines(pipe: impl std::io::Read + Send + 'static, sender: mpsc::Sender<String>) {
    std::thread::spawn(move || {
        for line in BufReader::new(pipe).lines().map_while(Result::ok) {
            if let Some(url) = extract_serve_web_url(&line) {
                let _ = sender.send(url);
                break;
            }
        }
    });
}

fn extract_serve_web_url(output: &str) -> Option<String> {
    output.lines().rev().find_map(|line| {
        let (_, raw_url) = line.split_once(SERVE_WEB_URL_PREFIX)?;
        let url = raw_url.trim();
        (!url.is_empty()).then(|| url.to_string())
    })
}

fn vscode_folder_url(base_url: &str, directory_path: &str) -> String {
    let (without_fragment, fragment) = base_url
        .split_once('#')
        .map(|(base, fragment)| (base, Some(fragment)))
        .unwrap_or((base_url, None));
    let (path, query) = without_fragment
        .split_once('?')
        .map(|(path, query)| (path, Some(query)))
        .unwrap_or((without_fragment, None));
    let mut query_items: Vec<String> = query
        .into_iter()
        .flat_map(|query| query.split('&'))
        .filter(|item| !item.is_empty() && !item.starts_with("folder="))
        .map(str::to_string)
        .collect();
    query_items.push(format!(
        "folder={}",
        percent_encode_query_value(directory_path)
    ));
    let mut url = format!("{path}?{}", query_items.join("&"));
    if let Some(fragment) = fragment {
        url.push('#');
        url.push_str(fragment);
    }
    url
}

fn percent_encode_query_value(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(*byte as char);
            }
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}

fn find_code_launcher() -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    let pathext = std::env::var_os("PATHEXT");
    find_code_launcher_in_path(
        path,
        pathext,
        |candidate| candidate.is_file(),
        cfg!(windows),
    )
}

fn find_code_launcher_in_path(
    path: OsString,
    pathext: Option<OsString>,
    is_file: impl Fn(&Path) -> bool,
    windows: bool,
) -> Option<PathBuf> {
    let extensions = code_launcher_extensions(pathext, windows);
    for directory in std::env::split_paths(&path) {
        for extension in &extensions {
            let candidate = directory.join(format!("code{extension}"));
            if is_file(&candidate) {
                return Some(candidate);
            }
        }
    }
    None
}

fn code_launcher_extensions(pathext: Option<OsString>, windows: bool) -> Vec<String> {
    if !windows {
        return vec![String::new()];
    }
    let mut extensions: Vec<String> = pathext
        .as_deref()
        .and_then(|value| value.to_str())
        .map(|value| {
            value
                .split(';')
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(|value| {
                    if value.starts_with('.') {
                        value.to_ascii_lowercase()
                    } else {
                        format!(".{}", value.to_ascii_lowercase())
                    }
                })
                .collect()
        })
        .unwrap_or_else(|| vec![".cmd".to_string(), ".exe".to_string(), ".bat".to_string()]);
    if !extensions.iter().any(|extension| extension.is_empty()) {
        extensions.insert(0, String::new());
    }
    extensions
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    #[test]
    fn code_launcher_extensions_follow_windows_pathext() {
        assert_eq!(
            code_launcher_extensions(Some(OsString::from(".CMD;.EXE;BAT")), true),
            vec![
                "".to_string(),
                ".cmd".to_string(),
                ".exe".to_string(),
                ".bat".to_string(),
            ]
        );
        assert_eq!(code_launcher_extensions(None, false), vec!["".to_string()]);
    }

    #[test]
    fn find_code_launcher_in_path_checks_path_order_and_extensions() {
        let sep = if cfg!(windows) { ";" } else { ":" };
        let path = OsString::from(format!("C:/missing{sep}C:/tools"));
        let found = find_code_launcher_in_path(
            path,
            Some(OsString::from(".CMD;.EXE")),
            |candidate| {
                candidate.to_string_lossy().ends_with("C:/tools\\code.cmd")
                    || candidate.to_string_lossy().ends_with("C:/tools/code.cmd")
            },
            true,
        );
        assert!(found.is_some_and(|path| path.ends_with("code.cmd")));
    }

    #[test]
    fn extract_serve_web_url_reads_the_latest_reported_url() {
        assert_eq!(
            extract_serve_web_url(
                "noise\nWeb UI available at http://127.0.0.1:8000/?tkn=old\nWeb UI available at http://127.0.0.1:9000/"
            ),
            Some("http://127.0.0.1:9000/".to_string())
        );
    }

    #[test]
    fn vscode_folder_url_replaces_folder_query_and_preserves_fragment() {
        assert_eq!(
            vscode_folder_url(
                "http://127.0.0.1:8000/?tkn=abc&folder=old#workspace",
                "C:/Users/A Project",
            ),
            "http://127.0.0.1:8000/?tkn=abc&folder=C%3A%2FUsers%2FA%20Project#workspace"
        );
    }
}
