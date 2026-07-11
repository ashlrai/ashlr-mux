use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Shutdown, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use serde_json::{json, Value};
use tempfile::TempDir;
use url::form_urlencoded::byte_serialize;

const TOKEN: &str = "abcdefghijklmnop";
const PROTOCOL: &str =
    "wait-v2 remote-stream manifest-refresh react-app-v2 executable-bound branch-picker-v1";

struct Server {
    child: Child,
    port: u16,
}

impl Server {
    fn start(root: &Path, wait_timeout: Option<&str>) -> Self {
        let mut command = Command::new(env!("CARGO_BIN_EXE_cmux"));
        command
            .args(["diff-viewer-server", "--root"])
            .arg(root)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        if let Some(timeout) = wait_timeout {
            command.env("CMUX_DIFF_VIEWER_WAIT_TIMEOUT_SECONDS", timeout);
        }
        let mut child = command.spawn().expect("start diff viewer server");
        let mut stdout = BufReader::new(child.stdout.take().expect("server stdout"));
        let mut line = String::new();
        stdout.read_line(&mut line).expect("read server port");
        let port = line.trim().parse().expect("numeric server port");
        Self { child, port }
    }

    fn request(&self, request: &str) -> String {
        let mut stream = TcpStream::connect(("127.0.0.1", self.port)).expect("connect to server");
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        stream.write_all(request.as_bytes()).expect("write request");
        stream.shutdown(Shutdown::Write).unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).expect("read response");
        response
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn manifest(root: &Path, token: &str, files: Vec<Value>) {
    fs::write(
        root.join(format!(".manifest-{TOKEN}.json")),
        serde_json::to_vec(&json!({ "token": token, "files": files })).unwrap(),
    )
    .unwrap();
}

fn entry(request_path: &str, file_path: &Path, mime_type: &str) -> Value {
    json!({
        "request_path": request_path,
        "file_path": file_path.to_string_lossy(),
        "mime_type": mime_type,
    })
}

fn remote_entry(request_path: &str, remote_url: &str) -> Value {
    json!({
        "request_path": request_path,
        "file_path": "",
        "mime_type": "text/x-diff",
        "remote_url": remote_url,
    })
}

fn query_value(value: &str) -> String {
    byte_serialize(value.as_bytes()).collect()
}

fn body(response: &str) -> &str {
    response.split_once("\r\n\r\n").unwrap().1
}

#[test]
fn serves_health_manifest_files_head_and_method_contract() {
    let root = TempDir::new().unwrap();
    let page = root.path().join("index.html");
    let script = root.path().join("main.mjs");
    fs::write(&page, "<!doctype html><h1>diff</h1>").unwrap();
    fs::write(&script, "export const ready = true;\n").unwrap();
    manifest(
        root.path(),
        TOKEN,
        vec![
            entry("/index.html", &page, "text/html"),
            entry("/main.mjs", &script, "text/javascript"),
            remote_entry("/pull.patch", "https://github.com/owner/repo/pull/1.diff"),
        ],
    );

    let server = Server::start(root.path(), None);
    let health =
        server.request("GET /__cmux_diff_viewer_healthz HTTP/1.1\r\nHost: localhost\r\n\r\n");
    assert!(health.starts_with("HTTP/1.1 200 OK\r\n"));
    assert_eq!(body(&health), format!("ok {PROTOCOL}\n"));

    let file = server.request(&format!(
        "GET /{TOKEN}/index.html HTTP/1.1\r\nHost: localhost\r\n\r\n"
    ));
    assert!(file.starts_with("HTTP/1.1 200 OK\r\n"));
    assert!(file.contains("Content-Type: text/html; charset=utf-8\r\n"));
    assert!(file.contains("Cross-Origin-Resource-Policy: same-origin\r\n"));
    assert!(file.contains("Origin-Agent-Cluster: ?1\r\n"));
    assert!(file.contains("Referrer-Policy: no-referrer\r\n"));
    assert!(file.contains("X-Content-Type-Options: nosniff\r\n"));
    assert!(file.contains(&format!(
        "X-CMUX-Diff-Viewer-Origin: http://127.0.0.1:{}\r\n",
        server.port
    )));
    assert_eq!(body(&file), "<!doctype html><h1>diff</h1>");

    let head = server.request(&format!(
        "HEAD /{TOKEN}/main.mjs HTTP/1.1\r\nHost: localhost\r\n\r\n"
    ));
    assert!(head.starts_with("HTTP/1.1 200 OK\r\n"));
    assert!(head.contains("Content-Length: 27\r\n"));
    assert_eq!(body(&head), "");

    let remote_head = server.request(&format!(
        "HEAD /{TOKEN}/pull.patch HTTP/1.1\r\nHost: localhost\r\n\r\n"
    ));
    assert!(remote_head.starts_with("HTTP/1.1 200 OK\r\n"));
    assert!(remote_head.contains("X-CMUX-Diff-Viewer-Remote: github\r\n"));
    assert_eq!(body(&remote_head), "");

    let late = root.path().join("late.js");
    fs::write(&late, "window.late = true;\n").unwrap();
    manifest(
        root.path(),
        TOKEN,
        vec![entry("/late.js", &late, "text/javascript")],
    );
    let refreshed = server.request(&format!(
        "GET /{TOKEN}/late.js HTTP/1.1\r\nHost: localhost\r\n\r\n"
    ));
    assert!(refreshed.starts_with("HTTP/1.1 200 OK\r\n"));
    assert_eq!(body(&refreshed), "window.late = true;\n");

    let method = server.request("POST / HTTP/1.1\r\nHost: localhost\r\n\r\n");
    assert!(method.starts_with("HTTP/1.1 405 Method Not Allowed\r\n"));
    assert!(method.contains("Allow: GET, HEAD\r\n"));
    let missing = server.request("GET /unknown HTTP/1.1\r\nHost: localhost\r\n\r\n");
    assert!(missing.starts_with("HTTP/1.1 404 Not Found\r\n"));

    let state: Value =
        serde_json::from_slice(&fs::read(root.path().join(".server.json")).unwrap()).unwrap();
    assert_eq!(state["port"], server.port);
    assert_eq!(state["pid"], server.child.id());
    assert_eq!(state["protocolVersion"], PROTOCOL);
    assert_eq!(
        PathBuf::from(state["rootPath"].as_str().unwrap()),
        fs::canonicalize(root.path()).unwrap()
    );
    assert!(state["executablePath"].as_str().is_some());
}

#[test]
fn rejects_manifest_token_mismatch_and_files_outside_root() {
    let root = TempDir::new().unwrap();
    let outside = TempDir::new().unwrap();
    let page = outside.path().join("index.html");
    fs::write(&page, "outside").unwrap();
    manifest(
        root.path(),
        "ponmlkjihgfedcba",
        vec![entry("/index.html", &page, "text/html")],
    );
    let server = Server::start(root.path(), None);

    let mismatch = server.request(&format!(
        "GET /{TOKEN}/index.html HTTP/1.1\r\nHost: localhost\r\n\r\n"
    ));
    assert!(mismatch.starts_with("HTTP/1.1 500 Internal Server Error\r\n"));

    manifest(
        root.path(),
        TOKEN,
        vec![entry("/index.html", &page, "text/html")],
    );
    let escaped = server.request(&format!(
        "GET /{TOKEN}/index.html HTTP/1.1\r\nHost: localhost\r\n\r\n"
    ));
    assert!(escaped.starts_with("HTTP/1.1 500 Internal Server Error\r\n"));
}

#[test]
fn bounds_deferred_wait_requests_with_render_failure_page() {
    let root = TempDir::new().unwrap();
    let page = root.path().join("pending.html");
    fs::write(
        &page,
        "<!doctype html><body data-cmux-diff-pending=\"true\">pending</body>",
    )
    .unwrap();
    manifest(
        root.path(),
        TOKEN,
        vec![entry("/pending.html", &page, "text/html")],
    );
    let server = Server::start(root.path(), Some("0.05"));
    let response = server.request(&format!(
        "GET /__cmux_diff_viewer_wait/{TOKEN}/pending.html HTTP/1.1\r\nHost: localhost\r\n\r\n"
    ));
    assert!(response.starts_with("HTTP/1.1 504 Gateway Timeout\r\n"));
    assert!(body(&response).contains("Could not render this diff"));
    assert!(!body(&response).contains("data-cmux-diff-pending=\"true\""));
}

#[test]
fn deferred_wait_returns_the_replaced_page() {
    let root = TempDir::new().unwrap();
    let page = root.path().join("pending.html");
    fs::write(
        &page,
        "<!doctype html><body data-cmux-diff-pending=\"true\">pending</body>",
    )
    .unwrap();
    manifest(
        root.path(),
        TOKEN,
        vec![entry("/pending.html", &page, "text/html")],
    );
    let server = Server::start(root.path(), Some("2"));
    let replacement = page.clone();
    let writer = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(50));
        fs::write(replacement, "<!doctype html><h1>ready</h1>").unwrap();
    });
    let response = server.request(&format!(
        "GET /__cmux_diff_viewer_wait/{TOKEN}/pending.html HTTP/1.1\r\nHost: localhost\r\n\r\n"
    ));
    writer.join().unwrap();
    assert!(response.starts_with("HTTP/1.1 200 OK\r\n"));
    assert_eq!(body(&response), "<!doctype html><h1>ready</h1>");
}

#[test]
fn authenticated_refs_and_branch_routes_use_the_persisted_session() {
    let root = TempDir::new().unwrap();
    let repo = TempDir::new().unwrap();
    let git = |args: &[&str]| {
        let status = Command::new("git")
            .arg("-C")
            .arg(repo.path())
            .args(args)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?}");
    };
    git(&["init", "--quiet"]);
    git(&["config", "user.email", "parity@example.test"]);
    git(&["config", "user.name", "Parity Test"]);
    git(&["config", "core.autocrlf", "false"]);
    fs::write(repo.path().join("file.txt"), "base\n").unwrap();
    git(&["add", "file.txt"]);
    git(&["commit", "--quiet", "-m", "base"]);
    git(&["branch", "base"]);
    fs::write(repo.path().join("file.txt"), "changed\n").unwrap();
    git(&["commit", "--quiet", "-am", "change"]);

    fs::write(
        root.path().join(".branch-session-g.json"),
        serde_json::to_vec(&json!({
            "token": TOKEN,
            "groupID": "g",
            "repoRoot": repo.path().to_string_lossy(),
            "allowedRepoRoots": [repo.path().to_string_lossy()],
            "layout": "auto",
            "layoutSource": "default",
            "appearance": {},
            "repoSourceFiles": {},
        }))
        .unwrap(),
    )
    .unwrap();
    let server = Server::start(root.path(), None);
    let encoded_repo = query_value(&repo.path().to_string_lossy());

    let refs = server.request(&format!(
        "GET /__cmux_diff_viewer_refs?repo={encoded_repo}&token={TOKEN}&base=base HTTP/1.1\r\nHost: localhost\r\n\r\n"
    ));
    assert!(refs.starts_with("HTTP/1.1 200 OK\r\n"));
    assert!(body(&refs).contains("\"groups\""));
    assert!(body(&refs).contains("base"));

    let branch = server.request(&format!(
        "GET /__cmux_diff_viewer_branch?group=g&repo={encoded_repo}&base=base&token={TOKEN} HTTP/1.1\r\nHost: localhost\r\n\r\n"
    ));
    assert!(branch.starts_with("HTTP/1.1 302 Found\r\n"));
    assert!(branch.contains(&format!(
        "Location: http://127.0.0.1:{}/{TOKEN}/diff-g-branch.html\r\n",
        server.port
    )));
    assert!(root.path().join(TOKEN).join("diff-g-branch.html").is_file());
    let generated = server.request(&format!(
        "GET /{TOKEN}/diff-g-branch.html HTTP/1.1\r\nHost: localhost\r\n\r\n"
    ));
    assert!(generated.starts_with("HTTP/1.1 200 OK\r\n"));
}

#[test]
fn reports_exact_root_argument_errors() {
    for (args, expected) in [
        (
            vec!["diff-viewer-server"],
            "diff-viewer-server requires --root",
        ),
        (
            vec!["diff-viewer-server", "--root"],
            "diff-viewer-server --root requires a path",
        ),
        (
            vec!["diff-viewer-server", "wat"],
            "Unexpected diff-viewer-server argument: wat",
        ),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_cmux"))
            .args(args)
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert_eq!(
            String::from_utf8_lossy(&output.stderr).trim(),
            format!("Error: {expected}")
        );
    }
}
