use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::sync::{Arc, Mutex};

use cmux_terminal::engine::TerminalGrid;
use tauri::{AppHandle, Emitter, Manager};

use super::{
    base64_encode, session, TerminalExit, TerminalListeningPorts, TerminalOutput,
    TerminalRuntimeSnapshot, TerminalState, TerminalTitleParser, TERMINAL_EXIT_EVENT,
    TERMINAL_OUTPUT_EVENT,
};

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
        let registry = terminal_state.runtime_registry();
        if registry.reserved_panel_ids.contains(normalized_panel_id) {
            return Err(format!("terminal panel {normalized_panel_id} is reserved"));
        }
        registry
            .sessions
            .iter()
            .find_map(|(id, session)| {
                (session.panel_id.as_deref() == Some(normalized_panel_id)).then_some(*id)
            })
            .ok_or_else(|| format!("unknown terminal panel {normalized_panel_id}"))?
    };
    scan_terminal_listening_ports(app, terminal_state, session_state, id)
}

pub(super) fn scan_terminal_listening_ports(
    app: &AppHandle,
    terminal_state: &TerminalState,
    session_state: &session::SessionState,
    id: u32,
) -> Result<TerminalListeningPorts, String> {
    let (panel_id, root_pid) = {
        let registry = terminal_state.runtime_registry();
        if registry.reserved_session_ids.contains(&id) {
            return Err(format!("terminal session {id} is reserved"));
        }
        let session = registry
            .sessions
            .get(&id)
            .ok_or_else(|| format!("unknown terminal session {id}"))?;
        (session.panel_id.clone(), session.root_pid)
    };

    let ports = match root_pid {
        Some(root_pid) => scan_listening_ports_for_root_pid(root_pid)?,
        None => Vec::new(),
    };
    if let Some(panel_id) = panel_id.as_deref() {
        session::set_panel_listening_ports_for_panel(app, session_state, panel_id, &ports)?;
    }

    Ok(TerminalListeningPorts {
        id,
        panel_id,
        ports,
    })
}

pub(super) fn emit_terminal_output_chunk(
    app: &AppHandle,
    id: u32,
    panel_id: Option<&str>,
    bytes: &[u8],
    titles: &[String],
) -> Result<(), String> {
    if let Some(panel_id) = panel_id {
        for title in titles {
            let state = app.state::<session::SessionState>();
            match session::set_process_title_for_panel(app, state.inner(), panel_id, title) {
                Ok(_) => {}
                Err(error) => {
                    eprintln!("[terminal] failed to persist process title: {error}");
                }
            }
        }
    }
    app.emit(
        TERMINAL_OUTPUT_EVENT,
        TerminalOutput {
            id,
            data: base64_encode(bytes),
        },
    )
    .map_err(|error| error.to_string())
}

/// Read the child's output until EOF, emitting each chunk to the webview.
pub(super) fn pump_reader(
    app: AppHandle,
    id: u32,
    panel_id: Option<String>,
    grid: Arc<Mutex<TerminalGrid>>,
    title_parser: Arc<Mutex<TerminalTitleParser>>,
    mut reader: Box<dyn Read + Send>,
) {
    let mut buf = [0u8; 4096];
    loop {
        match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                if let Ok(mut grid) = grid.lock() {
                    grid.advance(&buf[..n]);
                }
                let titles = match title_parser.lock() {
                    Ok(mut parser) => parser.consume(&buf[..n]),
                    Err(_) => break,
                };
                if emit_terminal_output_chunk(&app, id, panel_id.as_deref(), &buf[..n], &titles)
                    .is_err()
                {
                    break;
                }
            }
            Err(_) => break,
        }
    }
    let _ = app.emit(TERMINAL_EXIT_EVENT, TerminalExit { id });
}

pub(super) fn descendant_pid_set(root_pid: u32, parent_pairs: &[(u32, u32)]) -> HashSet<u32> {
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

pub(super) fn ports_for_pid_set(pids: &HashSet<u32>, pid_ports: &[(u32, u16)]) -> Vec<u16> {
    let mut ports: Vec<u16> = pid_ports
        .iter()
        .filter_map(|(pid, port)| pids.contains(pid).then_some(*port))
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    ports.sort_unstable();
    ports
}

pub(super) fn tcp_port_from_owner_pid_row(raw_port: u32) -> Option<u16> {
    let port = u16::from_be((raw_port & 0xffff) as u16);
    (port != 0).then_some(port)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ProcessSnapshotEntry {
    pub(super) pid: u32,
    pub(super) parent_pid: u32,
    pub(super) name: Option<String>,
}

pub(super) fn terminal_runtime_snapshot_from_processes(
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
pub(super) fn process_snapshot_entries() -> Result<Vec<ProcessSnapshotEntry>, String> {
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
pub(super) fn process_snapshot_entries() -> Result<Vec<ProcessSnapshotEntry>, String> {
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
