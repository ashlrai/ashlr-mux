//! Local remote-workspace proxy broker building blocks.
//!
//! This module ports the protocol parsing portion of macOS
//! `RemoteDaemonProxySession`: a loopback listener accepts either SOCKS5 or
//! HTTP CONNECT, resolves the requested target, preserves any bytes pipelined
//! after the handshake, and hands the target/pending payload to the daemon
//! stream layer. The actual daemon RPC transport is wired in a later slice.

#![allow(dead_code)]

use std::collections::HashMap;
use std::collections::VecDeque;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, Ipv6Addr, Shutdown, SocketAddr, TcpListener, TcpStream};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use base64::Engine as _;
use serde::Deserialize;
use serde_json::{json, Value};

pub(crate) const MAX_HANDSHAKE_BYTES: usize = 64 * 1024;
const MAX_PROXY_OBSERVATION_BYTES: usize = 256 * 1024;
const DEFAULT_DAEMON_OPEN_TIMEOUT: Duration = Duration::from_secs(10);
const DAEMON_PROXY_WRITE_TIMEOUT: Duration = Duration::from_secs(8);
const DAEMON_PROXY_CLOSE_TIMEOUT: Duration = Duration::from_secs(4);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProxyTarget {
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Socks5Greeting {
    pub consumed_bytes: usize,
    pub accepts_no_auth: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Socks5ConnectRequest {
    pub target: ProxyTarget,
    pub command: u8,
    pub consumed_bytes: usize,
    pub pending_payload: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HttpConnectRequest {
    pub target: ProxyTarget,
    pub consumed_bytes: usize,
    pub pending_payload: Vec<u8>,
    pub is_connect: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ProxyHandshakeProtocol {
    Socks5,
    HttpConnect,
    HttpForward,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ProxyHandshakeStep {
    NeedMoreData,
    SendResponse(Vec<u8>),
    SendResponseAndClose(Vec<u8>),
    OpenStream {
        protocol: ProxyHandshakeProtocol,
        target: ProxyTarget,
        success_response: Vec<u8>,
        failure_response: Vec<u8>,
        pending_payload: Vec<u8>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HandshakeProtocolState {
    Undecided,
    Socks5,
    HttpConnect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Socks5Stage {
    Greeting,
    Request,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProxyHandshake {
    protocol: HandshakeProtocolState,
    socks_stage: Socks5Stage,
    buffer: Vec<u8>,
}

impl Default for ProxyHandshake {
    fn default() -> Self {
        Self {
            protocol: HandshakeProtocolState::Undecided,
            socks_stage: Socks5Stage::Greeting,
            buffer: Vec::new(),
        }
    }
}

impl ProxyHandshake {
    pub(crate) fn push(&mut self, bytes: &[u8]) -> Result<Vec<ProxyHandshakeStep>, String> {
        if self.buffer.len() + bytes.len() > MAX_HANDSHAKE_BYTES {
            return Err(format!(
                "proxy handshake exceeded {MAX_HANDSHAKE_BYTES} bytes"
            ));
        }
        self.buffer.extend_from_slice(bytes);
        let mut steps = Vec::new();
        loop {
            match self.next_step()? {
                ProxyHandshakeStep::NeedMoreData => {
                    steps.push(ProxyHandshakeStep::NeedMoreData);
                    return Ok(steps);
                }
                step @ ProxyHandshakeStep::OpenStream { .. }
                | step @ ProxyHandshakeStep::SendResponseAndClose(_) => {
                    steps.push(step);
                    return Ok(steps);
                }
                step @ ProxyHandshakeStep::SendResponse(_) => steps.push(step),
            }
        }
    }

    fn next_step(&mut self) -> Result<ProxyHandshakeStep, String> {
        if matches!(self.protocol, HandshakeProtocolState::Undecided) {
            let Some(first) = self.buffer.first().copied() else {
                return Ok(ProxyHandshakeStep::NeedMoreData);
            };
            self.protocol = if first == 0x05 {
                HandshakeProtocolState::Socks5
            } else {
                HandshakeProtocolState::HttpConnect
            };
        }

        match self.protocol {
            HandshakeProtocolState::Undecided => Ok(ProxyHandshakeStep::NeedMoreData),
            HandshakeProtocolState::Socks5 => self.next_socks5_step(),
            HandshakeProtocolState::HttpConnect => self.next_http_connect_step(),
        }
    }

    fn next_socks5_step(&mut self) -> Result<ProxyHandshakeStep, String> {
        match self.socks_stage {
            Socks5Stage::Greeting => {
                let Some(greeting) = parse_socks5_greeting(&self.buffer)? else {
                    return Ok(ProxyHandshakeStep::NeedMoreData);
                };
                self.buffer.drain(..greeting.consumed_bytes);
                if !greeting.accepts_no_auth {
                    return Ok(ProxyHandshakeStep::SendResponseAndClose(
                        socks5_no_auth_response(false).to_vec(),
                    ));
                }
                self.socks_stage = Socks5Stage::Request;
                Ok(ProxyHandshakeStep::SendResponse(
                    socks5_no_auth_response(true).to_vec(),
                ))
            }
            Socks5Stage::Request => {
                let Some(request) = parse_socks5_connect_request(&self.buffer)? else {
                    return Ok(ProxyHandshakeStep::NeedMoreData);
                };
                self.buffer.clear();
                if request.command != 0x01 {
                    return Ok(ProxyHandshakeStep::SendResponseAndClose(
                        [0x05, 0x07, 0x00, 0x01, 0, 0, 0, 0, 0, 0].to_vec(),
                    ));
                }
                Ok(ProxyHandshakeStep::OpenStream {
                    protocol: ProxyHandshakeProtocol::Socks5,
                    target: request.target,
                    success_response: socks5_connect_response(true).to_vec(),
                    failure_response: socks5_connect_response(false).to_vec(),
                    pending_payload: request.pending_payload,
                })
            }
        }
    }

    fn next_http_connect_step(&mut self) -> Result<ProxyHandshakeStep, String> {
        let Some(request) = parse_http_connect_request(&self.buffer)? else {
            return Ok(ProxyHandshakeStep::NeedMoreData);
        };
        self.buffer.clear();
        Ok(ProxyHandshakeStep::OpenStream {
            protocol: if request.is_connect {
                ProxyHandshakeProtocol::HttpConnect
            } else {
                ProxyHandshakeProtocol::HttpForward
            },
            target: request.target,
            success_response: if request.is_connect {
                http_connect_response(true).to_vec()
            } else {
                Vec::new()
            },
            failure_response: http_connect_response(false).to_vec(),
            pending_payload: request.pending_payload,
        })
    }
}

pub(crate) trait ProxyStream: Read + Write + Send {
    fn try_clone_box(&self) -> io::Result<Box<dyn ProxyStream>>;
    fn shutdown(&self, how: Shutdown) -> io::Result<()>;
}

impl ProxyStream for TcpStream {
    fn try_clone_box(&self) -> io::Result<Box<dyn ProxyStream>> {
        self.try_clone()
            .map(|stream| Box::new(stream) as Box<dyn ProxyStream>)
    }

    fn shutdown(&self, how: Shutdown) -> io::Result<()> {
        TcpStream::shutdown(self, how)
    }
}

pub(crate) trait ProxyConnector: Send + Sync {
    fn open_stream(
        &self,
        target: &ProxyTarget,
        protocol: ProxyHandshakeProtocol,
    ) -> Result<Box<dyn ProxyStream>, String>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProxyTrafficObservation {
    pub protocol: ProxyHandshakeProtocol,
    pub target: ProxyTarget,
    pub upstream_prefix: Vec<u8>,
    pub upstream_truncated: bool,
    pub downstream_prefix: Vec<u8>,
    pub downstream_truncated: bool,
    pub started_at_ms: u64,
    pub completed_at_ms: u64,
}

pub(crate) trait ProxyTrafficObserver: Send + Sync {
    fn observe(&self, observation: ProxyTrafficObservation);
}

#[derive(Debug, Default)]
struct ProxyTrafficCapture {
    bytes: Vec<u8>,
    truncated: bool,
}

impl ProxyTrafficCapture {
    fn append(&mut self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        let remaining = MAX_PROXY_OBSERVATION_BYTES.saturating_sub(self.bytes.len());
        if remaining == 0 {
            self.truncated = true;
            return;
        }
        let capture_len = remaining.min(bytes.len());
        self.bytes.extend_from_slice(&bytes[..capture_len]);
        if capture_len < bytes.len() {
            self.truncated = true;
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DaemonProxyStreamEvent {
    Data(Vec<u8>),
    Eof,
    Error(String),
}

#[derive(Debug, Deserialize)]
struct DaemonRpcError {
    code: String,
    message: String,
}

#[derive(Debug, Deserialize)]
struct DaemonRpcResponse {
    id: u64,
    ok: bool,
    #[serde(default)]
    result: Option<Value>,
    #[serde(default)]
    error: Option<DaemonRpcError>,
}

#[derive(Debug, Deserialize)]
struct DaemonRpcEvent {
    event: String,
    #[serde(default)]
    stream_id: String,
    #[serde(default)]
    data_base64: String,
    #[serde(default)]
    error: String,
}

#[derive(Debug, Deserialize)]
struct DaemonRpcFrameProbe {
    #[serde(default)]
    id: Option<u64>,
}

pub(crate) fn daemon_proxy_open_request(
    id: u64,
    target: &ProxyTarget,
    timeout_ms: u64,
) -> Result<String, String> {
    daemon_rpc_request_line(
        id,
        "proxy.open",
        json!({
            "host": target.host,
            "port": target.port,
            "timeout_ms": timeout_ms,
        }),
    )
}

pub(crate) fn daemon_proxy_write_request(
    id: u64,
    stream_id: &str,
    data: &[u8],
    timeout_ms: u64,
) -> Result<String, String> {
    daemon_rpc_request_line(
        id,
        "proxy.write",
        json!({
            "stream_id": stream_id,
            "data_base64": base64::engine::general_purpose::STANDARD.encode(data),
            "timeout_ms": timeout_ms,
        }),
    )
}

pub(crate) fn daemon_proxy_close_request(id: u64, stream_id: &str) -> Result<String, String> {
    daemon_rpc_request_line(
        id,
        "proxy.close",
        json!({
            "stream_id": stream_id,
        }),
    )
}

pub(crate) fn daemon_proxy_subscribe_request(id: u64, stream_id: &str) -> Result<String, String> {
    daemon_rpc_request_line(
        id,
        "proxy.stream.subscribe",
        json!({
            "stream_id": stream_id,
        }),
    )
}

fn daemon_rpc_request_line(id: u64, method: &str, params: Value) -> Result<String, String> {
    let encoded = serde_json::to_string(&json!({
        "id": id,
        "method": method,
        "params": params,
    }))
    .map_err(|error| format!("failed to encode daemon RPC request: {error}"))?;
    Ok(cmux_ipc::append_line(&encoded))
}

pub(crate) fn daemon_proxy_open_stream_id(line: &str, expected_id: u64) -> Result<String, String> {
    let result = daemon_rpc_success_result(line, expected_id)?;
    result
        .get("stream_id")
        .and_then(Value::as_str)
        .map(str::to_string)
        .filter(|stream_id| !stream_id.is_empty())
        .ok_or_else(|| "proxy.open response missing stream_id".to_string())
}

pub(crate) fn daemon_proxy_written_bytes(line: &str, expected_id: u64) -> Result<u64, String> {
    let result = daemon_rpc_success_result(line, expected_id)?;
    result
        .get("written")
        .and_then(Value::as_u64)
        .ok_or_else(|| "proxy.write response missing written byte count".to_string())
}

pub(crate) fn daemon_rpc_success_result(line: &str, expected_id: u64) -> Result<Value, String> {
    let response: DaemonRpcResponse = serde_json::from_str(line.trim_end())
        .map_err(|error| format!("failed to decode daemon RPC response: {error}"))?;
    if response.id != expected_id {
        return Err(format!(
            "daemon RPC response id mismatch: got {}, expected {expected_id}",
            response.id
        ));
    }
    if !response.ok {
        let Some(error) = response.error else {
            return Err("daemon RPC failed without error details".to_string());
        };
        return Err(format!("{}: {}", error.code, error.message));
    }
    Ok(response.result.unwrap_or(Value::Null))
}

pub(crate) fn daemon_proxy_stream_event(
    line: &str,
    expected_stream_id: &str,
) -> Result<Option<DaemonProxyStreamEvent>, String> {
    let Some((stream_id, event)) = daemon_proxy_stream_event_frame(line)? else {
        return Ok(None);
    };
    if stream_id != expected_stream_id {
        return Ok(None);
    }
    Ok(Some(event))
}

fn daemon_proxy_stream_event_frame(
    line: &str,
) -> Result<Option<(String, DaemonProxyStreamEvent)>, String> {
    let event: DaemonRpcEvent = serde_json::from_str(line.trim_end())
        .map_err(|error| format!("failed to decode daemon RPC event: {error}"))?;
    if !event.event.starts_with("proxy.stream.") {
        return Ok(None);
    }
    let stream_id = event.stream_id;
    match event.event.as_str() {
        "proxy.stream.data" => {
            let data = base64::engine::general_purpose::STANDARD
                .decode(event.data_base64)
                .map_err(|error| format!("proxy.stream.data payload was not base64: {error}"))?;
            Ok(Some((stream_id, DaemonProxyStreamEvent::Data(data))))
        }
        "proxy.stream.eof" => Ok(Some((stream_id, DaemonProxyStreamEvent::Eof))),
        "proxy.stream.error" => Ok(Some((
            stream_id,
            DaemonProxyStreamEvent::Error(event.error),
        ))),
        other => Err(format!("unknown proxy stream event: {other}")),
    }
}

fn daemon_rpc_frame_id(line: &str) -> Option<u64> {
    serde_json::from_str::<DaemonRpcFrameProbe>(line.trim_end())
        .ok()
        .and_then(|frame| frame.id)
}

pub(crate) struct DaemonProxyRpcClient {
    writer: Mutex<Box<dyn Write + Send>>,
    pending: Mutex<HashMap<u64, mpsc::Sender<String>>>,
    stream_events: Mutex<HashMap<String, mpsc::Sender<DaemonProxyStreamEvent>>>,
    next_id: AtomicU64,
}

impl DaemonProxyRpcClient {
    pub(crate) fn start<R, W>(reader: R, writer: W) -> Arc<Self>
    where
        R: Read + Send + 'static,
        W: Write + Send + 'static,
    {
        let client = Arc::new(Self {
            writer: Mutex::new(Box::new(writer)),
            pending: Mutex::new(HashMap::new()),
            stream_events: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
        });
        let reader_client = Arc::clone(&client);
        let _ = thread::Builder::new()
            .name("cmux-remote-proxy-rpc-reader".to_string())
            .spawn(move || reader_client.read_loop(reader));
        client
    }

    fn read_loop<R>(&self, reader: R)
    where
        R: Read,
    {
        let mut reader = BufReader::new(reader);
        let mut line = String::new();
        loop {
            line.clear();
            let read = match reader.read_line(&mut line) {
                Ok(read) => read,
                Err(_) => break,
            };
            if read == 0 {
                break;
            }
            if line.trim().is_empty() {
                continue;
            }

            if let Some(id) = daemon_rpc_frame_id(&line) {
                let sender = {
                    let mut pending = self
                        .pending
                        .lock()
                        .expect("daemon proxy pending map mutex poisoned");
                    pending.remove(&id)
                };
                if let Some(sender) = sender {
                    let _ = sender.send(line.clone());
                }
                continue;
            }

            let Ok(Some((stream_id, event))) = daemon_proxy_stream_event_frame(&line) else {
                continue;
            };
            let sender = {
                let stream_events = self
                    .stream_events
                    .lock()
                    .expect("daemon proxy event map mutex poisoned");
                stream_events.get(&stream_id).cloned()
            };
            if let Some(sender) = sender {
                let _ = sender.send(event);
            }
        }
    }

    fn next_request_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::SeqCst)
    }

    fn request_response(
        &self,
        build_line: impl FnOnce(u64) -> Result<String, String>,
        timeout: Duration,
    ) -> Result<(u64, String), String> {
        let id = self.next_request_id();
        let line = build_line(id)?;
        let (sender, receiver) = mpsc::channel();
        {
            let mut pending = self
                .pending
                .lock()
                .expect("daemon proxy pending map mutex poisoned");
            pending.insert(id, sender);
        }
        let write_result = {
            let mut writer = self
                .writer
                .lock()
                .expect("daemon proxy writer mutex poisoned");
            writer
                .write_all(line.as_bytes())
                .and_then(|_| writer.flush())
                .map_err(|error| format!("failed to write daemon RPC request: {error}"))
        };
        if let Err(error) = write_result {
            let mut pending = self
                .pending
                .lock()
                .expect("daemon proxy pending map mutex poisoned");
            pending.remove(&id);
            return Err(error);
        }

        receiver
            .recv_timeout(timeout)
            .map(|line| (id, line))
            .map_err(|_| format!("daemon RPC request {id} timed out"))
    }

    fn subscribe_stream(
        &self,
        stream_id: &str,
        timeout: Duration,
    ) -> Result<mpsc::Receiver<DaemonProxyStreamEvent>, String> {
        let (sender, receiver) = mpsc::channel();
        {
            let mut stream_events = self
                .stream_events
                .lock()
                .expect("daemon proxy event map mutex poisoned");
            stream_events.insert(stream_id.to_string(), sender);
        }

        let response =
            self.request_response(|id| daemon_proxy_subscribe_request(id, stream_id), timeout);
        match response {
            Ok((id, line)) => {
                daemon_rpc_success_result(&line, id)?;
                Ok(receiver)
            }
            Err(error) => {
                self.unsubscribe_stream(stream_id);
                Err(error)
            }
        }
    }

    fn unsubscribe_stream(&self, stream_id: &str) {
        let mut stream_events = self
            .stream_events
            .lock()
            .expect("daemon proxy event map mutex poisoned");
        stream_events.remove(stream_id);
    }

    fn proxy_open_stream(
        self: &Arc<Self>,
        target: &ProxyTarget,
        timeout: Duration,
    ) -> Result<Box<dyn ProxyStream>, String> {
        let (id, line) = self.request_response(
            |id| daemon_proxy_open_request(id, target, millis(timeout)),
            timeout,
        )?;
        let stream_id = daemon_proxy_open_stream_id(&line, id)?;
        let events = self.subscribe_stream(&stream_id, timeout)?;
        Ok(Box::new(DaemonProxyStream {
            inner: Arc::new(DaemonProxyStreamInner {
                client: Arc::clone(self),
                stream_id,
                events: Mutex::new(events),
                buffered: Mutex::new(VecDeque::new()),
                closed: AtomicBool::new(false),
            }),
        }))
    }

    fn proxy_write_stream(
        &self,
        stream_id: &str,
        data: &[u8],
        timeout: Duration,
    ) -> io::Result<usize> {
        let (id, line) = self
            .request_response(
                |id| daemon_proxy_write_request(id, stream_id, data, millis(timeout)),
                timeout,
            )
            .map_err(io_other)?;
        let written = daemon_proxy_written_bytes(&line, id).map_err(io_other)?;
        usize::try_from(written).map_err(|_| io_other("daemon proxy write count overflow"))
    }

    fn proxy_close_stream(&self, stream_id: &str, timeout: Duration) -> Result<(), String> {
        let (id, line) =
            self.request_response(|id| daemon_proxy_close_request(id, stream_id), timeout)?;
        daemon_rpc_success_result(&line, id)?;
        Ok(())
    }
}

pub(crate) struct DaemonProxyConnector {
    client: Arc<DaemonProxyRpcClient>,
    timeout: Duration,
}

impl DaemonProxyConnector {
    pub(crate) fn new(client: Arc<DaemonProxyRpcClient>) -> Self {
        Self {
            client,
            timeout: DEFAULT_DAEMON_OPEN_TIMEOUT,
        }
    }

    pub(crate) fn with_timeout(client: Arc<DaemonProxyRpcClient>, timeout: Duration) -> Self {
        Self { client, timeout }
    }
}

impl ProxyConnector for DaemonProxyConnector {
    fn open_stream(
        &self,
        target: &ProxyTarget,
        _protocol: ProxyHandshakeProtocol,
    ) -> Result<Box<dyn ProxyStream>, String> {
        self.client.proxy_open_stream(target, self.timeout)
    }
}

pub(crate) struct DirectTcpProxyConnector {
    target_override: Option<ProxyTarget>,
}

impl DirectTcpProxyConnector {
    pub(crate) fn new(target_override: Option<ProxyTarget>) -> Self {
        Self { target_override }
    }
}

impl ProxyConnector for DirectTcpProxyConnector {
    fn open_stream(
        &self,
        target: &ProxyTarget,
        _protocol: ProxyHandshakeProtocol,
    ) -> Result<Box<dyn ProxyStream>, String> {
        let dial_target = self.target_override.as_ref().unwrap_or(target);
        let stream =
            TcpStream::connect((dial_target.host.as_str(), dial_target.port)).map_err(|error| {
                format!(
                    "failed to connect proxy target {}:{}: {error}",
                    dial_target.host, dial_target.port
                )
            })?;
        Ok(Box::new(stream))
    }
}

pub(crate) struct DaemonProxyProcess {
    child: Arc<Mutex<Child>>,
    client: Arc<DaemonProxyRpcClient>,
    stderr_handle: Option<JoinHandle<()>>,
}

impl DaemonProxyProcess {
    pub(crate) fn spawn(program: &str, args: &[String]) -> Result<Self, String> {
        let mut child = Command::new(program)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| format!("failed to start remote daemon stdio process: {error}"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "remote daemon stdout was not piped".to_string())?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| "remote daemon stdin was not piped".to_string())?;
        let stderr_handle = child.stderr.take().map(|stderr| {
            thread::Builder::new()
                .name("cmux-remote-proxy-daemon-stderr".to_string())
                .spawn(move || drain_daemon_stderr(stderr))
                .ok()
        });
        let client = DaemonProxyRpcClient::start(stdout, stdin);
        Ok(Self {
            child: Arc::new(Mutex::new(child)),
            client,
            stderr_handle: stderr_handle.flatten(),
        })
    }

    pub(crate) fn client(&self) -> Arc<DaemonProxyRpcClient> {
        Arc::clone(&self.client)
    }

    pub(crate) fn connector(&self) -> DaemonProxyConnector {
        DaemonProxyConnector::new(self.client())
    }
}

pub(crate) fn spawn_ssh_daemon_proxy_process(
    config: &cmux_ssh::SshBatchConfiguration,
    remote_daemon_path: &str,
) -> Result<DaemonProxyProcess, String> {
    let remote_daemon_path = remote_daemon_path.trim();
    if remote_daemon_path.is_empty() {
        return Err("remote daemon path is required to start SSH proxy transport".to_string());
    }
    let args = config.daemon_transport_arguments(remote_daemon_path);
    DaemonProxyProcess::spawn("ssh", &args)
}

pub(crate) fn spawn_ssh_daemon_proxy_process_from_relay_map(
    config: &cmux_ssh::SshBatchConfiguration,
    remote_relay_port: u16,
) -> Result<DaemonProxyProcess, String> {
    let args = config
        .daemon_transport_arguments_from_relay_map(i64::from(remote_relay_port))
        .ok_or_else(|| "remote daemon relay port must be 1-65535".to_string())?;
    DaemonProxyProcess::spawn("ssh", &args)
}

impl Drop for DaemonProxyProcess {
    fn drop(&mut self) {
        if let Ok(mut child) = self.child.lock() {
            match child.try_wait() {
                Ok(Some(_)) => {}
                Ok(None) => {
                    let _ = child.kill();
                    let _ = child.wait();
                }
                Err(_) => {
                    let _ = child.kill();
                }
            }
        }
        if let Some(handle) = self.stderr_handle.take() {
            let _ = handle.join();
        }
    }
}

fn drain_daemon_stderr<R>(reader: R)
where
    R: Read,
{
    let mut reader = BufReader::new(reader);
    let mut buffer = String::new();
    loop {
        buffer.clear();
        match reader.read_line(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(_) => {
                let message = buffer.trim_end();
                if !message.is_empty() {
                    eprintln!("[cmux-remote-daemon] {message}");
                }
            }
        }
    }
}

struct DaemonProxyStreamInner {
    client: Arc<DaemonProxyRpcClient>,
    stream_id: String,
    events: Mutex<mpsc::Receiver<DaemonProxyStreamEvent>>,
    buffered: Mutex<VecDeque<u8>>,
    closed: AtomicBool,
}

impl DaemonProxyStreamInner {
    fn close_once(&self) -> Result<(), String> {
        if self.closed.swap(true, Ordering::SeqCst) {
            return Ok(());
        }
        self.client.unsubscribe_stream(&self.stream_id);
        self.client
            .proxy_close_stream(&self.stream_id, DAEMON_PROXY_CLOSE_TIMEOUT)
    }
}

impl Drop for DaemonProxyStreamInner {
    fn drop(&mut self) {
        let _ = self.close_once();
    }
}

pub(crate) struct DaemonProxyStream {
    inner: Arc<DaemonProxyStreamInner>,
}

impl Read for DaemonProxyStream {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        if output.is_empty() {
            return Ok(0);
        }

        loop {
            {
                let mut buffered = self
                    .inner
                    .buffered
                    .lock()
                    .expect("daemon proxy stream buffer mutex poisoned");
                let count = output.len().min(buffered.len());
                for slot in output.iter_mut().take(count) {
                    *slot = buffered
                        .pop_front()
                        .expect("buffer length checked before pop");
                }
                if count > 0 {
                    return Ok(count);
                }
            }

            let event = {
                let events = self
                    .inner
                    .events
                    .lock()
                    .expect("daemon proxy stream event receiver mutex poisoned");
                events.recv().map_err(|_| {
                    io::Error::new(io::ErrorKind::UnexpectedEof, "daemon proxy stream closed")
                })?
            };
            match event {
                DaemonProxyStreamEvent::Data(data) => {
                    let mut buffered = self
                        .inner
                        .buffered
                        .lock()
                        .expect("daemon proxy stream buffer mutex poisoned");
                    buffered.extend(data);
                }
                DaemonProxyStreamEvent::Eof => return Ok(0),
                DaemonProxyStreamEvent::Error(error) => return Err(io_other(error)),
            }
        }
    }
}

impl Write for DaemonProxyStream {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        self.inner.client.proxy_write_stream(
            &self.inner.stream_id,
            data,
            DAEMON_PROXY_WRITE_TIMEOUT,
        )
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl ProxyStream for DaemonProxyStream {
    fn try_clone_box(&self) -> io::Result<Box<dyn ProxyStream>> {
        Ok(Box::new(Self {
            inner: Arc::clone(&self.inner),
        }))
    }

    fn shutdown(&self, how: Shutdown) -> io::Result<()> {
        if matches!(how, Shutdown::Both) {
            self.inner.close_once().map_err(io_other)?;
        }
        Ok(())
    }
}

fn millis(duration: Duration) -> u64 {
    duration.as_millis().try_into().unwrap_or(u64::MAX)
}

fn io_other(error: impl ToString) -> io::Error {
    io::Error::new(io::ErrorKind::Other, error.to_string())
}

pub(crate) struct LoopbackProxyBroker {
    local_addr: SocketAddr,
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl LoopbackProxyBroker {
    pub(crate) fn start(port: u16, connector: Arc<dyn ProxyConnector>) -> Result<Self, String> {
        Self::start_with_observer(port, connector, None)
    }

    pub(crate) fn start_with_observer(
        port: u16,
        connector: Arc<dyn ProxyConnector>,
        observer: Option<Arc<dyn ProxyTrafficObserver>>,
    ) -> Result<Self, String> {
        let listener = TcpListener::bind(("127.0.0.1", port))
            .map_err(|error| format!("failed to bind local proxy listener: {error}"))?;
        listener
            .set_nonblocking(true)
            .map_err(|error| format!("failed to configure local proxy listener: {error}"))?;
        let local_addr = listener
            .local_addr()
            .map_err(|error| format!("failed to read local proxy address: {error}"))?;
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);
        let handle = thread::Builder::new()
            .name(format!("cmux-remote-proxy-{}", local_addr.port()))
            .spawn(move || accept_loop(listener, connector, observer, thread_stop))
            .map_err(|error| format!("failed to start local proxy listener: {error}"))?;
        Ok(Self {
            local_addr,
            stop,
            handle: Some(handle),
        })
    }

    pub(crate) fn local_port(&self) -> u16 {
        self.local_addr.port()
    }

    pub(crate) fn proxy_url(&self) -> String {
        loopback_socks5_proxy_url(self.local_port())
    }

    pub(crate) fn http_proxy_url(&self) -> String {
        loopback_http_proxy_url(self.local_port())
    }

    pub(crate) fn stop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for LoopbackProxyBroker {
    fn drop(&mut self) {
        self.stop();
    }
}

#[derive(Default)]
pub(crate) struct RemoteProxyBrokerState {
    brokers: Mutex<HashMap<String, RemoteWorkspaceProxyRuntime>>,
    direct_panel_brokers: Mutex<HashMap<String, LoopbackProxyBroker>>,
}

struct RemoteWorkspaceProxyRuntime {
    broker: LoopbackProxyBroker,
    _process: Option<DaemonProxyProcess>,
    panel_brokers: HashMap<String, LoopbackProxyBroker>,
}

impl RemoteProxyBrokerState {
    pub(crate) fn start_workspace_broker(
        &self,
        workspace_id: &str,
        port: u16,
        connector: Arc<dyn ProxyConnector>,
    ) -> Result<String, String> {
        if workspace_id.trim().is_empty() {
            return Err("workspace id is required to start a remote proxy broker".to_string());
        }

        self.stop_workspace_broker(workspace_id);
        let broker = LoopbackProxyBroker::start(port, connector)?;
        let proxy_url = broker.proxy_url();
        let mut guard = self
            .brokers
            .lock()
            .expect("remote proxy broker state mutex poisoned");
        guard.insert(
            workspace_id.to_string(),
            RemoteWorkspaceProxyRuntime {
                broker,
                _process: None,
                panel_brokers: HashMap::new(),
            },
        );
        Ok(proxy_url)
    }

    pub(crate) fn start_ssh_workspace_broker(
        &self,
        workspace_id: &str,
        port: u16,
        ssh_config: &cmux_ssh::SshBatchConfiguration,
        remote_daemon_path: &str,
        observer: Option<Arc<dyn ProxyTrafficObserver>>,
    ) -> Result<String, String> {
        if workspace_id.trim().is_empty() {
            return Err("workspace id is required to start a remote proxy broker".to_string());
        }

        self.stop_workspace_broker(workspace_id);
        let process = spawn_ssh_daemon_proxy_process(ssh_config, remote_daemon_path)?;
        let connector = Arc::new(process.connector());
        let broker = LoopbackProxyBroker::start_with_observer(port, connector, observer)?;
        let proxy_url = broker.proxy_url();
        let mut guard = self
            .brokers
            .lock()
            .expect("remote proxy broker state mutex poisoned");
        guard.insert(
            workspace_id.to_string(),
            RemoteWorkspaceProxyRuntime {
                broker,
                _process: Some(process),
                panel_brokers: HashMap::new(),
            },
        );
        Ok(proxy_url)
    }

    pub(crate) fn start_ssh_workspace_broker_from_relay_map(
        &self,
        workspace_id: &str,
        port: u16,
        ssh_config: &cmux_ssh::SshBatchConfiguration,
        remote_relay_port: u16,
        observer: Option<Arc<dyn ProxyTrafficObserver>>,
    ) -> Result<String, String> {
        if workspace_id.trim().is_empty() {
            return Err("workspace id is required to start a remote proxy broker".to_string());
        }

        self.stop_workspace_broker(workspace_id);
        let process = spawn_ssh_daemon_proxy_process_from_relay_map(ssh_config, remote_relay_port)?;
        let connector = Arc::new(process.connector());
        let broker = LoopbackProxyBroker::start_with_observer(port, connector, observer)?;
        let proxy_url = broker.proxy_url();
        let mut guard = self
            .brokers
            .lock()
            .expect("remote proxy broker state mutex poisoned");
        guard.insert(
            workspace_id.to_string(),
            RemoteWorkspaceProxyRuntime {
                broker,
                _process: Some(process),
                panel_brokers: HashMap::new(),
            },
        );
        Ok(proxy_url)
    }

    pub(crate) fn start_workspace_panel_broker(
        &self,
        workspace_id: &str,
        panel_id: &str,
        observer: Option<Arc<dyn ProxyTrafficObserver>>,
    ) -> Result<String, String> {
        if panel_id.trim().is_empty() {
            return Err("panel id is required to start a pane proxy broker".to_string());
        }
        let mut guard = self
            .brokers
            .lock()
            .expect("remote proxy broker state mutex poisoned");
        let runtime = guard
            .get_mut(workspace_id)
            .ok_or_else(|| "workspace remote proxy runtime is not running".to_string())?;
        let process = runtime
            ._process
            .as_ref()
            .ok_or_else(|| "workspace remote proxy runtime has no daemon process".to_string())?;
        runtime.panel_brokers.remove(panel_id);
        let connector = Arc::new(process.connector());
        let broker = LoopbackProxyBroker::start_with_observer(0, connector, observer)?;
        let proxy_url = broker.proxy_url();
        runtime.panel_brokers.insert(panel_id.to_string(), broker);
        Ok(proxy_url)
    }

    pub(crate) fn start_direct_panel_broker(
        &self,
        panel_id: &str,
        target_override: Option<ProxyTarget>,
        observer: Option<Arc<dyn ProxyTrafficObserver>>,
    ) -> Result<String, String> {
        if panel_id.trim().is_empty() {
            return Err("panel id is required to start a direct pane proxy broker".to_string());
        }

        self.stop_panel_broker(panel_id);
        let connector = Arc::new(DirectTcpProxyConnector::new(target_override));
        let broker = LoopbackProxyBroker::start_with_observer(0, connector, observer)?;
        let proxy_url = broker.http_proxy_url();
        let mut guard = self
            .direct_panel_brokers
            .lock()
            .expect("direct proxy broker state mutex poisoned");
        guard.insert(panel_id.to_string(), broker);
        Ok(proxy_url)
    }

    pub(crate) fn stop_workspace_broker(&self, workspace_id: &str) -> bool {
        let removed = {
            let mut guard = self
                .brokers
                .lock()
                .expect("remote proxy broker state mutex poisoned");
            guard.remove(workspace_id)
        };
        removed.is_some()
    }

    pub(crate) fn stop_panel_broker(&self, panel_id: &str) -> bool {
        let removed_remote = {
            let mut guard = self
                .brokers
                .lock()
                .expect("remote proxy broker state mutex poisoned");
            guard
                .values_mut()
                .any(|runtime| runtime.panel_brokers.remove(panel_id).is_some())
        };
        let removed_direct = self
            .direct_panel_brokers
            .lock()
            .expect("direct proxy broker state mutex poisoned")
            .remove(panel_id)
            .is_some();
        removed_remote || removed_direct
    }

    pub(crate) fn workspace_broker_proxy_url(&self, workspace_id: &str) -> Option<String> {
        let guard = self
            .brokers
            .lock()
            .expect("remote proxy broker state mutex poisoned");
        guard
            .get(workspace_id)
            .map(|runtime| runtime.broker.proxy_url())
    }

    pub(crate) fn stop_all(&self) {
        let mut guard = self
            .brokers
            .lock()
            .expect("remote proxy broker state mutex poisoned");
        guard.clear();
        drop(guard);
        self.direct_panel_brokers
            .lock()
            .expect("direct proxy broker state mutex poisoned")
            .clear();
    }
}

fn accept_loop(
    listener: TcpListener,
    connector: Arc<dyn ProxyConnector>,
    observer: Option<Arc<dyn ProxyTrafficObserver>>,
    stop: Arc<AtomicBool>,
) {
    while !stop.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((client, _addr)) => {
                let connector = Arc::clone(&connector);
                let observer = observer.as_ref().map(Arc::clone);
                let _ = thread::Builder::new()
                    .name("cmux-remote-proxy-session".to_string())
                    .spawn(move || {
                        let _ = handle_proxy_client(client, connector, observer);
                    });
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(25));
            }
            Err(_) => break,
        }
    }
}

fn handle_proxy_client(
    mut client: TcpStream,
    connector: Arc<dyn ProxyConnector>,
    observer: Option<Arc<dyn ProxyTrafficObserver>>,
) -> Result<(), String> {
    let mut handshake = ProxyHandshake::default();
    let mut buffer = [0u8; 32 * 1024];
    loop {
        let read = client
            .read(&mut buffer)
            .map_err(|error| format!("failed to read proxy client handshake: {error}"))?;
        if read == 0 {
            return Ok(());
        }
        for step in handshake.push(&buffer[..read])? {
            match step {
                ProxyHandshakeStep::NeedMoreData => {}
                ProxyHandshakeStep::SendResponse(response) => {
                    client.write_all(&response).map_err(|error| {
                        format!("failed to write proxy handshake response: {error}")
                    })?;
                }
                ProxyHandshakeStep::SendResponseAndClose(response) => {
                    let _ = client.write_all(&response);
                    let _ = client.shutdown(Shutdown::Both);
                    return Ok(());
                }
                ProxyHandshakeStep::OpenStream {
                    protocol,
                    target,
                    success_response,
                    failure_response,
                    pending_payload,
                } => {
                    let mut remote = match connector.open_stream(&target, protocol.clone()) {
                        Ok(remote) => remote,
                        Err(error) => {
                            let _ = client.write_all(&failure_response);
                            let _ = client.shutdown(Shutdown::Both);
                            return Err(error);
                        }
                    };
                    client.write_all(&success_response).map_err(|error| {
                        format!("failed to write proxy open success response: {error}")
                    })?;
                    if !pending_payload.is_empty() {
                        remote.write_all(&pending_payload).map_err(|error| {
                            format!("failed to forward pipelined payload: {error}")
                        })?;
                    }
                    return relay_bidirectional(
                        client,
                        remote,
                        observer,
                        protocol,
                        target,
                        pending_payload,
                    );
                }
            }
        }
    }
}

fn relay_bidirectional(
    mut client: TcpStream,
    mut remote: Box<dyn ProxyStream>,
    observer: Option<Arc<dyn ProxyTrafficObserver>>,
    protocol: ProxyHandshakeProtocol,
    target: ProxyTarget,
    pending_payload: Vec<u8>,
) -> Result<(), String> {
    let started_at_ms = now_ms();
    let upstream_capture = Arc::new(Mutex::new(ProxyTrafficCapture::default()));
    if !pending_payload.is_empty() {
        upstream_capture
            .lock()
            .expect("proxy upstream capture mutex poisoned")
            .append(&pending_payload);
    }
    let downstream_capture = Arc::new(Mutex::new(ProxyTrafficCapture::default()));
    let mut client_reader = client
        .try_clone()
        .map_err(|error| format!("failed to clone proxy client stream: {error}"))?;
    let mut remote_writer = remote
        .try_clone_box()
        .map_err(|error| format!("failed to clone remote proxy stream: {error}"))?;
    let upstream_thread_capture = Arc::clone(&upstream_capture);

    let upstream = thread::Builder::new()
        .name("cmux-remote-proxy-upstream".to_string())
        .spawn(move || {
            let result = copy_with_capture(
                &mut client_reader,
                &mut remote_writer,
                &upstream_thread_capture,
            );
            let _ = remote_writer.flush();
            let _ = remote_writer.shutdown(Shutdown::Write);
            result
        })
        .map_err(|error| format!("failed to start proxy upstream relay: {error}"))?;

    let downstream_result = copy_with_capture(&mut remote, &mut client, &downstream_capture);
    let _ = client.flush();
    let _ = client.shutdown(Shutdown::Write);
    let upstream_result = upstream
        .join()
        .map_err(|_| io_other("proxy upstream relay panicked"));
    let _ = client.shutdown(Shutdown::Read);
    let _ = remote.shutdown(Shutdown::Both);
    if let Some(observer) = observer {
        let completed_at_ms = now_ms();
        let upstream = upstream_capture
            .lock()
            .expect("proxy upstream capture mutex poisoned");
        let downstream = downstream_capture
            .lock()
            .expect("proxy downstream capture mutex poisoned");
        observer.observe(ProxyTrafficObservation {
            protocol,
            target,
            upstream_prefix: upstream.bytes.clone(),
            upstream_truncated: upstream.truncated,
            downstream_prefix: downstream.bytes.clone(),
            downstream_truncated: downstream.truncated,
            started_at_ms,
            completed_at_ms,
        });
    }
    let upstream_result =
        upstream_result.map_err(|error| format!("proxy upstream relay failed: {error}"))?;
    upstream_result.map_err(|error| format!("proxy upstream relay failed: {error}"))?;
    downstream_result.map_err(|error| format!("proxy downstream relay failed: {error}"))?;
    Ok(())
}

fn copy_with_capture<R, W>(
    reader: &mut R,
    writer: &mut W,
    capture: &Arc<Mutex<ProxyTrafficCapture>>,
) -> io::Result<u64>
where
    R: Read + ?Sized,
    W: Write + ?Sized,
{
    let mut buffer = [0u8; 32 * 1024];
    let mut total = 0u64;
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            return Ok(total);
        }
        {
            let mut capture = capture
                .lock()
                .expect("proxy traffic capture mutex poisoned");
            capture.append(&buffer[..read]);
        }
        writer.write_all(&buffer[..read])?;
        total = total.saturating_add(read as u64);
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(0)
}

pub(crate) fn loopback_socks5_proxy_url(port: u16) -> String {
    format!("socks5://127.0.0.1:{port}")
}

pub(crate) fn loopback_http_proxy_url(port: u16) -> String {
    format!("http://127.0.0.1:{port}")
}

pub(crate) fn socks5_no_auth_response(accepted: bool) -> [u8; 2] {
    [0x05, if accepted { 0x00 } else { 0xff }]
}

pub(crate) fn socks5_connect_response(succeeded: bool) -> [u8; 10] {
    [
        0x05,
        if succeeded { 0x00 } else { 0x05 },
        0x00,
        0x01,
        0x00,
        0x00,
        0x00,
        0x00,
        0x00,
        0x00,
    ]
}

pub(crate) fn http_connect_response(succeeded: bool) -> &'static [u8] {
    if succeeded {
        b"HTTP/1.1 200 Connection Established\r\n\r\n"
    } else {
        b"HTTP/1.1 502 Bad Gateway\r\n\r\n"
    }
}

pub(crate) fn parse_socks5_greeting(bytes: &[u8]) -> Result<Option<Socks5Greeting>, String> {
    if bytes.len() > MAX_HANDSHAKE_BYTES {
        return Err(format!(
            "proxy handshake exceeded {MAX_HANDSHAKE_BYTES} bytes"
        ));
    }
    if bytes.len() < 2 {
        return Ok(None);
    }
    if bytes[0] != 0x05 {
        return Err("invalid SOCKS version".to_string());
    }
    let method_count = bytes[1] as usize;
    let total = 2 + method_count;
    if bytes.len() < total {
        return Ok(None);
    }
    Ok(Some(Socks5Greeting {
        consumed_bytes: total,
        accepts_no_auth: bytes[2..total].contains(&0x00),
    }))
}

pub(crate) fn parse_socks5_connect_request(
    bytes: &[u8],
) -> Result<Option<Socks5ConnectRequest>, String> {
    if bytes.len() > MAX_HANDSHAKE_BYTES {
        return Err(format!(
            "proxy handshake exceeded {MAX_HANDSHAKE_BYTES} bytes"
        ));
    }
    if bytes.len() < 4 {
        return Ok(None);
    }
    if bytes[0] != 0x05 {
        return Err("invalid SOCKS version".to_string());
    }

    let command = bytes[1];
    let address_type = bytes[3];
    let mut cursor = 4usize;
    let host = match address_type {
        0x01 => {
            if bytes.len() < cursor + 4 + 2 {
                return Ok(None);
            }
            let address = Ipv4Addr::new(
                bytes[cursor],
                bytes[cursor + 1],
                bytes[cursor + 2],
                bytes[cursor + 3],
            );
            cursor += 4;
            address.to_string()
        }
        0x03 => {
            if bytes.len() < cursor + 1 {
                return Ok(None);
            }
            let length = bytes[cursor] as usize;
            cursor += 1;
            if bytes.len() < cursor + length + 2 {
                return Ok(None);
            }
            let host = std::str::from_utf8(&bytes[cursor..cursor + length])
                .map_err(|_| "SOCKS domain host must be UTF-8".to_string())?
                .trim()
                .to_string();
            cursor += length;
            host
        }
        0x04 => {
            if bytes.len() < cursor + 16 + 2 {
                return Ok(None);
            }
            let mut octets = [0u8; 16];
            octets.copy_from_slice(&bytes[cursor..cursor + 16]);
            cursor += 16;
            Ipv6Addr::from(octets).to_string()
        }
        _ => return Err("invalid SOCKS address type".to_string()),
    };
    if host.is_empty() {
        return Err("empty SOCKS host".to_string());
    }
    let port = u16::from_be_bytes([bytes[cursor], bytes[cursor + 1]]);
    cursor += 2;
    if port == 0 {
        return Err("invalid SOCKS port".to_string());
    }

    Ok(Some(Socks5ConnectRequest {
        target: ProxyTarget { host, port },
        command,
        consumed_bytes: cursor,
        pending_payload: bytes[cursor..].to_vec(),
    }))
}

pub(crate) fn parse_http_connect_request(
    bytes: &[u8],
) -> Result<Option<HttpConnectRequest>, String> {
    if bytes.len() > MAX_HANDSHAKE_BYTES {
        return Err(format!(
            "proxy handshake exceeded {MAX_HANDSHAKE_BYTES} bytes"
        ));
    }
    let Some(head_end) = find_http_head_end(bytes) else {
        return Ok(None);
    };
    let head = std::str::from_utf8(&bytes[..head_end])
        .map_err(|_| "HTTP CONNECT head must be UTF-8".to_string())?;
    let mut lines = head.split("\r\n");
    let request_line = lines
        .next()
        .ok_or_else(|| "missing HTTP CONNECT request line".to_string())?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default();
    let target = parts.next().unwrap_or_default();
    let version = parts.next().unwrap_or_default();
    if !version.starts_with("HTTP/") {
        return Err("invalid HTTP proxy request version".to_string());
    }
    let consumed_bytes = head_end + 4;
    let is_connect = method.eq_ignore_ascii_case("CONNECT");
    let target = if is_connect {
        parse_host_port(target)?
    } else {
        parse_absolute_http_proxy_target(target)?
    };
    Ok(Some(HttpConnectRequest {
        target,
        consumed_bytes,
        pending_payload: if is_connect {
            bytes[consumed_bytes..].to_vec()
        } else {
            bytes.to_vec()
        },
        is_connect,
    }))
}

fn parse_absolute_http_proxy_target(raw: &str) -> Result<ProxyTarget, String> {
    let parsed = url::Url::parse(raw)
        .map_err(|_| "HTTP proxy request target must be an absolute URL".to_string())?;
    match parsed.scheme() {
        "http" => {}
        scheme => {
            return Err(format!(
                "HTTP proxy forward only supports http:// targets, got {scheme}://"
            ));
        }
    }
    let host = parsed
        .host_str()
        .filter(|host| !host.trim().is_empty())
        .ok_or_else(|| "HTTP proxy request target is missing a host".to_string())?;
    let port = parsed.port_or_known_default().unwrap_or(80);
    Ok(ProxyTarget {
        host: host.to_string(),
        port,
    })
}

fn find_http_head_end(bytes: &[u8]) -> Option<usize> {
    bytes.windows(4).position(|window| window == b"\r\n\r\n")
}

fn parse_host_port(raw: &str) -> Result<ProxyTarget, String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err("missing proxy target".to_string());
    }
    let (host, port) = if let Some(rest) = raw.strip_prefix('[') {
        let (host, port_part) = rest
            .split_once("]:")
            .ok_or_else(|| "invalid bracketed proxy target".to_string())?;
        (host, port_part)
    } else {
        raw.rsplit_once(':')
            .ok_or_else(|| "proxy target must include host and port".to_string())?
    };
    let host = host.trim();
    if host.is_empty() {
        return Err("proxy target host is empty".to_string());
    }
    let port = port
        .parse::<u16>()
        .ok()
        .filter(|port| *port > 0)
        .ok_or_else(|| "proxy target port must be 1-65535".to_string())?;
    Ok(ProxyTarget {
        host: host.to_string(),
        port,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Condvar, MutexGuard};
    use std::time::Instant;

    struct ChannelReader {
        receiver: mpsc::Receiver<Vec<u8>>,
        buffered: VecDeque<u8>,
    }

    impl ChannelReader {
        fn new(receiver: mpsc::Receiver<Vec<u8>>) -> Self {
            Self {
                receiver,
                buffered: VecDeque::new(),
            }
        }
    }

    impl Read for ChannelReader {
        fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
            if output.is_empty() {
                return Ok(0);
            }
            while self.buffered.is_empty() {
                match self.receiver.recv_timeout(Duration::from_secs(5)) {
                    Ok(bytes) => self.buffered.extend(bytes),
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        return Err(io::Error::new(
                            io::ErrorKind::TimedOut,
                            "timed out waiting for scripted daemon frame",
                        ));
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(0),
                }
            }
            let count = output.len().min(self.buffered.len());
            for slot in output.iter_mut().take(count) {
                *slot = self
                    .buffered
                    .pop_front()
                    .expect("buffer length checked before pop");
            }
            Ok(count)
        }
    }

    #[derive(Clone, Default)]
    struct CapturedWriter {
        inner: Arc<Mutex<CapturedWriterInner>>,
    }

    #[derive(Default)]
    struct CapturedWriterInner {
        buffered: Vec<u8>,
        lines: Vec<String>,
    }

    impl CapturedWriter {
        fn wait_for_line(&self, index: usize) -> String {
            self.wait_for_line_timeout(index, Duration::from_secs(5))
                .unwrap_or_else(|| {
                    panic!("timed out waiting for captured daemon request line {index}")
                })
        }

        fn wait_for_line_timeout(&self, index: usize, timeout: Duration) -> Option<String> {
            let deadline = Instant::now() + timeout;
            loop {
                {
                    let inner = self.lock();
                    if let Some(line) = inner.lines.get(index) {
                        return Some(line.clone());
                    }
                }
                if Instant::now() >= deadline {
                    return None;
                }
                thread::sleep(Duration::from_millis(10));
            }
        }

        fn lock(&self) -> MutexGuard<'_, CapturedWriterInner> {
            self.inner.lock().expect("captured writer mutex poisoned")
        }
    }

    impl Write for CapturedWriter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            let mut inner = self.lock();
            inner.buffered.extend_from_slice(bytes);
            while let Some(index) = inner.buffered.iter().position(|byte| *byte == b'\n') {
                let line: Vec<u8> = inner.buffered.drain(..=index).collect();
                inner.lines.push(String::from_utf8_lossy(&line).to_string());
            }
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[derive(Clone)]
    struct MemoryProxyStream {
        read_bytes: Arc<Mutex<VecDeque<u8>>>,
        written: Arc<Mutex<Vec<u8>>>,
    }

    impl MemoryProxyStream {
        fn new(read_bytes: &[u8]) -> Self {
            Self {
                read_bytes: Arc::new(Mutex::new(read_bytes.iter().copied().collect())),
                written: Arc::new(Mutex::new(Vec::new())),
            }
        }

        fn written(&self) -> Vec<u8> {
            self.written
                .lock()
                .expect("memory proxy stream written mutex poisoned")
                .clone()
        }
    }

    impl Read for MemoryProxyStream {
        fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
            let mut read_bytes = self
                .read_bytes
                .lock()
                .expect("memory proxy stream read mutex poisoned");
            let count = output.len().min(read_bytes.len());
            for slot in output.iter_mut().take(count) {
                *slot = read_bytes
                    .pop_front()
                    .expect("read length checked before pop");
            }
            Ok(count)
        }
    }

    impl Write for MemoryProxyStream {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.written
                .lock()
                .expect("memory proxy stream written mutex poisoned")
                .extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl ProxyStream for MemoryProxyStream {
        fn try_clone_box(&self) -> io::Result<Box<dyn ProxyStream>> {
            Ok(Box::new(self.clone()))
        }

        fn shutdown(&self, _how: Shutdown) -> io::Result<()> {
            Ok(())
        }
    }

    #[derive(Default)]
    struct RelayWriteBarrierState {
        first_write_started: bool,
        release_first_write: bool,
    }

    #[derive(Clone)]
    struct BarrierProxyStream {
        read_bytes: Arc<Mutex<VecDeque<u8>>>,
        written: Arc<Mutex<Vec<u8>>>,
        barrier: Arc<(Mutex<RelayWriteBarrierState>, Condvar)>,
    }

    impl BarrierProxyStream {
        fn new(read_bytes: &[u8]) -> Self {
            Self {
                read_bytes: Arc::new(Mutex::new(read_bytes.iter().copied().collect())),
                written: Arc::new(Mutex::new(Vec::new())),
                barrier: Arc::new((
                    Mutex::new(RelayWriteBarrierState::default()),
                    Condvar::new(),
                )),
            }
        }

        fn wait_for_first_write(&self) {
            let (lock, ready) = &*self.barrier;
            let state = lock.lock().expect("relay write barrier mutex poisoned");
            let (state, timeout) = ready
                .wait_timeout_while(state, Duration::from_secs(5), |state| {
                    !state.first_write_started
                })
                .expect("relay write barrier mutex poisoned");
            assert!(
                state.first_write_started,
                "upstream relay never reached remote writer"
            );
            assert!(
                !timeout.timed_out(),
                "timed out waiting for upstream relay write"
            );
        }

        fn release_first_write(&self) {
            let (lock, ready) = &*self.barrier;
            let mut state = lock.lock().expect("relay write barrier mutex poisoned");
            state.release_first_write = true;
            ready.notify_all();
        }

        fn written(&self) -> Vec<u8> {
            self.written
                .lock()
                .expect("barrier proxy written mutex poisoned")
                .clone()
        }
    }

    impl Read for BarrierProxyStream {
        fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
            let mut read_bytes = self
                .read_bytes
                .lock()
                .expect("barrier proxy read mutex poisoned");
            let count = output.len().min(read_bytes.len());
            for slot in output.iter_mut().take(count) {
                *slot = read_bytes
                    .pop_front()
                    .expect("read length checked before pop");
            }
            Ok(count)
        }
    }

    impl Write for BarrierProxyStream {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            let (lock, ready) = &*self.barrier;
            let mut state = lock.lock().expect("relay write barrier mutex poisoned");
            if !state.first_write_started {
                state.first_write_started = true;
                ready.notify_all();
                state = ready
                    .wait_while(state, |state| !state.release_first_write)
                    .expect("relay write barrier mutex poisoned");
            }
            drop(state);
            self.written
                .lock()
                .expect("barrier proxy written mutex poisoned")
                .extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl ProxyStream for BarrierProxyStream {
        fn try_clone_box(&self) -> io::Result<Box<dyn ProxyStream>> {
            Ok(Box::new(self.clone()))
        }

        fn shutdown(&self, _how: Shutdown) -> io::Result<()> {
            Ok(())
        }
    }

    struct ChannelTrafficObserver {
        sender: mpsc::Sender<ProxyTrafficObservation>,
    }

    impl ProxyTrafficObserver for ChannelTrafficObserver {
        fn observe(&self, observation: ProxyTrafficObservation) {
            self.sender.send(observation).unwrap();
        }
    }

    fn send_daemon_frame(sender: &mpsc::Sender<Vec<u8>>, frame: Value) {
        let line = cmux_ipc::append_line(&serde_json::to_string(&frame).unwrap());
        sender.send(line.into_bytes()).unwrap();
    }

    fn request_id(line: &str) -> u64 {
        serde_json::from_str::<Value>(line.trim_end()).unwrap()["id"]
            .as_u64()
            .unwrap()
    }

    fn request_method(line: &str) -> String {
        serde_json::from_str::<Value>(line.trim_end()).unwrap()["method"]
            .as_str()
            .unwrap()
            .to_string()
    }

    fn open_scripted_daemon_stream(
        client: Arc<DaemonProxyRpcClient>,
        writer: &CapturedWriter,
        daemon_sender: &mpsc::Sender<Vec<u8>>,
        first_line_index: usize,
        stream_id: &str,
    ) -> Box<dyn ProxyStream> {
        let connector = DaemonProxyConnector::with_timeout(client, Duration::from_secs(2));
        let target = ProxyTarget {
            host: "example.com".to_string(),
            port: 443,
        };
        let (stream_sender, stream_receiver) = mpsc::channel();
        thread::spawn(move || {
            stream_sender
                .send(connector.open_stream(&target, ProxyHandshakeProtocol::Socks5))
                .unwrap();
        });

        let open = writer.wait_for_line(first_line_index);
        assert_eq!(request_method(&open), "proxy.open");
        send_daemon_frame(
            daemon_sender,
            json!({
                "id": request_id(&open),
                "ok": true,
                "result": { "stream_id": stream_id },
            }),
        );
        let subscribe = writer.wait_for_line(first_line_index + 1);
        assert_eq!(request_method(&subscribe), "proxy.stream.subscribe");
        send_daemon_frame(
            daemon_sender,
            json!({
                "id": request_id(&subscribe),
                "ok": true,
                "result": {},
            }),
        );
        stream_receiver
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .unwrap()
    }

    #[test]
    fn socks5_greeting_accepts_no_auth_method() {
        let parsed = parse_socks5_greeting(&[0x05, 0x02, 0x02, 0x00])
            .unwrap()
            .expect("complete greeting");
        assert_eq!(parsed.consumed_bytes, 4);
        assert!(parsed.accepts_no_auth);
        assert_eq!(socks5_no_auth_response(true), [0x05, 0x00]);
    }

    #[test]
    fn socks5_greeting_rejects_missing_no_auth_method() {
        let parsed = parse_socks5_greeting(&[0x05, 0x01, 0x02])
            .unwrap()
            .expect("complete greeting");
        assert_eq!(parsed.consumed_bytes, 3);
        assert!(!parsed.accepts_no_auth);
        assert_eq!(socks5_no_auth_response(false), [0x05, 0xff]);
    }

    #[test]
    fn socks5_request_parses_domain_and_preserves_pipelined_payload() {
        let mut bytes = vec![0x05, 0x01, 0x00, 0x03, 11];
        bytes.extend_from_slice(b"example.com");
        bytes.extend_from_slice(&443u16.to_be_bytes());
        bytes.extend_from_slice(b"GET / HTTP/1.1\r\n\r\n");

        let parsed = parse_socks5_connect_request(&bytes)
            .unwrap()
            .expect("complete request");
        assert_eq!(
            parsed.target,
            ProxyTarget {
                host: "example.com".to_string(),
                port: 443,
            }
        );
        assert_eq!(parsed.command, 0x01);
        assert_eq!(parsed.pending_payload, b"GET / HTTP/1.1\r\n\r\n");
        assert_eq!(
            socks5_connect_response(true),
            [0x05, 0x00, 0x00, 0x01, 0, 0, 0, 0, 0, 0]
        );
    }

    #[test]
    fn socks5_request_parses_ipv4_and_ipv6_targets() {
        let ipv4 = [0x05, 0x01, 0x00, 0x01, 127, 0, 0, 1, 0x1f, 0x90];
        assert_eq!(
            parse_socks5_connect_request(&ipv4).unwrap().unwrap().target,
            ProxyTarget {
                host: "127.0.0.1".to_string(),
                port: 8080,
            }
        );

        let mut ipv6 = vec![0x05, 0x01, 0x00, 0x04];
        ipv6.extend_from_slice(&Ipv6Addr::LOCALHOST.octets());
        ipv6.extend_from_slice(&3000u16.to_be_bytes());
        assert_eq!(
            parse_socks5_connect_request(&ipv6).unwrap().unwrap().target,
            ProxyTarget {
                host: "::1".to_string(),
                port: 3000,
            }
        );
    }

    #[test]
    fn socks5_request_waits_for_incomplete_data_and_rejects_invalid_values() {
        assert!(parse_socks5_connect_request(&[0x05, 0x01])
            .unwrap()
            .is_none());
        assert!(parse_socks5_connect_request(&[0x04, 0x01, 0x00, 0x01]).is_err());
        assert!(parse_socks5_connect_request(&[0x05, 0x01, 0x00, 0x09]).is_err());
        let invalid_port = [0x05, 0x01, 0x00, 0x01, 127, 0, 0, 1, 0, 0];
        assert!(parse_socks5_connect_request(&invalid_port).is_err());
    }

    #[test]
    fn http_connect_parses_host_port_and_preserves_pipelined_payload() {
        let bytes = b"CONNECT example.com:443 HTTP/1.1\r\nHost: example.com:443\r\n\r\nhello";
        let parsed = parse_http_connect_request(bytes)
            .unwrap()
            .expect("complete connect request");
        assert_eq!(
            parsed.target,
            ProxyTarget {
                host: "example.com".to_string(),
                port: 443,
            }
        );
        assert_eq!(parsed.pending_payload, b"hello");
        assert!(parsed.is_connect);
        assert_eq!(
            http_connect_response(true),
            b"HTTP/1.1 200 Connection Established\r\n\r\n"
        );
    }

    #[test]
    fn http_proxy_forward_parses_absolute_form_target_and_preserves_request() {
        let bytes =
            b"GET http://example.com:8080/path?q=1 HTTP/1.1\r\nHost: example.com:8080\r\n\r\n";
        let parsed = parse_http_connect_request(bytes)
            .unwrap()
            .expect("complete absolute-form request");
        assert_eq!(
            parsed.target,
            ProxyTarget {
                host: "example.com".to_string(),
                port: 8080,
            }
        );
        assert_eq!(parsed.pending_payload, bytes);
        assert!(!parsed.is_connect);
    }

    #[test]
    fn http_connect_parses_bracketed_ipv6_target() {
        let parsed = parse_http_connect_request(b"CONNECT [::1]:3000 HTTP/1.1\r\n\r\n")
            .unwrap()
            .expect("complete connect request");
        assert_eq!(
            parsed.target,
            ProxyTarget {
                host: "::1".to_string(),
                port: 3000,
            }
        );
    }

    #[test]
    fn http_connect_waits_for_complete_head_and_rejects_invalid_requests() {
        assert!(
            parse_http_connect_request(b"CONNECT example.com:443 HTTP/1.1\r\n")
                .unwrap()
                .is_none()
        );
        assert!(parse_http_connect_request(b"GET / HTTP/1.1\r\n\r\n").is_err());
        assert!(parse_http_connect_request(b"CONNECT example.com HTTP/1.1\r\n\r\n").is_err());
        assert!(parse_http_connect_request(b"CONNECT example.com:0 HTTP/1.1\r\n\r\n").is_err());
    }

    #[test]
    fn loopback_socks5_proxy_url_matches_webview_proxy_format() {
        assert_eq!(loopback_socks5_proxy_url(31337), "socks5://127.0.0.1:31337");
        assert_eq!(loopback_http_proxy_url(31337), "http://127.0.0.1:31337");
    }

    #[test]
    fn handshake_state_machine_handles_split_socks5_greeting_and_request() {
        let mut handshake = ProxyHandshake::default();
        assert_eq!(
            handshake.push(&[0x05, 0x01]).unwrap(),
            vec![ProxyHandshakeStep::NeedMoreData]
        );
        assert_eq!(
            handshake.push(&[0x00]).unwrap(),
            vec![
                ProxyHandshakeStep::SendResponse(vec![0x05, 0x00]),
                ProxyHandshakeStep::NeedMoreData,
            ]
        );

        let mut request = vec![0x05, 0x01, 0x00, 0x03, 11];
        request.extend_from_slice(b"example.com");
        request.extend_from_slice(&443u16.to_be_bytes());
        request.extend_from_slice(b"payload");
        assert_eq!(
            handshake.push(&request).unwrap(),
            vec![ProxyHandshakeStep::OpenStream {
                protocol: ProxyHandshakeProtocol::Socks5,
                target: ProxyTarget {
                    host: "example.com".to_string(),
                    port: 443,
                },
                success_response: socks5_connect_response(true).to_vec(),
                failure_response: socks5_connect_response(false).to_vec(),
                pending_payload: b"payload".to_vec(),
            }]
        );
    }

    #[test]
    fn handshake_state_machine_handles_http_connect_in_one_step() {
        let mut handshake = ProxyHandshake::default();
        assert_eq!(
            handshake
                .push(b"CONNECT example.com:443 HTTP/1.1\r\n\r\nhello")
                .unwrap(),
            vec![ProxyHandshakeStep::OpenStream {
                protocol: ProxyHandshakeProtocol::HttpConnect,
                target: ProxyTarget {
                    host: "example.com".to_string(),
                    port: 443,
                },
                success_response: http_connect_response(true).to_vec(),
                failure_response: http_connect_response(false).to_vec(),
                pending_payload: b"hello".to_vec(),
            }]
        );
    }

    #[test]
    fn handshake_state_machine_rejects_socks5_no_supported_auth() {
        let mut handshake = ProxyHandshake::default();
        assert_eq!(
            handshake.push(&[0x05, 0x01, 0x02]).unwrap(),
            vec![ProxyHandshakeStep::SendResponseAndClose(vec![0x05, 0xff])]
        );
    }

    #[test]
    fn proxy_traffic_capture_is_bounded_and_marks_truncation() {
        let mut capture = ProxyTrafficCapture::default();
        capture.append(&vec![b'a'; MAX_PROXY_OBSERVATION_BYTES - 1]);
        assert!(!capture.truncated);
        capture.append(b"bc");
        assert_eq!(capture.bytes.len(), MAX_PROXY_OBSERVATION_BYTES);
        assert!(capture.truncated);
        assert_eq!(capture.bytes[MAX_PROXY_OBSERVATION_BYTES - 1], b'b');
    }

    #[test]
    fn relay_bidirectional_observes_pipelined_upstream_and_downstream_prefixes() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let address = listener.local_addr().unwrap();
        let mut client = TcpStream::connect(address).unwrap();
        let (server, _) = listener.accept().unwrap();
        let remote_response = b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\npong";
        let remote = MemoryProxyStream::new(remote_response);
        let remote_written = remote.clone();
        let (sender, receiver) = mpsc::channel();
        let observer = Arc::new(ChannelTrafficObserver { sender });

        let handle = thread::spawn(move || {
            relay_bidirectional(
                server,
                Box::new(remote),
                Some(observer),
                ProxyHandshakeProtocol::Socks5,
                ProxyTarget {
                    host: "example.com".to_string(),
                    port: 80,
                },
                b"GET /first HTTP/1.1\r\n".to_vec(),
            )
            .unwrap();
        });

        client.write_all(b"Host: example.com\r\n\r\n").unwrap();
        client.shutdown(Shutdown::Write).unwrap();
        let mut response = Vec::new();
        client.read_to_end(&mut response).unwrap();
        handle.join().unwrap();

        assert_eq!(response, remote_response);
        assert_eq!(remote_written.written(), b"Host: example.com\r\n\r\n");
        let observation = receiver.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(observation.protocol, ProxyHandshakeProtocol::Socks5);
        assert_eq!(
            observation.target,
            ProxyTarget {
                host: "example.com".to_string(),
                port: 80,
            }
        );
        assert_eq!(
            observation.upstream_prefix,
            b"GET /first HTTP/1.1\r\nHost: example.com\r\n\r\n"
        );
        assert_eq!(observation.downstream_prefix, remote_response);
        assert!(!observation.upstream_truncated);
        assert!(!observation.downstream_truncated);
        assert!(observation.completed_at_ms >= observation.started_at_ms);
    }

    #[test]
    fn relay_bidirectional_drains_upstream_after_downstream_eof_before_read_shutdown() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let address = listener.local_addr().unwrap();
        let mut client = TcpStream::connect(address).unwrap();
        let (server, _) = listener.accept().unwrap();
        let remote_response = b"response-before-final-upstream";
        let remote = BarrierProxyStream::new(remote_response);
        let remote_written = remote.clone();

        let handle = thread::spawn(move || {
            relay_bidirectional(
                server,
                Box::new(remote),
                None,
                ProxyHandshakeProtocol::Socks5,
                ProxyTarget {
                    host: "example.com".to_string(),
                    port: 80,
                },
                Vec::new(),
            )
        });

        client.write_all(b"prefix-").unwrap();
        remote_written.wait_for_first_write();

        let mut response = Vec::new();
        client.read_to_end(&mut response).unwrap();
        let final_write = client.write_all(b"final");
        let final_shutdown = client.shutdown(Shutdown::Write);
        remote_written.release_first_write();
        let relay_result = handle.join().unwrap();

        final_write.expect("local final bytes must remain writable after downstream EOF");
        final_shutdown.expect("local write half-close must remain valid after downstream EOF");
        relay_result.unwrap();
        assert_eq!(response, remote_response);
        assert_eq!(remote_written.written(), b"prefix-final");
    }

    #[test]
    fn daemon_proxy_rpc_builders_match_daemon_wire_shape() {
        let target = ProxyTarget {
            host: "example.com".to_string(),
            port: 443,
        };
        let open = daemon_proxy_open_request(7, &target, 10_000).unwrap();
        let value: Value = serde_json::from_str(open.trim_end()).unwrap();
        assert_eq!(value["id"], json!(7));
        assert_eq!(value["method"], json!("proxy.open"));
        assert_eq!(value["params"]["host"], json!("example.com"));
        assert_eq!(value["params"]["port"], json!(443));
        assert_eq!(value["params"]["timeout_ms"], json!(10_000));

        let write = daemon_proxy_write_request(8, "s-1", b"ping", 8_000).unwrap();
        let value: Value = serde_json::from_str(write.trim_end()).unwrap();
        assert_eq!(value["method"], json!("proxy.write"));
        assert_eq!(value["params"]["stream_id"], json!("s-1"));
        assert_eq!(value["params"]["data_base64"], json!("cGluZw=="));

        let close = daemon_proxy_close_request(9, "s-1").unwrap();
        let value: Value = serde_json::from_str(close.trim_end()).unwrap();
        assert_eq!(value["method"], json!("proxy.close"));

        let subscribe = daemon_proxy_subscribe_request(10, "s-1").unwrap();
        let value: Value = serde_json::from_str(subscribe.trim_end()).unwrap();
        assert_eq!(value["method"], json!("proxy.stream.subscribe"));
    }

    #[test]
    fn daemon_proxy_rpc_decodes_success_and_error_responses() {
        assert_eq!(
            daemon_proxy_open_stream_id(r#"{"id":1,"ok":true,"result":{"stream_id":"s-42"}}"#, 1,)
                .unwrap(),
            "s-42"
        );
        assert_eq!(
            daemon_proxy_written_bytes(r#"{"id":2,"ok":true,"result":{"written":4}}"#, 2).unwrap(),
            4
        );
        assert!(daemon_rpc_success_result(
            r#"{"id":3,"ok":false,"error":{"code":"open_failed","message":"refused"}}"#,
            3,
        )
        .unwrap_err()
        .contains("open_failed: refused"));
        assert!(
            daemon_rpc_success_result(r#"{"id":4,"ok":true,"result":{}}"#, 99)
                .unwrap_err()
                .contains("id mismatch")
        );
    }

    #[test]
    fn daemon_proxy_rpc_decodes_stream_events_for_expected_stream() {
        assert_eq!(
            daemon_proxy_stream_event(
                r#"{"event":"proxy.stream.data","stream_id":"s-1","data_base64":"cG9uZw=="}"#,
                "s-1",
            )
            .unwrap(),
            Some(DaemonProxyStreamEvent::Data(b"pong".to_vec()))
        );
        assert_eq!(
            daemon_proxy_stream_event(
                r#"{"event":"proxy.stream.eof","stream_id":"s-1","data_base64":""}"#,
                "s-1",
            )
            .unwrap(),
            Some(DaemonProxyStreamEvent::Eof)
        );
        assert_eq!(
            daemon_proxy_stream_event(
                r#"{"event":"proxy.stream.error","stream_id":"s-1","error":"boom"}"#,
                "s-1",
            )
            .unwrap(),
            Some(DaemonProxyStreamEvent::Error("boom".to_string()))
        );
        assert_eq!(
            daemon_proxy_stream_event(
                r#"{"event":"proxy.stream.data","stream_id":"other","data_base64":"cG9uZw=="}"#,
                "s-1",
            )
            .unwrap(),
            None
        );
        assert_eq!(
            daemon_proxy_stream_event(r#"{"event":"pty.output","stream_id":"s-1"}"#, "s-1")
                .unwrap(),
            None
        );
    }

    #[test]
    fn daemon_proxy_connector_routes_requests_and_stream_events() {
        let (daemon_sender, daemon_receiver) = mpsc::channel::<Vec<u8>>();
        let writer = CapturedWriter::default();
        let client =
            DaemonProxyRpcClient::start(ChannelReader::new(daemon_receiver), writer.clone());
        let connector = DaemonProxyConnector::with_timeout(client, Duration::from_secs(2));
        let target = ProxyTarget {
            host: "example.com".to_string(),
            port: 443,
        };
        let (stream_sender, stream_receiver) = mpsc::channel();
        thread::spawn(move || {
            let stream = connector.open_stream(&target, ProxyHandshakeProtocol::Socks5);
            stream_sender.send(stream).unwrap();
        });

        let open = writer.wait_for_line(0);
        assert_eq!(request_method(&open), "proxy.open");
        send_daemon_frame(
            &daemon_sender,
            json!({
                "id": request_id(&open),
                "ok": true,
                "result": { "stream_id": "stream-1" },
            }),
        );

        let subscribe = writer.wait_for_line(1);
        assert_eq!(request_method(&subscribe), "proxy.stream.subscribe");
        send_daemon_frame(
            &daemon_sender,
            json!({
                "id": request_id(&subscribe),
                "ok": true,
                "result": {},
            }),
        );

        let stream = stream_receiver
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .unwrap();
        let (write_sender, write_receiver) = mpsc::channel();
        thread::spawn(move || {
            let mut stream = stream;
            let result = stream.write_all(b"ping");
            write_sender.send((stream, result)).unwrap();
        });
        let write = writer.wait_for_line(2);
        let write_value: Value = serde_json::from_str(write.trim_end()).unwrap();
        assert_eq!(write_value["method"], json!("proxy.write"));
        assert_eq!(write_value["params"]["stream_id"], json!("stream-1"));
        assert_eq!(write_value["params"]["data_base64"], json!("cGluZw=="));
        assert_eq!(write_value["params"]["timeout_ms"], json!(8_000));
        send_daemon_frame(
            &daemon_sender,
            json!({
                "id": request_id(&write),
                "ok": true,
                "result": { "written": 4 },
            }),
        );

        let (mut stream, write_result) =
            write_receiver.recv_timeout(Duration::from_secs(5)).unwrap();
        write_result.unwrap();

        send_daemon_frame(
            &daemon_sender,
            json!({
                "event": "proxy.stream.data",
                "stream_id": "stream-1",
                "data_base64": "cG9uZw==",
            }),
        );
        let mut payload = [0u8; 4];
        stream.read_exact(&mut payload).unwrap();
        assert_eq!(&payload, b"pong");

        let (close_sender, close_receiver) = mpsc::channel();
        thread::spawn(move || {
            stream.shutdown(Shutdown::Both).unwrap();
            close_sender.send(()).unwrap();
        });
        let close = writer.wait_for_line(3);
        assert_eq!(request_method(&close), "proxy.close");
        send_daemon_frame(
            &daemon_sender,
            json!({
                "id": request_id(&close),
                "ok": true,
                "result": {},
            }),
        );
        close_receiver.recv_timeout(Duration::from_secs(5)).unwrap();
    }

    #[test]
    fn daemon_proxy_stream_write_half_close_preserves_events_until_both_close() {
        let (daemon_sender, daemon_receiver) = mpsc::channel::<Vec<u8>>();
        let writer = CapturedWriter::default();
        let client =
            DaemonProxyRpcClient::start(ChannelReader::new(daemon_receiver), writer.clone());
        let connector = DaemonProxyConnector::with_timeout(client, Duration::from_secs(2));
        let target = ProxyTarget {
            host: "example.com".to_string(),
            port: 443,
        };
        let (stream_sender, stream_receiver) = mpsc::channel();
        thread::spawn(move || {
            stream_sender
                .send(connector.open_stream(&target, ProxyHandshakeProtocol::Socks5))
                .unwrap();
        });

        let open = writer.wait_for_line(0);
        send_daemon_frame(
            &daemon_sender,
            json!({
                "id": request_id(&open),
                "ok": true,
                "result": { "stream_id": "stream-half-close" },
            }),
        );
        let subscribe = writer.wait_for_line(1);
        send_daemon_frame(
            &daemon_sender,
            json!({
                "id": request_id(&subscribe),
                "ok": true,
                "result": {},
            }),
        );
        let mut stream = stream_receiver
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .unwrap();

        let write_half = stream.try_clone_box().unwrap();
        let (half_sender, half_receiver) = mpsc::channel();
        thread::spawn(move || {
            half_sender
                .send(write_half.shutdown(Shutdown::Write))
                .unwrap();
        });

        let unexpected_close = writer.wait_for_line_timeout(2, Duration::from_millis(250));
        if let Some(close) = &unexpected_close {
            send_daemon_frame(
                &daemon_sender,
                json!({
                    "id": request_id(close),
                    "ok": true,
                    "result": {},
                }),
            );
        }
        half_receiver
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .unwrap();
        assert!(
            unexpected_close.is_none(),
            "Shutdown::Write must not send proxy.close"
        );

        send_daemon_frame(
            &daemon_sender,
            json!({
                "event": "proxy.stream.data",
                "stream_id": "stream-half-close",
                "data_base64": "cG9uZw==",
            }),
        );
        let mut payload = [0u8; 4];
        stream.read_exact(&mut payload).unwrap();
        assert_eq!(&payload, b"pong");

        let (close_sender, close_receiver) = mpsc::channel();
        thread::spawn(move || {
            close_sender.send(stream.shutdown(Shutdown::Both)).unwrap();
        });
        let close = writer.wait_for_line(2);
        assert_eq!(request_method(&close), "proxy.close");
        send_daemon_frame(
            &daemon_sender,
            json!({
                "id": request_id(&close),
                "ok": true,
                "result": {},
            }),
        );
        close_receiver
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .unwrap();
        assert!(
            writer
                .wait_for_line_timeout(3, Duration::from_millis(250))
                .is_none(),
            "daemon stream must close at most once"
        );
    }

    #[test]
    fn daemon_proxy_stream_final_clone_drop_closes_and_unregisters_once() {
        let (daemon_sender, daemon_receiver) = mpsc::channel::<Vec<u8>>();
        let writer = CapturedWriter::default();
        let client =
            DaemonProxyRpcClient::start(ChannelReader::new(daemon_receiver), writer.clone());
        let stream = open_scripted_daemon_stream(
            Arc::clone(&client),
            &writer,
            &daemon_sender,
            0,
            "stream-drop",
        );
        let clone_one = stream.try_clone_box().unwrap();
        let clone_two = stream.try_clone_box().unwrap();
        let (drop_sender, drop_receiver) = mpsc::channel();
        thread::spawn(move || {
            drop((stream, clone_one, clone_two));
            drop_sender.send(()).unwrap();
        });

        let close = writer.wait_for_line_timeout(2, Duration::from_millis(500));
        if let Some(close) = &close {
            send_daemon_frame(
                &daemon_sender,
                json!({
                    "id": request_id(close),
                    "ok": true,
                    "result": {},
                }),
            );
        }
        drop_receiver.recv_timeout(Duration::from_secs(5)).unwrap();
        let close = close.expect("dropping the final stream clone must send proxy.close");
        assert_eq!(request_method(&close), "proxy.close");
        assert!(
            writer
                .wait_for_line_timeout(3, Duration::from_millis(250))
                .is_none(),
            "final clone drop must send exactly one proxy.close"
        );
        assert!(
            !client
                .stream_events
                .lock()
                .expect("daemon proxy event map mutex poisoned")
                .contains_key("stream-drop"),
            "final clone drop must unregister its stream subscription"
        );
    }

    #[test]
    fn daemon_proxy_stream_both_then_drop_does_not_close_or_unregister_twice() {
        let (daemon_sender, daemon_receiver) = mpsc::channel::<Vec<u8>>();
        let writer = CapturedWriter::default();
        let client =
            DaemonProxyRpcClient::start(ChannelReader::new(daemon_receiver), writer.clone());
        let stream = open_scripted_daemon_stream(
            Arc::clone(&client),
            &writer,
            &daemon_sender,
            0,
            "stream-both-drop",
        );
        let clone = stream.try_clone_box().unwrap();
        let (close_sender, close_receiver) = mpsc::channel();
        thread::spawn(move || {
            let result = stream.shutdown(Shutdown::Both);
            close_sender.send((stream, clone, result)).unwrap();
        });

        let close = writer.wait_for_line(2);
        assert_eq!(request_method(&close), "proxy.close");
        send_daemon_frame(
            &daemon_sender,
            json!({
                "id": request_id(&close),
                "ok": true,
                "result": {},
            }),
        );
        let (stream, clone, close_result) =
            close_receiver.recv_timeout(Duration::from_secs(5)).unwrap();
        close_result.unwrap();
        drop((stream, clone));

        assert!(
            writer
                .wait_for_line_timeout(3, Duration::from_millis(250))
                .is_none(),
            "drop after Shutdown::Both must not send another proxy.close"
        );
        assert!(
            !client
                .stream_events
                .lock()
                .expect("daemon proxy event map mutex poisoned")
                .contains_key("stream-both-drop"),
            "Shutdown::Both must leave the stream subscription unregistered"
        );
    }

    #[test]
    fn daemon_rpc_correlates_out_of_order_responses_around_stream_events() {
        let (daemon_sender, daemon_receiver) = mpsc::channel::<Vec<u8>>();
        let writer = CapturedWriter::default();
        let client =
            DaemonProxyRpcClient::start(ChannelReader::new(daemon_receiver), writer.clone());
        let (result_sender, result_receiver) = mpsc::channel();

        for label in ["first", "second"] {
            let client = Arc::clone(&client);
            let result_sender = result_sender.clone();
            thread::spawn(move || {
                let result = client.request_response(
                    |id| daemon_rpc_request_line(id, label, json!({})),
                    Duration::from_secs(2),
                );
                result_sender.send((label, result)).unwrap();
            });
        }
        drop(result_sender);

        let first_line = writer.wait_for_line(0);
        let second_line = writer.wait_for_line(1);
        send_daemon_frame(
            &daemon_sender,
            json!({
                "event": "proxy.stream.data",
                "stream_id": "unsubscribed",
                "data_base64": "aWdub3JlZA==",
            }),
        );
        send_daemon_frame(
            &daemon_sender,
            json!({
                "id": request_id(&second_line),
                "ok": true,
                "result": { "method": request_method(&second_line) },
            }),
        );
        send_daemon_frame(
            &daemon_sender,
            json!({
                "id": request_id(&first_line),
                "ok": true,
                "result": { "method": request_method(&first_line) },
            }),
        );

        let mut completed = Vec::new();
        for _ in 0..2 {
            let (label, result) = result_receiver
                .recv_timeout(Duration::from_secs(5))
                .unwrap();
            let (id, line) = result.unwrap();
            assert_eq!(id, request_id(&line));
            let value: Value = serde_json::from_str(line.trim_end()).unwrap();
            assert_eq!(value["result"]["method"], json!(label));
            completed.push(label);
        }
        completed.sort_unstable();
        assert_eq!(completed, vec!["first", "second"]);
    }
}
