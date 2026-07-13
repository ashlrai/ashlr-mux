//! Transition layer for the window-lifecycle v2 family (parity batch 1B):
//! `window.create` / `window.close` / `window.focus`, `surface.refresh`, and
//! `surface.resume.set|get|clear`, plus the v1 line-protocol commands
//! `new_window` / `focus_window` / `close_window`.
//!
//! Contract of record: docs/parity/contracts/window_lifecycle.json, audited
//! from pinned canonical commit e1825d40d52b4ae4f4bcb0b7e0dfc744dd20a452.
//!
//! Follows the `pane_surface_lifecycle` pattern: pure dispatch functions over
//! `AppSessionSnapshot` returning a transition (result + events + effects)
//! that is testable without a live window. OS-coupled semantics (key-window
//! activation, quit-confirmation alert, renderer redraw, blocking resume
//! approval alert) are modeled as deterministic effects the executor layer
//! honors — see the contract's `headless_impossibility_flags`.

use cmux_core::session::AppSessionSnapshot;
use cmux_core::session::SessionSurfaceResumeBindingSnapshot;
use cmux_ipc::ControlCallResult;
use serde_json::{Map, Value};

use super::pane_surface_lifecycle::LifecycleEvent;

/// Effects the production executor honors for window-lifecycle transitions.
///
/// Platform mapping notes (canonical -> Windows):
/// - `WindowCreate { activate: false }` models canonical orderFront-WITHOUT-
///   activation for socket-created windows (shouldSuppressSocketCommandActivation,
///   TerminalController.swift:407-409; AppDelegate.swift:8862-8868). On the
///   Tauri port this is a hidden-build + show() without set_focus().
/// - `WindowFocus` is the full canonical focus intent (unhide + deminiaturize
///   + makeKeyAndOrderFront + app activate, MainWindowVisibilityController.swift:134-196);
///   real Win32 foregrounding is a platform_equivalent verified live.
/// - `QuitConfirmation`/`AppTerminate` model the last-window shouldClose veto
///   (AppDelegate.swift:16232-16239, 12831-12856); canonical bypasses the
///   alert under XCTest, so tests pin the effect, never a dialog.
/// - `ResumeApprovalPrompt` models the blocking resume-approval NSAlert which
///   canonical bypasses under XCTest (TerminalController+ControlSurfaceContext4.swift:104-110).
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[allow(dead_code)] // Variants land with the red suite; dispatch consumes them as slices go green.
pub(super) enum WindowLifecycleEffect {
    WindowCreate {
        window_id: String,
        activate: bool,
        failure_code: &'static str,
        failure_message: &'static str,
    },
    /// Move the active-TabManager pointer (app-internal), NOT OS focus
    /// (TerminalControllerControlCommandContext.swift:71-76; v1
    /// TerminalController.swift:11899-11921).
    SetActiveWindow { window_id: String },
    WindowFocus { window_id: String },
    WindowCloseCommit { window_id: String },
    RecordClosedWindowHistory { window_id: String },
    PersistWindowGeometry { window_id: String },
    /// Remote-tmux workspaces detach (server survives) on window close
    /// (AppDelegate.swift:8809-8830).
    RemoteWorkspaceDetach {
        workspace_id: String,
        destination: String,
    },
    QuitConfirmation { window_id: String },
    AppTerminate { window_id: String },
    TerminalRefresh {
        surface_id: String,
        reason: &'static str,
    },
    ResumeApprovalPrompt {
        surface_id: String,
        source: Option<String>,
        auto_resume: bool,
    },
    PersistSession,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct WindowLifecycleContext {
    pub active_window_id: Option<String>,
    /// Canonical handleQuitShortcutWarning: quit-confirmation not required ->
    /// NSApp.terminate immediately; required -> async confirmation alert
    /// (AppDelegate.swift:12831-12856).
    pub quit_confirmation_required: bool,
    /// Injected clock for `resume_binding.updated_at` (double epoch seconds,
    /// TerminalController+ControlSurfaceContext4.swift:170-179).
    pub now_epoch_seconds: f64,
    /// Production supplies the pre-allocated window label; `None` mints a
    /// fresh UUID (canonical availableWindowIdForNewMainWindow,
    /// AppDelegate.swift:8637-8638).
    pub new_window_id: Option<String>,
    /// Production supplies the allocated initial panel id; `None` mints a UUID.
    pub new_surface_id: Option<String>,
}

#[derive(Debug)]
pub(super) struct WindowLifecycleTransition {
    pub snapshot: AppSessionSnapshot,
    pub result: ControlCallResult,
    pub changed: bool,
    pub events: Vec<LifecycleEvent>,
    pub effects: Vec<WindowLifecycleEffect>,
}

pub(super) fn dispatch_window_lifecycle_request(
    snapshot: &AppSessionSnapshot,
    _method: &str,
    _params: &Map<String, Value>,
    _context: &WindowLifecycleContext,
) -> WindowLifecycleTransition {
    WindowLifecycleTransition {
        snapshot: snapshot.clone(),
        result: ControlCallResult::Err {
            code: "method_not_found".into(),
            message: "Unknown lifecycle method".into(),
            data: None,
        },
        changed: false,
        events: Vec::new(),
        effects: Vec::new(),
    }
}

/// Canonical `surfaceResumeBindingPayload` with explicit nulls
/// (ControlCommandCoordinator+Surface3.swift:131-144,147-167). ONE
/// implementation shared by surface.resume.* and surface.list rows.
pub(super) fn resume_binding_payload(
    _binding: Option<&SessionSurfaceResumeBindingSnapshot>,
) -> Value {
    Value::Null
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum V1WindowCommand {
    NewWindow,
    FocusWindow(Option<String>),
    CloseWindow(Option<String>),
}

pub(super) fn parse_v1_window_command(_line: &str) -> Option<V1WindowCommand> {
    None
}

/// Map a parsed v1 command to its v2 method + params, or the early v1 error
/// reply when the argument is missing.
pub(super) fn v1_window_request(
    _command: &V1WindowCommand,
) -> Result<(&'static str, Map<String, Value>), String> {
    Err("ERROR: Invalid window id".into())
}

/// Format the v1 line reply for a dispatched command result
/// (TerminalController.swift:11899-11928).
pub(super) fn v1_window_reply(_command: &V1WindowCommand, _result: &ControlCallResult) -> String {
    "ERROR: Failed to create window".into()
}
