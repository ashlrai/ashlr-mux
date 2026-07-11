//! Token-jailed loopback HTTP server for generated diff-viewer pages.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddrV4, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use cmux_diff::manifest::manifest_file_name;
use cmux_diff::DiffSessionRegistry;
use serde::Deserialize;
use serde_json::{json, Value};
use url::Url;

use crate::diff_viewer_cli::{
    run_diff_viewer_branch_command_with_root, run_diff_viewer_refs_command,
};
use crate::invocation::CliError;

const PROTOCOL_VERSION: &str =
    "wait-v2 remote-stream manifest-refresh react-app-v2 executable-bound branch-picker-v1";

pub struct DiffViewerServer {
    listener: TcpListener,
    root: Arc<PathBuf>,
    port: u16,
}

#[derive(Debug)]
struct Request {
    method: String,
    path: String,
    query: HashMap<String, String>,
}

#[derive(Debug, Clone, Deserialize)]
struct ManifestFile {
    request_path: String,
    file_path: String,
    mime_type: String,
    #[serde(default)]
    remote_url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Manifest {
    token: String,
    files: Vec<ManifestFile>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BranchSession {
    token: String,
    #[serde(rename = "groupID", alias = "groupId", alias = "group_id")]
    group_id: String,
    #[serde(alias = "allowed_repo_roots")]
    allowed_repo_roots: Vec<String>,
}

struct Response {
    status: u16,
    reason: &'static str,
    headers: BTreeMap<String, String>,
    body: ResponseBody,
}

enum ResponseBody {
    Bytes(Vec<u8>),
    File { path: PathBuf, len: u64 },
    Remote(String),
}

impl DiffViewerServer {
    pub fn prepare(args: &[String]) -> Result<Self, CliError> {
        let root = parse_root(args)?;
        validate_root(&root)?;
        let root = fs::canonicalize(root).map_err(|error| {
            CliError::new(format!("Failed to resolve diff viewer root: {error}"))
        })?;
        let listener =
            TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)).map_err(|error| {
                CliError::new(format!("Failed to bind diff viewer server socket: {error}"))
            })?;
        let port = listener
            .local_addr()
            .map_err(|error| {
                CliError::new(format!(
                    "Failed to inspect diff viewer server socket: {error}"
                ))
            })?
            .port();
        write_server_state(&root, port)?;
        Ok(Self {
            listener,
            root: Arc::new(root),
            port,
        })
    }

    #[must_use]
    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn run(self) -> Result<(), CliError> {
        for incoming in self.listener.incoming() {
            let stream = incoming.map_err(|error| {
                CliError::new(format!("Diff viewer server accept failed: {error}"))
            })?;
            let root = Arc::clone(&self.root);
            let port = self.port;
            thread::spawn(move || handle_connection(stream, port, &root));
        }
        Ok(())
    }
}

fn parse_root(args: &[String]) -> Result<PathBuf, CliError> {
    let mut root = None;
    let mut index = 0usize;
    while index < args.len() {
        if args[index] == "--root" {
            let Some(path) = args.get(index + 1) else {
                return Err(CliError::new("diff-viewer-server --root requires a path"));
            };
            root = Some(PathBuf::from(path));
            index += 2;
        } else {
            return Err(CliError::new(format!(
                "Unexpected diff-viewer-server argument: {}",
                args[index]
            )));
        }
    }
    root.ok_or_else(|| CliError::new("diff-viewer-server requires --root"))
}

fn validate_root(root: &Path) -> Result<(), CliError> {
    let metadata = fs::metadata(root).map_err(|_| {
        CliError::new(format!(
            "Unsafe diff viewer directory is not a directory: {}",
            root.display()
        ))
    })?;
    if !metadata.is_dir() {
        return Err(CliError::new(format!(
            "Unsafe diff viewer directory is not a directory: {}",
            root.display()
        )));
    }
    Ok(())
}

fn write_server_state(root: &Path, port: u16) -> Result<(), CliError> {
    let executable = std::env::current_exe()
        .ok()
        .and_then(|path| fs::canonicalize(path).ok())
        .map(|path| path.to_string_lossy().into_owned());
    let state = json!({
        "executablePath": executable,
        "pid": std::process::id(),
        "port": port,
        "protocolVersion": PROTOCOL_VERSION,
        "rootPath": root.to_string_lossy(),
    });
    fs::write(
        root.join(".server.json"),
        serde_json::to_vec_pretty(&state).unwrap_or_default(),
    )
    .map_err(|error| CliError::new(format!("Failed to write diff viewer server state: {error}")))
}

fn handle_connection(mut stream: TcpStream, port: u16, root: &Path) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(120)));
    match read_request(&mut stream).and_then(|request| route(request, port, root)) {
        Ok((response, head)) => {
            let _ = write_response(&mut stream, response, head);
        }
        Err(_) => {
            let _ = write_response(
                &mut stream,
                text_response(500, "Internal Server Error", "500 Internal Server Error\n"),
                false,
            );
        }
    }
}

fn read_request(stream: &mut TcpStream) -> Result<Request, CliError> {
    let mut bytes = Vec::new();
    let mut chunk = [0u8; 1024];
    while bytes.len() < 16 * 1024 && !bytes.windows(4).any(|part| part == b"\r\n\r\n") {
        let count = stream.read(&mut chunk).map_err(|error| {
            CliError::new(format!("Failed to read diff viewer request: {error}"))
        })?;
        if count == 0 {
            return Err(CliError::new("Invalid diff viewer request"));
        }
        bytes.extend_from_slice(&chunk[..count]);
    }
    let header =
        std::str::from_utf8(&bytes).map_err(|_| CliError::new("Invalid diff viewer request"))?;
    let line = header
        .split("\r\n")
        .next()
        .ok_or_else(|| CliError::new("Invalid diff viewer request"))?;
    let mut parts = line.split_whitespace();
    let method = parts
        .next()
        .ok_or_else(|| CliError::new("Invalid diff viewer request"))?
        .to_uppercase();
    let raw_target = parts
        .next()
        .ok_or_else(|| CliError::new("Invalid diff viewer request"))?;
    let (path, raw_query) =
        if raw_target.starts_with("http://") || raw_target.starts_with("https://") {
            let url = Url::parse(raw_target)
                .map_err(|_| CliError::new("Invalid diff viewer request target"))?;
            (
                url.path().to_string(),
                url.query().unwrap_or_default().to_string(),
            )
        } else {
            let (path, query) = raw_target.split_once('?').unwrap_or((raw_target, ""));
            (path.to_string(), query.to_string())
        };
    if !path.starts_with('/') {
        return Err(CliError::new("Invalid diff viewer request path"));
    }
    Ok(Request {
        method,
        path,
        query: parse_query(&raw_query),
    })
}

fn parse_query(query: &str) -> HashMap<String, String> {
    let mut values = HashMap::new();
    for pair in query.split('&').filter(|pair| !pair.is_empty()) {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        values
            .entry(percent_decode(key))
            .or_insert_with(|| percent_decode(value));
    }
    values
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let (Some(high), Some(low)) = (hex(bytes[index + 1]), hex(bytes[index + 2])) {
                output.push((high << 4) | low);
                index += 3;
                continue;
            }
        }
        output.push(if bytes[index] == b'+' {
            b' '
        } else {
            bytes[index]
        });
        index += 1;
    }
    String::from_utf8_lossy(&output).into_owned()
}

fn hex(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

fn route(request: Request, port: u16, root: &Path) -> Result<(Response, bool), CliError> {
    let head = request.method == "HEAD";
    if request.method != "GET" && !head {
        let mut response = text_response(405, "Method Not Allowed", "405 Method Not Allowed\n");
        response.headers.insert("Allow".into(), "GET, HEAD".into());
        return Ok((response, false));
    }
    if request.path == "/__cmux_diff_viewer_healthz" {
        return Ok((
            text_response(200, "OK", &format!("ok {PROTOCOL_VERSION}\n")),
            head,
        ));
    }
    if request.path == "/__cmux_diff_viewer_refs" {
        return Ok((refs_response(&request, root)?, head));
    }
    if request.path == "/__cmux_diff_viewer_branch" {
        return Ok((branch_response(&request, port, root)?, head));
    }
    if let Some(target) = request.path.strip_prefix("/__cmux_diff_viewer_wait/") {
        let target = format!("/{target}");
        let Some(file) = allowed_file(root, &target)? else {
            return Ok((not_found(), head));
        };
        if file.mime_type != "text/html" {
            return Ok((not_found(), head));
        }
        if !wait_for_replacement(&file) {
            return Ok((wait_timeout(), head));
        }
        return Ok((file_response(file, port), head));
    }
    let Some(file) = allowed_file(root, &request.path)? else {
        return Ok((not_found(), head));
    };
    Ok((file_response(file, port), head))
}

fn allowed_file(root: &Path, route_path: &str) -> Result<Option<ManifestFile>, CliError> {
    let Some(without_slash) = route_path.strip_prefix('/') else {
        return Ok(None);
    };
    let Some((token, request_tail)) = without_slash.split_once('/') else {
        return Ok(None);
    };
    let request_path = format!("/{request_tail}");
    if !DiffSessionRegistry::is_valid_token(token)
        || !DiffSessionRegistry::is_valid_request_path(&request_path)
    {
        return Ok(None);
    }
    let data = match fs::read(root.join(manifest_file_name(token))) {
        Ok(data) => data,
        Err(_) => return Ok(None),
    };
    let manifest: Manifest =
        serde_json::from_slice(&data).map_err(|_| CliError::new("Invalid diff viewer manifest"))?;
    if manifest.token != token || manifest.files.is_empty() || manifest.files.len() > 4096 {
        return Err(CliError::new("Invalid diff viewer manifest"));
    }
    let mut seen = HashSet::new();
    let mut selected = None;
    for mut file in manifest.files {
        if !seen.insert(file.request_path.clone())
            || !DiffSessionRegistry::is_valid_request_path(&file.request_path)
            || !DiffSessionRegistry::is_allowed_mime_type(&file.mime_type)
            || !DiffSessionRegistry::path_extension_matches_mime_type(
                &file.request_path,
                &file.mime_type,
            )
        {
            return Err(CliError::new("Invalid diff viewer manifest entry"));
        }
        if let Some(remote) = file.remote_url.as_deref() {
            if file.mime_type != "text/x-diff"
                || !file.file_path.is_empty()
                || !trusted_remote_patch(remote)
            {
                return Err(CliError::new("Invalid diff viewer remote manifest entry"));
            }
        } else {
            let path = fs::canonicalize(&file.file_path)
                .map_err(|_| CliError::new("Invalid diff viewer manifest file"))?;
            if path == root || !path.starts_with(root) || !path.is_file() {
                return Err(CliError::new(
                    "Diff viewer manifest file is outside the viewer directory",
                ));
            }
            file.file_path = path.to_string_lossy().into_owned();
        }
        if file.request_path == request_path {
            selected = Some(file);
        }
    }
    Ok(selected)
}

fn file_response(file: ManifestFile, port: u16) -> Response {
    let body = if let Some(remote) = file.remote_url.as_deref() {
        ResponseBody::Remote(remote.to_string())
    } else {
        let len = fs::metadata(&file.file_path)
            .map(|metadata| metadata.len())
            .unwrap_or(0);
        ResponseBody::File {
            path: PathBuf::from(&file.file_path),
            len,
        }
    };
    let mut response = Response {
        status: 200,
        reason: "OK",
        headers: BTreeMap::new(),
        body,
    };
    response
        .headers
        .insert("Content-Type".into(), content_type(&file.mime_type));
    response.headers.insert(
        "X-CMUX-Diff-Viewer-Origin".into(),
        format!("http://127.0.0.1:{port}"),
    );
    response
        .headers
        .insert("Origin-Agent-Cluster".into(), "?1".into());
    response
        .headers
        .insert("Referrer-Policy".into(), "no-referrer".into());
    if file.remote_url.is_some() {
        response
            .headers
            .insert("X-CMUX-Diff-Viewer-Remote".into(), "github".into());
    }
    response
}

fn trusted_remote_patch(raw: &str) -> bool {
    let Ok(url) = Url::parse(raw) else {
        return false;
    };
    if url.scheme() != "https"
        || url.host_str() != Some("github.com")
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return false;
    }
    let parts = url
        .path_segments()
        .map(|segments| segments.collect::<Vec<_>>())
        .unwrap_or_default();
    if parts.len() != 4
        || !safe_github_segment(parts[0])
        || !safe_github_segment(parts[1])
        || parts[2] != "pull"
    {
        return false;
    }
    let Some(number) = parts[3].strip_suffix(".diff") else {
        return false;
    };
    let Ok(number) = number.parse::<u64>() else {
        return false;
    };
    number > 0
        && raw
            == format!(
                "https://github.com/{}/{}/pull/{number}.diff",
                parts[0], parts[1]
            )
}

fn safe_github_segment(segment: &str) -> bool {
    !segment.is_empty()
        && segment
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn wait_for_replacement(file: &ManifestFile) -> bool {
    let timeout = std::env::var("CMUX_DIFF_VIEWER_WAIT_TIMEOUT_SECONDS")
        .ok()
        .and_then(|raw| raw.trim().parse::<f64>().ok())
        .filter(|value| value.is_finite())
        .unwrap_or(120.0)
        .clamp(0.05, 600.0);
    let deadline = Instant::now() + Duration::from_secs_f64(timeout);
    while file_is_pending(Path::new(&file.file_path)) {
        if Instant::now() >= deadline {
            return false;
        }
        thread::sleep(Duration::from_millis(10));
    }
    true
}

fn file_is_pending(path: &Path) -> bool {
    let mut head = vec![0u8; 8192];
    let Ok(mut file) = fs::File::open(path) else {
        return false;
    };
    let Ok(count) = file.read(&mut head) else {
        return false;
    };
    std::str::from_utf8(&head[..count])
        .is_ok_and(|text| text.contains("data-cmux-diff-pending=\"true\""))
}

fn wait_timeout() -> Response {
    let body = "<!doctype html><html><head><meta charset=\"utf-8\"><title>Loading diff...</title></head><body><main><h1>Loading diff...</h1><p>Could not render this diff. Check the patch input and try again.</p></main></body></html>";
    html_response(504, "Gateway Timeout", body)
}

fn refs_response(request: &Request, root: &Path) -> Result<Response, CliError> {
    let (Some(repo), Some(token)) = (request.query.get("repo"), request.query.get("token")) else {
        return Ok(not_found());
    };
    if !session_allows_repo(root, token, repo, None) {
        return Ok(not_found());
    }
    let mut args = vec![
        "--repo".into(),
        repo.clone(),
        "--token".into(),
        token.clone(),
    ];
    if let Some(base) = request.query.get("base") {
        args.extend(["--base".into(), base.clone()]);
    }
    let output = run_diff_viewer_refs_command(&args, Path::new(repo))?;
    Ok(json_response(200, "OK", output.into_bytes()))
}

fn branch_response(request: &Request, port: u16, root: &Path) -> Result<Response, CliError> {
    let (Some(group), Some(repo), Some(base), Some(token)) = (
        request.query.get("group"),
        request.query.get("repo"),
        request.query.get("base"),
        request.query.get("token"),
    ) else {
        return Ok(not_found());
    };
    if !valid_group(group) || !session_allows_repo(root, token, repo, Some(group)) {
        return Ok(not_found());
    }
    let args = vec![
        "--repo".into(),
        repo.clone(),
        "--token".into(),
        token.clone(),
        "--group".into(),
        group.clone(),
        "--base".into(),
        base.clone(),
    ];
    let output = match run_diff_viewer_branch_command_with_root(&args, Path::new(repo), root) {
        Ok(output) => output,
        Err(_) => return Ok(not_found()),
    };
    let value: Value =
        serde_json::from_str(&output).map_err(|_| CliError::new("invalid branch output"))?;
    let Some(path) = value.get("request_path").and_then(Value::as_str) else {
        return Ok(not_found());
    };
    let mut response = text_response(302, "Found", "302 Found\n");
    response.headers.insert(
        "Location".into(),
        format!("http://127.0.0.1:{port}/{token}{path}"),
    );
    Ok(response)
}

fn session_allows_repo(root: &Path, token: &str, repo: &str, group: Option<&str>) -> bool {
    if !DiffSessionRegistry::is_valid_token(token) {
        return false;
    }
    let Ok(repo) = fs::canonicalize(repo) else {
        return false;
    };
    let sessions = if let Some(group) = group {
        vec![root.join(format!(".branch-session-{group}.json"))]
    } else {
        match fs::read_dir(root) {
            Ok(entries) => entries
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| {
                    path.file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| {
                            name.starts_with(".branch-session-") && name.ends_with(".json")
                        })
                })
                .collect(),
            Err(_) => return false,
        }
    };
    sessions.into_iter().any(|path| {
        fs::read(path)
            .ok()
            .and_then(|data| serde_json::from_slice::<BranchSession>(&data).ok())
            .is_some_and(|session| {
                session.token == token
                    && group.is_none_or(|group| session.group_id == group)
                    && session
                        .allowed_repo_roots
                        .iter()
                        .filter_map(|allowed| fs::canonicalize(allowed).ok())
                        .any(|allowed| allowed == repo)
            })
    })
}

fn valid_group(group: &str) -> bool {
    (1..=64).contains(&group.len())
        && group
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

fn content_type(mime: &str) -> String {
    if mime.starts_with("text/") {
        format!("{mime}; charset=utf-8")
    } else {
        mime.to_string()
    }
}

fn not_found() -> Response {
    text_response(404, "Not Found", "404 Not Found\n")
}

fn text_response(status: u16, reason: &'static str, body: &str) -> Response {
    let mut response = Response {
        status,
        reason,
        headers: BTreeMap::new(),
        body: ResponseBody::Bytes(body.as_bytes().to_vec()),
    };
    response
        .headers
        .insert("Content-Type".into(), "text/plain; charset=utf-8".into());
    response
}

fn html_response(status: u16, reason: &'static str, body: &str) -> Response {
    let mut response = text_response(status, reason, body);
    response
        .headers
        .insert("Content-Type".into(), "text/html; charset=utf-8".into());
    response
}

fn json_response(status: u16, reason: &'static str, body: Vec<u8>) -> Response {
    let mut response = Response {
        status,
        reason,
        headers: BTreeMap::new(),
        body: ResponseBody::Bytes(body),
    };
    response.headers.insert(
        "Content-Type".into(),
        "application/json; charset=utf-8".into(),
    );
    response
}

fn write_response(
    stream: &mut TcpStream,
    mut response: Response,
    head: bool,
) -> std::io::Result<()> {
    let remote = match &response.body {
        ResponseBody::Remote(remote) => Some(remote.clone()),
        _ => None,
    };
    if let Some(remote) = remote {
        if head {
            return write_response_header(stream, &mut response, None);
        }
        return write_remote_response(stream, response, remote);
    }

    let content_length = match &response.body {
        ResponseBody::Bytes(body) => body.len() as u64,
        ResponseBody::File { len, .. } => *len,
        ResponseBody::Remote(_) => unreachable!(),
    };
    write_response_header(stream, &mut response, Some(content_length))?;
    if head {
        return stream.flush();
    }
    match response.body {
        ResponseBody::Bytes(body) => stream.write_all(&body)?,
        ResponseBody::File { path, .. } => {
            let mut file = fs::File::open(path)?;
            let mut chunk = [0u8; 64 * 1024];
            loop {
                let count = file.read(&mut chunk)?;
                if count == 0 {
                    break;
                }
                stream.write_all(&chunk[..count])?;
            }
        }
        ResponseBody::Remote(_) => unreachable!(),
    }
    stream.flush()
}

fn write_remote_response(
    stream: &mut TcpStream,
    mut response: Response,
    remote: String,
) -> std::io::Result<()> {
    let mut child = match Command::new(curl_executable())
        .args([
            "-fL",
            "--silent",
            "--show-error",
            "--max-time",
            "120",
            &remote,
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(child) => child,
        Err(_) => {
            return write_response(
                stream,
                text_response(502, "Bad Gateway", "502 Bad Gateway\n"),
                false,
            );
        }
    };
    let Some(mut stdout) = child.stdout.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return write_response(
            stream,
            text_response(502, "Bad Gateway", "502 Bad Gateway\n"),
            false,
        );
    };
    let result = (|| {
        let mut chunk = vec![0u8; 64 * 1024];
        let first_count = stdout.read(&mut chunk)?;
        if first_count == 0 {
            let status = child.wait()?;
            if !status.success() {
                return write_response(
                    stream,
                    text_response(502, "Bad Gateway", "502 Bad Gateway\n"),
                    false,
                );
            }
            write_response_header(stream, &mut response, None)?;
            return stream.flush();
        }

        write_response_header(stream, &mut response, None)?;
        stream.write_all(&chunk[..first_count])?;
        loop {
            let count = stdout.read(&mut chunk)?;
            if count == 0 {
                break;
            }
            stream.write_all(&chunk[..count])?;
        }
        stream.flush()
    })();
    if result.is_err() {
        let _ = child.kill();
    }
    let _ = child.wait();
    result
}

#[cfg(windows)]
fn curl_executable() -> &'static str {
    "curl.exe"
}

#[cfg(not(windows))]
fn curl_executable() -> &'static str {
    "curl"
}

fn write_response_header(
    stream: &mut TcpStream,
    response: &mut Response,
    content_length: Option<u64>,
) -> std::io::Result<()> {
    response
        .headers
        .insert("Cache-Control".into(), "no-store".into());
    response.headers.insert("Connection".into(), "close".into());
    response
        .headers
        .insert("Cross-Origin-Resource-Policy".into(), "same-origin".into());
    response
        .headers
        .insert("X-Content-Type-Options".into(), "nosniff".into());
    if let Some(content_length) = content_length {
        response
            .headers
            .insert("Content-Length".into(), content_length.to_string());
    }
    write!(
        stream,
        "HTTP/1.1 {} {}\r\n",
        response.status, response.reason
    )?;
    for (key, value) in &response.headers {
        write!(stream, "{key}: {value}\r\n")?;
    }
    stream.write_all(b"\r\n")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_string()).collect()
    }

    #[test]
    fn root_parser_and_query_decoder_match_the_cli_contract() {
        assert_eq!(
            parse_root(&[]).unwrap_err().message,
            "diff-viewer-server requires --root"
        );
        assert_eq!(
            parse_root(&strings(&["--root"])).unwrap_err().message,
            "diff-viewer-server --root requires a path"
        );
        assert_eq!(
            parse_root(&strings(&["wat"])).unwrap_err().message,
            "Unexpected diff-viewer-server argument: wat"
        );
        assert_eq!(percent_decode("C%3A%5Crepo+one"), "C:\\repo one");
        assert_eq!(parse_query("repo=first&%72epo=second")["repo"], "first");
    }

    #[test]
    fn remote_patch_allowlist_is_exact() {
        assert!(trusted_remote_patch(
            "https://github.com/owner/repo/pull/123.diff"
        ));
        assert!(!trusted_remote_patch(
            "https://github.com/a/b/commit/x.diff"
        ));
        assert!(!trusted_remote_patch(
            "https://github.com/owner/repo/pull/123.diff?token=x"
        ));
        assert!(!trusted_remote_patch(
            "https://example.com/owner/repo/pull/123.diff"
        ));
        assert!(!trusted_remote_patch(
            "https://github.com/owner/repo/pull/0.diff"
        ));
        assert!(!trusted_remote_patch(
            "https://github.com/owner%2Frepo/name/pull/1.diff"
        ));
        assert!(!trusted_remote_patch(
            "https://GitHub.com/owner/repo/pull/1.diff"
        ));
    }
}
