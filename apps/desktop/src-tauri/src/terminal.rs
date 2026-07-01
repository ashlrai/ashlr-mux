//! MVP terminal bridge (windows-port vertical slice).
//!
//! Spawns a ConPTY-backed shell per session over the shared
//! [`cmux_terminal::conpty::ConPty`] boundary and pumps its raw output to the
//! webview as base64-encoded `cmux://terminal-output` events. The frontend
//! renders those bytes with xterm.js, sends keystrokes back through
//! `terminal_write`, and drives the explicit Windows resize (there is **no
//! SIGWINCH**) through `terminal_resize`.
//!
//! Bytes are base64-framed rather than sent as UTF-8 strings because a read
//! chunk can split a multi-byte sequence at an arbitrary boundary; the webview
//! decodes with `atob` and feeds the raw `Uint8Array` to `term.write`.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;

use cmux_terminal::conpty::{ConPty, ConPtyCommand, ConPtySize};
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

/// Event carrying a chunk of terminal output to the webview.
const TERMINAL_OUTPUT_EVENT: &str = "cmux://terminal-output";
/// Event signalling that a session's shell exited and the pump thread ended.
const TERMINAL_EXIT_EVENT: &str = "cmux://terminal-exit";

/// One live shell: its PTY (kept for resize/kill) plus the single input writer.
struct TerminalSession {
    pty: ConPty,
    writer: Box<dyn Write + Send>,
}

/// Managed Tauri state: the set of open terminal sessions keyed by id.
#[derive(Default)]
pub struct TerminalState {
    sessions: Mutex<HashMap<u32, TerminalSession>>,
    next_id: AtomicU32,
}

#[derive(Serialize, Clone)]
struct TerminalOutput {
    id: u32,
    /// Standard base64 (with padding) of the raw output bytes.
    data: String,
}

#[derive(Serialize, Clone)]
struct TerminalExit {
    id: u32,
}

/// The shell a fresh session launches. PowerShell is present on every supported
/// Windows install; the Unix arm keeps the crate buildable/testable off-Windows.
fn default_shell_command() -> ConPtyCommand {
    if cfg!(windows) {
        ConPtyCommand::new("powershell.exe").arg("-NoLogo")
    } else {
        ConPtyCommand::new("/bin/bash").arg("-l")
    }
}

/// Open a new shell session on a pseudo console of `cols`x`rows` and start a
/// dedicated thread pumping its output to the webview. Returns the session id.
#[tauri::command]
pub fn terminal_open(
    app: AppHandle,
    state: State<'_, TerminalState>,
    cols: Option<u16>,
    rows: Option<u16>,
) -> Result<u32, String> {
    let size = ConPtySize::new(cols.unwrap_or(80).max(1), rows.unwrap_or(24).max(1));
    let command = default_shell_command();

    let pty = ConPty::spawn(&command, size).map_err(|e| e.to_string())?;
    // Clone the reader before taking the writer; both are independent handles
    // onto the master side.
    let reader = pty.reader().map_err(|e| e.to_string())?;
    let writer = pty.take_writer().map_err(|e| e.to_string())?;

    let id = state.next_id.fetch_add(1, Ordering::Relaxed);

    let pump_app = app.clone();
    std::thread::Builder::new()
        .name(format!("cmux-terminal-pump-{id}"))
        .spawn(move || pump_reader(pump_app, id, reader))
        .map_err(|e| e.to_string())?;

    state
        .sessions
        .lock()
        .expect("terminal sessions mutex poisoned")
        .insert(id, TerminalSession { pty, writer });

    Ok(id)
}

/// Write keystrokes (xterm `onData`) into a session's shell.
#[tauri::command]
pub fn terminal_write(state: State<'_, TerminalState>, id: u32, data: String) -> Result<(), String> {
    let mut sessions = state
        .sessions
        .lock()
        .expect("terminal sessions mutex poisoned");
    let session = sessions
        .get_mut(&id)
        .ok_or_else(|| format!("unknown terminal session {id}"))?;
    session
        .writer
        .write_all(data.as_bytes())
        .map_err(|e| e.to_string())?;
    session.writer.flush().map_err(|e| e.to_string())?;
    Ok(())
}

/// Resize a session's pseudo console (the explicit Windows analogue of SIGWINCH).
#[tauri::command]
pub fn terminal_resize(
    state: State<'_, TerminalState>,
    id: u32,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    let mut sessions = state
        .sessions
        .lock()
        .expect("terminal sessions mutex poisoned");
    let session = sessions
        .get_mut(&id)
        .ok_or_else(|| format!("unknown terminal session {id}"))?;
    session
        .pty
        .resize(ConPtySize::new(cols.max(1), rows.max(1)))
        .map_err(|e| e.to_string())
}

/// Kill a session's shell and drop its PTY. The pump thread observes EOF on the
/// severed output pipe and exits on its own.
#[tauri::command]
pub fn terminal_close(state: State<'_, TerminalState>, id: u32) -> Result<(), String> {
    let removed = state
        .sessions
        .lock()
        .expect("terminal sessions mutex poisoned")
        .remove(&id);
    if let Some(mut session) = removed {
        let _ = session.pty.kill();
    }
    Ok(())
}

/// Read the child's output until EOF, emitting each chunk to the webview.
fn pump_reader(app: AppHandle, id: u32, mut reader: Box<dyn Read + Send>) {
    let mut buf = [0u8; 4096];
    loop {
        match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                let payload = TerminalOutput {
                    id,
                    data: base64_encode(&buf[..n]),
                };
                if app.emit(TERMINAL_OUTPUT_EVENT, payload).is_err() {
                    break;
                }
            }
            Err(_) => break,
        }
    }
    let _ = app.emit(TERMINAL_EXIT_EVENT, TerminalExit { id });
}

/// Standard base64 (RFC 4648, with `=` padding). Hand-rolled to keep the MVP
/// dependency-free; the webview inverse is `atob`.
fn base64_encode(input: &[u8]) -> String {
    const ALPHABET: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(ALPHABET[((n >> 18) & 63) as usize] as char);
        out.push(ALPHABET[((n >> 12) & 63) as usize] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[((n >> 6) & 63) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::base64_encode;

    #[test]
    fn base64_matches_rfc_test_vectors() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foob"), "Zm9vYg==");
        assert_eq!(base64_encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn base64_round_trips_all_byte_values() {
        // Encode every byte value and confirm padding/length invariants hold for
        // a chunk that is not a multiple of three.
        let bytes: Vec<u8> = (0u8..=255).collect();
        let encoded = base64_encode(&bytes);
        assert_eq!(encoded.len(), bytes.len().div_ceil(3) * 4);
        assert!(encoded.is_ascii());
        // 256 bytes -> 256 % 3 == 1 -> exactly two '=' pad chars at the end.
        assert!(encoded.ends_with("=="));
        assert_eq!(encoded.matches('=').count(), 2);
    }

    #[test]
    fn base64_encodes_high_bytes_without_panicking() {
        // Non-UTF-8 bytes must encode fine — this is exactly why the output
        // bridge is base64 rather than a UTF-8 string.
        assert_eq!(base64_encode(&[0xff, 0xfe, 0xfd]), "//79");
        assert_eq!(base64_encode(&[0x00]), "AA==");
    }
}
