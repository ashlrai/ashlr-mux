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

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::io::{Read, Write};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{mpsc, Arc, Condvar, Mutex, MutexGuard, Weak};
use std::time::{Duration, Instant};

use cmux_terminal::conpty::{ConPty, ConPtyCommand, ConPtySize};
use cmux_terminal::engine::{GridSize, TerminalGrid};
use serde::Serialize;
use tauri::{AppHandle, State};

use crate::session;

mod process_runtime;

#[cfg(test)]
use process_runtime::{
    descendant_pid_set, ports_for_pid_set, tcp_port_from_owner_pid_row, ProcessSnapshotEntry,
};
use process_runtime::{
    emit_terminal_output_chunk, process_snapshot_entries, pump_reader,
    terminal_runtime_snapshot_from_processes,
};
pub(crate) use process_runtime::{scan_listening_ports_for_root_pid, scan_panel_listening_ports};

/// Event carrying a chunk of terminal output to the webview.
const TERMINAL_OUTPUT_EVENT: &str = "cmux://terminal-output";
/// Event signalling that a session's shell exited and the pump thread ended.
const TERMINAL_EXIT_EVENT: &str = "cmux://terminal-exit";
const TERMINAL_PENDING_INPUT_LIMIT: usize = 1024 * 1024;

trait TerminalProcess: Send {
    fn kill(&mut self) -> Result<(), String>;
    fn resize(&mut self, size: ConPtySize) -> Result<(), String>;
    fn try_wait(&mut self) -> Result<Option<u32>, String>;
}

impl TerminalProcess for ConPty {
    fn kill(&mut self) -> Result<(), String> {
        ConPty::kill(self).map_err(|error| error.to_string())
    }

    fn resize(&mut self, size: ConPtySize) -> Result<(), String> {
        ConPty::resize(self, size).map_err(|error| error.to_string())
    }

    fn try_wait(&mut self) -> Result<Option<u32>, String> {
        ConPty::try_wait(self).map_err(|error| error.to_string())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TerminalInputOutcome {
    Sent,
    Queued,
    InputQueueFull,
    SurfaceUnavailable,
    ProcessExited,
}

struct TerminalInputTransport {
    writer: Mutex<Box<dyn Write + Send>>,
    pending: Mutex<TerminalPendingInput>,
}

impl TerminalInputTransport {
    fn new(writer: Box<dyn Write + Send>) -> Self {
        Self {
            writer: Mutex::new(writer),
            pending: Mutex::new(TerminalPendingInput::default()),
        }
    }
}

#[derive(Default)]
struct TerminalPendingInput {
    entries: VecDeque<TerminalPendingEntry>,
    bytes: usize,
    owner: Option<u64>,
    next_owner: u64,
    writer_failed: bool,
}

struct TerminalPendingEntry {
    data: Arc<[u8]>,
    offset: usize,
}

type TerminalPendingChunk = (Arc<[u8]>, usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TerminalInputClaim {
    Direct(u64),
    Recovery(u64, TerminalInputOutcome),
    Outcome(TerminalInputOutcome),
}

struct TerminalDrainLease<'a> {
    pending: &'a Mutex<TerminalPendingInput>,
    owner: u64,
}

impl Drop for TerminalDrainLease<'_> {
    fn drop(&mut self) {
        if let Ok(mut pending) = self.pending.lock() {
            if std::thread::panicking() {
                pending.fail_owner(self.owner);
            } else {
                pending.release_owner(self.owner);
            }
        }
    }
}

impl TerminalPendingInput {
    fn enqueue(&mut self, data: &[u8]) -> TerminalInputOutcome {
        if data.len() > TERMINAL_PENDING_INPUT_LIMIT.saturating_sub(self.bytes) {
            return TerminalInputOutcome::InputQueueFull;
        }
        self.entries.push_back(TerminalPendingEntry {
            data: Arc::from(data),
            offset: 0,
        });
        self.bytes += data.len();
        TerminalInputOutcome::Queued
    }

    fn claim(&mut self, data: &[u8]) -> TerminalInputClaim {
        if self.owner.is_none() && self.entries.is_empty() {
            return TerminalInputClaim::Direct(self.claim_owner());
        }

        let outcome = self.enqueue(data);
        if self.owner.is_some() {
            TerminalInputClaim::Outcome(outcome)
        } else {
            TerminalInputClaim::Recovery(self.claim_owner(), outcome)
        }
    }

    fn claim_owner(&mut self) -> u64 {
        self.next_owner = self.next_owner.wrapping_add(1);
        let owner = self.next_owner;
        self.owner = Some(owner);
        owner
    }

    fn front_for_drain(&mut self, owner: u64) -> Result<Option<TerminalPendingChunk>, ()> {
        if self.owner != Some(owner) {
            return Err(());
        }
        let front = self
            .entries
            .front()
            .map(|entry| (entry.data.clone(), entry.offset));
        if front.is_none() {
            self.owner = None;
        }
        Ok(front)
    }

    fn advance_front(
        &mut self,
        owner: u64,
        data: &Arc<[u8]>,
        offset: usize,
        written: usize,
    ) -> bool {
        if self.owner != Some(owner) {
            return false;
        }
        let Some(front) = self.entries.front_mut() else {
            return false;
        };
        if !Arc::ptr_eq(&front.data, data)
            || front.offset != offset
            || written > front.data.len().saturating_sub(front.offset)
            || written > self.bytes
        {
            return false;
        }
        front.offset += written;
        self.bytes -= written;
        true
    }

    fn commit_flushed_front(&mut self, owner: u64, data: &Arc<[u8]>) -> bool {
        if self.owner != Some(owner) {
            return false;
        }
        let matches = self.entries.front().is_some_and(|front| {
            Arc::ptr_eq(&front.data, data) && front.offset == front.data.len()
        });
        if matches {
            self.entries.pop_front();
        }
        matches
    }

    fn release_owner(&mut self, owner: u64) {
        if self.owner == Some(owner) {
            self.owner = None;
        }
    }

    fn fail_owner(&mut self, owner: u64) {
        if self.owner == Some(owner) {
            self.owner = None;
            self.writer_failed = true;
        }
    }
}

struct TerminalOperationState {
    accepting: bool,
    active: usize,
}

impl Default for TerminalOperationState {
    fn default() -> Self {
        Self {
            accepting: true,
            active: 0,
        }
    }
}

#[derive(Default)]
struct TerminalOperationGate {
    state: Mutex<TerminalOperationState>,
    drained: Condvar,
}

impl TerminalOperationGate {
    fn claim(self: &Arc<Self>) -> Option<TerminalOperationLease> {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if !state.accepting {
            return None;
        }
        state.active += 1;
        Some(TerminalOperationLease { gate: self.clone() })
    }

    fn begin_transfer(&self) {
        self.state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .accepting = false;
    }

    fn wait_for_drain(&self) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        while state.active != 0 {
            state = self
                .drained
                .wait(state)
                .unwrap_or_else(|error| error.into_inner());
        }
    }

    fn reopen(&self) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        debug_assert_eq!(state.active, 0);
        state.accepting = true;
    }
}

struct TerminalOperationLease {
    gate: Arc<TerminalOperationGate>,
}

impl Drop for TerminalOperationLease {
    fn drop(&mut self) {
        let mut state = self
            .gate
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        debug_assert!(state.active > 0);
        state.active -= 1;
        if state.active == 0 {
            self.gate.drained.notify_all();
        }
    }
}

struct TerminalPumpActivation {
    sender: Mutex<Option<mpsc::Sender<()>>>,
}

enum TerminalPumpReadiness {
    Published,
    ConsumerReady,
}

impl TerminalPumpActivation {
    fn pending() -> (Arc<Self>, mpsc::Receiver<()>) {
        let (sender, receiver) = mpsc::channel();
        (
            Arc::new(Self {
                sender: Mutex::new(Some(sender)),
            }),
            receiver,
        )
    }

    #[cfg(test)]
    fn active() -> Arc<Self> {
        Arc::new(Self {
            sender: Mutex::new(None),
        })
    }

    fn advance(&self, readiness: TerminalPumpReadiness) {
        if matches!(readiness, TerminalPumpReadiness::Published) {
            return;
        }
        let sender = self
            .sender
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take();
        if let Some(sender) = sender {
            let _ = sender.send(());
        }
    }
}

/// One live shell: its PTY (kept for resize/kill) plus the single input writer.
struct TerminalSession {
    pty: Arc<Mutex<Box<dyn TerminalProcess>>>,
    input: Arc<TerminalInputTransport>,
    grid: Arc<Mutex<TerminalGrid>>,
    title_parser: Arc<Mutex<TerminalTitleParser>>,
    operations: Arc<TerminalOperationGate>,
    pump_activation: Arc<TerminalPumpActivation>,
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
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TerminalMaterializationEvent {
    Input(Vec<u8>),
    ProcessOutput(Vec<u8>),
}

impl TerminalMaterializationEvent {
    fn byte_len(&self) -> usize {
        match self {
            Self::Input(bytes) | Self::ProcessOutput(bytes) => bytes.len(),
        }
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TerminalMaterializationSpec {
    working_directory: Option<String>,
    initial_command: Option<String>,
    initial_input: Vec<u8>,
    environment: BTreeMap<String, String>,
}

#[allow(dead_code)]
impl TerminalMaterializationSpec {
    pub(crate) fn new(
        working_directory: Option<String>,
        initial_command: Option<String>,
        initial_input: Vec<u8>,
        environment: BTreeMap<String, String>,
    ) -> Self {
        Self {
            working_directory,
            initial_command,
            initial_input,
            environment,
        }
    }
}

struct TerminalPublishedRuntime {
    session_id: u32,
    operations: Weak<TerminalOperationGate>,
}

enum TerminalMaterializationPhase {
    Dormant,
    Starting,
    Published(TerminalPublishedRuntime),
    FencedDormant,
    FencedPublished(TerminalPublishedRuntime),
}

struct TerminalMaterializationSlot {
    spec: TerminalMaterializationSpec,
    events: VecDeque<TerminalMaterializationEvent>,
    bytes: usize,
    generation: u64,
    phase: TerminalMaterializationPhase,
    flushing: bool,
}

impl TerminalMaterializationSlot {
    fn batch_bytes(events: &[TerminalMaterializationEvent]) -> Option<usize> {
        events
            .iter()
            .try_fold(0_usize, |total, event| total.checked_add(event.byte_len()))
    }

    fn append(&mut self, events: Vec<TerminalMaterializationEvent>, bytes: usize) {
        self.bytes += bytes;
        self.events.extend(events);
    }

    fn matches(&self, lease: &TerminalMaterializationLease) -> bool {
        self.generation == lease.generation && self.spec == lease.spec
    }

    fn owns_start(&self, lease: &TerminalMaterializationLease) -> bool {
        self.matches(lease) && matches!(self.phase, TerminalMaterializationPhase::Starting)
    }

    fn runtime_is_live(
        runtime: &TerminalPublishedRuntime,
        sessions: &HashMap<u32, TerminalSession>,
        panel_id: &str,
    ) -> bool {
        let Some(operations) = runtime.operations.upgrade() else {
            return false;
        };
        sessions.get(&runtime.session_id).is_some_and(|session| {
            session.panel_id.as_deref() == Some(panel_id)
                && Arc::ptr_eq(&session.operations, &operations)
        })
    }

    fn accepts_demand(&self, registry: &TerminalRuntimeRegistry, panel_id: &str) -> bool {
        match &self.phase {
            TerminalMaterializationPhase::Published(runtime) => {
                Self::runtime_is_live(runtime, &registry.sessions, panel_id)
            }
            TerminalMaterializationPhase::FencedPublished(runtime) => {
                registry.reserved_panel_ids.contains(panel_id)
                    && registry.reserved_session_ids.contains(&runtime.session_id)
                    && runtime.operations.upgrade().is_some()
            }
            _ => true,
        }
    }

    fn begin_lifecycle_fence(&mut self) {
        let phase = std::mem::replace(&mut self.phase, TerminalMaterializationPhase::FencedDormant);
        self.phase = match phase {
            TerminalMaterializationPhase::Dormant | TerminalMaterializationPhase::Starting => {
                TerminalMaterializationPhase::FencedDormant
            }
            TerminalMaterializationPhase::Published(runtime) => {
                TerminalMaterializationPhase::FencedPublished(runtime)
            }
            fenced @ (TerminalMaterializationPhase::FencedDormant
            | TerminalMaterializationPhase::FencedPublished(_)) => fenced,
        };
    }

    fn rollback_lifecycle_fence(&mut self) {
        let phase = std::mem::replace(&mut self.phase, TerminalMaterializationPhase::Dormant);
        self.phase = match phase {
            TerminalMaterializationPhase::FencedDormant => TerminalMaterializationPhase::Dormant,
            TerminalMaterializationPhase::FencedPublished(runtime) => {
                TerminalMaterializationPhase::Published(runtime)
            }
            phase => phase,
        };
    }
}

#[allow(dead_code)]
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct TerminalMaterializationLease {
    panel_id: String,
    generation: u64,
    spec: TerminalMaterializationSpec,
}

#[allow(dead_code)]
impl TerminalMaterializationLease {
    fn new(panel_id: &str, generation: u64, spec: &TerminalMaterializationSpec) -> Self {
        Self {
            panel_id: panel_id.to_string(),
            generation,
            spec: spec.clone(),
        }
    }

    pub(crate) fn generation(&self) -> u64 {
        self.generation
    }
}

#[allow(dead_code)]
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum TerminalMaterializationDemand {
    Noop,
    Live(Vec<TerminalMaterializationEvent>),
    Queued,
    Start(TerminalMaterializationLease),
    InputQueueFull,
    SurfaceUnavailable,
}

#[allow(dead_code)]
struct TerminalMaterializationPublishError {
    message: String,
    session: TerminalSession,
}

impl std::fmt::Debug for TerminalMaterializationPublishError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TerminalMaterializationPublishError")
            .field("message", &self.message)
            .finish_non_exhaustive()
    }
}

#[derive(Default)]
struct TerminalRuntimeRegistry {
    sessions: HashMap<u32, TerminalSession>,
    reserved_session_ids: BTreeSet<u32>,
    reserved_panel_ids: BTreeSet<String>,
    materializations: HashMap<String, TerminalMaterializationSlot>,
    next_materialization_generation: u64,
}

impl TerminalRuntimeRegistry {
    fn next_materialization_generation(&mut self) -> u64 {
        self.next_materialization_generation = self.next_materialization_generation.wrapping_add(1);
        if self.next_materialization_generation == 0 {
            self.next_materialization_generation = 1;
        }
        self.next_materialization_generation
    }

    fn has_live_panel(&self, panel_id: &str) -> bool {
        self.sessions
            .values()
            .any(|session| session.panel_id.as_deref() == Some(panel_id))
    }

    fn begin_materialization_fence(&mut self, panel_id: &str) {
        if let Some(slot) = self.materializations.get_mut(panel_id) {
            slot.begin_lifecycle_fence();
        }
    }

    fn rollback_materialization_fence(&mut self, panel_id: &str) {
        if let Some(slot) = self.materializations.get_mut(panel_id) {
            slot.rollback_lifecycle_fence();
        }
    }
}

#[derive(Default)]
pub struct TerminalState {
    registry: Mutex<TerminalRuntimeRegistry>,
    next_id: AtomicU32,
}

impl TerminalState {
    fn runtime_registry(&self) -> MutexGuard<'_, TerminalRuntimeRegistry> {
        self.registry
            .lock()
            .expect("terminal runtime registry mutex poisoned")
    }

    fn try_runtime_registry(&self) -> Result<MutexGuard<'_, TerminalRuntimeRegistry>, String> {
        self.registry
            .lock()
            .map_err(|_| "terminal runtime registry mutex poisoned".to_string())
    }
}

#[allow(dead_code)]
pub(crate) fn request_terminal_materialization(
    state: &TerminalState,
    panel_id: &str,
    spec: &TerminalMaterializationSpec,
    mut events: Vec<TerminalMaterializationEvent>,
) -> TerminalMaterializationDemand {
    events.retain(|event| event.byte_len() != 0);
    if events.is_empty() {
        return TerminalMaterializationDemand::Noop;
    }
    let panel_id = panel_id.trim();
    if panel_id.is_empty() {
        return TerminalMaterializationDemand::SurfaceUnavailable;
    }
    let Ok(mut registry) = state.try_runtime_registry() else {
        return TerminalMaterializationDemand::SurfaceUnavailable;
    };

    if let Some(slot) = registry.materializations.get(panel_id) {
        if slot.spec != *spec || !slot.accepts_demand(&registry, panel_id) {
            return TerminalMaterializationDemand::SurfaceUnavailable;
        }
        let Some(bytes) = TerminalMaterializationSlot::batch_bytes(&events) else {
            return TerminalMaterializationDemand::InputQueueFull;
        };
        if bytes > TERMINAL_PENDING_INPUT_LIMIT.saturating_sub(slot.bytes) {
            return TerminalMaterializationDemand::InputQueueFull;
        }
        let restart = matches!(slot.phase, TerminalMaterializationPhase::Dormant);
        let resume =
            matches!(slot.phase, TerminalMaterializationPhase::Published(_)) && !slot.flushing;
        let generation = restart.then(|| registry.next_materialization_generation());
        let slot = registry
            .materializations
            .get_mut(panel_id)
            .expect("materialization remains under registry lock");
        slot.append(events, bytes);
        if let Some(generation) = generation {
            slot.generation = generation;
            slot.phase = TerminalMaterializationPhase::Starting;
            return TerminalMaterializationDemand::Start(TerminalMaterializationLease::new(
                panel_id, generation, spec,
            ));
        }
        if resume {
            return TerminalMaterializationDemand::Start(TerminalMaterializationLease::new(
                panel_id,
                slot.generation,
                spec,
            ));
        }
        return TerminalMaterializationDemand::Queued;
    }

    if registry.reserved_panel_ids.contains(panel_id) {
        return TerminalMaterializationDemand::SurfaceUnavailable;
    }
    if registry.has_live_panel(panel_id) {
        return TerminalMaterializationDemand::Live(events);
    }
    let Some(bytes) = TerminalMaterializationSlot::batch_bytes(&events) else {
        return TerminalMaterializationDemand::InputQueueFull;
    };
    if bytes > TERMINAL_PENDING_INPUT_LIMIT {
        return TerminalMaterializationDemand::InputQueueFull;
    }
    let generation = registry.next_materialization_generation();
    registry.materializations.insert(
        panel_id.to_string(),
        TerminalMaterializationSlot {
            spec: spec.clone(),
            events: events.into(),
            bytes,
            generation,
            phase: TerminalMaterializationPhase::Starting,
            flushing: false,
        },
    );
    TerminalMaterializationDemand::Start(TerminalMaterializationLease::new(
        panel_id, generation, spec,
    ))
}

pub(crate) fn request_live_terminal_input(
    state: &TerminalState,
    panel_id: &str,
    mut events: Vec<TerminalMaterializationEvent>,
) -> TerminalMaterializationDemand {
    events.retain(|event| event.byte_len() != 0);
    if events.is_empty() {
        return TerminalMaterializationDemand::Noop;
    }
    let panel_id = panel_id.trim();
    if panel_id.is_empty() {
        return TerminalMaterializationDemand::SurfaceUnavailable;
    }
    match state.try_runtime_registry() {
        Ok(registry)
            if !registry.reserved_panel_ids.contains(panel_id)
                && registry.has_live_panel(panel_id) =>
        {
            TerminalMaterializationDemand::Live(events)
        }
        _ => TerminalMaterializationDemand::SurfaceUnavailable,
    }
}

#[allow(dead_code)]
pub(crate) fn retry_terminal_materialization_start(
    state: &TerminalState,
    panel_id: &str,
    spec: &TerminalMaterializationSpec,
) -> Result<Option<TerminalMaterializationLease>, String> {
    let panel_id = panel_id.trim();
    if panel_id.is_empty() {
        return Ok(None);
    }
    let mut registry = state.try_runtime_registry()?;
    let retryable = registry.materializations.get(panel_id).is_some_and(|slot| {
        slot.spec == *spec && matches!(slot.phase, TerminalMaterializationPhase::Dormant)
    });
    if !retryable || registry.reserved_panel_ids.contains(panel_id) {
        return Ok(None);
    }
    let generation = registry.next_materialization_generation();
    let slot = registry
        .materializations
        .get_mut(panel_id)
        .expect("retryable materialization remains under registry lock");
    slot.generation = generation;
    slot.phase = TerminalMaterializationPhase::Starting;
    Ok(Some(TerminalMaterializationLease::new(
        panel_id, generation, spec,
    )))
}

#[allow(dead_code)]
pub(crate) fn retry_terminal_materialization_flush(
    state: &TerminalState,
    panel_id: &str,
    spec: &TerminalMaterializationSpec,
) -> Result<Option<TerminalMaterializationLease>, String> {
    let panel_id = panel_id.trim();
    if panel_id.is_empty() {
        return Ok(None);
    }
    let registry = state.try_runtime_registry()?;
    let Some(slot) = registry.materializations.get(panel_id) else {
        return Ok(None);
    };
    let resumable = slot.spec == *spec
        && !slot.events.is_empty()
        && !slot.flushing
        && !registry.reserved_panel_ids.contains(panel_id)
        && match &slot.phase {
            TerminalMaterializationPhase::Published(runtime) => {
                TerminalMaterializationSlot::runtime_is_live(runtime, &registry.sessions, panel_id)
            }
            _ => false,
        };
    Ok(resumable.then(|| TerminalMaterializationLease::new(panel_id, slot.generation, spec)))
}

#[allow(dead_code)]
pub(crate) fn owns_terminal_materialization_start(
    state: &TerminalState,
    lease: &TerminalMaterializationLease,
) -> Result<bool, String> {
    let registry = state.try_runtime_registry()?;
    Ok(registry
        .materializations
        .get(&lease.panel_id)
        .is_some_and(|slot| slot.owns_start(lease)))
}

#[allow(dead_code)]
pub(crate) fn fail_terminal_materialization_start(
    state: &TerminalState,
    lease: &TerminalMaterializationLease,
) -> Result<bool, String> {
    let mut registry = state.try_runtime_registry()?;
    let Some(slot) = registry.materializations.get_mut(&lease.panel_id) else {
        return Ok(false);
    };
    if !slot.owns_start(lease) {
        return Ok(false);
    }
    slot.phase = TerminalMaterializationPhase::Dormant;
    Ok(true)
}

#[allow(dead_code)]
pub(crate) fn cancel_terminal_materialization(
    state: &TerminalState,
    lease: &TerminalMaterializationLease,
) -> Result<Option<Vec<TerminalMaterializationEvent>>, String> {
    let mut registry = state.try_runtime_registry()?;
    let cancellable = registry
        .materializations
        .get(&lease.panel_id)
        .is_some_and(|slot| slot.owns_start(lease));
    if !cancellable {
        return Ok(None);
    }
    let slot = registry
        .materializations
        .remove(&lease.panel_id)
        .expect("matching materialization remains under registry lock");
    Ok(Some(slot.events.into()))
}

#[allow(dead_code)]
fn publish_terminal_materialization_runtime(
    state: &TerminalState,
    lease: &TerminalMaterializationLease,
    session_id: u32,
    session: TerminalSession,
) -> Result<(), TerminalMaterializationPublishError> {
    let mut registry = match state.try_runtime_registry() {
        Ok(registry) => registry,
        Err(message) => return Err(TerminalMaterializationPublishError { message, session }),
    };
    let valid_slot = registry
        .materializations
        .get(&lease.panel_id)
        .is_some_and(|slot| slot.owns_start(lease));
    let identity_available = session.panel_id.as_deref() == Some(lease.panel_id.as_str())
        && !registry.sessions.contains_key(&session_id)
        && !registry.reserved_session_ids.contains(&session_id)
        && !registry.reserved_panel_ids.contains(&lease.panel_id)
        && !registry.has_live_panel(&lease.panel_id);
    if !valid_slot || !identity_available {
        return Err(TerminalMaterializationPublishError {
            message: "terminal materialization publication lost exact ownership".to_string(),
            session,
        });
    }
    let operations = Arc::downgrade(&session.operations);
    registry.sessions.insert(session_id, session);
    registry
        .materializations
        .get_mut(&lease.panel_id)
        .expect("validated materialization remains under registry lock")
        .phase = TerminalMaterializationPhase::Published(TerminalPublishedRuntime {
        session_id,
        operations,
    });
    Ok(())
}

#[allow(dead_code)]
pub(crate) fn drain_terminal_materialization_event(
    state: &TerminalState,
    lease: &TerminalMaterializationLease,
) -> Result<Option<TerminalMaterializationEvent>, String> {
    let mut registry = state.try_runtime_registry()?;
    let owns_runtime = registry
        .materializations
        .get(&lease.panel_id)
        .is_some_and(|slot| {
            !slot.flushing
                && slot.matches(lease)
                && match &slot.phase {
                    TerminalMaterializationPhase::Published(runtime) => {
                        TerminalMaterializationSlot::runtime_is_live(
                            runtime,
                            &registry.sessions,
                            &lease.panel_id,
                        )
                    }
                    _ => false,
                }
        });
    if !owns_runtime {
        return Err("terminal materialization lease does not own a live runtime".to_string());
    }
    let slot = registry
        .materializations
        .get_mut(&lease.panel_id)
        .expect("owned materialization remains under registry lock");
    if let Some(event) = slot.events.pop_front() {
        slot.bytes -= event.byte_len();
        return Ok(Some(event));
    }
    registry.materializations.remove(&lease.panel_id);
    Ok(None)
}

enum TerminalMaterializationPreparation {
    Spawn(u32),
    Flush,
}

fn prepare_terminal_materialization(
    state: &TerminalState,
    lease: &TerminalMaterializationLease,
) -> Result<TerminalMaterializationPreparation, String> {
    let registry = state.try_runtime_registry()?;
    let slot = registry
        .materializations
        .get(&lease.panel_id)
        .ok_or_else(|| "terminal materialization ownership lost".to_string())?;
    if !slot.matches(lease) || registry.reserved_panel_ids.contains(&lease.panel_id) {
        return Err("terminal materialization ownership lost".to_string());
    }
    match &slot.phase {
        TerminalMaterializationPhase::Starting => {
            let id = loop {
                let candidate = state.next_id.fetch_add(1, Ordering::Relaxed);
                if !registry.reserved_session_ids.contains(&candidate)
                    && !registry.sessions.contains_key(&candidate)
                {
                    break candidate;
                }
            };
            Ok(TerminalMaterializationPreparation::Spawn(id))
        }
        TerminalMaterializationPhase::Published(runtime)
            if !slot.flushing
                && !slot.events.is_empty()
                && TerminalMaterializationSlot::runtime_is_live(
                    runtime,
                    &registry.sessions,
                    &lease.panel_id,
                ) =>
        {
            Ok(TerminalMaterializationPreparation::Flush)
        }
        _ => Err("terminal materialization ownership lost".to_string()),
    }
}

fn terminal_runtime_matches(
    runtime: &TerminalPublishedRuntime,
    session_id: u32,
    operations: &Arc<TerminalOperationGate>,
) -> bool {
    runtime.session_id == session_id
        && runtime
            .operations
            .upgrade()
            .is_some_and(|owned| Arc::ptr_eq(&owned, operations))
}

struct TerminalMaterializationFlushGuard<'a> {
    state: &'a TerminalState,
    lease: TerminalMaterializationLease,
    session_id: u32,
    operations: Arc<TerminalOperationGate>,
    process: Arc<Mutex<Box<dyn TerminalProcess>>>,
    input: Arc<TerminalInputTransport>,
    grid: Arc<Mutex<TerminalGrid>>,
    title_parser: Arc<Mutex<TerminalTitleParser>>,
    pump_activation: Arc<TerminalPumpActivation>,
    _operation: TerminalOperationLease,
    finished: bool,
}

impl TerminalMaterializationFlushGuard<'_> {
    fn owns_published_runtime(&self, slot: &TerminalMaterializationSlot) -> bool {
        slot.matches(&self.lease)
            && slot.flushing
            && match &slot.phase {
                TerminalMaterializationPhase::Published(runtime) => {
                    terminal_runtime_matches(runtime, self.session_id, &self.operations)
                }
                _ => false,
            }
    }

    fn owns_runtime(&self, slot: &TerminalMaterializationSlot) -> bool {
        slot.matches(&self.lease)
            && match &slot.phase {
                TerminalMaterializationPhase::Published(runtime)
                | TerminalMaterializationPhase::FencedPublished(runtime) => {
                    terminal_runtime_matches(runtime, self.session_id, &self.operations)
                }
                _ => false,
            }
    }

    fn take_event(&self) -> Result<Option<TerminalMaterializationEvent>, String> {
        let mut registry = self.state.try_runtime_registry()?;
        let owns_flush = registry
            .materializations
            .get(&self.lease.panel_id)
            .is_some_and(|slot| {
                self.owns_published_runtime(slot)
                    && match &slot.phase {
                        TerminalMaterializationPhase::Published(runtime) => {
                            TerminalMaterializationSlot::runtime_is_live(
                                runtime,
                                &registry.sessions,
                                &self.lease.panel_id,
                            )
                        }
                        _ => unreachable!("published ownership checked above"),
                    }
            });
        if !owns_flush {
            return Err("terminal materialization flush ownership lost".to_string());
        }
        let slot = registry
            .materializations
            .get_mut(&self.lease.panel_id)
            .expect("validated flush remains under registry lock");
        let event = slot.events.pop_front();
        if let Some(event) = event.as_ref() {
            slot.bytes -= event.byte_len();
        }
        Ok(event)
    }

    fn try_finish(&mut self) -> Result<bool, String> {
        let mut registry = self.state.try_runtime_registry()?;
        let finishable = registry
            .materializations
            .get(&self.lease.panel_id)
            .is_some_and(|slot| {
                self.owns_published_runtime(slot)
                    && slot.events.is_empty()
                    && match &slot.phase {
                        TerminalMaterializationPhase::Published(runtime) => {
                            TerminalMaterializationSlot::runtime_is_live(
                                runtime,
                                &registry.sessions,
                                &self.lease.panel_id,
                            )
                        }
                        _ => unreachable!("published ownership checked above"),
                    }
            });
        if finishable {
            registry.materializations.remove(&self.lease.panel_id);
            self.finished = true;
            return Ok(true);
        }
        let still_owned = registry
            .materializations
            .get(&self.lease.panel_id)
            .is_some_and(|slot| self.owns_published_runtime(slot));
        if still_owned {
            Ok(false)
        } else {
            Err("terminal materialization flush ownership lost".to_string())
        }
    }

    fn abort(&mut self) -> (bool, bool) {
        let (mut registry, registry_was_poisoned) = terminal_registry_for_exact_cleanup(self.state);
        let abortable = registry
            .materializations
            .get(&self.lease.panel_id)
            .is_some_and(|slot| self.owns_published_runtime(slot));
        if abortable {
            registry.materializations.remove(&self.lease.panel_id);
            self.finished = true;
        }
        (abortable, registry_was_poisoned)
    }
}

impl Drop for TerminalMaterializationFlushGuard<'_> {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        let (mut registry, _) = terminal_registry_for_exact_cleanup(self.state);
        if let Some(slot) = registry.materializations.get_mut(&self.lease.panel_id) {
            if self.owns_runtime(slot) {
                slot.flushing = false;
            }
        }
    }
}

fn begin_terminal_materialization_flush<'a>(
    state: &'a TerminalState,
    lease: TerminalMaterializationLease,
) -> Result<TerminalMaterializationFlushGuard<'a>, String> {
    let mut registry = state.try_runtime_registry()?;
    let (session_id, operations) = {
        let slot = registry
            .materializations
            .get(&lease.panel_id)
            .ok_or_else(|| "terminal materialization flush ownership lost".to_string())?;
        if !slot.matches(&lease) || slot.flushing {
            return Err("terminal materialization flush ownership lost".to_string());
        }
        let TerminalMaterializationPhase::Published(runtime) = &slot.phase else {
            return Err("terminal materialization flush ownership lost".to_string());
        };
        let operations = runtime
            .operations
            .upgrade()
            .ok_or_else(|| "terminal materialization runtime expired".to_string())?;
        (runtime.session_id, operations)
    };
    let (process, input, grid, title_parser, pump_activation, operation) = {
        let session = registry
            .sessions
            .get(&session_id)
            .filter(|session| {
                session.panel_id.as_deref() == Some(lease.panel_id.as_str())
                    && Arc::ptr_eq(&session.operations, &operations)
            })
            .ok_or_else(|| "terminal materialization runtime ownership lost".to_string())?;
        let operation = session
            .operations
            .claim()
            .ok_or_else(|| "terminal materialization runtime is fenced".to_string())?;
        (
            session.pty.clone(),
            session.input.clone(),
            session.grid.clone(),
            session.title_parser.clone(),
            session.pump_activation.clone(),
            operation,
        )
    };
    registry
        .materializations
        .get_mut(&lease.panel_id)
        .expect("validated flush remains under registry lock")
        .flushing = true;
    drop(registry);
    Ok(TerminalMaterializationFlushGuard {
        state,
        lease,
        session_id,
        operations,
        process,
        input,
        grid,
        title_parser,
        pump_activation,
        _operation: operation,
        finished: false,
    })
}

fn kill_terminal_session(session: &TerminalSession) -> Result<(), String> {
    session
        .pty
        .lock()
        .map_err(|_| "terminal process mutex poisoned".to_string())?
        .kill()
}

#[allow(dead_code)]
fn fulfill_terminal_materialization_with<Spawn, Emit>(
    state: &TerminalState,
    lease: TerminalMaterializationLease,
    size: ConPtySize,
    spawn: Spawn,
    mut emit_output: Emit,
) -> Result<u32, String>
where
    Spawn: FnOnce(
        u32,
        &str,
        &TerminalMaterializationSpec,
        ConPtySize,
    ) -> Result<TerminalSession, String>,
    Emit: FnMut(u32, &[u8], &[String]) -> Result<(), String>,
{
    let preparation = prepare_terminal_materialization(state, &lease)?;
    if let TerminalMaterializationPreparation::Spawn(id) = preparation {
        let session = match spawn(id, &lease.panel_id, &lease.spec, size) {
            Ok(session) => session,
            Err(error) => {
                let _ = fail_terminal_materialization_start(state, &lease);
                return Err(error);
            }
        };
        if let Err(publication) =
            publish_terminal_materialization_runtime(state, &lease, id, session)
        {
            let cleanup = kill_terminal_session(&publication.session).err();
            let _ = fail_terminal_materialization_start(state, &lease);
            return Err(match cleanup {
                Some(cleanup) => format!(
                    "{}; terminal startup cleanup failed: {cleanup}",
                    publication.message
                ),
                None => publication.message,
            });
        }
    }

    let mut flush = begin_terminal_materialization_flush(state, lease)?;
    loop {
        let Some(event) = flush.take_event()? else {
            if flush.try_finish()? {
                flush
                    .pump_activation
                    .advance(TerminalPumpReadiness::ConsumerReady);
                return Ok(flush.session_id);
            }
            continue;
        };
        let applied = match event {
            TerminalMaterializationEvent::Input(bytes) => {
                match flush
                    .grid
                    .lock()
                    .map(|grid| terminal_input_bytes_for_grid(&grid, &bytes))
                {
                    Ok(bytes) => {
                        match send_terminal_input(
                            flush.process.clone(),
                            flush.input.clone(),
                            &bytes,
                        ) {
                            TerminalInputOutcome::Sent | TerminalInputOutcome::Queued => Ok(()),
                            outcome => Err(format!(
                                "terminal materialization input failed: {outcome:?}"
                            )),
                        }
                    }
                    Err(_) => Err("terminal grid mutex poisoned".to_string()),
                }
            }
            TerminalMaterializationEvent::ProcessOutput(bytes) => {
                let titles = match flush.grid.lock() {
                    Ok(mut grid) => {
                        grid.advance(&bytes);
                        drop(grid);
                        flush
                            .title_parser
                            .lock()
                            .map_err(|_| "terminal title parser mutex poisoned".to_string())
                            .map(|mut parser| parser.consume(&bytes))
                    }
                    Err(_) => Err("terminal grid mutex poisoned".to_string()),
                };
                titles.and_then(|titles| emit_output(flush.session_id, &bytes, &titles))
            }
        };
        if let Err(error) = applied {
            let (aborted, registry_was_poisoned) = flush.abort();
            if aborted {
                flush
                    .pump_activation
                    .advance(TerminalPumpReadiness::ConsumerReady);
            }
            return Err(if registry_was_poisoned {
                format!("{error}; terminal runtime registry mutex poisoned")
            } else {
                error
            });
        }
        if flush.try_finish()? {
            flush
                .pump_activation
                .advance(TerminalPumpReadiness::ConsumerReady);
            return Ok(flush.session_id);
        }
    }
}

fn terminal_registry_for_exact_cleanup(
    state: &TerminalState,
) -> (MutexGuard<'_, TerminalRuntimeRegistry>, bool) {
    match state.registry.lock() {
        Ok(registry) => (registry, false),
        Err(error) => (error.into_inner(), true),
    }
}

#[allow(dead_code)]
pub(crate) struct TerminalPanelRuntimeLease {
    panel_ids: BTreeSet<String>,
    sessions: BTreeMap<u32, TerminalSession>,
    completed_session_ids: BTreeSet<u32>,
}

#[allow(dead_code)]
pub(crate) struct TerminalPanelRollbackError {
    pub(crate) message: String,
    pub(crate) lease: TerminalPanelRuntimeLease,
}

#[allow(dead_code)]
pub(crate) struct TerminalPanelFinalizeError {
    pub(crate) failures: Vec<String>,
    pub(crate) retry: TerminalPanelRuntimeLease,
}

#[allow(dead_code)]
pub(crate) fn detach_terminal_panels_for_control(
    state: &TerminalState,
    panel_ids: &BTreeSet<String>,
) -> Result<TerminalPanelRuntimeLease, String> {
    let panel_ids = panel_ids
        .iter()
        .filter_map(|panel_id| {
            let panel_id = panel_id.trim();
            (!panel_id.is_empty()).then(|| panel_id.to_string())
        })
        .collect::<BTreeSet<_>>();
    let mut registry = state.try_runtime_registry()?;
    if let Some(panel_id) = panel_ids
        .iter()
        .find(|panel_id| registry.reserved_panel_ids.contains(*panel_id))
    {
        return Err(format!("terminal panel {panel_id} is already reserved"));
    }
    let session_ids = registry
        .sessions
        .iter()
        .filter_map(|(id, session)| {
            session
                .panel_id
                .as_ref()
                .is_some_and(|panel_id| panel_ids.contains(panel_id))
                .then_some(*id)
        })
        .collect::<BTreeSet<_>>();
    if let Some(id) = session_ids
        .iter()
        .find(|id| registry.reserved_session_ids.contains(id))
    {
        return Err(format!("terminal session {id} is already reserved"));
    }

    for id in &session_ids {
        registry
            .sessions
            .get(id)
            .expect("prechecked terminal session")
            .operations
            .begin_transfer();
    }
    for panel_id in &panel_ids {
        registry.begin_materialization_fence(panel_id);
    }

    let mut sessions = BTreeMap::new();
    for id in &session_ids {
        sessions.insert(
            *id,
            registry
                .sessions
                .remove(id)
                .expect("prechecked terminal session"),
        );
    }
    registry
        .reserved_panel_ids
        .extend(panel_ids.iter().cloned());
    registry.reserved_session_ids.extend(session_ids);
    drop(registry);
    for session in sessions.values() {
        session.operations.wait_for_drain();
    }
    Ok(TerminalPanelRuntimeLease {
        panel_ids,
        sessions,
        completed_session_ids: BTreeSet::new(),
    })
}

#[allow(dead_code)]
pub(crate) fn rollback_terminal_panels_for_control(
    state: &TerminalState,
    lease: TerminalPanelRuntimeLease,
) -> Result<(), TerminalPanelRollbackError> {
    let mut registry = match state.try_runtime_registry() {
        Ok(registry) => registry,
        Err(message) => return Err(TerminalPanelRollbackError { message, lease }),
    };
    let ownership_lost = lease
        .panel_ids
        .iter()
        .any(|panel_id| !registry.reserved_panel_ids.contains(panel_id))
        || lease
            .sessions
            .keys()
            .chain(lease.completed_session_ids.iter())
            .any(|id| !registry.reserved_session_ids.contains(id));
    let collision = lease
        .sessions
        .keys()
        .any(|id| registry.sessions.contains_key(id))
        || registry.sessions.values().any(|session| {
            session
                .panel_id
                .as_ref()
                .is_some_and(|panel_id| lease.panel_ids.contains(panel_id))
        });
    if ownership_lost || collision {
        return Err(TerminalPanelRollbackError {
            message: "terminal rollback collision".to_string(),
            lease,
        });
    }

    for (id, session) in lease.sessions {
        session.operations.reopen();
        registry.sessions.insert(id, session);
        registry.reserved_session_ids.remove(&id);
    }
    for id in lease.completed_session_ids {
        registry.reserved_session_ids.remove(&id);
    }
    for panel_id in lease.panel_ids {
        registry.reserved_panel_ids.remove(&panel_id);
        registry.rollback_materialization_fence(&panel_id);
    }
    Ok(())
}

#[allow(dead_code)]
pub(crate) fn finalize_terminal_panels_for_control(
    state: &TerminalState,
    lease: TerminalPanelRuntimeLease,
) -> Result<(), TerminalPanelFinalizeError> {
    let guard = match state.try_runtime_registry() {
        Ok(registry) => registry,
        Err(error) => {
            return Err(TerminalPanelFinalizeError {
                failures: vec![error],
                retry: lease,
            });
        }
    };
    let ownership_lost = lease
        .panel_ids
        .iter()
        .any(|panel_id| !guard.reserved_panel_ids.contains(panel_id))
        || lease
            .sessions
            .keys()
            .chain(lease.completed_session_ids.iter())
            .any(|id| !guard.reserved_session_ids.contains(id));
    drop(guard);
    if ownership_lost {
        return Err(TerminalPanelFinalizeError {
            failures: vec!["terminal finalize reservation lost".to_string()],
            retry: lease,
        });
    }

    let TerminalPanelRuntimeLease {
        panel_ids,
        sessions,
        mut completed_session_ids,
    } = lease;
    let mut failures = Vec::new();
    let mut retry_sessions = BTreeMap::new();
    for (id, session) in sessions {
        let kill = session
            .pty
            .lock()
            .map_err(|_| "terminal process mutex poisoned".to_string())
            .and_then(|mut process| process.kill());
        match kill {
            Ok(()) => {
                completed_session_ids.insert(id);
            }
            Err(error) => {
                failures.push(format!("terminal runtime {id} kill failed: {error}"));
                retry_sessions.insert(id, session);
            }
        }
    }
    let retry_panel_ids = retry_sessions
        .values()
        .filter_map(|session| session.panel_id.clone())
        .collect::<BTreeSet<_>>();

    let mut registry = match state.try_runtime_registry() {
        Ok(registry) => registry,
        Err(error) => {
            failures.push(error);
            return Err(TerminalPanelFinalizeError {
                failures,
                retry: TerminalPanelRuntimeLease {
                    panel_ids,
                    sessions: retry_sessions,
                    completed_session_ids,
                },
            });
        }
    };
    for id in completed_session_ids {
        registry.reserved_session_ids.remove(&id);
    }
    for panel_id in panel_ids {
        if !retry_panel_ids.contains(&panel_id) {
            registry.materializations.remove(&panel_id);
            registry.reserved_panel_ids.remove(&panel_id);
        }
    }
    if retry_sessions.is_empty() {
        Ok(())
    } else {
        drop(registry);
        Err(TerminalPanelFinalizeError {
            failures,
            retry: TerminalPanelRuntimeLease {
                panel_ids: retry_panel_ids,
                sessions: retry_sessions,
                completed_session_ids: BTreeSet::new(),
            },
        })
    }
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

struct TerminalProcessComponents {
    process: Box<dyn TerminalProcess>,
    reader: Box<dyn Read + Send>,
    writer: Box<dyn Write + Send>,
    root_pid: Option<u32>,
}

fn cleanup_failed_conpty(mut pty: ConPty, error: String) -> String {
    let kill = pty.kill().err();
    let deadline = Instant::now() + Duration::from_secs(2);
    let wait = loop {
        match pty.try_wait() {
            Ok(Some(_)) => break None,
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(10));
            }
            Ok(None) => break Some("timed out waiting for terminal startup cleanup".to_string()),
            Err(wait) => break Some(format!("terminal startup wait failed: {wait}")),
        }
    };
    match (kill, wait) {
        (None, None) => error,
        (kill, wait) => {
            let cleanup = kill
                .into_iter()
                .map(|kill| format!("terminal startup kill failed: {kill}"))
                .chain(wait)
                .collect::<Vec<_>>()
                .join("; ");
            format!("{error}; {cleanup}")
        }
    }
}

fn spawn_terminal_process_components(
    spec: &TerminalMaterializationSpec,
    size: ConPtySize,
) -> Result<TerminalProcessComponents, String> {
    let command = default_shell_command(
        spec.working_directory.as_deref(),
        spec.initial_command.as_deref(),
        Some(spec.environment.clone()),
    );
    let pty = ConPty::spawn(&command, size).map_err(|error| error.to_string())?;
    let reader = match pty.reader() {
        Ok(reader) => reader,
        Err(error) => return Err(cleanup_failed_conpty(pty, error.to_string())),
    };
    let mut writer = match pty.take_writer() {
        Ok(writer) => writer,
        Err(error) => {
            drop(reader);
            return Err(cleanup_failed_conpty(pty, error.to_string()));
        }
    };
    if let Err(error) = write_terminal_initial_bytes(writer.as_mut(), &spec.initial_input) {
        drop(writer);
        drop(reader);
        return Err(cleanup_failed_conpty(pty, error));
    }
    let root_pid = pty.process_id();
    Ok(TerminalProcessComponents {
        process: Box::new(pty),
        reader,
        writer,
        root_pid,
    })
}

fn spawn_terminal_session(
    app: &AppHandle,
    id: u32,
    panel_id: Option<String>,
    spec: &TerminalMaterializationSpec,
    size: ConPtySize,
) -> Result<TerminalSession, String> {
    let TerminalProcessComponents {
        process,
        reader,
        writer,
        root_pid,
    } = spawn_terminal_process_components(spec, size)?;
    let process = Arc::new(Mutex::new(process));
    let grid = Arc::new(Mutex::new(TerminalGrid::new(GridSize::new(
        usize::from(size.cols),
        usize::from(size.rows),
    ))));
    let title_parser = Arc::new(Mutex::new(TerminalTitleParser::default()));
    let (pump_activation, pump_ready) = TerminalPumpActivation::pending();
    let pump_app = app.clone();
    let pump_panel_id = panel_id.clone();
    let pump_grid = grid.clone();
    let pump_title_parser = title_parser.clone();
    if let Err(error) = std::thread::Builder::new()
        .name(format!("cmux-terminal-pump-{id}"))
        .spawn(move || {
            if pump_ready.recv().is_ok() {
                pump_reader(
                    pump_app,
                    id,
                    pump_panel_id,
                    pump_grid,
                    pump_title_parser,
                    reader,
                );
            }
        })
    {
        let cleanup = process
            .lock()
            .map_err(|_| "terminal process mutex poisoned".to_string())
            .and_then(|mut process| process.kill())
            .err();
        return Err(match cleanup {
            Some(cleanup) => format!("{error}; terminal startup cleanup failed: {cleanup}"),
            None => error.to_string(),
        });
    }
    Ok(TerminalSession {
        pty: process,
        input: Arc::new(TerminalInputTransport::new(writer)),
        grid,
        title_parser,
        operations: Arc::new(TerminalOperationGate::default()),
        pump_activation,
        panel_id,
        root_pid,
    })
}

#[allow(dead_code)]
pub(crate) fn materialize_terminal_for_input(
    app: &AppHandle,
    state: &TerminalState,
    lease: TerminalMaterializationLease,
    cols: Option<u16>,
    rows: Option<u16>,
) -> Result<u32, String> {
    let size = ConPtySize::new(cols.unwrap_or(80).max(1), rows.unwrap_or(24).max(1));
    let output_panel_id = lease.panel_id.clone();
    fulfill_terminal_materialization_with(
        state,
        lease,
        size,
        |id, panel_id, spec, size| {
            spawn_terminal_session(app, id, Some(panel_id.to_string()), spec, size)
        },
        |id, bytes, titles| {
            emit_terminal_output_chunk(app, id, Some(&output_panel_id), bytes, titles)
        },
    )
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

struct TerminalOpenIdentityReservation {
    id: u32,
    panel_id: Option<String>,
    fenced_operations: Vec<Arc<TerminalOperationGate>>,
}

enum TerminalOpenReservation {
    Existing(u32),
    Reserved(TerminalOpenIdentityReservation),
}

fn terminal_panel_operation_gates(
    registry: &TerminalRuntimeRegistry,
    panel_id: &str,
) -> Vec<Arc<TerminalOperationGate>> {
    registry
        .sessions
        .values()
        .filter(|session| session.panel_id.as_deref() == Some(panel_id))
        .map(|session| session.operations.clone())
        .collect()
}

fn reserve_terminal_open_for_control(
    state: &TerminalState,
    panel_id: Option<&str>,
    reuse_existing: bool,
) -> Result<TerminalOpenReservation, String> {
    let panel_id = panel_id.and_then(|value| {
        let trimmed = value.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_string())
    });
    let mut registry = state.try_runtime_registry()?;
    if let Some(panel_id) = panel_id
        .as_ref()
        .filter(|panel_id| registry.reserved_panel_ids.contains(*panel_id))
    {
        return Err(format!("terminal panel {panel_id} is reserved"));
    }
    if let Some(existing) = reusable_panel_session_id(
        registry
            .sessions
            .iter()
            .map(|(id, session)| (*id, session.panel_id.as_deref())),
        panel_id.as_deref(),
        reuse_existing,
    ) {
        return Ok(TerminalOpenReservation::Existing(existing));
    }
    if let Some(panel_id) = panel_id
        .as_ref()
        .filter(|panel_id| registry.materializations.contains_key(*panel_id))
    {
        return Err(format!(
            "terminal panel {panel_id} is materializing under another owner"
        ));
    }

    let id = loop {
        let candidate = state.next_id.fetch_add(1, Ordering::Relaxed);
        if !registry.reserved_session_ids.contains(&candidate)
            && !registry.sessions.contains_key(&candidate)
        {
            break candidate;
        }
    };
    let fenced_operations = panel_id
        .as_ref()
        .map(|panel_id| terminal_panel_operation_gates(&registry, panel_id))
        .unwrap_or_default();
    for operations in &fenced_operations {
        operations.begin_transfer();
    }
    registry.reserved_session_ids.insert(id);
    if let Some(panel_id) = panel_id.as_ref() {
        registry.reserved_panel_ids.insert(panel_id.clone());
    }
    drop(registry);
    for operations in &fenced_operations {
        operations.wait_for_drain();
    }
    Ok(TerminalOpenReservation::Reserved(
        TerminalOpenIdentityReservation {
            id,
            panel_id,
            fenced_operations,
        },
    ))
}

fn rollback_terminal_open_reservation_for_control(
    state: &TerminalState,
    reservation: TerminalOpenIdentityReservation,
) -> Result<(), String> {
    let (mut registry, registry_was_poisoned) = terminal_registry_for_exact_cleanup(state);
    if !registry.reserved_session_ids.contains(&reservation.id)
        || reservation
            .panel_id
            .as_ref()
            .is_some_and(|panel_id| !registry.reserved_panel_ids.contains(panel_id))
    {
        return Err("terminal open reservation lost".to_string());
    }
    for operations in &reservation.fenced_operations {
        operations.reopen();
    }
    registry.reserved_session_ids.remove(&reservation.id);
    if let Some(panel_id) = reservation.panel_id {
        registry.reserved_panel_ids.remove(&panel_id);
    }
    if registry_was_poisoned {
        Err("terminal runtime registry mutex poisoned".to_string())
    } else {
        Ok(())
    }
}

fn publish_terminal_open_reservation(
    state: &TerminalState,
    reservation: &TerminalOpenIdentityReservation,
    session: TerminalSession,
) -> Result<u32, (String, TerminalSession)> {
    if session.panel_id != reservation.panel_id {
        return Err(("terminal open identity mismatch".to_string(), session));
    }
    let mut registry = match state.try_runtime_registry() {
        Ok(registry) => registry,
        Err(error) => return Err((error, session)),
    };
    let owns_reservation = registry.reserved_session_ids.contains(&reservation.id)
        && reservation
            .panel_id
            .as_ref()
            .is_none_or(|panel_id| registry.reserved_panel_ids.contains(panel_id));
    let conflicts_with_materialization = reservation
        .panel_id
        .as_ref()
        .is_some_and(|panel_id| registry.materializations.contains_key(panel_id));
    if !owns_reservation
        || conflicts_with_materialization
        || registry.sessions.contains_key(&reservation.id)
    {
        return Err(("terminal open reservation lost".to_string(), session));
    }

    for operations in &reservation.fenced_operations {
        operations.reopen();
    }
    registry.reserved_session_ids.remove(&reservation.id);
    if let Some(panel_id) = reservation.panel_id.as_ref() {
        registry.reserved_panel_ids.remove(panel_id);
    }
    registry.sessions.insert(reservation.id, session);
    Ok(reservation.id)
}

#[cfg(test)]
fn write_terminal_initial_input(
    writer: &mut (dyn Write + Send),
    initial_input: Option<&str>,
) -> Result<(), String> {
    let Some(input) = initial_input.filter(|input| !input.is_empty()) else {
        return Ok(());
    };
    write_terminal_initial_bytes(writer, input.as_bytes())
}

fn write_terminal_initial_bytes(
    writer: &mut (dyn Write + Send),
    input: &[u8],
) -> Result<(), String> {
    if input.is_empty() {
        return Ok(());
    }
    writer
        .write_all(input)
        .and_then(|_| writer.flush())
        .map_err(|error| error.to_string())
}

struct TerminalOpenReservationGuard<'a> {
    state: &'a TerminalState,
    reservation: Option<TerminalOpenIdentityReservation>,
}

impl<'a> TerminalOpenReservationGuard<'a> {
    fn new(state: &'a TerminalState, reservation: TerminalOpenIdentityReservation) -> Self {
        Self {
            state,
            reservation: Some(reservation),
        }
    }

    fn reservation(&self) -> &TerminalOpenIdentityReservation {
        self.reservation
            .as_ref()
            .expect("active terminal open reservation")
    }

    fn disarm(mut self) {
        self.reservation = None;
    }

    fn rollback(mut self, error: String) -> String {
        let reservation = self
            .reservation
            .take()
            .expect("active terminal open reservation");
        match rollback_terminal_open_reservation_for_control(self.state, reservation) {
            Ok(()) => error,
            Err(rollback) => format!("{error}; {rollback}"),
        }
    }
}

impl Drop for TerminalOpenReservationGuard<'_> {
    fn drop(&mut self) {
        if let Some(reservation) = self.reservation.take() {
            let _ = rollback_terminal_open_reservation_for_control(self.state, reservation);
        }
    }
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
    let reservation = match reserve_terminal_open_for_control(state, panel_id, reuse_existing)? {
        TerminalOpenReservation::Existing(id) => return Ok(id),
        TerminalOpenReservation::Reserved(reservation) => reservation,
    };
    let guard = TerminalOpenReservationGuard::new(state, reservation);
    let id = guard.reservation().id;
    let panel_id = guard.reservation().panel_id.clone();
    let size = ConPtySize::new(cols.unwrap_or(80).max(1), rows.unwrap_or(24).max(1));
    let spec = TerminalMaterializationSpec::new(
        cwd.map(str::to_string),
        initial_command.map(str::to_string),
        initial_input.unwrap_or_default().as_bytes().to_vec(),
        environment.unwrap_or_default(),
    );
    // The exact identity reservation above must precede spawn_terminal_session and its
    // ConPty::spawn boundary; publication rechecks that reservation after external work.
    let session = match spawn_terminal_session(app, id, panel_id, &spec, size) {
        Ok(session) => session,
        Err(error) => return Err(guard.rollback(error)),
    };
    let pump_activation = session.pump_activation.clone();
    match publish_terminal_open_reservation(state, guard.reservation(), session) {
        Ok(id) => {
            pump_activation.advance(TerminalPumpReadiness::Published);
            guard.disarm();
            Ok(id)
        }
        Err((error, session)) => {
            let kill_error = session
                .pty
                .lock()
                .map_err(|_| "terminal process mutex poisoned".to_string())
                .and_then(|mut process| process.kill())
                .err();
            let error = match kill_error {
                Some(kill) => format!("{error}; terminal startup cleanup failed: {kill}"),
                None => error,
            };
            Err(guard.rollback(error))
        }
    }
}

pub(crate) fn terminal_has_panel_for_control(state: &TerminalState, panel_id: &str) -> bool {
    state.try_runtime_registry().is_ok_and(|registry| {
        !registry.reserved_panel_ids.contains(panel_id)
            && registry
                .sessions
                .values()
                .any(|session| session.panel_id.as_deref() == Some(panel_id))
    })
}

pub(crate) fn terminal_ids_for_panel_for_control(
    state: &TerminalState,
    panel_id: &str,
) -> Vec<u32> {
    state.try_runtime_registry().map_or_else(
        |_| Vec::new(),
        |registry| {
            if registry.reserved_panel_ids.contains(panel_id) {
                return Vec::new();
            }
            registry
                .sessions
                .iter()
                .filter_map(|(id, session)| {
                    (session.panel_id.as_deref() == Some(panel_id)).then_some(*id)
                })
                .collect()
        },
    )
}

fn ensure_terminal_session_available(
    registry: &TerminalRuntimeRegistry,
    id: u32,
    session: &TerminalSession,
) -> Result<(), String> {
    if registry.reserved_session_ids.contains(&id) {
        return Err(format!("terminal session {id} is reserved"));
    }
    if let Some(panel_id) = session
        .panel_id
        .as_ref()
        .filter(|panel_id| registry.reserved_panel_ids.contains(*panel_id))
    {
        return Err(format!("terminal panel {panel_id} is reserved"));
    }
    Ok(())
}

pub(crate) fn terminal_shutdown_id_preserving_authority_for_control(
    state: &TerminalState,
    id: u32,
) -> Result<(), String> {
    let (process, _operation) = {
        let registry = state.try_runtime_registry()?;
        let session = registry
            .sessions
            .get(&id)
            .ok_or_else(|| format!("terminal runtime {id} is unavailable"))?;
        ensure_terminal_session_available(&registry, id, session)?;
        let operation = session
            .operations
            .claim()
            .ok_or_else(|| format!("terminal session {id} is reserved"))?;
        (session.pty.clone(), operation)
    };
    let result = process
        .lock()
        .map_err(|_| "terminal process mutex poisoned".to_string())?
        .kill();
    result
}

struct TerminalSessionTransfer {
    id: u32,
    panel_id: Option<String>,
    operations: Arc<TerminalOperationGate>,
    fenced_operations: Vec<Arc<TerminalOperationGate>>,
    process: Arc<Mutex<Box<dyn TerminalProcess>>>,
}

fn begin_terminal_session_transfer(
    state: &TerminalState,
    id: u32,
) -> Result<Option<TerminalSessionTransfer>, String> {
    let transfer = {
        let mut registry = state.try_runtime_registry()?;
        if registry.reserved_session_ids.contains(&id) {
            return Err(format!("terminal session {id} is reserved"));
        }
        let Some(session) = registry.sessions.get(&id) else {
            return Ok(None);
        };
        ensure_terminal_session_available(&registry, id, session)?;
        let panel_id = session.panel_id.clone();
        let operations = session.operations.clone();
        let fenced_operations = match panel_id.as_ref() {
            Some(panel_id) => terminal_panel_operation_gates(&registry, panel_id),
            None => vec![operations.clone()],
        };
        let transfer = TerminalSessionTransfer {
            id,
            panel_id,
            operations,
            fenced_operations,
            process: session.pty.clone(),
        };
        for operations in &transfer.fenced_operations {
            operations.begin_transfer();
        }
        if let Some(panel_id) = transfer.panel_id.as_ref() {
            registry.begin_materialization_fence(panel_id);
        }
        registry.reserved_session_ids.insert(id);
        if let Some(panel_id) = transfer.panel_id.as_ref() {
            registry.reserved_panel_ids.insert(panel_id.clone());
        }
        transfer
    };
    for operations in &transfer.fenced_operations {
        operations.wait_for_drain();
    }
    Ok(Some(transfer))
}

fn terminal_session_transfer_is_owned(
    registry: &TerminalRuntimeRegistry,
    transfer: &TerminalSessionTransfer,
) -> bool {
    registry.reserved_session_ids.contains(&transfer.id)
        && transfer
            .panel_id
            .as_ref()
            .is_none_or(|panel_id| registry.reserved_panel_ids.contains(panel_id))
        && registry.sessions.get(&transfer.id).is_some_and(|session| {
            Arc::ptr_eq(&session.operations, &transfer.operations)
                && session.panel_id == transfer.panel_id
        })
}

fn release_terminal_session_transfer(
    state: &TerminalState,
    transfer: TerminalSessionTransfer,
    remove: bool,
) -> Result<(), String> {
    let (mut registry, registry_was_poisoned) = terminal_registry_for_exact_cleanup(state);
    if !terminal_session_transfer_is_owned(&registry, &transfer) {
        return Err("terminal session transfer ownership lost".to_string());
    }
    let removed = if remove {
        let removed = registry.sessions.remove(&transfer.id);
        if let Some(panel_id) = transfer.panel_id.as_ref() {
            registry.materializations.remove(panel_id);
        }
        for operations in &transfer.fenced_operations {
            if !Arc::ptr_eq(operations, &transfer.operations) {
                operations.reopen();
            }
        }
        removed
    } else {
        for operations in &transfer.fenced_operations {
            operations.reopen();
        }
        None
    };
    registry.reserved_session_ids.remove(&transfer.id);
    if let Some(panel_id) = transfer.panel_id {
        registry.reserved_panel_ids.remove(&panel_id);
        if !remove {
            registry.rollback_materialization_fence(&panel_id);
        }
    }
    drop(registry);
    drop(removed);
    if registry_was_poisoned {
        Err("terminal runtime registry mutex poisoned".to_string())
    } else {
        Ok(())
    }
}

pub(crate) fn terminal_remove_id_for_control(state: &TerminalState, id: u32) -> Result<(), String> {
    let Some(transfer) = begin_terminal_session_transfer(state, id)? else {
        return Ok(());
    };
    release_terminal_session_transfer(state, transfer, true)
}

/// Write keystrokes (xterm `onData`) into a session's shell.
#[tauri::command]
pub fn terminal_write(
    state: State<'_, TerminalState>,
    id: u32,
    data: String,
) -> Result<(), String> {
    terminal_write_id_for_control(state.inner(), id, data.as_bytes())
}

fn terminal_write_id_for_control(
    state: &TerminalState,
    id: u32,
    data: &[u8],
) -> Result<(), String> {
    if data.is_empty() {
        return Ok(());
    }
    let (process, input, activation, _operation) = {
        let registry = state.try_runtime_registry()?;
        let session = registry
            .sessions
            .get(&id)
            .ok_or_else(|| format!("unknown terminal session {id}"))?;
        ensure_terminal_session_available(&registry, id, session)?;
        let operation = session
            .operations
            .claim()
            .ok_or_else(|| format!("terminal session {id} is reserved"))?;
        (
            session.pty.clone(),
            session.input.clone(),
            session.pump_activation.clone(),
            operation,
        )
    };
    activation.advance(TerminalPumpReadiness::ConsumerReady);
    accepted_terminal_input(send_terminal_input(process, input, data))
}

pub(crate) fn terminal_write_panel(
    state: &TerminalState,
    panel_id: &str,
    data: &str,
) -> Result<(), String> {
    terminal_write_panel_bytes(state, panel_id, data.as_bytes())
}

pub(crate) fn terminal_write_panel_bytes(
    state: &TerminalState,
    panel_id: &str,
    data: &[u8],
) -> Result<(), String> {
    accepted_terminal_input(terminal_send_panel_bytes_for_control(state, panel_id, data))
}

fn accepted_terminal_input(outcome: TerminalInputOutcome) -> Result<(), String> {
    match outcome {
        TerminalInputOutcome::Sent | TerminalInputOutcome::Queued => Ok(()),
        outcome => Err(format!("terminal input failed: {outcome:?}")),
    }
}

pub(crate) fn terminal_send_panel_bytes_for_control(
    state: &TerminalState,
    panel_id: &str,
    data: &[u8],
) -> TerminalInputOutcome {
    if data.is_empty() {
        return TerminalInputOutcome::Sent;
    }
    let normalized_panel_id = panel_id.trim();
    if normalized_panel_id.is_empty() {
        return TerminalInputOutcome::SurfaceUnavailable;
    }
    let handles = match state.try_runtime_registry() {
        Ok(registry) if !registry.reserved_panel_ids.contains(normalized_panel_id) => registry
            .sessions
            .iter()
            .find(|(_, session)| session.panel_id.as_deref() == Some(normalized_panel_id))
            .and_then(|(id, session)| {
                ensure_terminal_session_available(&registry, *id, session)
                    .ok()
                    .and_then(|_| session.operations.claim())
                    .map(|operation| {
                        (
                            session.pty.clone(),
                            session.input.clone(),
                            session.grid.clone(),
                            session.pump_activation.clone(),
                            operation,
                        )
                    })
            }),
        _ => return TerminalInputOutcome::SurfaceUnavailable,
    };
    let Some((process, input, grid, activation, _operation)) = handles else {
        return TerminalInputOutcome::SurfaceUnavailable;
    };
    let data = match grid.lock() {
        Ok(grid) => terminal_input_bytes_for_grid(&grid, data),
        Err(_) => return TerminalInputOutcome::SurfaceUnavailable,
    };
    activation.advance(TerminalPumpReadiness::ConsumerReady);
    send_terminal_input(process, input, &data)
}

pub(crate) fn terminal_apply_materialization_events_for_control(
    app: &AppHandle,
    state: &TerminalState,
    panel_id: &str,
    events: Vec<TerminalMaterializationEvent>,
) -> TerminalInputOutcome {
    terminal_apply_materialization_events_with(state, panel_id, events, |id, bytes, titles| {
        emit_terminal_output_chunk(app, id, Some(panel_id), bytes, titles)
    })
}

fn terminal_apply_materialization_events_with<Emit>(
    state: &TerminalState,
    panel_id: &str,
    events: Vec<TerminalMaterializationEvent>,
    mut emit_output: Emit,
) -> TerminalInputOutcome
where
    Emit: FnMut(u32, &[u8], &[String]) -> Result<(), String>,
{
    if events.is_empty() {
        return TerminalInputOutcome::Sent;
    }
    let panel_id = panel_id.trim();
    let handles = match state.try_runtime_registry() {
        Ok(registry) if !registry.reserved_panel_ids.contains(panel_id) => registry
            .sessions
            .iter()
            .find(|(_, session)| session.panel_id.as_deref() == Some(panel_id))
            .and_then(|(id, session)| {
                ensure_terminal_session_available(&registry, *id, session)
                    .ok()
                    .and_then(|_| session.operations.claim())
                    .map(|operation| {
                        (
                            *id,
                            session.pty.clone(),
                            session.input.clone(),
                            session.grid.clone(),
                            session.title_parser.clone(),
                            session.pump_activation.clone(),
                            operation,
                        )
                    })
            }),
        _ => return TerminalInputOutcome::SurfaceUnavailable,
    };
    let Some((id, process, input, grid, title_parser, activation, _operation)) = handles else {
        return TerminalInputOutcome::SurfaceUnavailable;
    };
    activation.advance(TerminalPumpReadiness::ConsumerReady);

    let mut queued = false;
    for event in events {
        let outcome = match event {
            TerminalMaterializationEvent::Input(bytes) => match grid
                .lock()
                .map(|grid| terminal_input_bytes_for_grid(&grid, &bytes))
            {
                Ok(bytes) => send_terminal_input(process.clone(), input.clone(), &bytes),
                Err(_) => TerminalInputOutcome::SurfaceUnavailable,
            },
            TerminalMaterializationEvent::ProcessOutput(bytes) => {
                let titles = match grid.lock() {
                    Ok(mut grid) => {
                        grid.advance(&bytes);
                        drop(grid);
                        match title_parser.lock() {
                            Ok(mut parser) => parser.consume(&bytes),
                            Err(_) => return TerminalInputOutcome::SurfaceUnavailable,
                        }
                    }
                    Err(_) => return TerminalInputOutcome::SurfaceUnavailable,
                };
                if emit_output(id, &bytes, &titles).is_err() {
                    return TerminalInputOutcome::SurfaceUnavailable;
                }
                TerminalInputOutcome::Sent
            }
        };
        match outcome {
            TerminalInputOutcome::Sent => {}
            TerminalInputOutcome::Queued => queued = true,
            failure => return failure,
        }
    }
    if queued {
        TerminalInputOutcome::Queued
    } else {
        TerminalInputOutcome::Sent
    }
}

fn terminal_input_bytes_for_grid(grid: &TerminalGrid, data: &[u8]) -> Vec<u8> {
    if !grid.application_cursor_keys_enabled() {
        return data.to_vec();
    }
    let mut encoded = Vec::with_capacity(data.len());
    let mut index = 0;
    while index < data.len() {
        if index + 2 < data.len()
            && data[index] == 0x1b
            && matches!(data[index + 1], b'[' | b'O')
            && matches!(data[index + 2], b'A' | b'B' | b'C' | b'D' | b'H' | b'F')
        {
            encoded.extend_from_slice(&[0x1b, b'O', data[index + 2]]);
            index += 3;
        } else {
            encoded.push(data[index]);
            index += 1;
        }
    }
    encoded
}

fn send_terminal_input(
    process: Arc<Mutex<Box<dyn TerminalProcess>>>,
    input: Arc<TerminalInputTransport>,
    data: &[u8],
) -> TerminalInputOutcome {
    if data.is_empty() {
        return TerminalInputOutcome::Sent;
    }
    let process_exited = match process.lock() {
        Ok(mut process) => match process.try_wait() {
            Ok(status) => status.is_some(),
            Err(_) => return TerminalInputOutcome::SurfaceUnavailable,
        },
        Err(_) => return TerminalInputOutcome::SurfaceUnavailable,
    };
    if process_exited {
        return TerminalInputOutcome::ProcessExited;
    }

    let claim = match input.pending.lock() {
        Ok(mut pending) => {
            if pending.writer_failed || input.writer.is_poisoned() {
                pending.writer_failed = true;
                return TerminalInputOutcome::SurfaceUnavailable;
            }
            pending.claim(data)
        }
        Err(_) => return TerminalInputOutcome::SurfaceUnavailable,
    };
    let (owner, direct, completion) = match claim {
        TerminalInputClaim::Direct(owner) => (owner, true, TerminalInputOutcome::Sent),
        TerminalInputClaim::Recovery(owner, outcome) => (owner, false, outcome),
        TerminalInputClaim::Outcome(outcome) => return outcome,
    };
    let _drain_lease = TerminalDrainLease {
        pending: &input.pending,
        owner,
    };

    let mut writer = match input.writer.lock() {
        Ok(writer) => writer,
        Err(_) => {
            if let Ok(mut pending) = input.pending.lock() {
                pending.fail_owner(owner);
            }
            return TerminalInputOutcome::SurfaceUnavailable;
        }
    };
    if direct {
        if let Err(outcome) = write_terminal_bytes(writer.as_mut(), data) {
            return outcome;
        }
    }

    loop {
        let front = match input.pending.lock() {
            Ok(mut pending) => pending.front_for_drain(owner),
            Err(_) => return completion,
        };
        let Ok(Some((front, offset))) = front else {
            return completion;
        };

        if offset < front.len() {
            let written = match writer.write(&front[offset..]) {
                Ok(0) | Err(_) => {
                    return completion;
                }
                Ok(written) => written,
            };
            let Ok(mut pending) = input.pending.lock() else {
                return completion;
            };
            if !pending.advance_front(owner, &front, offset, written) {
                return completion;
            }
            continue;
        }

        if writer.flush().is_err() {
            return completion;
        }
        let Ok(mut pending) = input.pending.lock() else {
            return completion;
        };
        if !pending.commit_flushed_front(owner, &front) {
            return completion;
        }
    }
}

fn write_terminal_bytes(writer: &mut dyn Write, bytes: &[u8]) -> Result<(), TerminalInputOutcome> {
    writer
        .write_all(bytes)
        .and_then(|_| writer.flush())
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::BrokenPipe {
                TerminalInputOutcome::ProcessExited
            } else {
                TerminalInputOutcome::SurfaceUnavailable
            }
        })
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
        let registry = state.runtime_registry();
        if registry.reserved_panel_ids.contains(normalized_panel_id) {
            return Err(format!("terminal panel {normalized_panel_id} is reserved"));
        }
        registry
            .sessions
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
        let registry = state.runtime_registry();
        if registry.reserved_panel_ids.contains(normalized_panel_id) {
            return Err(format!("terminal panel {normalized_panel_id} is reserved"));
        }
        registry
            .sessions
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
        let registry = state.runtime_registry();
        let mut sessions: Vec<_> = registry
            .sessions
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
    let registry = state.registry.lock().ok()?;
    if registry.reserved_panel_ids.contains(panel_id) {
        return None;
    }
    let grid = registry
        .sessions
        .values()
        .find(|session| session.panel_id.as_deref() == Some(panel_id))?
        .grid
        .clone();
    drop(registry);
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
    terminal_resize_id_for_control(state.inner(), id, cols, rows)
}

fn terminal_resize_id_for_control(
    state: &TerminalState,
    id: u32,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    let (process, grid, activation, _operation) = {
        let registry = state.try_runtime_registry()?;
        let session = registry
            .sessions
            .get(&id)
            .ok_or_else(|| format!("unknown terminal session {id}"))?;
        ensure_terminal_session_available(&registry, id, session)?;
        let operation = session
            .operations
            .claim()
            .ok_or_else(|| format!("terminal session {id} is reserved"))?;
        (
            session.pty.clone(),
            session.grid.clone(),
            session.pump_activation.clone(),
            operation,
        )
    };
    activation.advance(TerminalPumpReadiness::ConsumerReady);
    process
        .lock()
        .map_err(|_| "terminal process mutex poisoned".to_string())?
        .resize(ConPtySize::new(cols.max(1), rows.max(1)))?;
    grid.lock()
        .map_err(|_| "terminal grid mutex poisoned".to_string())?
        .resize(GridSize::new(cols.max(1) as usize, rows.max(1) as usize));
    Ok(())
}

/// Kill a session's shell and drop its PTY. The pump thread observes EOF on the
/// severed output pipe and exits on its own.
#[tauri::command]
pub fn terminal_close(state: State<'_, TerminalState>, id: u32) -> Result<(), String> {
    terminal_close_id_for_control(state.inner(), id)
}

fn terminal_close_id_for_control(state: &TerminalState, id: u32) -> Result<(), String> {
    let Some(transfer) = begin_terminal_session_transfer(state, id)? else {
        return Ok(());
    };
    let kill = match transfer.process.lock() {
        Ok(mut process) => process.kill(),
        Err(_) => Err("terminal process mutex poisoned".to_string()),
    };
    if let Err(error) = kill {
        return match release_terminal_session_transfer(state, transfer, false) {
            Ok(()) => Err(error),
            Err(rollback) => Err(format!("{error}; {rollback}")),
        };
    }
    release_terminal_session_transfer(state, transfer, true)
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
    process_runtime::scan_terminal_listening_ports(
        &app,
        terminal_state.inner(),
        session_state.inner(),
        id,
    )
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
    include!("terminal/tests/materialization.rs");
    include!("terminal/tests/runtime.rs");
}
