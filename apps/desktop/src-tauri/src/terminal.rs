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

use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::{Read, Write};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use cmux_terminal::conpty::{ConPty, ConPtyCommand, ConPtySize};
use cmux_terminal::engine::{GridSize, TerminalGrid};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::session;

/// Event carrying a chunk of terminal output to the webview.
const TERMINAL_OUTPUT_EVENT: &str = "cmux://terminal-output";
/// Event signalling that a session's shell exited and the pump thread ended.
const TERMINAL_EXIT_EVENT: &str = "cmux://terminal-exit";

/// One live shell: its PTY (kept for resize/kill) plus the single input writer.
struct TerminalSession {
    pty: ConPty,
    writer: Box<dyn Write + Send>,
    grid: Arc<Mutex<TerminalGrid>>,
    panel_id: Option<String>,
    root_pid: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct TerminalRuntimeSnapshot {
    pub(crate) id: u32,
    pub(crate) panel_id: Option<String>,
    pub(crate) root_pid: Option<u32>,
    pub(crate) descendant_pids: Vec<u32>,
    pub(crate) child_pids: Vec<u32>,
    pub(crate) process_count: usize,
    pub(crate) foreground_pid: Option<u32>,
    pub(crate) foreground_process_name: Option<String>,
    pub(crate) foreground_process_source: String,
    pub(crate) process_error: Option<String>,
}

#[derive(Debug, Default)]
struct TerminalTitleParser {
    state: TerminalTitleParserState,
}

#[derive(Debug, Default)]
enum TerminalTitleParserState {
    #[default]
    Ground,
    Escape,
    Osc {
        body: Vec<u8>,
    },
    OscEscape {
        body: Vec<u8>,
    },
}

impl TerminalTitleParser {
    const MAX_OSC_TITLE_BYTES: usize = 4096;

    fn consume(&mut self, bytes: &[u8]) -> Vec<String> {
        let mut titles = Vec::new();
        for &byte in bytes {
            let state = std::mem::take(&mut self.state);
            self.state = match state {
                TerminalTitleParserState::Ground if byte == 0x1b => {
                    TerminalTitleParserState::Escape
                }
                TerminalTitleParserState::Ground => TerminalTitleParserState::Ground,
                TerminalTitleParserState::Escape if byte == b']' => {
                    TerminalTitleParserState::Osc { body: Vec::new() }
                }
                TerminalTitleParserState::Escape if byte == 0x1b => {
                    TerminalTitleParserState::Escape
                }
                TerminalTitleParserState::Escape => TerminalTitleParserState::Ground,
                TerminalTitleParserState::Osc { body } if byte == 0x07 => {
                    if let Some(title) = Self::title_from_body(&body) {
                        titles.push(title);
                    }
                    TerminalTitleParserState::Ground
                }
                TerminalTitleParserState::Osc { body } if byte == 0x1b => {
                    TerminalTitleParserState::OscEscape { body }
                }
                TerminalTitleParserState::Osc { mut body } => {
                    if body.len() < Self::MAX_OSC_TITLE_BYTES {
                        body.push(byte);
                        TerminalTitleParserState::Osc { body }
                    } else {
                        TerminalTitleParserState::Ground
                    }
                }
                TerminalTitleParserState::OscEscape { body } if byte == b'\\' => {
                    if let Some(title) = Self::title_from_body(&body) {
                        titles.push(title);
                    }
                    TerminalTitleParserState::Ground
                }
                TerminalTitleParserState::OscEscape { mut body } if byte == 0x07 => {
                    body.push(0x1b);
                    if let Some(title) = Self::title_from_body(&body) {
                        titles.push(title);
                    }
                    TerminalTitleParserState::Ground
                }
                TerminalTitleParserState::OscEscape { mut body } if byte == 0x1b => {
                    if body.len() < Self::MAX_OSC_TITLE_BYTES {
                        body.push(0x1b);
                    }
                    TerminalTitleParserState::OscEscape { body }
                }
                TerminalTitleParserState::OscEscape { mut body } => {
                    if body.len() + 1 < Self::MAX_OSC_TITLE_BYTES {
                        body.push(0x1b);
                        body.push(byte);
                        TerminalTitleParserState::Osc { body }
                    } else {
                        TerminalTitleParserState::Ground
                    }
                }
            };
        }
        titles
    }

    fn title_from_body(body: &[u8]) -> Option<String> {
        let separator = body.iter().position(|byte| *byte == b';')?;
        let kind = std::str::from_utf8(&body[..separator]).ok()?;
        if kind != "0" && kind != "2" {
            return None;
        }
        let title = String::from_utf8_lossy(&body[separator + 1..])
            .trim()
            .to_string();
        (!title.is_empty()).then_some(title)
    }
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

#[derive(Serialize, Clone)]
pub struct TerminalListeningPorts {
    pub(crate) id: u32,
    pub(crate) panel_id: Option<String>,
    pub(crate) ports: Vec<u16>,
}

/// The shell a fresh session launches. PowerShell is present on every supported
/// Windows install; the Unix arm keeps the crate buildable/testable off-Windows.
fn default_shell_command(
    cwd: Option<&str>,
    initial_command: Option<&str>,
    environment: Option<BTreeMap<String, String>>,
) -> ConPtyCommand {
    let command = if cfg!(windows) {
        let command = ConPtyCommand::new("powershell.exe").arg("-NoLogo");
        match initial_command.filter(|command| !command.trim().is_empty()) {
            Some(initial_command) => command.arg("-NoExit").arg("-Command").arg(initial_command),
            None => command,
        }
    } else {
        let command = ConPtyCommand::new("/bin/bash").arg("-l");
        match initial_command.filter(|command| !command.trim().is_empty()) {
            Some(initial_command) => command.arg("-c").arg(initial_command),
            None => command,
        }
    };
    let command = match cwd.filter(|path| !path.is_empty()) {
        Some(path) => command.cwd(path),
        None => command,
    };
    match environment {
        Some(environment) => environment
            .into_iter()
            .fold(command, |command, (key, value)| command.env(key, value)),
        None => command,
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
    panel_id: Option<String>,
    cwd: Option<String>,
    initial_command: Option<String>,
    initial_input: Option<String>,
    environment: Option<BTreeMap<String, String>>,
) -> Result<u32, String> {
    terminal_open_with_policy(
        &app,
        state.inner(),
        panel_id.as_deref(),
        cwd.as_deref(),
        initial_command.as_deref(),
        initial_input.as_deref(),
        environment,
        cols,
        rows,
        true,
    )
}

pub(crate) fn terminal_open_for_control(
    app: &AppHandle,
    state: &TerminalState,
    panel_id: Option<&str>,
    cwd: Option<&str>,
    initial_command: Option<&str>,
    initial_input: Option<&str>,
    environment: Option<BTreeMap<String, String>>,
    cols: Option<u16>,
    rows: Option<u16>,
) -> Result<u32, String> {
    terminal_open_with_policy(
        app,
        state,
        panel_id,
        cwd,
        initial_command,
        initial_input,
        environment,
        cols,
        rows,
        false,
    )
}

fn reusable_panel_session_id<'a>(
    mut sessions: impl Iterator<Item = (u32, Option<&'a str>)>,
    panel_id: Option<&str>,
    reuse_existing: bool,
) -> Option<u32> {
    let panel_id = reuse_existing.then_some(panel_id).flatten()?;
    sessions.find_map(|(id, candidate)| (candidate == Some(panel_id)).then_some(id))
}

#[allow(clippy::too_many_arguments)]
fn terminal_open_with_policy(
    app: &AppHandle,
    state: &TerminalState,
    panel_id: Option<&str>,
    cwd: Option<&str>,
    initial_command: Option<&str>,
    initial_input: Option<&str>,
    environment: Option<BTreeMap<String, String>>,
    cols: Option<u16>,
    rows: Option<u16>,
    reuse_existing: bool,
) -> Result<u32, String> {
    let panel_id = panel_id.and_then(|value| {
        let trimmed = value.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_string())
    });
    let mut sessions = state
        .sessions
        .lock()
        .expect("terminal sessions mutex poisoned");
    if let Some(existing) = reusable_panel_session_id(
        sessions
            .iter()
            .map(|(id, session)| (*id, session.panel_id.as_deref())),
        panel_id.as_deref(),
        reuse_existing,
    ) {
        return Ok(existing);
    }
    let size = ConPtySize::new(cols.unwrap_or(80).max(1), rows.unwrap_or(24).max(1));
    let command = default_shell_command(cwd, initial_command, environment);

    let pty = ConPty::spawn(&command, size).map_err(|e| e.to_string())?;
    // Clone the reader before taking the writer; both are independent handles
    // onto the master side.
    let reader = pty.reader().map_err(|e| e.to_string())?;
    let mut writer = pty.take_writer().map_err(|e| e.to_string())?;
    if let Some(input) = initial_input.filter(|input| !input.is_empty()) {
        writer
            .write_all(input.as_bytes())
            .map_err(|e| e.to_string())?;
        writer.flush().map_err(|e| e.to_string())?;
    }

    let id = state.next_id.fetch_add(1, Ordering::Relaxed);
    let root_pid = pty.process_id();
    let grid = Arc::new(Mutex::new(TerminalGrid::new(GridSize::new(
        size.cols as usize,
        size.rows as usize,
    ))));
    let pump_app = app.clone();
    let pump_panel_id = panel_id.clone();
    let pump_grid = grid.clone();
    std::thread::Builder::new()
        .name(format!("cmux-terminal-pump-{id}"))
        .spawn(move || pump_reader(pump_app, id, pump_panel_id, pump_grid, reader))
        .map_err(|e| e.to_string())?;

    sessions.insert(
        id,
        TerminalSession {
            pty,
            writer,
            grid,
            panel_id,
            root_pid,
        },
    );

    Ok(id)
}

pub(crate) fn terminal_close_panel_for_control(
    state: &TerminalState,
    panel_id: &str,
) -> Result<bool, String> {
    let removed = {
        let mut sessions = state
            .sessions
            .lock()
            .expect("terminal sessions mutex poisoned");
        let id = sessions.iter().find_map(|(id, session)| {
            (session.panel_id.as_deref() == Some(panel_id)).then_some(*id)
        });
        id.and_then(|id| sessions.remove(&id))
    };
    if let Some(mut session) = removed {
        session.pty.kill().map_err(|error| error.to_string())?;
        Ok(true)
    } else {
        Ok(false)
    }
}

pub(crate) fn terminal_close_id_for_control(
    state: &TerminalState,
    id: u32,
) -> Result<bool, String> {
    let removed = state
        .sessions
        .lock()
        .expect("terminal sessions mutex poisoned")
        .remove(&id);
    if let Some(mut session) = removed {
        session.pty.kill().map_err(|error| error.to_string())?;
        Ok(true)
    } else {
        Ok(false)
    }
}

pub(crate) fn terminal_close_panel_except_for_control(
    state: &TerminalState,
    panel_id: &str,
    keep_id: u32,
) -> Result<(), String> {
    let removed = {
        let mut sessions = state
            .sessions
            .lock()
            .expect("terminal sessions mutex poisoned");
        let ids = sessions
            .iter()
            .filter_map(|(id, session)| {
                (*id != keep_id && session.panel_id.as_deref() == Some(panel_id)).then_some(*id)
            })
            .collect::<Vec<_>>();
        ids.into_iter()
            .filter_map(|id| sessions.remove(&id))
            .collect::<Vec<_>>()
    };
    for mut session in removed {
        session.pty.kill().map_err(|error| error.to_string())?;
    }
    Ok(())
}

/// Write keystrokes (xterm `onData`) into a session's shell.
#[tauri::command]
pub fn terminal_write(
    state: State<'_, TerminalState>,
    id: u32,
    data: String,
) -> Result<(), String> {
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

pub(crate) fn terminal_write_panel(
    state: &TerminalState,
    panel_id: &str,
    data: &str,
) -> Result<(), String> {
    let normalized_panel_id = panel_id.trim();
    if normalized_panel_id.is_empty() {
        return Err("missing terminal panel id".to_string());
    }
    let mut sessions = state
        .sessions
        .lock()
        .expect("terminal sessions mutex poisoned");
    let session = sessions
        .values_mut()
        .find(|session| session.panel_id.as_deref() == Some(normalized_panel_id))
        .ok_or_else(|| format!("unknown terminal panel {normalized_panel_id}"))?;
    session
        .writer
        .write_all(data.as_bytes())
        .map_err(|e| e.to_string())?;
    session.writer.flush().map_err(|e| e.to_string())?;
    Ok(())
}

pub(crate) fn terminal_read_panel(
    state: &TerminalState,
    panel_id: &str,
    include_scrollback: bool,
    line_limit: Option<usize>,
) -> Result<String, String> {
    let normalized_panel_id = panel_id.trim();
    if normalized_panel_id.is_empty() {
        return Err("missing terminal panel id".to_string());
    }
    let grid = {
        let sessions = state
            .sessions
            .lock()
            .expect("terminal sessions mutex poisoned");
        sessions
            .values()
            .find(|session| session.panel_id.as_deref() == Some(normalized_panel_id))
            .map(|session| session.grid.clone())
            .ok_or_else(|| format!("unknown terminal panel {normalized_panel_id}"))?
    };
    let grid = grid
        .lock()
        .map_err(|_| "terminal grid mutex poisoned".to_string())?;
    Ok(terminal_text(&grid, include_scrollback, line_limit))
}

pub(crate) fn terminal_clear_history_panel(
    state: &TerminalState,
    panel_id: &str,
) -> Result<(), String> {
    let normalized_panel_id = panel_id.trim();
    if normalized_panel_id.is_empty() {
        return Err("missing terminal panel id".to_string());
    }
    let grid = {
        let sessions = state
            .sessions
            .lock()
            .expect("terminal sessions mutex poisoned");
        sessions
            .values()
            .find(|session| session.panel_id.as_deref() == Some(normalized_panel_id))
            .map(|session| session.grid.clone())
            .ok_or_else(|| format!("unknown terminal panel {normalized_panel_id}"))?
    };
    let mut grid = grid
        .lock()
        .map_err(|_| "terminal grid mutex poisoned".to_string())?;
    grid.clear_history();
    Ok(())
}

fn terminal_text(
    grid: &TerminalGrid,
    include_scrollback: bool,
    line_limit: Option<usize>,
) -> String {
    grid.text_lines(include_scrollback, line_limit).join("\n")
}

pub(crate) fn terminal_runtime_snapshots(state: &TerminalState) -> Vec<TerminalRuntimeSnapshot> {
    let sessions = {
        let sessions = state
            .sessions
            .lock()
            .expect("terminal sessions mutex poisoned");
        let mut sessions: Vec<_> = sessions
            .iter()
            .map(|(id, session)| (*id, session.panel_id.clone(), session.root_pid))
            .collect();
        sessions.sort_by_key(|(id, _, _)| *id);
        sessions
    };
    let process_entries = process_snapshot_entries();
    sessions
        .into_iter()
        .map(|(id, panel_id, root_pid)| {
            terminal_runtime_snapshot_from_processes(id, panel_id, root_pid, &process_entries)
        })
        .collect()
}

pub(crate) fn terminal_grid_size_for_panel(
    state: &TerminalState,
    panel_id: &str,
) -> Option<GridSize> {
    let grid = state
        .sessions
        .lock()
        .ok()?
        .values()
        .find(|session| session.panel_id.as_deref() == Some(panel_id))?
        .grid
        .clone();
    grid.lock().ok().map(|grid| grid.size())
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
        .map_err(|e| e.to_string())?;
    session
        .grid
        .lock()
        .map_err(|_| "terminal grid mutex poisoned".to_string())?
        .resize(GridSize::new(cols.max(1) as usize, rows.max(1) as usize));
    Ok(())
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

/// Scan the process tree rooted at a live terminal and update the owning
/// panel's session listening-port facts. On non-Windows builds this is a safe
/// no-op until a platform scanner is added.
#[tauri::command]
pub fn terminal_scan_listening_ports(
    app: AppHandle,
    terminal_state: State<'_, TerminalState>,
    session_state: State<'_, session::SessionState>,
    id: u32,
) -> Result<TerminalListeningPorts, String> {
    scan_terminal_listening_ports(&app, terminal_state.inner(), session_state.inner(), id)
}

pub(crate) fn scan_panel_listening_ports(
    app: &AppHandle,
    terminal_state: &TerminalState,
    session_state: &session::SessionState,
    panel_id: &str,
) -> Result<TerminalListeningPorts, String> {
    let normalized_panel_id = panel_id.trim();
    if normalized_panel_id.is_empty() {
        return Err("missing terminal panel id".to_string());
    }
    let id = {
        let sessions = terminal_state
            .sessions
            .lock()
            .expect("terminal sessions mutex poisoned");
        sessions
            .iter()
            .find_map(|(id, session)| {
                (session.panel_id.as_deref() == Some(normalized_panel_id)).then_some(*id)
            })
            .ok_or_else(|| format!("unknown terminal panel {normalized_panel_id}"))?
    };
    scan_terminal_listening_ports(app, terminal_state, session_state, id)
}

pub(crate) fn scan_terminal_listening_ports(
    app: &AppHandle,
    terminal_state: &TerminalState,
    session_state: &session::SessionState,
    id: u32,
) -> Result<TerminalListeningPorts, String> {
    let (panel_id, root_pid) = {
        let sessions = terminal_state
            .sessions
            .lock()
            .expect("terminal sessions mutex poisoned");
        let session = sessions
            .get(&id)
            .ok_or_else(|| format!("unknown terminal session {id}"))?;
        (session.panel_id.clone(), session.root_pid)
    };

    let ports = match root_pid {
        Some(root_pid) => scan_listening_ports_for_root_pid(root_pid)?,
        None => Vec::new(),
    };
    if let Some(panel_id) = panel_id.as_deref() {
        session::set_panel_listening_ports_for_panel(app, session_state, panel_id, &ports);
    }

    Ok(TerminalListeningPorts {
        id,
        panel_id,
        ports,
    })
}

/// Read the child's output until EOF, emitting each chunk to the webview.
fn pump_reader(
    app: AppHandle,
    id: u32,
    panel_id: Option<String>,
    grid: Arc<Mutex<TerminalGrid>>,
    mut reader: Box<dyn Read + Send>,
) {
    let mut buf = [0u8; 4096];
    let mut title_parser = TerminalTitleParser::default();
    loop {
        match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                if let Ok(mut grid) = grid.lock() {
                    grid.advance(&buf[..n]);
                }
                if let Some(panel_id) = panel_id.as_deref() {
                    for title in title_parser.consume(&buf[..n]) {
                        let state = app.state::<session::SessionState>();
                        session::set_process_title_for_panel(&app, state.inner(), panel_id, &title);
                    }
                }
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

fn descendant_pid_set(root_pid: u32, parent_pairs: &[(u32, u32)]) -> HashSet<u32> {
    let mut tree = HashSet::from([root_pid]);
    let mut queue = vec![root_pid];
    let mut cursor = 0;
    while cursor < queue.len() {
        let parent = queue[cursor];
        for &(pid, parent_pid) in parent_pairs {
            if parent_pid == parent && pid != 0 && pid != parent && tree.insert(pid) {
                queue.push(pid);
            }
        }
        cursor += 1;
    }
    tree
}

fn ports_for_pid_set(pids: &HashSet<u32>, pid_ports: &[(u32, u16)]) -> Vec<u16> {
    let mut ports: Vec<u16> = pid_ports
        .iter()
        .filter_map(|(pid, port)| pids.contains(pid).then_some(*port))
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    ports.sort_unstable();
    ports
}

fn tcp_port_from_owner_pid_row(raw_port: u32) -> Option<u16> {
    let port = u16::from_be((raw_port & 0xffff) as u16);
    (port != 0).then_some(port)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProcessSnapshotEntry {
    pid: u32,
    parent_pid: u32,
    name: Option<String>,
}

fn terminal_runtime_snapshot_from_processes(
    id: u32,
    panel_id: Option<String>,
    root_pid: Option<u32>,
    process_entries: &Result<Vec<ProcessSnapshotEntry>, String>,
) -> TerminalRuntimeSnapshot {
    let Some(root_pid) = root_pid else {
        return TerminalRuntimeSnapshot {
            id,
            panel_id,
            root_pid: None,
            descendant_pids: Vec::new(),
            child_pids: Vec::new(),
            process_count: 0,
            foreground_pid: None,
            foreground_process_name: None,
            foreground_process_source: "unavailable".to_string(),
            process_error: None,
        };
    };
    let Ok(entries) = process_entries else {
        return TerminalRuntimeSnapshot {
            id,
            panel_id,
            root_pid: Some(root_pid),
            descendant_pids: vec![root_pid],
            child_pids: Vec::new(),
            process_count: 1,
            foreground_pid: Some(root_pid),
            foreground_process_name: None,
            foreground_process_source: "root_process".to_string(),
            process_error: process_entries.as_ref().err().cloned(),
        };
    };
    let parent_pairs: Vec<_> = entries
        .iter()
        .map(|entry| (entry.pid, entry.parent_pid))
        .collect();
    let descendants = descendant_pid_set(root_pid, &parent_pairs);
    let mut descendant_pids: Vec<_> = descendants.iter().copied().collect();
    descendant_pids.sort_unstable();
    let mut child_pids: Vec<_> = entries
        .iter()
        .filter_map(|entry| (entry.parent_pid == root_pid).then_some(entry.pid))
        .filter(|pid| descendants.contains(pid))
        .collect();
    child_pids.sort_unstable();
    let foreground_pid = deepest_leaf_pid(root_pid, entries, &descendants).or(Some(root_pid));
    let foreground_process_name = foreground_pid.and_then(|pid| {
        entries
            .iter()
            .find(|entry| entry.pid == pid)
            .and_then(|entry| entry.name.clone())
    });
    let foreground_process_source = match foreground_pid {
        Some(pid) if pid != root_pid => "pid_tree_leaf_approximation",
        Some(_) => "root_process",
        None => "unavailable",
    };
    TerminalRuntimeSnapshot {
        id,
        panel_id,
        root_pid: Some(root_pid),
        descendant_pids,
        child_pids,
        process_count: descendants.len(),
        foreground_pid,
        foreground_process_name,
        foreground_process_source: foreground_process_source.to_string(),
        process_error: None,
    }
}

fn deepest_leaf_pid(
    root_pid: u32,
    entries: &[ProcessSnapshotEntry],
    descendants: &HashSet<u32>,
) -> Option<u32> {
    let mut children_by_parent: HashMap<u32, Vec<u32>> = HashMap::new();
    for entry in entries {
        if descendants.contains(&entry.pid) && descendants.contains(&entry.parent_pid) {
            children_by_parent
                .entry(entry.parent_pid)
                .or_default()
                .push(entry.pid);
        }
    }
    let mut best = (0usize, root_pid);
    let mut stack = vec![(root_pid, 0usize)];
    while let Some((pid, depth)) = stack.pop() {
        let children = children_by_parent.get(&pid).cloned().unwrap_or_default();
        if children.is_empty() && (depth > best.0 || (depth == best.0 && pid > best.1)) {
            best = (depth, pid);
        }
        for child in children {
            stack.push((child, depth + 1));
        }
    }
    Some(best.1)
}

#[cfg(windows)]
pub(crate) fn scan_listening_ports_for_root_pid(root_pid: u32) -> Result<Vec<u16>, String> {
    let pids = descendant_pid_set(root_pid, &process_parent_pairs()?);
    Ok(ports_for_pid_set(&pids, &tcp_listener_pid_ports()?))
}

#[cfg(not(windows))]
pub(crate) fn scan_listening_ports_for_root_pid(_root_pid: u32) -> Result<Vec<u16>, String> {
    Ok(Vec::new())
}

#[cfg(windows)]
fn process_parent_pairs() -> Result<Vec<(u32, u32)>, String> {
    Ok(process_snapshot_entries()?
        .into_iter()
        .map(|entry| (entry.pid, entry.parent_pid))
        .collect())
}

#[cfg(windows)]
fn process_snapshot_entries() -> Result<Vec<ProcessSnapshotEntry>, String> {
    use windows::Win32::{
        Foundation::CloseHandle,
        System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
            TH32CS_SNAPPROCESS,
        },
    };

    let mut entries = Vec::new();
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0)
            .map_err(|error| format!("CreateToolhelp32Snapshot: {error}"))?;
        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        if Process32FirstW(snapshot, &mut entry).is_ok() {
            loop {
                entries.push(ProcessSnapshotEntry {
                    pid: entry.th32ProcessID,
                    parent_pid: entry.th32ParentProcessID,
                    name: process_entry_name(&entry.szExeFile),
                });
                if Process32NextW(snapshot, &mut entry).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snapshot);
    }
    Ok(entries)
}

#[cfg(windows)]
fn process_entry_name(raw: &[u16]) -> Option<String> {
    let end = raw
        .iter()
        .position(|value| *value == 0)
        .unwrap_or(raw.len());
    let name = String::from_utf16_lossy(&raw[..end]).trim().to_string();
    (!name.is_empty()).then_some(name)
}

#[cfg(not(windows))]
fn process_snapshot_entries() -> Result<Vec<ProcessSnapshotEntry>, String> {
    Ok(Vec::new())
}

#[cfg(windows)]
fn tcp_listener_pid_ports() -> Result<Vec<(u32, u16)>, String> {
    let mut pid_ports = tcp4_listener_pid_ports()?;
    pid_ports.extend(tcp6_listener_pid_ports()?);
    Ok(pid_ports)
}

#[cfg(windows)]
fn tcp4_listener_pid_ports() -> Result<Vec<(u32, u16)>, String> {
    use std::ffi::c_void;

    use windows::Win32::{
        NetworkManagement::IpHelper::{
            GetExtendedTcpTable, MIB_TCPTABLE_OWNER_PID, TCP_TABLE_OWNER_PID_LISTENER,
        },
        Networking::WinSock::AF_INET,
    };

    let mut size = 0u32;
    unsafe {
        let _ = GetExtendedTcpTable(
            None,
            &mut size,
            false,
            AF_INET.0 as u32,
            TCP_TABLE_OWNER_PID_LISTENER,
            0,
        );
    }
    if size == 0 {
        return Ok(Vec::new());
    }

    let mut buffer = vec![0u8; size as usize];
    let status = unsafe {
        GetExtendedTcpTable(
            Some(buffer.as_mut_ptr().cast::<c_void>()),
            &mut size,
            false,
            AF_INET.0 as u32,
            TCP_TABLE_OWNER_PID_LISTENER,
            0,
        )
    };
    if status != 0 {
        return Err(format!("GetExtendedTcpTable failed with status {status}"));
    }

    let table = unsafe { &*(buffer.as_ptr() as *const MIB_TCPTABLE_OWNER_PID) };
    let rows =
        unsafe { std::slice::from_raw_parts(table.table.as_ptr(), table.dwNumEntries as usize) };
    Ok(rows
        .iter()
        .filter_map(|row| {
            tcp_port_from_owner_pid_row(row.dwLocalPort).map(|port| (row.dwOwningPid, port))
        })
        .collect())
}

#[cfg(windows)]
fn tcp6_listener_pid_ports() -> Result<Vec<(u32, u16)>, String> {
    use std::ffi::c_void;

    use windows::Win32::{
        NetworkManagement::IpHelper::{
            GetExtendedTcpTable, MIB_TCP6TABLE_OWNER_PID, TCP_TABLE_OWNER_PID_LISTENER,
        },
        Networking::WinSock::AF_INET6,
    };

    let mut size = 0u32;
    unsafe {
        let _ = GetExtendedTcpTable(
            None,
            &mut size,
            false,
            AF_INET6.0 as u32,
            TCP_TABLE_OWNER_PID_LISTENER,
            0,
        );
    }
    if size == 0 {
        return Ok(Vec::new());
    }

    let mut buffer = vec![0u8; size as usize];
    let status = unsafe {
        GetExtendedTcpTable(
            Some(buffer.as_mut_ptr().cast::<c_void>()),
            &mut size,
            false,
            AF_INET6.0 as u32,
            TCP_TABLE_OWNER_PID_LISTENER,
            0,
        )
    };
    if status != 0 {
        return Err(format!(
            "GetExtendedTcpTable IPv6 failed with status {status}"
        ));
    }

    let table = unsafe { &*(buffer.as_ptr() as *const MIB_TCP6TABLE_OWNER_PID) };
    let rows =
        unsafe { std::slice::from_raw_parts(table.table.as_ptr(), table.dwNumEntries as usize) };
    Ok(rows
        .iter()
        .filter_map(|row| {
            tcp_port_from_owner_pid_row(row.dwLocalPort).map(|port| (row.dwOwningPid, port))
        })
        .collect())
}

/// Standard base64 (RFC 4648, with `=` padding). Hand-rolled to keep the MVP
/// dependency-free; the webview inverse is `atob`.
fn base64_encode(input: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
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
    use std::collections::BTreeMap;

    use super::{
        base64_encode, default_shell_command, descendant_pid_set, ports_for_pid_set,
        reusable_panel_session_id, tcp_port_from_owner_pid_row,
        terminal_runtime_snapshot_from_processes, terminal_text, ProcessSnapshotEntry,
        TerminalTitleParser,
    };
    use cmux_terminal::engine::{GridSize, TerminalGrid};

    #[test]
    fn ui_attach_reuses_staged_panel_but_control_replace_reserves_a_new_session() {
        let staged = [(41_u32, Some("dock-surface")), (42, Some("other"))];
        assert_eq!(
            reusable_panel_session_id(staged.into_iter(), Some("dock-surface"), true),
            Some(41),
            "the UI open attaches to the one staged live terminal"
        );
        assert_eq!(
            reusable_panel_session_id(staged.into_iter(), Some("dock-surface"), false),
            None,
            "control staging, including TerminalReplace, must create a distinct session"
        );
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

    #[test]
    fn terminal_text_reads_viewport_scrollback_and_line_tail() {
        let mut grid = TerminalGrid::new(GridSize::new(20, 2));
        grid.advance(b"one\r\ntwo\r\nthree");

        assert_eq!(terminal_text(&grid, false, None), "two\nthree");
        assert_eq!(terminal_text(&grid, true, None), "one\ntwo\nthree");
        assert_eq!(terminal_text(&grid, true, Some(2)), "two\nthree");
    }

    #[test]
    fn default_shell_command_carries_a_working_directory_when_provided() {
        let command = default_shell_command(Some("C:/repo"), None, None);
        assert_eq!(
            command.cwd.as_deref(),
            Some(std::path::Path::new("C:/repo"))
        );
    }

    #[test]
    fn default_shell_command_ignores_an_empty_working_directory() {
        let command = default_shell_command(Some(""), None, None);
        assert_eq!(command.cwd, None);
    }

    #[test]
    fn default_shell_command_applies_startup_command_and_environment() {
        let command = default_shell_command(
            Some("C:/repo"),
            Some("echo ready"),
            Some(BTreeMap::from([("CMUX_FORK".to_string(), "1".to_string())])),
        );
        assert_eq!(
            command.cwd.as_deref(),
            Some(std::path::Path::new("C:/repo"))
        );
        assert_eq!(command.env.get("CMUX_FORK").map(String::as_str), Some("1"));
        if cfg!(windows) {
            assert_eq!(command.program, "powershell.exe");
            assert!(command.args.iter().any(|arg| arg == "-NoExit"));
            assert_eq!(command.args.last().map(String::as_str), Some("echo ready"));
        } else {
            assert_eq!(command.program, "/bin/bash");
            assert_eq!(command.args, vec!["-l", "-c", "echo ready"]);
        }
    }

    #[test]
    fn terminal_title_parser_extracts_bel_terminated_window_titles() {
        let mut parser = TerminalTitleParser::default();

        assert_eq!(
            parser.consume(b"\x1b]0;Claude Code loading\x07"),
            vec!["Claude Code loading"]
        );
        assert_eq!(
            parser.consume(b"\x1b]1;icon only\x07"),
            Vec::<String>::new()
        );
    }

    #[test]
    fn terminal_title_parser_extracts_st_terminated_titles_across_chunks() {
        let mut parser = TerminalTitleParser::default();

        assert!(parser.consume(b"prefix\x1b]2;cargo ").is_empty());
        assert_eq!(parser.consume(b"test\x1b\\suffix"), vec!["cargo test"]);
    }

    #[test]
    fn terminal_title_parser_ignores_shell_integration_osc_sequences() {
        let mut parser = TerminalTitleParser::default();

        assert_eq!(
            parser.consume(b"\x1b]133;A\x07\x1b]2;pwsh\x07"),
            vec!["pwsh"]
        );
    }

    #[test]
    fn descendant_pid_set_includes_root_and_nested_children() {
        let pids = descendant_pid_set(
            10,
            &[(10, 1), (11, 10), (12, 11), (20, 1), (21, 20), (13, 10)],
        );
        assert_eq!(pids, [10, 11, 12, 13].into_iter().collect());
    }

    #[test]
    fn terminal_runtime_snapshot_reports_process_tree_and_foreground_leaf() {
        let snapshot = terminal_runtime_snapshot_from_processes(
            7,
            Some("surface-1".to_string()),
            Some(10),
            &Ok(vec![
                ProcessSnapshotEntry {
                    pid: 10,
                    parent_pid: 1,
                    name: Some("powershell.exe".to_string()),
                },
                ProcessSnapshotEntry {
                    pid: 11,
                    parent_pid: 10,
                    name: Some("node.exe".to_string()),
                },
                ProcessSnapshotEntry {
                    pid: 12,
                    parent_pid: 11,
                    name: Some("vite.exe".to_string()),
                },
                ProcessSnapshotEntry {
                    pid: 20,
                    parent_pid: 1,
                    name: Some("other.exe".to_string()),
                },
            ]),
        );

        assert_eq!(snapshot.id, 7);
        assert_eq!(snapshot.panel_id.as_deref(), Some("surface-1"));
        assert_eq!(snapshot.root_pid, Some(10));
        assert_eq!(snapshot.descendant_pids, vec![10, 11, 12]);
        assert_eq!(snapshot.child_pids, vec![11]);
        assert_eq!(snapshot.process_count, 3);
        assert_eq!(snapshot.foreground_pid, Some(12));
        assert_eq!(
            snapshot.foreground_process_name.as_deref(),
            Some("vite.exe")
        );
        assert_eq!(
            snapshot.foreground_process_source,
            "pid_tree_leaf_approximation"
        );
        assert_eq!(snapshot.process_error, None);
    }

    #[test]
    fn terminal_runtime_snapshot_falls_back_to_root_on_process_scan_error() {
        let snapshot = terminal_runtime_snapshot_from_processes(
            3,
            Some("surface-2".to_string()),
            Some(99),
            &Err("snapshot failed".to_string()),
        );

        assert_eq!(snapshot.root_pid, Some(99));
        assert_eq!(snapshot.descendant_pids, vec![99]);
        assert_eq!(snapshot.foreground_pid, Some(99));
        assert_eq!(snapshot.foreground_process_source, "root_process");
        assert_eq!(snapshot.process_error.as_deref(), Some("snapshot failed"));
    }

    #[test]
    fn ports_for_pid_set_sorts_and_deduplicates_matching_listener_ports() {
        let pids = [10, 11].into_iter().collect();
        assert_eq!(
            ports_for_pid_set(&pids, &[(10, 5173), (20, 9000), (11, 3000), (10, 3000)]),
            vec![3000, 5173]
        );
    }

    #[test]
    fn tcp_port_from_owner_pid_row_decodes_network_byte_order() {
        assert_eq!(
            tcp_port_from_owner_pid_row(u16::to_be(5173) as u32),
            Some(5173)
        );
        assert_eq!(tcp_port_from_owner_pid_row(0), None);
    }
}
