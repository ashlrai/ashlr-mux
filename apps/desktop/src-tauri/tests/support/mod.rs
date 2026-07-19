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

    #[allow(dead_code)]
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
        let canonical_encode_failure = json!({
            "ok": false,
            "error": {"code": "encode_error", "message": "Failed to encode JSON"},
        });
        if envelope.get("id") != Some(&json!(id)) && envelope != canonical_encode_failure {
            return Err(format!(
                "response id mismatch for {method}: expected {id}; raw={raw}"
            ));
        }
        Ok(envelope)
    }
}

// Each integration-test crate compiles this shared module independently;
// only event-stream process proofs construct this helper.
#[allow(dead_code)]
pub struct PipeEventStream {
    pipe: NamedPipeClient,
}

#[allow(dead_code)]
impl PipeEventStream {
    async fn subscribe(
        pipe_path: &str,
        names: &[&str],
        after_seq: Option<u64>,
        wait: Duration,
    ) -> Result<(Self, Value), String> {
        let mut pipe = connect_pipe(pipe_path, wait)
            .await
            .map_err(|error| format!("connect event stream: {error}"))?;
        let mut params = json!({"names": names, "no_heartbeat": true});
        if let Some(after_seq) = after_seq {
            params["after_seq"] = json!(after_seq);
        }
        let request = json!({
            "id": 1,
            "method": "events.stream",
            "params": params,
        })
        .to_string();
        timeout(RPC_TIMEOUT, write_frame(&mut pipe, &request))
            .await
            .map_err(|_| "timed out writing events.stream".to_string())?
            .map_err(|error| format!("failed writing events.stream: {error}"))?;
        let ack = read_json_frame(&mut pipe, "events.stream ack").await?;
        Ok((Self { pipe }, ack))
    }

    pub async fn next(&mut self) -> Result<Value, String> {
        read_json_frame(&mut self.pipe, "event stream frame").await
    }
}

#[allow(dead_code)]
async fn read_json_frame(pipe: &mut NamedPipeClient, context: &str) -> Result<Value, String> {
    let raw = timeout(RPC_TIMEOUT, read_frame(pipe, MAX_RPC_FRAME_BYTES))
        .await
        .map_err(|_| format!("timed out reading {context}"))?
        .map_err(|error| format!("failed reading {context}: {error}"))?
        .ok_or_else(|| format!("control pipe closed while reading {context}"))?;
    let raw = String::from_utf8(raw).map_err(|error| format!("non-UTF-8 {context}: {error}"))?;
    serde_json::from_str(&raw).map_err(|error| format!("invalid {context}: {error}; raw={raw}"))
}

// Each integration-test crate compiles this shared module independently; only
// the startup-restore process proof reads the restart-specific members.
#[allow(dead_code)]
pub struct DesktopFixture {
    child: Child,
    _profile: TempDir,
    desktop_exe: PathBuf,
    log_dir: PathBuf,
    pipe_base: String,
    pipe_path: String,
    launch_index: u32,
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
        let child = spawn_desktop(
            desktop_exe,
            log_dir,
            profile.path(),
            &pipe_base,
            &home,
            &local_app_data,
            &roaming_app_data,
            0,
        )?;
        let pid = child.id();
        Ok(Self {
            child,
            _profile: profile,
            desktop_exe: desktop_exe.to_path_buf(),
            log_dir: log_dir.to_path_buf(),
            pipe_base,
            pipe_path,
            launch_index: 0,
            pid,
        })
    }

    #[allow(dead_code)]
    pub fn restart(&mut self) -> Result<(), String> {
        self.stop()?;
        self.launch_index += 1;
        let profile = self._profile.path();
        let home = profile.join("profile");
        let child = spawn_desktop(
            &self.desktop_exe,
            &self.log_dir,
            profile,
            &self.pipe_base,
            &home,
            &home.join("AppData").join("Local"),
            &home.join("AppData").join("Roaming"),
            self.launch_index,
        )?;
        self.pid = child.id();
        self.child = child;
        Ok(())
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

    // Each integration-test crate compiles this shared module independently;
    // only event-stream process proofs subscribe through the fixture.
    #[allow(dead_code)]
    pub async fn subscribe(
        &mut self,
        names: &[&str],
        after_seq: u64,
        wait: Duration,
    ) -> Result<(PipeEventStream, Value), String> {
        let subscribed =
            PipeEventStream::subscribe(&self.pipe_path, names, Some(after_seq), wait).await;
        if let Ok(Some(status)) = self.child.try_wait() {
            return Err(format!(
                "owned cmux-desktop process {} exited while subscribing: {status}",
                self.pid
            ));
        }
        subscribed
    }

    // Each integration-test crate compiles this shared module independently;
    // only the create/input process proof inspects the isolated profile.
    #[allow(dead_code)]
    pub fn profile_path(&self) -> &Path {
        self._profile.path()
    }

    pub fn stop(&mut self) -> Result<(), String> {
        if self
            .child
            .try_wait()
            .map_err(|error| format!("query owned desktop {}: {error}", self.pid))?
            .is_none()
        {
            self.child
                .kill()
                .map_err(|error| format!("stop owned desktop {}: {error}", self.pid))?;
        }
        self.child
            .wait()
            .map(|_| ())
            .map_err(|error| format!("wait for owned desktop {}: {error}", self.pid))
    }
}

fn spawn_desktop(
    desktop_exe: &Path,
    log_dir: &Path,
    profile: &Path,
    pipe_base: &str,
    home: &Path,
    local_app_data: &Path,
    roaming_app_data: &Path,
    launch_index: u32,
) -> Result<Child, String> {
    std::fs::create_dir_all(log_dir)
        .map_err(|error| format!("create process-proof log directory: {error}"))?;
    let stdout = File::create(log_dir.join(format!("desktop-{launch_index}.stdout.log")))
        .map_err(|error| format!("create desktop stdout log: {error}"))?;
    let stderr = File::create(log_dir.join(format!("desktop-{launch_index}.stderr.log")))
        .map_err(|error| format!("create desktop stderr log: {error}"))?;
    Command::new(desktop_exe)
        .current_dir(profile)
        .env("CMUX_CONTROL_PIPE_NAME", pipe_base)
        .env("CMUX_TEST_DISABLE_SINGLE_INSTANCE", "1")
        .env("CMUX_PARITY_CAPTURE_HEADLESS", "1")
        .env("LOCALAPPDATA", local_app_data)
        .env("APPDATA", roaming_app_data)
        .env("USERPROFILE", home)
        .env("HOME", home)
        .env("RUST_BACKTRACE", "1")
        .env_remove("CMUX_SOCKET_PASSWORD")
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .spawn()
        .map_err(|error| format!("launch {}: {error}", desktop_exe.display()))
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
