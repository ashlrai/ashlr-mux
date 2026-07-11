//! Cloud VM PTY WebSocket terminal bridge.

use std::collections::HashMap;
use std::fs;
use std::io::{self, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use crossterm::terminal::{disable_raw_mode, enable_raw_mode, size};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::json;
use tokio::sync::mpsc;
use tokio::time::{interval, interval_at, timeout, Instant, MissedTickBehavior};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::{HeaderName, HeaderValue};
use tokio_tungstenite::tungstenite::Message;
use url::Url;

use crate::invocation::CliError;

pub const VM_PTY_CONNECT_USAGE: &str = "Usage: cmux vm-pty-connect --config <path>";
const OPEN_TIMEOUT: Duration = Duration::from_secs(15);
const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(5);
const RESIZE_INTERVAL: Duration = Duration::from_millis(250);

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VmPtyConfig {
    url: String,
    headers: HashMap<String, String>,
    token: String,
    session_id: String,
    attachment_id: String,
}

enum InputFrame {
    Data(Vec<u8>),
    Eof,
}

struct RawModeGuard(bool);

impl RawModeGuard {
    fn enter() -> Self {
        if io::stdin().is_terminal() && enable_raw_mode().is_ok() {
            Self(true)
        } else {
            Self(false)
        }
    }
}

impl Drop for RawModeGuard {
    fn drop(&mut self) {
        if self.0 {
            let _ = disable_raw_mode();
        }
    }
}

pub fn run_vm_pty_connect(args: &[String]) -> Result<(), CliError> {
    let (config_path, remaining) = parse_option(args, "--config");
    let (_vm_id, remaining) = parse_option(&remaining, "--id");
    if let Some(unknown) = remaining.iter().find(|argument| argument.starts_with("--")) {
        return Err(CliError::new(format!(
            "vm-pty-connect: unknown flag '{unknown}'"
        )));
    }
    let config_path = config_path.ok_or_else(|| CliError::new(VM_PTY_CONNECT_USAGE))?;
    let config_path = expand_tilde(&config_path);
    let data = fs::read(&config_path).map_err(|error| {
        CliError::new(format!(
            "vm-pty-connect: failed to read config '{}': {error}",
            config_path.display()
        ))
    })?;
    let _ = fs::remove_file(&config_path);
    let config: VmPtyConfig = serde_json::from_slice(&data)
        .map_err(|error| CliError::new(format!("vm-pty-connect: invalid config: {error}")))?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| CliError::new(format!("vm-pty-connect: runtime failed: {error}")))?;
    runtime.block_on(run_bridge(config))
}

async fn run_bridge(config: VmPtyConfig) -> Result<(), CliError> {
    let url = Url::parse(&config.url)
        .map_err(|_| CliError::new("vm-pty-connect: invalid websocket url"))?;
    if !matches!(url.scheme(), "ws" | "wss") {
        return Err(CliError::new("vm-pty-connect: invalid websocket url"));
    }
    let mut request = config
        .url
        .clone()
        .into_client_request()
        .map_err(|_| CliError::new("vm-pty-connect: invalid websocket url"))?;
    for (key, value) in &config.headers {
        let key = HeaderName::from_bytes(key.as_bytes()).map_err(|_| {
            CliError::new(format!("vm-pty-connect: invalid websocket header '{key}'"))
        })?;
        let value = HeaderValue::from_str(value)
            .map_err(|_| CliError::new("vm-pty-connect: invalid websocket header value"))?;
        request.headers_mut().insert(key, value);
    }
    let (stream, _) = timeout(OPEN_TIMEOUT, connect_async(request))
        .await
        .map_err(|_| CliError::new("vm-pty-connect: timed out opening websocket"))?
        .map_err(|error| {
            CliError::new(format!("vm-pty-connect: websocket open failed: {error}"))
        })?;
    let (mut sink, mut source) = stream.split();
    let (cols, rows) = terminal_size();
    let auth = json!({
        "type": "auth",
        "token": config.token,
        "session_id": config.session_id,
        "attachment_id": config.attachment_id,
        "cols": cols,
        "rows": rows,
    });
    sink.send(Message::Text(auth.to_string().into()))
        .await
        .map_err(websocket_error)?;
    loop {
        match source.next().await {
            Some(Ok(Message::Text(text))) if text.contains("\"ready\"") => break,
            Some(Ok(Message::Ping(data))) => {
                sink.send(Message::Pong(data))
                    .await
                    .map_err(websocket_error)?;
            }
            Some(Ok(Message::Close(_))) | None => {
                return Err(CliError::new(
                    "vm-pty-connect: websocket closed before ready",
                ));
            }
            Some(Err(error)) => return Err(websocket_error(error)),
            _ => {}
        }
    }

    let _raw_mode = RawModeGuard::enter();
    let (input_tx, mut input_rx) = mpsc::channel(8);
    std::thread::spawn(move || pump_stdin(input_tx));
    let mut last_size = (cols, rows);
    let mut resize = interval(RESIZE_INTERVAL);
    resize.set_missed_tick_behavior(MissedTickBehavior::Delay);
    resize.tick().await;
    let start = Instant::now() + KEEPALIVE_INTERVAL;
    let mut keepalive = interval_at(start, KEEPALIVE_INTERVAL);
    keepalive.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut waiting_for_pong = false;

    loop {
        tokio::select! {
            message = source.next() => match message {
                Some(Ok(Message::Binary(data))) => write_stdout(&data)?,
                Some(Ok(Message::Ping(data))) => {
                    let pong = sink.send(Message::Pong(data)).await;
                    if pong.is_err() { return Ok(()); }
                }
                Some(Ok(Message::Pong(_))) => waiting_for_pong = false,
                Some(Ok(Message::Close(_))) | None => return Ok(()),
                Some(Err(error)) => return Err(websocket_error(error)),
                _ => {}
            },
            input = input_rx.recv() => match input {
                Some(InputFrame::Data(data)) => {
                    if sink.send(Message::Binary(data.into())).await.is_err() { return Ok(()); }
                }
                Some(InputFrame::Eof) | None => {
                    let _ = sink.send(Message::Close(None)).await;
                    return Ok(());
                }
            },
            _ = resize.tick() => {
                let current = terminal_size();
                if current != last_size {
                    last_size = current;
                    let payload = json!({"type":"resize", "cols":current.0, "rows":current.1});
                    if sink.send(Message::Text(payload.to_string().into())).await.is_err() { return Ok(()); }
                }
            },
            _ = keepalive.tick() => {
                if waiting_for_pong { return Ok(()); }
                waiting_for_pong = true;
                if sink.send(Message::Ping(Vec::new().into())).await.is_err() { return Ok(()); }
            },
        }
    }
}

fn pump_stdin(sender: mpsc::Sender<InputFrame>) {
    let mut input = io::stdin().lock();
    let mut buffer = [0u8; 8192];
    loop {
        match input.read(&mut buffer) {
            Ok(0) | Err(_) => {
                let _ = sender.blocking_send(InputFrame::Eof);
                return;
            }
            Ok(count) => {
                if sender
                    .blocking_send(InputFrame::Data(buffer[..count].to_vec()))
                    .is_err()
                {
                    return;
                }
            }
        }
    }
}

fn write_stdout(data: &[u8]) -> Result<(), CliError> {
    let mut output = io::stdout().lock();
    if let Err(error) = output.write_all(data).and_then(|()| output.flush()) {
        if error.kind() != io::ErrorKind::BrokenPipe {
            return Err(CliError::new(format!(
                "vm-pty-connect: stdout write failed: {error}"
            )));
        }
    }
    Ok(())
}

fn websocket_error(error: tokio_tungstenite::tungstenite::Error) -> CliError {
    CliError::new(format!("vm-pty-connect: websocket error: {error}"))
}

fn terminal_size() -> (u16, u16) {
    size()
        .ok()
        .filter(|(cols, rows)| *cols > 0 && *rows > 0)
        .unwrap_or((80, 24))
}

fn parse_option(args: &[String], name: &str) -> (Option<String>, Vec<String>) {
    let mut value = None;
    let mut remaining = Vec::new();
    let mut index = 0;
    let mut past_terminator = false;
    let prefix = format!("{name}=");
    while index < args.len() {
        let argument = &args[index];
        if argument == "--" {
            past_terminator = true;
            remaining.push(argument.clone());
        } else if !past_terminator {
            if let Some(raw) = argument.strip_prefix(&prefix) {
                value = Some(raw.to_string());
            } else if argument == name && index + 1 < args.len() {
                value = Some(args[index + 1].clone());
                index += 1;
            } else {
                remaining.push(argument.clone());
            }
        } else {
            remaining.push(argument.clone());
        }
        index += 1;
    }
    (value, remaining)
}

fn expand_tilde(path: &str) -> PathBuf {
    if path == "~" || path.starts_with("~/") || path.starts_with("~\\") {
        if let Some(home) = std::env::var_os("USERPROFILE")
            .filter(|value| !value.is_empty())
            .or_else(|| std::env::var_os("HOME").filter(|value| !value.is_empty()))
        {
            let suffix = path.trim_start_matches('~').trim_start_matches(['/', '\\']);
            return Path::new(&home).join(suffix);
        }
    }
    PathBuf::from(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn parser_uses_last_value_and_rejects_remaining_long_flags() {
        let args = vec![
            "--config=first".into(),
            "positional".into(),
            "--config".into(),
            "last".into(),
            "--id=vm-1".into(),
        ];
        let (config, remaining) = parse_option(&args, "--config");
        let (id, remaining) = parse_option(&remaining, "--id");
        assert_eq!(config.as_deref(), Some("last"));
        assert_eq!(id.as_deref(), Some("vm-1"));
        assert_eq!(remaining, ["positional"]);
    }

    #[test]
    fn config_is_consumed_before_decode_failure() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("bad.json");
        fs::write(&path, "not json").unwrap();
        let error =
            run_vm_pty_connect(&["--config".into(), path.to_string_lossy().into()]).unwrap_err();
        assert!(error.message.starts_with("vm-pty-connect: invalid config:"));
        assert!(!path.exists());
    }

    #[test]
    fn missing_config_and_unknown_flag_errors_are_exact() {
        assert_eq!(
            run_vm_pty_connect(&[]).unwrap_err().message,
            VM_PTY_CONNECT_USAGE
        );
        assert_eq!(
            run_vm_pty_connect(&["--wat".into()]).unwrap_err().message,
            "vm-pty-connect: unknown flag '--wat'"
        );
    }
}
