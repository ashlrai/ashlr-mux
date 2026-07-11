use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::{Command, Stdio};
use std::thread;

use serde_json::{json, Value};
use tempfile::TempDir;
use tokio_tungstenite::tungstenite::{accept, Message};

#[test]
fn executable_bridges_binary_terminal_io_and_consumes_config() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut socket = accept(stream).unwrap();
        let Message::Text(auth) = socket.read().unwrap() else {
            panic!("expected auth text frame");
        };
        let auth: Value = serde_json::from_str(&auth).unwrap();
        assert_eq!(auth["type"], "auth");
        assert_eq!(auth["token"], "secret");
        assert_eq!(auth["session_id"], "session-1");
        assert_eq!(auth["attachment_id"], "attachment-1");
        assert!(auth["cols"].as_u64().is_some_and(|value| value > 0));
        assert!(auth["rows"].as_u64().is_some_and(|value| value > 0));
        socket
            .send(Message::Text(r#"{"type":"ready"}"#.into()))
            .unwrap();
        socket
            .send(Message::Binary(b"welcome:".to_vec().into()))
            .unwrap();
        loop {
            match socket.read().unwrap() {
                Message::Binary(data) => {
                    assert_eq!(&data[..], b"input");
                    socket
                        .send(Message::Binary(b"echo".to_vec().into()))
                        .unwrap();
                    socket.close(None).unwrap();
                    return;
                }
                Message::Ping(data) => socket.send(Message::Pong(data)).unwrap(),
                _ => {}
            }
        }
    });

    let temp = TempDir::new().unwrap();
    let config = temp.path().join("vm.json");
    fs::write(
        &config,
        serde_json::to_vec(&json!({
            "url": format!("ws://127.0.0.1:{port}"),
            "headers": {"X-Test": "yes"},
            "token": "secret",
            "sessionId": "session-1",
            "attachmentId": "attachment-1",
        }))
        .unwrap(),
    )
    .unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_cmux"))
        .args([
            "vm-pty-connect",
            "--config",
            &config.to_string_lossy(),
            "--id",
            "vm-1",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    stdin.write_all(b"input").unwrap();
    stdin.flush().unwrap();
    let status = child.wait().unwrap();
    drop(stdin);
    let mut stdout = Vec::new();
    child
        .stdout
        .take()
        .unwrap()
        .read_to_end(&mut stdout)
        .unwrap();
    let mut stderr = String::new();
    child
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut stderr)
        .unwrap();
    server.join().unwrap();
    assert!(status.success(), "{stderr}");
    assert_eq!(stdout, b"welcome:echo");
    assert!(!config.exists());
}
