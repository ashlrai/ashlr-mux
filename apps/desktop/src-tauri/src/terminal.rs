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

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::io::{Read, Write};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{mpsc, Arc, Condvar, Mutex, MutexGuard, Weak};
use std::time::{Duration, Instant};

use cmux_terminal::conpty::{ConPty, ConPtyCommand, ConPtySize};
use cmux_terminal::engine::{GridSize, TerminalGrid};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::session;

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
    let Some(bytes) = TerminalMaterializationSlot::batch_bytes(&events) else {
        return TerminalMaterializationDemand::InputQueueFull;
    };
    if bytes > TERMINAL_PENDING_INPUT_LIMIT {
        return TerminalMaterializationDemand::InputQueueFull;
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

pub(crate) fn scan_terminal_listening_ports(
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

fn emit_terminal_output_chunk(
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
fn pump_reader(
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
    use std::io::{self, Read, Write};
    use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};
    use std::sync::{mpsc, Arc, Barrier, Mutex, Weak};
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    use super::{
        base64_encode, default_shell_command, descendant_pid_set, ports_for_pid_set,
        reusable_panel_session_id, send_terminal_input, tcp_port_from_owner_pid_row,
        terminal_runtime_snapshot_from_processes, terminal_text, ProcessSnapshotEntry,
        TerminalInputOutcome, TerminalInputTransport, TerminalProcess, TerminalSession,
        TerminalState, TerminalTitleParser, TERMINAL_PENDING_INPUT_LIMIT,
    };
    use cmux_terminal::conpty::ConPtySize;
    use cmux_terminal::engine::{GridSize, TerminalGrid};

    struct TestProcess {
        wait: Result<Option<u32>, String>,
    }

    struct BlockingResizeProcess {
        entered: mpsc::Sender<()>,
        release: mpsc::Receiver<()>,
    }

    impl TerminalProcess for BlockingResizeProcess {
        fn kill(&mut self) -> Result<(), String> {
            Ok(())
        }

        fn resize(&mut self, _size: ConPtySize) -> Result<(), String> {
            self.entered.send(()).unwrap();
            self.release.recv().unwrap();
            Ok(())
        }

        fn try_wait(&mut self) -> Result<Option<u32>, String> {
            Ok(None)
        }
    }

    struct BlockingKillProcess {
        entered: mpsc::Sender<()>,
        release: mpsc::Receiver<()>,
    }

    struct LockCheckingProcess {
        state: Weak<TerminalState>,
        kills: Arc<AtomicUsize>,
    }

    impl TerminalProcess for LockCheckingProcess {
        fn kill(&mut self) -> Result<(), String> {
            let state = self.state.upgrade().expect("test terminal state");
            assert!(
                state.registry.try_lock().is_ok(),
                "kill ran under registry lock"
            );
            self.kills.fetch_add(1, AtomicOrdering::SeqCst);
            Ok(())
        }

        fn resize(&mut self, _size: ConPtySize) -> Result<(), String> {
            Ok(())
        }

        fn try_wait(&mut self) -> Result<Option<u32>, String> {
            let state = self.state.upgrade().expect("test terminal state");
            assert!(
                state.registry.try_lock().is_ok(),
                "process check ran under registry lock"
            );
            Ok(None)
        }
    }

    struct LockCheckingWriter {
        state: Weak<TerminalState>,
        captured: Arc<Mutex<Vec<u8>>>,
    }

    impl Write for LockCheckingWriter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            let state = self.state.upgrade().expect("test terminal state");
            assert!(
                state.registry.try_lock().is_ok(),
                "PTY write ran under registry lock"
            );
            self.captured.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl TerminalProcess for BlockingKillProcess {
        fn kill(&mut self) -> Result<(), String> {
            self.entered.send(()).unwrap();
            self.release.recv().unwrap();
            Ok(())
        }

        fn resize(&mut self, _size: ConPtySize) -> Result<(), String> {
            Ok(())
        }

        fn try_wait(&mut self) -> Result<Option<u32>, String> {
            Ok(None)
        }
    }

    struct FailingKillProcess;

    impl TerminalProcess for FailingKillProcess {
        fn kill(&mut self) -> Result<(), String> {
            Err("injected kill failure".to_string())
        }

        fn resize(&mut self, _size: ConPtySize) -> Result<(), String> {
            Ok(())
        }

        fn try_wait(&mut self) -> Result<Option<u32>, String> {
            Ok(None)
        }
    }

    struct PoisonRegistryDuringKillProcess {
        state: Weak<TerminalState>,
    }

    impl TerminalProcess for PoisonRegistryDuringKillProcess {
        fn kill(&mut self) -> Result<(), String> {
            let state = self.state.upgrade().expect("terminal state remains live");
            let poisoned = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let _guard = state.registry.lock().unwrap();
                panic!("poison registry after successful kill");
            }));
            assert!(poisoned.is_err());
            Ok(())
        }

        fn resize(&mut self, _size: ConPtySize) -> Result<(), String> {
            Ok(())
        }

        fn try_wait(&mut self) -> Result<Option<u32>, String> {
            Ok(None)
        }
    }

    impl TerminalProcess for TestProcess {
        fn kill(&mut self) -> Result<(), String> {
            Ok(())
        }

        fn resize(&mut self, _size: ConPtySize) -> Result<(), String> {
            Ok(())
        }

        fn try_wait(&mut self) -> Result<Option<u32>, String> {
            self.wait.clone()
        }
    }

    struct CapturingWriter(Arc<Mutex<Vec<u8>>>);

    impl Write for CapturingWriter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    struct ErrorWriter(io::ErrorKind);

    impl Write for ErrorWriter {
        fn write(&mut self, _bytes: &[u8]) -> io::Result<usize> {
            Err(io::Error::from(self.0))
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    struct BlockingWriter {
        entered: mpsc::Sender<()>,
        release: mpsc::Receiver<()>,
        captured: Arc<Mutex<Vec<u8>>>,
        block_once: bool,
    }

    impl Write for BlockingWriter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if self.block_once {
                self.block_once = false;
                self.entered.send(()).unwrap();
                self.release.recv().unwrap();
            }
            self.captured.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    struct FailSecondWriteOnce {
        entered: mpsc::Sender<()>,
        release: mpsc::Receiver<()>,
        captured: Arc<Mutex<Vec<u8>>>,
        writes: usize,
    }

    struct PartialQueuedFailureOnce {
        entered: mpsc::Sender<()>,
        release: mpsc::Receiver<()>,
        captured: Arc<Mutex<Vec<u8>>>,
        writes: usize,
    }

    struct PanicWriter {
        entered: mpsc::Sender<()>,
        release: mpsc::Receiver<()>,
    }

    struct BlockingNthWaitProcess {
        calls: usize,
        block_on: usize,
        entered: mpsc::Sender<()>,
        release: mpsc::Receiver<()>,
    }

    impl TerminalProcess for BlockingNthWaitProcess {
        fn kill(&mut self) -> Result<(), String> {
            Ok(())
        }

        fn resize(&mut self, _size: ConPtySize) -> Result<(), String> {
            Ok(())
        }

        fn try_wait(&mut self) -> Result<Option<u32>, String> {
            self.calls += 1;
            if self.calls == self.block_on {
                self.entered.send(()).unwrap();
                self.release.recv().unwrap();
            }
            Ok(None)
        }
    }

    impl Write for PanicWriter {
        fn write(&mut self, _bytes: &[u8]) -> io::Result<usize> {
            self.entered.send(()).unwrap();
            self.release.recv().unwrap();
            panic!("injected active-writer panic");
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl Write for PartialQueuedFailureOnce {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.writes += 1;
            if self.writes == 1 {
                self.entered.send(()).unwrap();
                self.release.recv().unwrap();
            } else if self.writes == 2 {
                let written = bytes.len().min(3);
                self.captured
                    .lock()
                    .unwrap()
                    .extend_from_slice(&bytes[..written]);
                return Ok(written);
            } else if self.writes == 3 {
                return Err(io::Error::other("injected failure after partial write"));
            }
            self.captured.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl Write for FailSecondWriteOnce {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.writes += 1;
            if self.writes == 1 {
                self.entered.send(()).unwrap();
                self.release.recv().unwrap();
            } else if self.writes == 2 {
                return Err(io::Error::other("injected queued-write failure"));
            }
            self.captured.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    fn test_process(exited: bool) -> Arc<Mutex<Box<dyn TerminalProcess>>> {
        Arc::new(Mutex::new(Box::new(TestProcess {
            wait: Ok(exited.then_some(0)),
        })))
    }

    fn test_transport(writer: impl Write + Send + 'static) -> Arc<TerminalInputTransport> {
        Arc::new(TerminalInputTransport::new(Box::new(writer)))
    }

    fn test_session(
        process: Arc<Mutex<Box<dyn TerminalProcess>>>,
        input: Arc<TerminalInputTransport>,
        panel_id: &str,
    ) -> TerminalSession {
        TerminalSession {
            pty: process,
            input,
            grid: Arc::new(Mutex::new(TerminalGrid::new(GridSize::new(80, 24)))),
            title_parser: Arc::new(Mutex::new(TerminalTitleParser::default())),
            operations: Arc::new(super::TerminalOperationGate::default()),
            pump_activation: super::TerminalPumpActivation::active(),
            panel_id: Some(panel_id.to_string()),
            root_pid: None,
        }
    }

    fn pending_test_session(
        process: Arc<Mutex<Box<dyn TerminalProcess>>>,
        input: Arc<TerminalInputTransport>,
        panel_id: &str,
    ) -> (TerminalSession, Arc<super::TerminalPumpActivation>) {
        let mut session = test_session(process, input, panel_id);
        let (activation, _ready) = super::TerminalPumpActivation::pending();
        session.pump_activation = activation.clone();
        (session, activation)
    }

    fn pump_is_pending(activation: &super::TerminalPumpActivation) -> bool {
        activation.sender.lock().unwrap().is_some()
    }

    fn redesign_spec(cwd: &str) -> super::TerminalMaterializationSpec {
        super::TerminalMaterializationSpec::new(
            Some(cwd.to_string()),
            Some("echo ready".to_string()),
            b"initial".to_vec(),
            BTreeMap::from([("CMUX_TEST".to_string(), "1".to_string())]),
        )
    }

    fn redesign_start(
        state: &TerminalState,
        panel_id: &str,
        spec: &super::TerminalMaterializationSpec,
        event: super::TerminalMaterializationEvent,
    ) -> super::TerminalMaterializationLease {
        match super::request_terminal_materialization(state, panel_id, spec, vec![event]) {
            super::TerminalMaterializationDemand::Start(lease) => lease,
            _ => panic!("first cold demand must own the sole start lease"),
        }
    }

    #[test]
    fn cold_materialization_redesign_empty_and_budget_are_atomic() {
        let state = TerminalState::default();
        let spec = redesign_spec("C:/repo");
        assert_eq!(
            super::request_terminal_materialization(&state, "   ", &spec, Vec::new()),
            super::TerminalMaterializationDemand::Noop
        );
        assert_eq!(
            super::request_terminal_materialization(
                &state,
                "panel-empty",
                &spec,
                vec![
                    super::TerminalMaterializationEvent::Input(Vec::new()),
                    super::TerminalMaterializationEvent::ProcessOutput(Vec::new()),
                ],
            ),
            super::TerminalMaterializationDemand::Noop
        );
        assert_eq!(
            state
                .registry
                .lock()
                .unwrap()
                .next_materialization_generation,
            0
        );

        let exact = vec![
            super::TerminalMaterializationEvent::Input(vec![
                b'i';
                TERMINAL_PENDING_INPUT_LIMIT - 4
            ]),
            super::TerminalMaterializationEvent::ProcessOutput(b"out!".to_vec()),
        ];
        let lease = match super::request_terminal_materialization(
            &state,
            "panel-budget",
            &spec,
            exact.clone(),
        ) {
            super::TerminalMaterializationDemand::Start(lease) => lease,
            _ => panic!("an exact-budget batch must start"),
        };
        assert_eq!(
            super::request_terminal_materialization(
                &state,
                "panel-budget",
                &spec,
                vec![super::TerminalMaterializationEvent::Input(vec![b'x'])],
            ),
            super::TerminalMaterializationDemand::InputQueueFull
        );
        assert_eq!(
            super::cancel_terminal_materialization(&state, &lease).unwrap(),
            Some(exact)
        );
        assert_eq!(
            super::request_terminal_materialization(
                &state,
                "panel-oversized",
                &spec,
                vec![super::TerminalMaterializationEvent::ProcessOutput(vec![
                    b'x';
                    TERMINAL_PENDING_INPUT_LIMIT + 1
                ])],
            ),
            super::TerminalMaterializationDemand::InputQueueFull
        );
    }

    #[test]
    fn cold_materialization_redesign_fifo_publication_and_live_handoff() {
        let state = TerminalState::default();
        let spec = redesign_spec("C:/repo");
        let first = super::TerminalMaterializationEvent::Input(b"first".to_vec());
        let second = super::TerminalMaterializationEvent::ProcessOutput(b"second".to_vec());
        let lease = redesign_start(&state, " panel-a ", &spec, first.clone());
        assert_eq!(
            super::request_terminal_materialization(&state, "panel-a", &spec, vec![second.clone()],),
            super::TerminalMaterializationDemand::Queued
        );
        assert_eq!(
            super::request_terminal_materialization(
                &state,
                "panel-a",
                &redesign_spec("C:/other"),
                vec![super::TerminalMaterializationEvent::Input(
                    b"wrong".to_vec()
                )],
            ),
            super::TerminalMaterializationDemand::SurfaceUnavailable
        );
        super::publish_terminal_materialization_runtime(
            &state,
            &lease,
            7,
            test_session(
                test_process(false),
                test_transport(CapturingWriter(Arc::new(Mutex::new(Vec::new())))),
                "panel-a",
            ),
        )
        .unwrap();
        for expected in [first, second] {
            assert_eq!(
                super::drain_terminal_materialization_event(&state, &lease).unwrap(),
                Some(expected)
            );
        }
        assert_eq!(
            super::drain_terminal_materialization_event(&state, &lease).unwrap(),
            None
        );
        let live = super::TerminalMaterializationEvent::Input(b"live".to_vec());
        assert_eq!(
            super::request_terminal_materialization(
                &state,
                "panel-a",
                &redesign_spec("C:/live"),
                vec![live.clone()],
            ),
            super::TerminalMaterializationDemand::Live(vec![live])
        );
    }

    #[test]
    fn cold_materialization_redesign_concurrency_and_poison_are_classified() {
        let state = Arc::new(TerminalState::default());
        let barrier = Arc::new(Barrier::new(3));
        let mut workers = Vec::new();
        for byte in *b"ab" {
            let state = state.clone();
            let barrier = barrier.clone();
            workers.push(std::thread::spawn(move || {
                barrier.wait();
                super::request_terminal_materialization(
                    &state,
                    "panel-race",
                    &redesign_spec("C:/repo"),
                    vec![super::TerminalMaterializationEvent::Input(vec![byte])],
                )
            }));
        }
        barrier.wait();
        let outcomes = workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            outcomes
                .iter()
                .filter(|outcome| matches!(outcome, super::TerminalMaterializationDemand::Start(_)))
                .count(),
            1
        );
        assert_eq!(
            outcomes
                .iter()
                .filter(|outcome| matches!(outcome, super::TerminalMaterializationDemand::Queued))
                .count(),
            1
        );

        let poisoned = Arc::new(TerminalState::default());
        let target = poisoned.clone();
        assert!(std::thread::spawn(move || {
            let _guard = target.registry.lock().unwrap();
            panic!("poison redesign registry");
        })
        .join()
        .is_err());
        assert_eq!(
            super::request_terminal_materialization(
                &poisoned,
                "panel-poisoned",
                &redesign_spec("C:/repo"),
                vec![super::TerminalMaterializationEvent::Input(
                    b"input".to_vec()
                )],
            ),
            super::TerminalMaterializationDemand::SurfaceUnavailable
        );
    }

    #[test]
    fn cold_materialization_redesign_full_failure_retries_without_append() {
        let state = TerminalState::default();
        let spec = redesign_spec("C:/repo");
        let retained =
            super::TerminalMaterializationEvent::Input(vec![b'x'; TERMINAL_PENDING_INPUT_LIMIT]);
        let first = redesign_start(&state, "panel-full", &spec, retained.clone());
        assert!(super::fail_terminal_materialization_start(&state, &first).unwrap());
        assert!(!super::owns_terminal_materialization_start(&state, &first).unwrap());
        let retry = super::retry_terminal_materialization_start(&state, "panel-full", &spec)
            .unwrap()
            .expect("full retained FIFO must retry without another byte");
        assert_ne!(first.generation(), retry.generation());
        assert_eq!(
            super::cancel_terminal_materialization(&state, &retry).unwrap(),
            Some(vec![retained])
        );
    }

    #[test]
    fn cold_materialization_redesign_starting_fence_invalidates_spawn() {
        let state = TerminalState::default();
        let spec = redesign_spec("C:/repo");
        let first = super::TerminalMaterializationEvent::Input(b"first".to_vec());
        let second = super::TerminalMaterializationEvent::ProcessOutput(b"second".to_vec());
        let stale = redesign_start(&state, "panel-starting-fence", &spec, first.clone());
        let panels = ["panel-starting-fence".to_string()].into_iter().collect();
        let lifecycle = super::detach_terminal_panels_for_control(&state, &panels).unwrap();
        assert!(!super::owns_terminal_materialization_start(&state, &stale).unwrap());
        assert_eq!(
            super::request_terminal_materialization(
                &state,
                "panel-starting-fence",
                &spec,
                vec![second.clone()],
            ),
            super::TerminalMaterializationDemand::Queued
        );
        assert!(
            super::retry_terminal_materialization_start(&state, "panel-starting-fence", &spec,)
                .unwrap()
                .is_none()
        );
        super::rollback_terminal_panels_for_control(&state, lifecycle)
            .map_err(|error| error.message)
            .unwrap();
        assert!(!super::owns_terminal_materialization_start(&state, &stale).unwrap());
        let retry =
            super::retry_terminal_materialization_start(&state, "panel-starting-fence", &spec)
                .unwrap()
                .expect("rollback restores dormant retry authority");
        assert_ne!(stale.generation(), retry.generation());
        assert_eq!(
            super::cancel_terminal_materialization(&state, &retry).unwrap(),
            Some(vec![first, second])
        );

        let final_state = TerminalState::default();
        let final_lease = redesign_start(
            &final_state,
            "panel-starting-finalize",
            &spec,
            super::TerminalMaterializationEvent::Input(b"discard".to_vec()),
        );
        let panels = ["panel-starting-finalize".to_string()]
            .into_iter()
            .collect();
        let lifecycle = super::detach_terminal_panels_for_control(&final_state, &panels).unwrap();
        assert!(super::finalize_terminal_panels_for_control(&final_state, lifecycle).is_ok());
        assert!(
            super::cancel_terminal_materialization(&final_state, &final_lease)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn cold_materialization_redesign_cancel_rejects_fenced_starting() {
        let state = TerminalState::default();
        let spec = redesign_spec("C:/repo");
        let first = super::TerminalMaterializationEvent::Input(b"first".to_vec());
        let stale = redesign_start(&state, "panel-cancel-fenced-start", &spec, first.clone());
        let panels = ["panel-cancel-fenced-start".to_string()]
            .into_iter()
            .collect();
        let lifecycle = super::detach_terminal_panels_for_control(&state, &panels).unwrap();

        assert!(super::cancel_terminal_materialization(&state, &stale)
            .unwrap()
            .is_none());
        super::rollback_terminal_panels_for_control(&state, lifecycle)
            .map_err(|error| error.message)
            .unwrap();
        let retry =
            super::retry_terminal_materialization_start(&state, "panel-cancel-fenced-start", &spec)
                .unwrap()
                .expect("rollback must retain the FIFO under fresh start authority");
        assert_eq!(
            super::cancel_terminal_materialization(&state, &retry).unwrap(),
            Some(vec![first])
        );
    }

    #[test]
    fn cold_materialization_redesign_cancel_rejects_published() {
        let state = TerminalState::default();
        let spec = redesign_spec("C:/repo");
        let first = super::TerminalMaterializationEvent::Input(b"first".to_vec());
        let lease = redesign_start(&state, "panel-cancel-published", &spec, first.clone());
        super::publish_terminal_materialization_runtime(
            &state,
            &lease,
            81,
            test_session(
                test_process(false),
                test_transport(CapturingWriter(Arc::new(Mutex::new(Vec::new())))),
                "panel-cancel-published",
            ),
        )
        .unwrap();

        assert!(super::cancel_terminal_materialization(&state, &lease)
            .unwrap()
            .is_none());
        assert_eq!(
            super::drain_terminal_materialization_event(&state, &lease).unwrap(),
            Some(first)
        );
    }

    #[test]
    fn cold_materialization_redesign_cancel_rejects_fenced_published() {
        let state = TerminalState::default();
        let spec = redesign_spec("C:/repo");
        let first = super::TerminalMaterializationEvent::Input(b"first".to_vec());
        let lease = redesign_start(
            &state,
            "panel-cancel-fenced-published",
            &spec,
            first.clone(),
        );
        super::publish_terminal_materialization_runtime(
            &state,
            &lease,
            82,
            test_session(
                test_process(false),
                test_transport(CapturingWriter(Arc::new(Mutex::new(Vec::new())))),
                "panel-cancel-fenced-published",
            ),
        )
        .unwrap();
        let transfer = super::begin_terminal_session_transfer(&state, 82)
            .unwrap()
            .unwrap();

        assert!(super::cancel_terminal_materialization(&state, &lease)
            .unwrap()
            .is_none());
        super::release_terminal_session_transfer(&state, transfer, false).unwrap();
        assert_eq!(
            super::drain_terminal_materialization_event(&state, &lease).unwrap(),
            Some(first)
        );
    }

    #[test]
    fn cold_materialization_redesign_detached_published_fence_is_append_only() {
        let state = TerminalState::default();
        let spec = redesign_spec("C:/repo");
        let first = super::TerminalMaterializationEvent::Input(b"first".to_vec());
        let second = super::TerminalMaterializationEvent::ProcessOutput(b"second".to_vec());
        let lease = redesign_start(&state, "panel-detached", &spec, first.clone());
        super::publish_terminal_materialization_runtime(
            &state,
            &lease,
            8,
            test_session(
                test_process(false),
                test_transport(CapturingWriter(Arc::new(Mutex::new(Vec::new())))),
                "panel-detached",
            ),
        )
        .unwrap();
        let panels = ["panel-detached".to_string()].into_iter().collect();
        let lifecycle = super::detach_terminal_panels_for_control(&state, &panels).unwrap();
        assert_eq!(
            super::request_terminal_materialization(
                &state,
                "panel-detached",
                &spec,
                vec![second.clone()],
            ),
            super::TerminalMaterializationDemand::Queued
        );
        assert!(super::drain_terminal_materialization_event(&state, &lease).is_err());
        super::rollback_terminal_panels_for_control(&state, lifecycle)
            .map_err(|error| error.message)
            .unwrap();
        for expected in [first, second] {
            assert_eq!(
                super::drain_terminal_materialization_event(&state, &lease).unwrap(),
                Some(expected)
            );
        }
    }

    #[test]
    fn cold_materialization_redesign_direct_transfer_fences_drain() {
        let state = TerminalState::default();
        let spec = redesign_spec("C:/repo");
        let first = super::TerminalMaterializationEvent::Input(b"first".to_vec());
        let second = super::TerminalMaterializationEvent::ProcessOutput(b"second".to_vec());
        let lease = redesign_start(&state, "panel-direct", &spec, first.clone());
        super::publish_terminal_materialization_runtime(
            &state,
            &lease,
            9,
            test_session(
                test_process(false),
                test_transport(CapturingWriter(Arc::new(Mutex::new(Vec::new())))),
                "panel-direct",
            ),
        )
        .unwrap();
        let transfer = super::begin_terminal_session_transfer(&state, 9)
            .unwrap()
            .unwrap();
        assert_eq!(
            super::request_terminal_materialization(
                &state,
                "panel-direct",
                &spec,
                vec![second.clone()],
            ),
            super::TerminalMaterializationDemand::Queued
        );
        assert!(super::drain_terminal_materialization_event(&state, &lease).is_err());
        super::release_terminal_session_transfer(&state, transfer, false).unwrap();
        for expected in [first, second] {
            assert_eq!(
                super::drain_terminal_materialization_event(&state, &lease).unwrap(),
                Some(expected)
            );
        }

        let remove_state = TerminalState::default();
        let remove_lease = redesign_start(
            &remove_state,
            "panel-direct-remove",
            &spec,
            super::TerminalMaterializationEvent::Input(b"discard".to_vec()),
        );
        super::publish_terminal_materialization_runtime(
            &remove_state,
            &remove_lease,
            10,
            test_session(
                test_process(false),
                test_transport(CapturingWriter(Arc::new(Mutex::new(Vec::new())))),
                "panel-direct-remove",
            ),
        )
        .unwrap();
        let transfer = super::begin_terminal_session_transfer(&remove_state, 10)
            .unwrap()
            .unwrap();
        super::release_terminal_session_transfer(&remove_state, transfer, true).unwrap();
        assert!(super::drain_terminal_materialization_event(&remove_state, &remove_lease).is_err());
    }

    #[test]
    fn cold_materialization_redesign_open_and_materialization_are_exclusive() {
        let state = TerminalState::default();
        let spec = redesign_spec("C:/repo");
        let stale = redesign_start(
            &state,
            "panel-exclusive",
            &spec,
            super::TerminalMaterializationEvent::Input(b"retained".to_vec()),
        );
        assert!(
            super::reserve_terminal_open_for_control(&state, Some("panel-exclusive"), false,)
                .is_err()
        );
        let panels = ["panel-exclusive".to_string()].into_iter().collect();
        let lifecycle = super::detach_terminal_panels_for_control(&state, &panels).unwrap();
        let publication = super::publish_terminal_materialization_runtime(
            &state,
            &stale,
            11,
            test_session(
                test_process(false),
                test_transport(CapturingWriter(Arc::new(Mutex::new(Vec::new())))),
                "panel-exclusive",
            ),
        )
        .unwrap_err();
        super::rollback_terminal_panels_for_control(&state, lifecycle)
            .map_err(|error| error.message)
            .unwrap();
        let retry = super::retry_terminal_materialization_start(&state, "panel-exclusive", &spec)
            .unwrap()
            .unwrap();
        super::publish_terminal_materialization_runtime(&state, &retry, 11, publication.session)
            .unwrap();

        let reserved = TerminalState::default();
        let reservation = match super::reserve_terminal_open_for_control(
            &reserved,
            Some("panel-reserved-first"),
            false,
        )
        .unwrap()
        {
            super::TerminalOpenReservation::Reserved(reservation) => reservation,
            super::TerminalOpenReservation::Existing(_) => panic!("unexpected existing runtime"),
        };
        assert_eq!(
            super::request_terminal_materialization(
                &reserved,
                "panel-reserved-first",
                &spec,
                vec![super::TerminalMaterializationEvent::Input(
                    b"blocked".to_vec()
                )],
            ),
            super::TerminalMaterializationDemand::SurfaceUnavailable
        );
        super::rollback_terminal_open_reservation_for_control(&reserved, reservation).unwrap();
    }

    #[test]
    fn conpty_materialization_rechecks_lifecycle_and_retries_spawn_failure() {
        let state = Arc::new(TerminalState::default());
        let spec = redesign_spec("C:/repo");
        let first = redesign_start(
            &state,
            "panel-recheck",
            &spec,
            super::TerminalMaterializationEvent::Input(b"retained".to_vec()),
        );
        let panels = ["panel-recheck".to_string()].into_iter().collect();
        let lifecycle = super::detach_terminal_panels_for_control(&state, &panels).unwrap();
        let spawned = Arc::new(AtomicUsize::new(0));
        let spawn_counter = spawned.clone();
        let error = super::fulfill_terminal_materialization_with(
            &state,
            first,
            ConPtySize::new(80, 24),
            move |_, _, _, _| {
                spawn_counter.fetch_add(1, AtomicOrdering::SeqCst);
                panic!("a fenced start must not invoke the external factory")
            },
            |_, _, _| Ok(()),
        )
        .unwrap_err();
        assert!(error.contains("ownership"));
        assert_eq!(spawned.load(AtomicOrdering::SeqCst), 0);

        super::rollback_terminal_panels_for_control(&state, lifecycle)
            .map_err(|error| error.message)
            .unwrap();
        let retry = super::retry_terminal_materialization_start(&state, "panel-recheck", &spec)
            .unwrap()
            .expect("rollback exposes a fresh start owner");
        let failed_generation = retry.generation();
        let error = super::fulfill_terminal_materialization_with(
            &state,
            retry,
            ConPtySize::new(80, 24),
            |_, _, _, _| Err("injected ConPTY spawn failure".to_string()),
            |_, _, _| Ok(()),
        )
        .unwrap_err();
        assert!(error.contains("injected ConPTY spawn failure"));
        let retry = super::retry_terminal_materialization_start(&state, "panel-recheck", &spec)
            .unwrap()
            .expect("spawn failure retains the FIFO for retry");
        assert_ne!(retry.generation(), failed_generation);
        assert_eq!(
            super::cancel_terminal_materialization(&state, &retry).unwrap(),
            Some(vec![super::TerminalMaterializationEvent::Input(
                b"retained".to_vec()
            )])
        );
    }

    #[test]
    fn conpty_materialization_publication_loss_kills_exact_runtime_and_retains_fifo() {
        let state = Arc::new(TerminalState::default());
        let spec = redesign_spec("C:/repo");
        let first = super::TerminalMaterializationEvent::Input(b"first".to_vec());
        let second = super::TerminalMaterializationEvent::ProcessOutput(b"second".to_vec());
        let lease = redesign_start(&state, "panel-stale-publish", &spec, first.clone());
        let lifecycle = Arc::new(Mutex::new(None));
        let saved_lifecycle = lifecycle.clone();
        let spawn_state = state.clone();
        let kills = Arc::new(AtomicUsize::new(0));
        let process_kills = kills.clone();
        let error = super::fulfill_terminal_materialization_with(
            &state,
            lease,
            ConPtySize::new(80, 24),
            move |_, panel_id, _, _| {
                assert!(
                    spawn_state.registry.try_lock().is_ok(),
                    "factory ran under registry lock"
                );
                let panels = [panel_id.to_string()].into_iter().collect();
                *saved_lifecycle.lock().unwrap() =
                    Some(super::detach_terminal_panels_for_control(&spawn_state, &panels).unwrap());
                Ok(test_session(
                    Arc::new(Mutex::new(Box::new(LockCheckingProcess {
                        state: Arc::downgrade(&spawn_state),
                        kills: process_kills,
                    }))),
                    test_transport(CapturingWriter(Arc::new(Mutex::new(Vec::new())))),
                    panel_id,
                ))
            },
            |_, _, _| panic!("stale publication must not drain its FIFO"),
        )
        .unwrap_err();
        assert!(error.contains("publication"));
        assert_eq!(kills.load(AtomicOrdering::SeqCst), 1);
        assert_eq!(
            super::request_terminal_materialization(
                &state,
                "panel-stale-publish",
                &spec,
                vec![second.clone()],
            ),
            super::TerminalMaterializationDemand::Queued
        );
        super::rollback_terminal_panels_for_control(
            &state,
            lifecycle.lock().unwrap().take().unwrap(),
        )
        .map_err(|error| error.message)
        .unwrap();
        let retry =
            super::retry_terminal_materialization_start(&state, "panel-stale-publish", &spec)
                .unwrap()
                .expect("publication loss retains a retryable FIFO");
        assert_eq!(
            super::cancel_terminal_materialization(&state, &retry).unwrap(),
            Some(vec![first, second])
        );
    }

    #[test]
    fn conpty_materialization_flushes_fifo_parser_and_concurrent_append_before_activation() {
        let state = Arc::new(TerminalState::default());
        let spec = redesign_spec("C:/repo");
        let lease = redesign_start(
            &state,
            "panel-fifo",
            &spec,
            super::TerminalMaterializationEvent::Input(b"one".to_vec()),
        );
        assert_eq!(
            super::request_terminal_materialization(
                &state,
                "panel-fifo",
                &spec,
                vec![super::TerminalMaterializationEvent::ProcessOutput(
                    b"\x1b]2;cold-title\x07visible".to_vec(),
                )],
            ),
            super::TerminalMaterializationDemand::Queued
        );
        let captured = Arc::new(Mutex::new(Vec::new()));
        let process_kills = Arc::new(AtomicUsize::new(0));
        let activation = Arc::new(Mutex::new(None));
        let spawn_activation = activation.clone();
        let spawn_state = state.clone();
        let writer_bytes = captured.clone();
        let output_state = state.clone();
        let output_spec = spec.clone();
        let id = super::fulfill_terminal_materialization_with(
            &state,
            lease,
            ConPtySize::new(80, 24),
            move |_, panel_id, _, _| {
                assert!(spawn_state.registry.try_lock().is_ok());
                let (session, pending) = pending_test_session(
                    Arc::new(Mutex::new(Box::new(LockCheckingProcess {
                        state: Arc::downgrade(&spawn_state),
                        kills: process_kills,
                    }))),
                    test_transport(LockCheckingWriter {
                        state: Arc::downgrade(&spawn_state),
                        captured: writer_bytes,
                    }),
                    panel_id,
                );
                *spawn_activation.lock().unwrap() = Some(pending);
                Ok(session)
            },
            move |id, bytes, titles| {
                assert!(output_state.registry.try_lock().is_ok());
                let (grid, pending) = {
                    let registry = output_state.runtime_registry();
                    let session = registry.sessions.get(&id).unwrap();
                    (session.grid.clone(), session.pump_activation.clone())
                };
                assert!(pump_is_pending(&pending));
                assert!(terminal_text(&grid.lock().unwrap(), false, None).contains("visible"));
                assert_eq!(bytes, b"\x1b]2;cold-title\x07visible");
                assert_eq!(titles, ["cold-title".to_string()]);
                assert_eq!(
                    super::request_terminal_materialization(
                        &output_state,
                        "panel-fifo",
                        &output_spec,
                        vec![super::TerminalMaterializationEvent::Input(
                            b"three".to_vec()
                        )],
                    ),
                    super::TerminalMaterializationDemand::Queued
                );
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(&*captured.lock().unwrap(), b"onethree");
        assert!(!pump_is_pending(
            activation.lock().unwrap().as_ref().unwrap()
        ));
        assert_eq!(
            super::request_terminal_materialization(
                &state,
                "panel-fifo",
                &spec,
                vec![super::TerminalMaterializationEvent::Input(b"live".to_vec())],
            ),
            super::TerminalMaterializationDemand::Live(vec![
                super::TerminalMaterializationEvent::Input(b"live".to_vec())
            ])
        );
        assert!(state.runtime_registry().sessions.contains_key(&id));
    }

    #[test]
    fn conpty_materialization_lifecycle_waits_and_rollback_resumes_without_spawn() {
        let state = Arc::new(TerminalState::default());
        let spec = redesign_spec("C:/repo");
        let lease = redesign_start(
            &state,
            "panel-fenced-flush",
            &spec,
            super::TerminalMaterializationEvent::ProcessOutput(b"claimed".to_vec()),
        );
        assert_eq!(
            super::request_terminal_materialization(
                &state,
                "panel-fenced-flush",
                &spec,
                vec![super::TerminalMaterializationEvent::Input(
                    b"retained".to_vec()
                )],
            ),
            super::TerminalMaterializationDemand::Queued
        );
        let captured = Arc::new(Mutex::new(Vec::new()));
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let fulfill_state = state.clone();
        let fulfill_captured = captured.clone();
        let fulfill = std::thread::spawn(move || {
            super::fulfill_terminal_materialization_with(
                &fulfill_state,
                lease,
                ConPtySize::new(80, 24),
                move |_, panel_id, _, _| {
                    Ok(test_session(
                        test_process(false),
                        test_transport(CapturingWriter(fulfill_captured)),
                        panel_id,
                    ))
                },
                move |_, _, _| {
                    entered_tx.send(()).unwrap();
                    release_rx.recv().unwrap();
                    Ok(())
                },
            )
        });
        entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();

        let detach_state = state.clone();
        let (detached_tx, detached_rx) = mpsc::channel();
        let detach = std::thread::spawn(move || {
            let panels = ["panel-fenced-flush".to_string()].into_iter().collect();
            let result = super::detach_terminal_panels_for_control(&detach_state, &panels);
            detached_tx.send(()).unwrap();
            result
        });
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if state
                .runtime_registry()
                .reserved_panel_ids
                .contains("panel-fenced-flush")
            {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "lifecycle fence was not established"
            );
            std::thread::yield_now();
        }
        assert!(
            detached_rx.try_recv().is_err(),
            "lifecycle must wait for claimed apply"
        );
        release_tx.send(()).unwrap();
        assert!(fulfill.join().unwrap().is_err());
        detached_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        let lifecycle = detach.join().unwrap().unwrap();
        assert!(
            captured.lock().unwrap().is_empty(),
            "fence prevented the next FIFO pop"
        );
        super::rollback_terminal_panels_for_control(&state, lifecycle)
            .map_err(|error| error.message)
            .unwrap();
        let resume =
            super::retry_terminal_materialization_flush(&state, "panel-fenced-flush", &spec)
                .unwrap()
                .expect("rollback exposes the published FIFO for resumption");
        super::fulfill_terminal_materialization_with(
            &state,
            resume,
            ConPtySize::new(80, 24),
            |_, _, _, _| panic!("published resume must not spawn a second ConPTY"),
            |_, _, _| Ok(()),
        )
        .unwrap();
        assert_eq!(&*captured.lock().unwrap(), b"retained");
    }

    #[test]
    fn conpty_materialization_apply_failure_discards_batch_without_stranding_runtime() {
        let state = Arc::new(TerminalState::default());
        let spec = redesign_spec("C:/repo");
        let lease = redesign_start(
            &state,
            "panel-apply-failure",
            &spec,
            super::TerminalMaterializationEvent::ProcessOutput(b"fails".to_vec()),
        );
        assert_eq!(
            super::request_terminal_materialization(
                &state,
                "panel-apply-failure",
                &spec,
                vec![super::TerminalMaterializationEvent::Input(
                    b"discarded".to_vec()
                )],
            ),
            super::TerminalMaterializationDemand::Queued
        );
        let activation = Arc::new(Mutex::new(None));
        let saved_activation = activation.clone();
        let error = super::fulfill_terminal_materialization_with(
            &state,
            lease,
            ConPtySize::new(80, 24),
            move |_, panel_id, _, _| {
                let (session, pending) = pending_test_session(
                    test_process(false),
                    test_transport(CapturingWriter(Arc::new(Mutex::new(Vec::new())))),
                    panel_id,
                );
                *saved_activation.lock().unwrap() = Some(pending);
                Ok(session)
            },
            |_, _, _| Err("injected output apply failure".to_string()),
        )
        .unwrap_err();
        assert!(error.contains("injected output apply failure"));
        assert!(!pump_is_pending(
            activation.lock().unwrap().as_ref().unwrap()
        ));
        assert_eq!(
            super::request_terminal_materialization(
                &state,
                "panel-apply-failure",
                &spec,
                vec![super::TerminalMaterializationEvent::Input(
                    b"later".to_vec()
                )],
            ),
            super::TerminalMaterializationDemand::Live(vec![
                super::TerminalMaterializationEvent::Input(b"later".to_vec())
            ])
        );
    }

    #[test]
    fn conpty_materialization_apply_failure_recovers_poisoned_registry() {
        let state = Arc::new(TerminalState::default());
        let spec = redesign_spec("C:/repo");
        let lease = redesign_start(
            &state,
            "panel-poisoned-abort",
            &spec,
            super::TerminalMaterializationEvent::ProcessOutput(b"fails".to_vec()),
        );
        assert_eq!(
            super::request_terminal_materialization(
                &state,
                "panel-poisoned-abort",
                &spec,
                vec![super::TerminalMaterializationEvent::Input(
                    b"discarded".to_vec()
                )],
            ),
            super::TerminalMaterializationDemand::Queued
        );
        let activation = Arc::new(Mutex::new(None));
        let saved_activation = activation.clone();
        let poison_state = state.clone();
        let error = super::fulfill_terminal_materialization_with(
            &state,
            lease,
            ConPtySize::new(80, 24),
            move |_, panel_id, _, _| {
                let (session, pending) = pending_test_session(
                    test_process(false),
                    test_transport(CapturingWriter(Arc::new(Mutex::new(Vec::new())))),
                    panel_id,
                );
                *saved_activation.lock().unwrap() = Some(pending);
                Ok(session)
            },
            move |_, _, _| {
                let state = poison_state.clone();
                assert!(std::thread::spawn(move || {
                    let _registry = state.registry.lock().unwrap();
                    panic!("poison registry during materialization apply failure");
                })
                .join()
                .is_err());
                Err("injected poisoned output apply failure".to_string())
            },
        )
        .unwrap_err();
        assert!(error.contains("injected poisoned output apply failure"));
        assert!(error.contains("terminal runtime registry mutex poisoned"));
        let registry = match state.registry.lock() {
            Ok(_) => panic!("registry must remain classified as poisoned"),
            Err(error) => error.into_inner(),
        };
        assert!(registry
            .sessions
            .values()
            .any(|session| { session.panel_id.as_deref() == Some("panel-poisoned-abort") }));
        assert!(!registry
            .materializations
            .contains_key("panel-poisoned-abort"));
        drop(registry);
        assert!(!pump_is_pending(
            activation.lock().unwrap().as_ref().unwrap()
        ));
    }

    #[cfg(windows)]
    #[test]
    fn conpty_materialization_real_round_trip_owns_process_and_spec() {
        let directory = std::env::temp_dir().join(format!(
            "cmux-conpty-materialization-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let expected_cwd = directory.to_string_lossy().to_string();
        let command = "$line=[Console]::In.ReadLine(); Write-Output ('CMUX-OWNED|' + $env:CMUX_OWNED_TEST + '|' + (Get-Location).Path + '|' + $line); Start-Sleep -Seconds 30";
        let spec = super::TerminalMaterializationSpec::new(
            Some(expected_cwd.clone()),
            Some(command.to_string()),
            b"initial-payload\r\n".to_vec(),
            BTreeMap::from([("CMUX_OWNED_TEST".to_string(), "environment-ok".to_string())]),
        );
        let components =
            super::spawn_terminal_process_components(&spec, ConPtySize::new(100, 30)).unwrap();
        assert!(components.root_pid.is_some_and(|pid| pid > 0));
        let mut process = components.process;
        let mut reader = components.reader;
        let mut writer = components.writer;
        let (output_tx, output_rx) = mpsc::channel();
        let reader_thread = std::thread::spawn(move || {
            let mut chunk = [0_u8; 4096];
            loop {
                match reader.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(read) => output_tx.send(chunk[..read].to_vec()).unwrap(),
                }
            }
        });
        let marker = format!("CMUX-OWNED|environment-ok|{}|initial-payload", expected_cwd);
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut output = Vec::new();
        let mut answered_cursor_request = false;
        while !String::from_utf8_lossy(&output).contains(&marker) {
            if !answered_cursor_request && output.windows(4).any(|bytes| bytes == b"\x1b[6n") {
                writer.write_all(b"\x1b[1;1R").unwrap();
                writer.flush().unwrap();
                answered_cursor_request = true;
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                let _ = process.kill();
                drop(writer);
                drop(process);
                reader_thread.join().unwrap();
                let _ = std::fs::remove_dir(&directory);
                panic!(
                    "ConPTY marker missing: {}",
                    String::from_utf8_lossy(&output)
                );
            }
            match output_rx.recv_timeout(remaining.min(Duration::from_millis(250))) {
                Ok(chunk) => output.extend(chunk),
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    let _ = process.kill();
                    drop(writer);
                    drop(process);
                    reader_thread.join().unwrap();
                    let _ = std::fs::remove_dir(&directory);
                    panic!(
                        "ConPTY reader closed before marker: {}",
                        String::from_utf8_lossy(&output)
                    );
                }
            }
        }
        process.kill().unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if process.try_wait().unwrap().is_some() {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "owned ConPTY did not exit after kill"
            );
            std::thread::yield_now();
        }
        drop(writer);
        drop(process);
        reader_thread.join().unwrap();
        std::fs::remove_dir(&directory).unwrap();
    }

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
    fn open_identity_reservation_fences_reuse_and_rolls_back_exactly() {
        let state = TerminalState::default();
        let reservation =
            match super::reserve_terminal_open_for_control(&state, Some("  panel-a  "), false)
                .expect("reserve a new terminal identity")
            {
                super::TerminalOpenReservation::Reserved(reservation) => reservation,
                super::TerminalOpenReservation::Existing(_) => {
                    panic!("unexpected existing runtime")
                }
            };

        {
            let registry = state.registry.lock().unwrap();
            assert!(registry.reserved_session_ids.contains(&reservation.id));
            assert!(registry.reserved_panel_ids.contains("panel-a"));
            assert!(registry.sessions.is_empty());
        }
        assert!(super::reserve_terminal_open_for_control(&state, Some("panel-a"), true).is_err());

        super::rollback_terminal_open_reservation_for_control(&state, reservation)
            .expect("roll back exact reservation");
        let registry = state.registry.lock().unwrap();
        assert!(registry.reserved_session_ids.is_empty());
        assert!(registry.reserved_panel_ids.is_empty());
        assert!(registry.sessions.is_empty());
    }

    #[test]
    fn ui_open_reuses_published_runtime_while_control_open_reserves_a_distinct_identity() {
        let state = TerminalState::default();
        state
            .next_id
            .store(42, std::sync::atomic::Ordering::Relaxed);
        state.registry.lock().unwrap().sessions.insert(
            41,
            test_session(test_process(false), test_transport(io::sink()), "panel-a"),
        );

        assert!(matches!(
            super::reserve_terminal_open_for_control(&state, Some("panel-a"), true).unwrap(),
            super::TerminalOpenReservation::Existing(41)
        ));
        let replacement =
            match super::reserve_terminal_open_for_control(&state, Some("panel-a"), false).unwrap()
            {
                super::TerminalOpenReservation::Reserved(reservation) => reservation,
                super::TerminalOpenReservation::Existing(_) => {
                    panic!("control open reused runtime")
                }
            };
        assert_eq!(replacement.id, 42);
        assert_eq!(replacement.panel_id.as_deref(), Some("panel-a"));
        assert!(state.registry.lock().unwrap().sessions.contains_key(&41));
        super::rollback_terminal_open_reservation_for_control(&state, replacement).unwrap();
    }

    #[test]
    fn open_publication_is_atomic_with_releasing_the_exact_reservation() {
        let state = TerminalState::default();
        let reservation =
            match super::reserve_terminal_open_for_control(&state, Some("panel-a"), false).unwrap()
            {
                super::TerminalOpenReservation::Reserved(reservation) => reservation,
                super::TerminalOpenReservation::Existing(_) => {
                    panic!("unexpected existing runtime")
                }
            };
        let id = reservation.id;
        let session = test_session(test_process(false), test_transport(io::sink()), "panel-a");

        let published = super::publish_terminal_open_reservation(&state, &reservation, session)
            .map_err(|(error, _session)| error)
            .expect("publish reserved terminal");
        assert_eq!(published, id);
        let registry = state.registry.lock().unwrap();
        assert!(registry.sessions.contains_key(&id));
        assert!(!registry.reserved_session_ids.contains(&id));
        assert!(!registry.reserved_panel_ids.contains("panel-a"));
    }

    #[test]
    fn abandoned_open_guard_releases_its_unpublished_identity() {
        let state = TerminalState::default();
        let reservation =
            match super::reserve_terminal_open_for_control(&state, Some("abandoned-panel"), false)
                .unwrap()
            {
                super::TerminalOpenReservation::Reserved(reservation) => reservation,
                super::TerminalOpenReservation::Existing(_) => {
                    panic!("unexpected existing runtime")
                }
            };
        {
            let _guard = super::TerminalOpenReservationGuard::new(&state, reservation);
            assert!(state
                .registry
                .lock()
                .unwrap()
                .reserved_panel_ids
                .contains("abandoned-panel"));
        }
        let registry = state.registry.lock().unwrap();
        assert!(registry.reserved_session_ids.is_empty());
        assert!(!registry.reserved_panel_ids.contains("abandoned-panel"));
    }

    #[test]
    fn startup_input_runs_while_the_reserved_registry_remains_available() {
        let state = Arc::new(TerminalState::default());
        let reservation =
            match super::reserve_terminal_open_for_control(&state, Some("startup-panel"), false)
                .unwrap()
            {
                super::TerminalOpenReservation::Reserved(reservation) => reservation,
                super::TerminalOpenReservation::Existing(_) => {
                    panic!("unexpected existing runtime")
                }
            };
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let startup = std::thread::spawn(move || {
            let mut writer = BlockingWriter {
                entered: entered_tx,
                release: release_rx,
                captured: Arc::new(Mutex::new(Vec::new())),
                block_once: true,
            };
            super::write_terminal_initial_input(&mut writer, Some("startup"))
        });
        entered_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("startup writer reached blocking I/O");
        let registry_available = state.registry.try_lock().is_ok();
        release_tx.send(()).unwrap();
        startup.join().unwrap().unwrap();
        assert!(registry_available, "startup I/O held the global registry");
        super::rollback_terminal_open_reservation_for_control(&state, reservation).unwrap();
    }

    #[test]
    fn resize_and_shutdown_process_io_do_not_hold_the_global_registry() {
        let resize_state = Arc::new(TerminalState::default());
        let (resize_entered_tx, resize_entered_rx) = mpsc::channel();
        let (resize_release_tx, resize_release_rx) = mpsc::channel();
        resize_state.registry.lock().unwrap().sessions.insert(
            1,
            test_session(
                Arc::new(Mutex::new(Box::new(BlockingResizeProcess {
                    entered: resize_entered_tx,
                    release: resize_release_rx,
                }))),
                test_transport(io::sink()),
                "resize-panel",
            ),
        );
        let resize_worker_state = resize_state.clone();
        let resize_worker = std::thread::spawn(move || {
            super::terminal_resize_id_for_control(&resize_worker_state, 1, 100, 30)
        });
        resize_entered_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("resize reached process I/O");
        let resize_registry_available = resize_state.registry.try_lock().is_ok();
        resize_release_tx.send(()).unwrap();
        resize_worker.join().unwrap().unwrap();
        assert!(resize_registry_available, "resize held the global registry");

        let shutdown_state = Arc::new(TerminalState::default());
        let (kill_entered_tx, kill_entered_rx) = mpsc::channel();
        let (kill_release_tx, kill_release_rx) = mpsc::channel();
        shutdown_state.registry.lock().unwrap().sessions.insert(
            2,
            test_session(
                Arc::new(Mutex::new(Box::new(BlockingKillProcess {
                    entered: kill_entered_tx,
                    release: kill_release_rx,
                }))),
                test_transport(io::sink()),
                "kill-panel",
            ),
        );
        let kill_worker_state = shutdown_state.clone();
        let kill_worker = std::thread::spawn(move || {
            super::terminal_shutdown_id_preserving_authority_for_control(&kill_worker_state, 2)
        });
        kill_entered_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("kill reached process I/O");
        let kill_registry_available = shutdown_state.registry.try_lock().is_ok();
        kill_release_tx.send(()).unwrap();
        kill_worker.join().unwrap().unwrap();
        assert!(kill_registry_available, "shutdown held the global registry");
    }

    #[test]
    fn finalization_kills_without_registry_lock_and_retains_failed_authority() {
        let state = Arc::new(TerminalState::default());
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        {
            let mut registry = state.registry.lock().unwrap();
            registry.sessions.insert(
                7,
                test_session(
                    Arc::new(Mutex::new(Box::new(BlockingKillProcess {
                        entered: entered_tx,
                        release: release_rx,
                    }))),
                    test_transport(io::sink()),
                    "ok-panel",
                ),
            );
            registry.sessions.insert(
                8,
                test_session(
                    Arc::new(Mutex::new(Box::new(FailingKillProcess))),
                    test_transport(io::sink()),
                    "retry-panel",
                ),
            );
        }
        let panels = ["ok-panel".to_string(), "retry-panel".to_string()]
            .into_iter()
            .collect();
        let lease = super::detach_terminal_panels_for_control(&state, &panels).unwrap();
        let worker_state = state.clone();
        let worker = std::thread::spawn(move || {
            super::finalize_terminal_panels_for_control(&worker_state, lease)
        });
        entered_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("finalize reached process kill");
        let registry_available = state.registry.try_lock().is_ok();
        release_tx.send(()).unwrap();
        let failure = worker.join().unwrap().expect_err("one kill must fail");
        assert!(registry_available, "finalize held the global registry");

        {
            let registry = state.registry.lock().unwrap();
            assert!(!registry.reserved_session_ids.contains(&7));
            assert!(!registry.reserved_panel_ids.contains("ok-panel"));
            assert!(registry.reserved_session_ids.contains(&8));
            assert!(registry.reserved_panel_ids.contains("retry-panel"));
            assert!(registry.sessions.is_empty());
        }
        super::rollback_terminal_panels_for_control(&state, failure.retry)
            .unwrap_or_else(|error| panic!("retry rollback failed: {}", error.message));
        assert_eq!(
            super::terminal_ids_for_panel_for_control(&state, "retry-panel"),
            vec![8]
        );
        assert!(super::terminal_ids_for_panel_for_control(&state, "ok-panel").is_empty());
    }

    #[test]
    fn rollback_collision_is_all_or_nothing_and_preserves_the_lease() {
        let state = TerminalState::default();
        {
            let mut registry = state.registry.lock().unwrap();
            registry.sessions.insert(
                10,
                test_session(test_process(false), test_transport(io::sink()), "panel-a"),
            );
            registry.sessions.insert(
                11,
                test_session(test_process(false), test_transport(io::sink()), "panel-b"),
            );
        }
        let panels = ["panel-a".to_string(), "panel-b".to_string()]
            .into_iter()
            .collect();
        let lease = super::detach_terminal_panels_for_control(&state, &panels).unwrap();
        state.registry.lock().unwrap().sessions.insert(
            10,
            test_session(test_process(false), test_transport(io::sink()), "intruder"),
        );

        let collision = super::rollback_terminal_panels_for_control(&state, lease)
            .expect_err("collision must preserve the entire lease");
        {
            let registry = state.registry.lock().unwrap();
            assert_eq!(registry.sessions.len(), 1);
            assert!(registry.sessions.contains_key(&10));
            assert!(!registry.sessions.contains_key(&11));
            assert!(registry.reserved_session_ids.contains(&10));
            assert!(registry.reserved_session_ids.contains(&11));
        }
        state.registry.lock().unwrap().sessions.remove(&10);
        super::rollback_terminal_panels_for_control(&state, collision.lease)
            .unwrap_or_else(|error| panic!("rollback after collision failed: {}", error.message));
        assert_eq!(
            super::terminal_ids_for_panel_for_control(&state, "panel-a"),
            vec![10]
        );
        assert_eq!(
            super::terminal_ids_for_panel_for_control(&state, "panel-b"),
            vec![11]
        );
    }

    #[test]
    fn poisoned_registry_returns_lifecycle_errors_without_panicking() {
        let state = Arc::new(TerminalState::default());
        let poison_state = state.clone();
        assert!(std::thread::spawn(move || {
            let _guard = poison_state.registry.lock().unwrap();
            panic!("poison terminal registry for lifecycle contract");
        })
        .join()
        .is_err());

        assert!(super::reserve_terminal_open_for_control(&state, Some("panel"), false).is_err());
        assert!(super::detach_terminal_panels_for_control(
            &state,
            &["panel".to_string()].into_iter().collect(),
        )
        .is_err());
        assert!(super::terminal_resize_id_for_control(&state, 1, 80, 24).is_err());
        assert!(super::terminal_shutdown_id_preserving_authority_for_control(&state, 1).is_err());
    }

    #[test]
    fn control_panel_reservation_fences_every_id_based_mutation_of_the_old_runtime() {
        let state = TerminalState::default();
        let captured = Arc::new(Mutex::new(Vec::new()));
        state
            .next_id
            .store(42, std::sync::atomic::Ordering::Relaxed);
        state.registry.lock().unwrap().sessions.insert(
            41,
            test_session(
                test_process(false),
                test_transport(CapturingWriter(captured.clone())),
                "panel-a",
            ),
        );
        let replacement =
            match super::reserve_terminal_open_for_control(&state, Some("panel-a"), false).unwrap()
            {
                super::TerminalOpenReservation::Reserved(reservation) => reservation,
                super::TerminalOpenReservation::Existing(_) => {
                    panic!("control open reused runtime")
                }
            };

        assert!(super::terminal_write_id_for_control(&state, 41, b"blocked").is_err());
        assert!(super::terminal_resize_id_for_control(&state, 41, 100, 30).is_err());
        assert!(super::terminal_shutdown_id_preserving_authority_for_control(&state, 41).is_err());
        assert!(super::terminal_remove_id_for_control(&state, 41).is_err());
        assert!(super::terminal_close_id_for_control(&state, 41).is_err());
        assert!(state.registry.lock().unwrap().sessions.contains_key(&41));

        super::rollback_terminal_open_reservation_for_control(&state, replacement).unwrap();
        super::terminal_write_id_for_control(&state, 41, b"reopened").unwrap();
        assert_eq!(&*captured.lock().unwrap(), b"reopened");
        let registry = state.registry.lock().unwrap();
        assert!(registry.reserved_session_ids.is_empty());
        assert!(registry.reserved_panel_ids.is_empty());
    }

    #[test]
    fn terminal_pump_stays_dormant_until_the_consumer_is_ready() {
        let (activation, ready) = super::TerminalPumpActivation::pending();
        let (emitted_tx, emitted_rx) = mpsc::channel();
        let pump = std::thread::spawn(move || {
            if ready.recv().is_ok() {
                emitted_tx.send(()).unwrap();
            }
        });

        activation.advance(super::TerminalPumpReadiness::Published);
        assert!(emitted_rx.recv_timeout(Duration::from_millis(50)).is_err());
        activation.advance(super::TerminalPumpReadiness::ConsumerReady);
        emitted_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("activation released dormant pump");
        pump.join().unwrap();
    }

    #[test]
    fn open_rollback_recovers_exact_authority_after_mid_transaction_registry_poison() {
        let state = Arc::new(TerminalState::default());
        let captured = Arc::new(Mutex::new(Vec::new()));
        state.registry.lock().unwrap().sessions.insert(
            41,
            test_session(
                test_process(false),
                test_transport(CapturingWriter(captured.clone())),
                "panel-a",
            ),
        );
        let reservation =
            match super::reserve_terminal_open_for_control(&state, Some("panel-a"), false).unwrap()
            {
                super::TerminalOpenReservation::Reserved(reservation) => reservation,
                super::TerminalOpenReservation::Existing(_) => {
                    panic!("control open reused runtime")
                }
            };
        let poison_state = state.clone();
        assert!(std::thread::spawn(move || {
            let _guard = poison_state.registry.lock().unwrap();
            panic!("poison registry during open transaction");
        })
        .join()
        .is_err());

        assert!(
            super::rollback_terminal_open_reservation_for_control(&state, reservation).is_err()
        );
        state.registry.clear_poison();
        {
            let registry = state.registry.lock().unwrap();
            assert!(registry.sessions.contains_key(&41));
            assert!(registry.reserved_session_ids.is_empty());
            assert!(registry.reserved_panel_ids.is_empty());
        }
        super::terminal_write_id_for_control(&state, 41, b"reopened").unwrap();
        assert_eq!(&*captured.lock().unwrap(), b"reopened");
    }

    #[test]
    fn detach_waits_for_inflight_input_resize_and_shutdown_before_returning_lease() {
        let input_state = Arc::new(TerminalState::default());
        let (input_entered_tx, input_entered_rx) = mpsc::channel();
        let (input_release_tx, input_release_rx) = mpsc::channel();
        input_state.registry.lock().unwrap().sessions.insert(
            1,
            test_session(
                test_process(false),
                test_transport(BlockingWriter {
                    entered: input_entered_tx,
                    release: input_release_rx,
                    captured: Arc::new(Mutex::new(Vec::new())),
                    block_once: true,
                }),
                "input-panel",
            ),
        );
        let input_worker_state = input_state.clone();
        let input_worker = std::thread::spawn(move || {
            super::terminal_write_id_for_control(&input_worker_state, 1, b"input")
        });
        input_entered_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("input reached writer");
        let input_detach_state = input_state.clone();
        let (input_lease_tx, input_lease_rx) = mpsc::channel();
        let input_detach = std::thread::spawn(move || {
            let lease = super::detach_terminal_panels_for_control(
                &input_detach_state,
                &["input-panel".to_string()].into_iter().collect(),
            )
            .unwrap();
            input_lease_tx.send(lease).unwrap();
        });
        assert!(input_lease_rx
            .recv_timeout(Duration::from_millis(50))
            .is_err());
        input_release_tx.send(()).unwrap();
        input_worker.join().unwrap().unwrap();
        let input_lease = input_lease_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        input_detach.join().unwrap();
        super::rollback_terminal_panels_for_control(&input_state, input_lease)
            .unwrap_or_else(|error| panic!("input rollback failed: {}", error.message));

        let resize_state = Arc::new(TerminalState::default());
        let (resize_entered_tx, resize_entered_rx) = mpsc::channel();
        let (resize_release_tx, resize_release_rx) = mpsc::channel();
        resize_state.registry.lock().unwrap().sessions.insert(
            2,
            test_session(
                Arc::new(Mutex::new(Box::new(BlockingResizeProcess {
                    entered: resize_entered_tx,
                    release: resize_release_rx,
                }))),
                test_transport(io::sink()),
                "resize-panel",
            ),
        );
        let resize_worker_state = resize_state.clone();
        let resize_worker = std::thread::spawn(move || {
            super::terminal_resize_id_for_control(&resize_worker_state, 2, 100, 30)
        });
        resize_entered_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("resize reached process");
        let resize_detach_state = resize_state.clone();
        let (resize_lease_tx, resize_lease_rx) = mpsc::channel();
        let resize_detach = std::thread::spawn(move || {
            let lease = super::detach_terminal_panels_for_control(
                &resize_detach_state,
                &["resize-panel".to_string()].into_iter().collect(),
            )
            .unwrap();
            resize_lease_tx.send(lease).unwrap();
        });
        assert!(resize_lease_rx
            .recv_timeout(Duration::from_millis(50))
            .is_err());
        resize_release_tx.send(()).unwrap();
        resize_worker.join().unwrap().unwrap();
        let resize_lease = resize_lease_rx
            .recv_timeout(Duration::from_secs(2))
            .unwrap();
        resize_detach.join().unwrap();
        super::rollback_terminal_panels_for_control(&resize_state, resize_lease)
            .unwrap_or_else(|error| panic!("resize rollback failed: {}", error.message));

        let shutdown_state = Arc::new(TerminalState::default());
        let (kill_entered_tx, kill_entered_rx) = mpsc::channel();
        let (kill_release_tx, kill_release_rx) = mpsc::channel();
        shutdown_state.registry.lock().unwrap().sessions.insert(
            3,
            test_session(
                Arc::new(Mutex::new(Box::new(BlockingKillProcess {
                    entered: kill_entered_tx,
                    release: kill_release_rx,
                }))),
                test_transport(io::sink()),
                "shutdown-panel",
            ),
        );
        let shutdown_worker_state = shutdown_state.clone();
        let shutdown_worker = std::thread::spawn(move || {
            super::terminal_shutdown_id_preserving_authority_for_control(&shutdown_worker_state, 3)
        });
        kill_entered_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("shutdown reached process");
        let shutdown_detach_state = shutdown_state.clone();
        let (shutdown_lease_tx, shutdown_lease_rx) = mpsc::channel();
        let shutdown_detach = std::thread::spawn(move || {
            let lease = super::detach_terminal_panels_for_control(
                &shutdown_detach_state,
                &["shutdown-panel".to_string()].into_iter().collect(),
            )
            .unwrap();
            shutdown_lease_tx.send(lease).unwrap();
        });
        assert!(shutdown_lease_rx
            .recv_timeout(Duration::from_millis(50))
            .is_err());
        kill_release_tx.send(()).unwrap();
        shutdown_worker.join().unwrap().unwrap();
        let shutdown_lease = shutdown_lease_rx
            .recv_timeout(Duration::from_secs(2))
            .unwrap();
        shutdown_detach.join().unwrap();
        super::rollback_terminal_panels_for_control(&shutdown_state, shutdown_lease)
            .unwrap_or_else(|error| panic!("shutdown rollback failed: {}", error.message));
    }

    #[test]
    fn finalize_retry_releases_completed_ids_after_registry_recovery() {
        let state = Arc::new(TerminalState::default());
        state.registry.lock().unwrap().sessions.insert(
            9,
            test_session(
                Arc::new(Mutex::new(Box::new(PoisonRegistryDuringKillProcess {
                    state: Arc::downgrade(&state),
                }))),
                test_transport(io::sink()),
                "panel",
            ),
        );
        let lease = super::detach_terminal_panels_for_control(
            &state,
            &["panel".to_string()].into_iter().collect(),
        )
        .unwrap();
        let failed = super::finalize_terminal_panels_for_control(&state, lease)
            .expect_err("post-kill registry poison must preserve retry authority");
        state.registry.clear_poison();
        super::finalize_terminal_panels_for_control(&state, failed.retry)
            .unwrap_or_else(|error| panic!("retry finalize failed: {:?}", error.failures));

        let registry = state.registry.lock().unwrap();
        assert!(registry.reserved_session_ids.is_empty());
        assert!(registry.reserved_panel_ids.is_empty());
        assert!(registry.sessions.is_empty());
    }

    #[test]
    fn close_failure_is_classified_and_retains_exact_runtime_authority() {
        let failing = TerminalState::default();
        failing.registry.lock().unwrap().sessions.insert(
            12,
            test_session(
                Arc::new(Mutex::new(Box::new(FailingKillProcess))),
                test_transport(io::sink()),
                "failing-panel",
            ),
        );
        assert!(super::terminal_close_id_for_control(&failing, 12).is_err());
        super::terminal_write_id_for_control(&failing, 12, b"still-owned").unwrap();
        {
            let registry = failing.registry.lock().unwrap();
            assert!(registry.sessions.contains_key(&12));
            assert!(registry.reserved_session_ids.is_empty());
            assert!(registry.reserved_panel_ids.is_empty());
        }

        let poisoned = TerminalState::default();
        let process = test_process(false);
        poisoned.registry.lock().unwrap().sessions.insert(
            13,
            test_session(
                process.clone(),
                test_transport(io::sink()),
                "poisoned-panel",
            ),
        );
        assert!(std::thread::spawn(move || {
            let _guard = process.lock().unwrap();
            panic!("poison process before close");
        })
        .join()
        .is_err());
        assert!(super::terminal_close_id_for_control(&poisoned, 13).is_err());
        let registry = poisoned.registry.lock().unwrap();
        assert!(registry.sessions.contains_key(&13));
        assert!(registry.reserved_session_ids.is_empty());
        assert!(registry.reserved_panel_ids.is_empty());
    }

    #[test]
    fn close_completes_exact_cleanup_after_post_kill_registry_poison() {
        let state = Arc::new(TerminalState::default());
        state.registry.lock().unwrap().sessions.insert(
            14,
            test_session(
                Arc::new(Mutex::new(Box::new(PoisonRegistryDuringKillProcess {
                    state: Arc::downgrade(&state),
                }))),
                test_transport(io::sink()),
                "poison-during-close",
            ),
        );

        assert!(super::terminal_close_id_for_control(&state, 14).is_err());
        state.registry.clear_poison();
        let registry = state.registry.lock().unwrap();
        assert!(!registry.sessions.contains_key(&14));
        assert!(registry.reserved_session_ids.is_empty());
        assert!(registry.reserved_panel_ids.is_empty());
    }

    #[test]
    fn empty_and_large_idle_live_input_bypass_the_pending_budget() {
        let captured = Arc::new(Mutex::new(Vec::new()));
        let input = test_transport(CapturingWriter(captured.clone()));
        let exited = test_process(true);

        assert_eq!(
            send_terminal_input(exited, input.clone(), b""),
            TerminalInputOutcome::Sent
        );
        assert_eq!(input.pending.lock().unwrap().bytes, 0);
        assert!(input.pending.lock().unwrap().entries.is_empty());

        let payload = vec![b'x'; TERMINAL_PENDING_INPUT_LIMIT + 1];
        assert_eq!(
            send_terminal_input(test_process(false), input, &payload),
            TerminalInputOutcome::Sent
        );
        assert_eq!(*captured.lock().unwrap(), payload);
    }

    #[test]
    fn contended_input_is_fifo_bounded_and_does_not_hold_the_registry() {
        let state = Arc::new(TerminalState::default());
        let captured = Arc::new(Mutex::new(Vec::new()));
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        state.runtime_registry().sessions.insert(
            1,
            test_session(
                test_process(false),
                test_transport(BlockingWriter {
                    entered: entered_tx,
                    release: release_rx,
                    captured: captured.clone(),
                    block_once: true,
                }),
                "panel",
            ),
        );

        let owner_state = state.clone();
        let owner = std::thread::spawn(move || {
            super::terminal_send_panel_bytes_for_control(&owner_state, "panel", b"first")
        });
        entered_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("owner reached writer I/O");
        assert!(state.registry.try_lock().is_ok());

        let queued = vec![b'q'; TERMINAL_PENDING_INPUT_LIMIT];
        assert_eq!(
            super::terminal_send_panel_bytes_for_control(&state, "panel", &queued),
            TerminalInputOutcome::Queued
        );
        assert_eq!(
            super::terminal_send_panel_bytes_for_control(&state, "panel", b"overflow"),
            TerminalInputOutcome::InputQueueFull
        );
        release_tx.send(()).unwrap();
        assert_eq!(owner.join().unwrap(), TerminalInputOutcome::Sent);

        let captured = captured.lock().unwrap();
        assert_eq!(&captured[..5], b"first");
        assert_eq!(&captured[5..], queued);
    }

    #[test]
    fn queued_write_failure_retains_fifo_and_preserves_caller_relative_outcomes() {
        let captured = Arc::new(Mutex::new(Vec::new()));
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let input = test_transport(FailSecondWriteOnce {
            entered: entered_tx,
            release: release_rx,
            captured: captured.clone(),
            writes: 0,
        });
        let process = test_process(false);

        let owner_input = input.clone();
        let owner_process = process.clone();
        let owner =
            std::thread::spawn(move || send_terminal_input(owner_process, owner_input, b"first"));
        entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(
            send_terminal_input(process.clone(), input.clone(), b"second"),
            TerminalInputOutcome::Queued
        );
        release_tx.send(()).unwrap();
        assert_eq!(owner.join().unwrap(), TerminalInputOutcome::Sent);
        {
            let pending = input.pending.lock().unwrap();
            assert_eq!(pending.bytes, b"second".len());
            assert_eq!(pending.entries.len(), 1);
        }

        assert_eq!(
            send_terminal_input(process, input.clone(), b"third"),
            TerminalInputOutcome::Queued
        );
        assert_eq!(&*captured.lock().unwrap(), b"firstsecondthird");
        let pending = input.pending.lock().unwrap();
        assert_eq!(pending.bytes, 0);
        assert!(pending.entries.is_empty());
    }

    #[test]
    fn partial_queued_write_recovers_from_the_exact_unwritten_offset() {
        let captured = Arc::new(Mutex::new(Vec::new()));
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let input = test_transport(PartialQueuedFailureOnce {
            entered: entered_tx,
            release: release_rx,
            captured: captured.clone(),
            writes: 0,
        });
        let process = test_process(false);

        let owner_input = input.clone();
        let owner_process = process.clone();
        let owner =
            std::thread::spawn(move || send_terminal_input(owner_process, owner_input, b"first"));
        entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(
            send_terminal_input(process.clone(), input.clone(), b"second"),
            TerminalInputOutcome::Queued
        );
        release_tx.send(()).unwrap();
        assert_eq!(owner.join().unwrap(), TerminalInputOutcome::Sent);

        assert_eq!(
            send_terminal_input(process, input.clone(), b"third"),
            TerminalInputOutcome::Queued
        );
        assert_eq!(&*captured.lock().unwrap(), b"firstsecondthird");
        let pending = input.pending.lock().unwrap();
        assert_eq!(pending.bytes, 0);
        assert!(pending.entries.is_empty());
    }

    #[test]
    fn writer_panic_releases_owner_and_rejects_new_input_without_queueing() {
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let input = test_transport(PanicWriter {
            entered: entered_tx,
            release: release_rx,
        });
        let process = test_process(false);

        let owner_input = input.clone();
        let owner_process = process.clone();
        let owner =
            std::thread::spawn(move || send_terminal_input(owner_process, owner_input, b"first"));
        entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(
            send_terminal_input(process.clone(), input.clone(), b"second"),
            TerminalInputOutcome::Queued
        );
        release_tx.send(()).unwrap();
        assert!(owner.join().is_err());
        let bytes_before = input.pending.lock().unwrap().bytes;

        assert_eq!(
            send_terminal_input(process, input.clone(), b"third"),
            TerminalInputOutcome::SurfaceUnavailable
        );
        assert_eq!(input.pending.lock().unwrap().bytes, bytes_before);
    }

    #[test]
    fn process_exit_precedes_writer_poison_classification() {
        let input = test_transport(CapturingWriter(Arc::new(Mutex::new(Vec::new()))));
        let poison_input = input.clone();
        assert!(std::thread::spawn(move || {
            let _guard = poison_input.writer.lock().unwrap();
            panic!("poison writer for precedence contract");
        })
        .join()
        .is_err());

        assert_eq!(
            send_terminal_input(test_process(true), input, b"input"),
            TerminalInputOutcome::ProcessExited
        );
    }

    #[test]
    fn process_query_race_cannot_enqueue_after_writer_poison() {
        let (writer_entered_tx, writer_entered_rx) = mpsc::channel();
        let (writer_release_tx, writer_release_rx) = mpsc::channel();
        let input = test_transport(PanicWriter {
            entered: writer_entered_tx,
            release: writer_release_rx,
        });
        let (query_entered_tx, query_entered_rx) = mpsc::channel();
        let (query_release_tx, query_release_rx) = mpsc::channel();
        let process: Arc<Mutex<Box<dyn TerminalProcess>>> =
            Arc::new(Mutex::new(Box::new(BlockingNthWaitProcess {
                calls: 0,
                block_on: 3,
                entered: query_entered_tx,
                release: query_release_rx,
            })));

        let owner_input = input.clone();
        let owner_process = process.clone();
        let owner =
            std::thread::spawn(move || send_terminal_input(owner_process, owner_input, b"first"));
        writer_entered_rx
            .recv_timeout(Duration::from_secs(2))
            .unwrap();
        assert_eq!(
            send_terminal_input(process.clone(), input.clone(), b"retained"),
            TerminalInputOutcome::Queued
        );

        let racer_input = input.clone();
        let racer_process = process.clone();
        let racer =
            std::thread::spawn(move || send_terminal_input(racer_process, racer_input, b"racer"));
        query_entered_rx
            .recv_timeout(Duration::from_secs(2))
            .unwrap();
        writer_release_tx.send(()).unwrap();
        assert!(owner.join().is_err());
        query_release_tx.send(()).unwrap();

        assert_eq!(
            racer.join().unwrap(),
            TerminalInputOutcome::SurfaceUnavailable
        );
        let pending = input.pending.lock().unwrap();
        assert_eq!(pending.bytes, b"retained".len());
        assert_eq!(pending.entries.len(), 1);
    }

    #[test]
    fn empty_compatibility_writes_succeed_before_target_resolution() {
        let state = TerminalState::default();

        assert_eq!(
            super::terminal_write_id_for_control(&state, 404, b""),
            Ok(())
        );
        assert_eq!(super::terminal_write_panel(&state, "missing", ""), Ok(()));
    }

    #[test]
    fn live_input_classifies_exit_and_writer_failures_exactly() {
        assert_eq!(
            send_terminal_input(
                test_process(true),
                test_transport(CapturingWriter(Arc::new(Mutex::new(Vec::new())))),
                b"input",
            ),
            TerminalInputOutcome::ProcessExited
        );
        assert_eq!(
            send_terminal_input(
                test_process(false),
                test_transport(ErrorWriter(io::ErrorKind::BrokenPipe)),
                b"input",
            ),
            TerminalInputOutcome::ProcessExited
        );
        assert_eq!(
            send_terminal_input(
                Arc::new(Mutex::new(Box::new(TestProcess {
                    wait: Err("injected process query failure".to_string()),
                }))),
                test_transport(CapturingWriter(Arc::new(Mutex::new(Vec::new())))),
                b"input",
            ),
            TerminalInputOutcome::SurfaceUnavailable
        );
        assert_eq!(
            send_terminal_input(
                test_process(false),
                test_transport(ErrorWriter(io::ErrorKind::Other)),
                b"input",
            ),
            TerminalInputOutcome::SurfaceUnavailable
        );
    }

    #[test]
    fn queued_compatibility_adapters_accept_without_retry() {
        let state = Arc::new(TerminalState::default());
        let captured = Arc::new(Mutex::new(Vec::new()));
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        state.runtime_registry().sessions.insert(
            7,
            test_session(
                test_process(false),
                test_transport(BlockingWriter {
                    entered: entered_tx,
                    release: release_rx,
                    captured: captured.clone(),
                    block_once: true,
                }),
                "panel",
            ),
        );

        let owner_state = state.clone();
        let owner = std::thread::spawn(move || {
            super::terminal_send_panel_bytes_for_control(&owner_state, "panel", b"first")
        });
        entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(
            super::terminal_write_panel(&state, "panel", "panel"),
            Ok(())
        );
        assert_eq!(
            super::terminal_write_id_for_control(&state, 7, b"id"),
            Ok(())
        );
        release_tx.send(()).unwrap();
        assert_eq!(owner.join().unwrap(), TerminalInputOutcome::Sent);
        assert_eq!(&*captured.lock().unwrap(), b"firstpanelid");
    }

    #[test]
    fn live_materialization_events_remain_ordered_after_input_queues() {
        let state = Arc::new(TerminalState::default());
        let captured = Arc::new(Mutex::new(Vec::new()));
        let emitted = Arc::new(Mutex::new(Vec::new()));
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        state.runtime_registry().sessions.insert(
            7,
            test_session(
                test_process(false),
                test_transport(BlockingWriter {
                    entered: entered_tx,
                    release: release_rx,
                    captured: captured.clone(),
                    block_once: true,
                }),
                "panel",
            ),
        );

        let owner_state = state.clone();
        let owner = std::thread::spawn(move || {
            super::terminal_send_panel_bytes_for_control(&owner_state, "panel", b"owner")
        });
        entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();

        let emitted_for_call = emitted.clone();
        assert_eq!(
            super::terminal_apply_materialization_events_with(
                &state,
                "panel",
                vec![
                    super::TerminalMaterializationEvent::Input(b"first".to_vec()),
                    super::TerminalMaterializationEvent::ProcessOutput(b"output".to_vec()),
                    super::TerminalMaterializationEvent::Input(b"second".to_vec()),
                ],
                move |_id, bytes, _titles| {
                    emitted_for_call.lock().unwrap().extend_from_slice(bytes);
                    Ok(())
                },
            ),
            TerminalInputOutcome::Queued
        );
        assert_eq!(&*emitted.lock().unwrap(), b"output");

        release_tx.send(()).unwrap();
        assert_eq!(owner.join().unwrap(), TerminalInputOutcome::Sent);
        assert_eq!(&*captured.lock().unwrap(), b"ownerfirstsecond");
    }

    #[test]
    fn control_input_navigation_tracks_application_cursor_mode() {
        let mut grid = TerminalGrid::new(GridSize::new(80, 24));
        assert_eq!(
            super::terminal_input_bytes_for_grid(&grid, b"\x1b[A\x1bOA\x1b[5~"),
            b"\x1b[A\x1bOA\x1b[5~"
        );

        grid.advance(b"\x1b[?1h");
        assert_eq!(
            super::terminal_input_bytes_for_grid(&grid, b"\x1b[A\x1bOB\x1b[5~\xc3\xa9"),
            b"\x1bOA\x1bOB\x1b[5~\xc3\xa9"
        );
    }

    #[test]
    fn live_only_terminal_input_never_starts_a_cold_runtime() {
        let state = TerminalState::default();
        let events = vec![super::TerminalMaterializationEvent::Input(
            b"remote".to_vec(),
        )];
        assert_eq!(
            super::request_live_terminal_input(&state, "remote", events.clone()),
            super::TerminalMaterializationDemand::SurfaceUnavailable
        );

        state.runtime_registry().sessions.insert(
            7,
            test_session(
                test_process(false),
                test_transport(CapturingWriter(Arc::new(Mutex::new(Vec::new())))),
                "remote",
            ),
        );
        assert_eq!(
            super::request_live_terminal_input(&state, "remote", events.clone()),
            super::TerminalMaterializationDemand::Live(events)
        );
        assert!(state.runtime_registry().materializations.is_empty());
    }

    #[test]
    fn poisoned_registry_returns_classified_input_failure() {
        let state = Arc::new(TerminalState::default());
        let poison_state = state.clone();
        assert!(std::thread::spawn(move || {
            let _guard = poison_state.registry.lock().unwrap();
            panic!("poison terminal registry for input contract");
        })
        .join()
        .is_err());

        assert_eq!(
            super::terminal_send_panel_bytes_for_control(&state, "panel", b"input"),
            TerminalInputOutcome::SurfaceUnavailable
        );
        assert!(super::terminal_write_id_for_control(&state, 1, b"input").is_err());
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
