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

use cmux_agent::{AgentExecutableResolver, AgentSessionLaunchPlan, AgentSessionProviderId};
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

/// Map a provider id string (`codex`/`claude`/`opencode`) to its enum.
fn parse_provider(agent_id: &str) -> Option<AgentSessionProviderId> {
    AgentSessionProviderId::ALL
        .into_iter()
        .find(|provider| provider.raw_value() == agent_id)
}

/// Build the interactive-launch command for a resolved agent plan.
///
/// The agent runs as its normal terminal TUI (no arguments) — deliberately
/// *not* `plan.arguments`, which are the headless stdio-transport flags a
/// terminal surface does not want. The resolver's rewritten `PATH` is carried
/// over so the agent's runtime (node/bun) resolves from the right place.
fn command_from_plan(plan: &AgentSessionLaunchPlan) -> ConPtyCommand {
    let mut command = ConPtyCommand::new(plan.executable_path.to_string_lossy().into_owned());
    if let Some(path) = plan.environment.get("PATH") {
        command = command.env("PATH", path);
    }
    command
}

/// Resolve an agent provider on this machine into an interactive-launch command.
fn resolve_agent_command(agent_id: &str) -> Result<ConPtyCommand, String> {
    let provider =
        parse_provider(agent_id).ok_or_else(|| format!("unknown agent provider {agent_id:?}"))?;
    let plan = AgentExecutableResolver::default()
        .resolve(provider)
        .map_err(|e| e.to_string())?;
    Ok(command_from_plan(&plan))
}

/// Open a new shell session on a pseudo console of `cols`x`rows` and start a
/// dedicated thread pumping its output to the webview. Returns the session id.
#[tauri::command]
pub fn terminal_open(
    app: AppHandle,
    state: State<'_, TerminalState>,
    cols: Option<u16>,
    rows: Option<u16>,
    agent: Option<String>,
    cwd: Option<String>,
) -> Result<u32, String> {
    let size = ConPtySize::new(cols.unwrap_or(80).max(1), rows.unwrap_or(24).max(1));

    let mut command = match agent.as_deref() {
        Some(agent_id) => resolve_agent_command(agent_id)?,
        None => default_shell_command(),
    };
    if let Some(dir) = cwd.as_deref().map(str::trim).filter(|dir| !dir.is_empty()) {
        command = command.cwd(dir);
    }

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
    use super::{base64_encode, command_from_plan, parse_provider};
    use cmux_agent::{AgentSessionLaunchPlan, AgentSessionProviderId};
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    #[test]
    fn parse_provider_maps_known_ids_and_rejects_others() {
        assert_eq!(parse_provider("codex"), Some(AgentSessionProviderId::Codex));
        assert_eq!(parse_provider("claude"), Some(AgentSessionProviderId::Claude));
        assert_eq!(
            parse_provider("opencode"),
            Some(AgentSessionProviderId::OpenCode)
        );
        assert_eq!(parse_provider("bogus"), None);
        // Case-sensitive: the JS layer sends the canonical lowercase id.
        assert_eq!(parse_provider("Claude"), None);
    }

    #[test]
    fn command_from_plan_launches_interactively_with_rewritten_path() {
        let plan = AgentSessionLaunchPlan {
            provider: AgentSessionProviderId::Claude,
            executable_path: PathBuf::from("C:\\rt\\claude.cmd"),
            // The headless transport args must NOT leak into the interactive TUI.
            arguments: AgentSessionProviderId::Claude.launch_arguments(),
            environment: BTreeMap::from([("PATH".into(), "C:\\rt;C:\\Windows".into())]),
        };

        let command = command_from_plan(&plan);

        assert_eq!(command.program, "C:\\rt\\claude.cmd");
        assert!(
            command.args.is_empty(),
            "interactive launch must drop plan.arguments (headless transport flags)"
        );
        assert_eq!(
            command.env.get("PATH").map(String::as_str),
            Some("C:\\rt;C:\\Windows")
        );
    }

    #[test]
    fn command_from_plan_without_path_sets_no_env_override() {
        let plan = AgentSessionLaunchPlan {
            provider: AgentSessionProviderId::Codex,
            executable_path: PathBuf::from("C:\\rt\\codex.exe"),
            arguments: Vec::new(),
            environment: BTreeMap::new(),
        };

        let command = command_from_plan(&plan);

        assert_eq!(command.program, "C:\\rt\\codex.exe");
        assert!(command.env.is_empty());
    }

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
