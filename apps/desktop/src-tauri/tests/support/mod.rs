#![cfg(windows)]

use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use cmux_ipc::{connect_pipe, control_pipe_path, read_frame, write_frame, MAX_RPC_FRAME_BYTES};
use serde_json::{json, Value};
use tempfile::TempDir;
use tokio::net::windows::named_pipe::NamedPipeClient;
use tokio::time::timeout;

const RPC_TIMEOUT: Duration = Duration::from_secs(5);

pub struct PipeRpc {
    pipe: NamedPipeClient,
    next_id: u64,
}

impl PipeRpc {
    async fn connect(pipe_path: &str, wait: Duration) -> std::io::Result<Self> {
        Ok(Self {
            pipe: connect_pipe(pipe_path, wait).await?,
            next_id: 1,
        })
    }

    pub async fn call(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let envelope = self.exchange(method, params).await?;
        if envelope.get("ok").and_then(Value::as_bool) == Some(true) {
            Ok(envelope.get("result").cloned().unwrap_or_else(|| json!({})))
        } else {
            Err(format!("{method} failed: {envelope}"))
        }
    }

    pub async fn call_error(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let envelope = self.exchange(method, params).await?;
        if envelope.get("ok").and_then(Value::as_bool) == Some(false) {
            Ok(envelope)
        } else {
            Err(format!("{method} unexpectedly succeeded: {envelope}"))
        }
    }

    async fn exchange(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let id = self.next_id;
        self.next_id += 1;
        let request = json!({"id": id, "method": method, "params": params}).to_string();
        timeout(RPC_TIMEOUT, write_frame(&mut self.pipe, &request))
            .await
            .map_err(|_| format!("timed out writing {method}"))?
            .map_err(|error| format!("failed writing {method}: {error}"))?;
        let raw = timeout(RPC_TIMEOUT, read_frame(&mut self.pipe, MAX_RPC_FRAME_BYTES))
            .await
            .map_err(|_| format!("timed out reading {method}"))?
            .map_err(|error| format!("failed reading {method}: {error}"))?
            .ok_or_else(|| format!("control pipe closed while reading {method}"))?;
        let raw = String::from_utf8(raw)
            .map_err(|error| format!("non-UTF-8 response to {method}: {error}"))?;
        let envelope: Value = serde_json::from_str(&raw)
            .map_err(|error| format!("invalid response to {method}: {error}; raw={raw}"))?;
        if envelope.get("id") != Some(&json!(id)) {
            return Err(format!(
                "response id mismatch for {method}: expected {id}; raw={raw}"
            ));
        }
        Ok(envelope)
    }
}

pub struct DesktopFixture {
    child: Child,
    profile: TempDir,
    pipe_path: String,
    pub pid: u32,
}

impl DesktopFixture {
    pub fn launch(desktop_exe: &Path, log_dir: &Path) -> Result<Self, String> {
        let profile = tempfile::Builder::new()
            .prefix("cmux-create-input-proof-")
            .tempdir()
            .map_err(|error| format!("create isolated profile: {error}"))?;
        let home = profile.path().join("profile");
        let local_app_data = home.join("AppData").join("Local");
        let roaming_app_data = home.join("AppData").join("Roaming");
        for path in [&home, &local_app_data, &roaming_app_data] {
            std::fs::create_dir_all(path)
                .map_err(|error| format!("create isolated {}: {error}", path.display()))?;
        }

        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let pipe_base = format!(
            "cmux-create-input-{}-{:x}",
            std::process::id(),
            nonce as u64
        );
        let pipe_path = control_pipe_path(&pipe_base)
            .map_err(|error| format!("build unique control pipe path: {error}"))?;
        std::fs::create_dir_all(log_dir)
            .map_err(|error| format!("create process-proof log directory: {error}"))?;
        let stdout = File::create(log_dir.join("terminal-create-input.stdout.log"))
            .map_err(|error| format!("create desktop stdout log: {error}"))?;
        let stderr = File::create(log_dir.join("terminal-create-input.stderr.log"))
            .map_err(|error| format!("create desktop stderr log: {error}"))?;

        let child = Command::new(desktop_exe)
            .current_dir(profile.path())
            .env("CMUX_CONTROL_PIPE_NAME", &pipe_base)
            .env("CMUX_TEST_DISABLE_SINGLE_INSTANCE", "1")
            .env("LOCALAPPDATA", &local_app_data)
            .env("APPDATA", &roaming_app_data)
            .env("USERPROFILE", &home)
            .env("HOME", &home)
            .env("RUST_BACKTRACE", "1")
            .env_remove("CMUX_SOCKET_PASSWORD")
            .stdout(Stdio::from(stdout))
            .stderr(Stdio::from(stderr))
            .spawn()
            .map_err(|error| format!("launch {}: {error}", desktop_exe.display()))?;
        let pid = child.id();
        Ok(Self {
            child,
            profile,
            pipe_path,
            pid,
        })
    }

    pub async fn connect(&mut self, wait: Duration) -> Result<PipeRpc, String> {
        let connected = PipeRpc::connect(&self.pipe_path, wait).await;
        if let Ok(Some(status)) = self.child.try_wait() {
            return Err(format!(
                "owned cmux-desktop process {} exited during startup: {status}",
                self.pid
            ));
        }
        connected.map_err(|error| {
            format!(
                "connect to control pipe {} for owned process {}: {error}",
                self.pipe_path, self.pid
            )
        })
    }

    pub fn profile_path(&self) -> &Path {
        self.profile.path()
    }
}

impl Drop for DesktopFixture {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
        }
        let _ = self.child.wait();
    }
}

pub fn process_proof_log_dir() -> PathBuf {
    PathBuf::from(r"C:\tmp\ashlr-parity-orchestrator")
}
