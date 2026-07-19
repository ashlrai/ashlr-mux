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
//!
//! Production session window identities are canonical UUIDs. Deterministic
//! tests use readable labels, so selector rejection is pinned as shape
//! validation (non-empty string that is not an unresolved `kind:N` handle
//! ref). Handle refs are resolved upstream by
//! `resolve_request_handle_refs`; a surviving `kind:N` literal means the ref
//! was unresolvable, which canonical rejects as invalid_params.

mod events;
use events::initial_workspace_events;
pub(super) use events::window_lifecycle_event;

use cmux_core::session::{
    AppSessionSnapshot, SessionSurfaceKindSnapshot, SessionSurfaceResumeBindingRecordSnapshot,
    SessionSurfaceResumeBindingSnapshot, SessionTabManagerSnapshot, SessionWindowSnapshot,
    SessionWorkspaceSnapshot,
};
use cmux_core::surface_lifecycle::SurfaceLifecycleModel;
use cmux_ipc::{ControlCallResult, JsonValue};
use serde_json::{json, Map, Value};
use uuid::Uuid;

use super::pane_surface_lifecycle::{
    dock_owner_from_workspace_selector, resolve_window_index, scope, LifecycleDispatchContext,
    LifecycleEvent,
};

/// surface.resume.* unavailable message (Self.surfaceWindowUnavailableMessage,
/// ControlCommandCoordinator+Surface.swift:79-80) — deliberately different
/// from surface.refresh's "TabManager not available".
const RESUME_UNAVAILABLE: &str = "cmux window is not available. Reopen the window and try again.";

/// Effects the production executor honors for window-lifecycle transitions.
///
/// Platform mapping notes (canonical -> Windows):
/// - `WindowCreate { activate: false }` models canonical orderFront-WITHOUT-
///   activation for socket-created windows (shouldSuppressSocketCommandActivation,
///   TerminalController.swift:407-409; AppDelegate.swift:8862-8868). On the
///   Tauri port this is a hidden-build + show() without set_focus().
/// - `WindowFocus` is the full canonical focus intent (unhide, deminiaturize,
///   makeKeyAndOrderFront, app activate — MainWindowVisibilityController.swift:134-196);
///   real Win32 foregrounding is a platform_equivalent verified live.
/// - `QuitConfirmation`/`AppTerminate` model the last-window shouldClose veto
///   (AppDelegate.swift:16232-16239, 12831-12856); canonical bypasses the
///   alert under XCTest, so tests pin the effect, never a dialog.
/// - `ResumeApprovalPrompt` models the blocking resume-approval NSAlert which
///   canonical bypasses under XCTest (TerminalController+ControlSurfaceContext4.swift:104-110).
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
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
    SetActiveWindow {
        window_id: String,
    },
    WindowFocus {
        window_id: String,
    },
    WindowCloseCommit {
        window_id: String,
        next_key_window_id: Option<String>,
    },
    RecordClosedWindowHistory {
        window_id: String,
    },
    PersistWindowGeometry {
        window_id: String,
    },
    /// Remote-tmux workspaces detach (server survives) on window close
    /// (AppDelegate.swift:8809-8830).
    RemoteWorkspaceDetach {
        workspace_id: String,
        destination: String,
    },
    /// Drop stale notifications for the closing window and each of its
    /// workspaces (unregisterMainWindow, AppDelegate.swift:16274-16280:
    /// clearNotifications(forTabId: removed.windowId) then per tab).
    ClearWindowNotifications {
        window_id: String,
        workspace_ids: Vec<String>,
    },
    QuitConfirmation {
        window_id: String,
    },
    AppTerminate {
        window_id: String,
    },
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
    pub key_window_id: Option<String>,
    pub previous_key_window_id: Option<String>,
    /// Canonical handleQuitShortcutWarning: quit-confirmation not required ->
    /// NSApp.terminate immediately; required -> async confirmation alert
    /// (AppDelegate.swift:12831-12856).
    pub quit_confirmation_required: bool,
    /// Injected clock for `resume_binding.updated_at` (double epoch seconds,
    /// TerminalController+ControlSurfaceContext4.swift:170-179).
    pub now_epoch_seconds: f64,
    /// Optional deterministic id injection for tests; production leaves this
    /// unset so the transition mints a fresh UUID (canonical
    /// availableWindowIdForNewMainWindow, AppDelegate.swift:8637-8638).
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
    method: &str,
    params: &Map<String, Value>,
    context: &WindowLifecycleContext,
) -> WindowLifecycleTransition {
    let mut transition = match method {
        "window.create" => window_create(snapshot, context),
        "window.close" => window_close(snapshot, params, context),
        "window.focus" => window_focus(snapshot, params, context),
        "surface.refresh" => surface_refresh(snapshot, params, context),
        "surface.resume.set" => surface_resume(snapshot, params, context, ResumeOp::Set),
        "surface.resume.get" => surface_resume(snapshot, params, context, ResumeOp::Get),
        "surface.resume.clear" => surface_resume(snapshot, params, context, ResumeOp::Clear),
        _ => error(
            snapshot,
            "method_not_found",
            "Unknown lifecycle method",
            None,
        ),
    };
    transition.changed = transition.snapshot != *snapshot;
    transition
}

// ---------------------------------------------------------------------------
// Shared transition plumbing
// ---------------------------------------------------------------------------

fn json_result(value: Value) -> ControlCallResult {
    ControlCallResult::Ok(JsonValue::try_from(value).expect("lifecycle payload is valid JSON"))
}

fn ok_transition(
    snapshot: AppSessionSnapshot,
    value: Value,
    events: Vec<LifecycleEvent>,
    effects: Vec<WindowLifecycleEffect>,
) -> WindowLifecycleTransition {
    WindowLifecycleTransition {
        snapshot,
        result: json_result(value),
        changed: false, // recomputed by dispatch
        events,
        effects,
    }
}

fn error(
    snapshot: &AppSessionSnapshot,
    code: &str,
    message: &str,
    data: Option<Value>,
) -> WindowLifecycleTransition {
    WindowLifecycleTransition {
        snapshot: snapshot.clone(),
        result: ControlCallResult::Err {
            code: code.into(),
            message: message.into(),
            data: data.and_then(|value| JsonValue::try_from(value).ok()),
        },
        changed: false,
        events: Vec::new(),
        effects: Vec::new(),
    }
}

/// Routing context for the shared pane/window resolution walk. Only the
/// active-window default matters for this family.
fn routing_context(context: &WindowLifecycleContext) -> LifecycleDispatchContext {
    LifecycleDispatchContext {
        browser_enabled: false,
        dock_available: false,
        active_window_id: context.active_window_id.clone(),
    }
}

const KNOWN_REF_KINDS: [&str; 7] = [
    "window",
    "workspace",
    "surface",
    "terminal",
    "tab",
    "pane",
    "workspace_group",
];

/// A `kind:N` literal that survived upstream handle-ref resolution means the
/// ref never resolved — canonical `uuid(params, key)` returns nil for it
/// (ControlCommandCoordinator.swift:211-218).
fn is_unresolved_handle_ref(value: &str) -> bool {
    value.split_once(':').is_some_and(|(kind, index)| {
        KNOWN_REF_KINDS.contains(&kind)
            && !index.is_empty()
            && index.bytes().all(|byte| byte.is_ascii_digit())
    })
}

/// Canonical `string()` trims whitespace/newlines and treats whitespace-only
/// as absent (ControlCommandCoordinator.swift:203-207).
fn valid_selector_id(value: &Value) -> Option<String> {
    let text = value.as_str()?.trim();
    (!text.is_empty() && !is_unresolved_handle_ref(text)).then(|| text.to_owned())
}

/// `uuid(params, "window_id")` twin: absent, non-string, whitespace-only, or
/// unresolvable-ref shapes are invalid.
fn required_window_id(params: &Map<String, Value>) -> Option<String> {
    valid_selector_id(params.get("window_id")?)
}

fn window_position(snapshot: &AppSessionSnapshot, window_id: &str) -> Option<usize> {
    snapshot
        .windows
        .iter()
        .position(|window| window.window_id.as_deref() == Some(window_id))
}

fn is_terminal_kind(kind: &SessionSurfaceKindSnapshot) -> bool {
    matches!(
        kind,
        SessionSurfaceKindSnapshot::Terminal | SessionSurfaceKindSnapshot::RemoteTerminal { .. }
    )
}

// ---------------------------------------------------------------------------
// v2:window.create
// ---------------------------------------------------------------------------

/// windowCreate() takes NO params — request.params are ignored entirely
/// (ControlCommandCoordinator+Window.swift:22-23,139). Creation mirrors
/// canonical createMainWindow (AppDelegate.swift:8624-8905): a fresh window
/// with one initial terminal workspace, and mirrors the production
/// `register_window_for_control` model (`auxiliary_window_snapshot`).
fn window_create(
    snapshot: &AppSessionSnapshot,
    context: &WindowLifecycleContext,
) -> WindowLifecycleTransition {
    let window_id = context
        .new_window_id
        .clone()
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    let surface_id = context
        .new_surface_id
        .clone()
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    let mut next = snapshot.clone();
    let workspace = crate::session::fresh_control_window_workspace(&surface_id);
    let selected_workspace_id = workspace.workspace_id.clone();
    next.windows.push(SessionWindowSnapshot {
        window_id: Some(window_id.clone()),
        selected_workspace_id,
        dock: None,
        tab_manager: SessionTabManagerSnapshot {
            selected_workspace_index: Some(0),
            workspaces: vec![workspace],
            workspace_groups: None,
        },
    });
    let created = next.windows.last().expect("window just pushed");
    // Emitted after registerMainWindow with origin=create (AppDelegate.swift:8860).
    // Activation is suppressed for every socket command and window.create is
    // not focus-intent, so no key transfer happens (is_key_window false).
    let mut events = initial_workspace_events(created);
    events.push(window_lifecycle_event(
        "window.created",
        "create",
        created,
        &window_id,
        false,
        false,
    ));
    // orderFront-only + defensive setActiveTabManager; persistence happens on
    // the NEXT session snapshot save, not immediately (contract
    // state_events_persistence for v2:window.create).
    let effects = vec![
        WindowLifecycleEffect::WindowCreate {
            window_id: window_id.clone(),
            activate: false,
            failure_code: "internal_error",
            failure_message: "Failed to create window",
        },
        WindowLifecycleEffect::SetActiveWindow {
            window_id: window_id.clone(),
        },
    ];
    ok_transition(next, json!({ "window_id": window_id }), events, effects)
}

// ---------------------------------------------------------------------------
// v2:window.close
// ---------------------------------------------------------------------------

fn window_close(
    snapshot: &AppSessionSnapshot,
    params: &Map<String, Value>,
    context: &WindowLifecycleContext,
) -> WindowLifecycleTransition {
    let Some(window_id) = required_window_id(params) else {
        return error(
            snapshot,
            "invalid_params",
            "Missing or invalid window_id",
            None,
        );
    };
    let Some(index) = window_position(snapshot, &window_id) else {
        // QUIRK: data mints a window_ref for the nonexistent id — the ref is
        // added by the wrapper's canonical error-data decoration
        // (ControlCommandCoordinator+Window.swift:155-161).
        return error(
            snapshot,
            "not_found",
            "Window not found",
            Some(json!({ "window_id": window_id })),
        );
    };
    let success = json!({ "window_id": window_id });
    if snapshot.windows.len() == 1 {
        // Last-window close is app-quit-or-veto, never a plain window close
        // (AppDelegate.swift:16232-16239,12831-12856). The reply is success:
        // it means performClose was INVOKED, not that the window closed.
        let effect = if context.quit_confirmation_required {
            WindowLifecycleEffect::QuitConfirmation {
                window_id: window_id.clone(),
            }
        } else {
            WindowLifecycleEffect::AppTerminate {
                window_id: window_id.clone(),
            }
        };
        return ok_transition(snapshot.clone(), success, vec![], vec![effect]);
    }
    let mut next = snapshot.clone();
    let closed = next.windows.remove(index);
    let was_key = context.key_window_id.as_deref() == Some(window_id.as_str());
    // unregisterMainWindow sequence (AppDelegate.swift:16241-16305): closed
    // history, geometry persist, window.closed publish, remote detach,
    // active-pointer repoint (key window else first remaining), session save.
    let next_key_window_id = if was_key {
        context
            .previous_key_window_id
            .as_ref()
            .filter(|candidate| {
                next.windows
                    .iter()
                    .any(|window| window.window_id.as_ref() == Some(candidate))
            })
            .cloned()
            .or_else(|| {
                next.windows
                    .first()
                    .and_then(|window| window.window_id.clone())
            })
    } else {
        None
    };
    let mut events = vec![window_lifecycle_event(
        "window.closed",
        "appkit_close",
        &closed,
        &window_id,
        was_key,
        was_key,
    )];
    if let Some(next_key_window_id) = next_key_window_id.as_deref() {
        events.push(window_lifecycle_event(
            "window.unkeyed",
            "appkit_key",
            &closed,
            &window_id,
            false,
            false,
        ));
        let next_key_window = next
            .windows
            .iter()
            .find(|window| window.window_id.as_deref() == Some(next_key_window_id))
            .expect("next key window selected from remaining windows");
        events.push(window_lifecycle_event(
            "window.keyed",
            "appkit_key",
            next_key_window,
            next_key_window_id,
            true,
            false,
        ));
    }
    let mut effects = vec![
        WindowLifecycleEffect::RecordClosedWindowHistory {
            window_id: window_id.clone(),
        },
        WindowLifecycleEffect::PersistWindowGeometry {
            window_id: window_id.clone(),
        },
    ];
    for workspace in &closed.tab_manager.workspaces {
        let Some(remote) = workspace.remote.as_ref().filter(|remote| remote.enabled) else {
            continue;
        };
        let (Some(workspace_id), Some(destination)) =
            (workspace.workspace_id.clone(), remote.destination.clone())
        else {
            continue;
        };
        effects.push(WindowLifecycleEffect::RemoteWorkspaceDetach {
            workspace_id,
            destination,
        });
    }
    effects.push(WindowLifecycleEffect::WindowCloseCommit {
        window_id: window_id.clone(),
        next_key_window_id: next_key_window_id.clone(),
    });
    // Drop stale notifications for the closing window and each of its
    // workspaces, before the repoint/save (AppDelegate.swift:16274-16280).
    effects.push(WindowLifecycleEffect::ClearWindowNotifications {
        window_id: window_id.clone(),
        workspace_ids: closed
            .tab_manager
            .workspaces
            .iter()
            .filter_map(|workspace| workspace.workspace_id.clone())
            .collect(),
    });
    if let Some(repoint) = next_key_window_id {
        effects.push(WindowLifecycleEffect::SetActiveWindow { window_id: repoint });
    }
    effects.push(WindowLifecycleEffect::PersistSession);
    ok_transition(next, success, events, effects)
}

// ---------------------------------------------------------------------------
// v2:window.focus
// ---------------------------------------------------------------------------

fn window_focus(
    snapshot: &AppSessionSnapshot,
    params: &Map<String, Value>,
    context: &WindowLifecycleContext,
) -> WindowLifecycleTransition {
    let Some(window_id) = required_window_id(params) else {
        return error(
            snapshot,
            "invalid_params",
            "Missing or invalid window_id",
            None,
        );
    };
    let Some(index) = window_position(snapshot, &window_id) else {
        return error(
            snapshot,
            "not_found",
            "Window not found",
            Some(json!({ "window_id": window_id })),
        );
    };
    // window.focus IS in focusIntentV2Methods: the one window.* method allowed
    // to steal OS focus (TerminalController.swift:253-275). window.focused is
    // published whenever the id resolves, even when already key
    // (AppDelegate.swift:5693-5700). v2 does NOT move the active TabManager
    // pointer itself — it relies on becoming key (contract adversarial note).
    let window = &snapshot.windows[index];
    let mut events = Vec::new();
    if context.key_window_id.as_deref() != Some(window_id.as_str()) {
        if let Some(previous_key) = context.key_window_id.as_deref().and_then(|key_window_id| {
            snapshot
                .windows
                .iter()
                .find(|window| window.window_id.as_deref() == Some(key_window_id))
        }) {
            let previous_key_id = previous_key
                .window_id
                .as_deref()
                .expect("matched key window identity");
            events.push(window_lifecycle_event(
                "window.unkeyed",
                "appkit_key",
                previous_key,
                previous_key_id,
                false,
                true,
            ));
        }
        events.push(window_lifecycle_event(
            "window.keyed",
            "appkit_key",
            window,
            &window_id,
            true,
            false,
        ));
    }
    events.push(window_lifecycle_event(
        "window.focused",
        "focus_request",
        window,
        &window_id,
        true,
        true,
    ));
    let effects = vec![WindowLifecycleEffect::WindowFocus {
        window_id: window_id.clone(),
    }];
    ok_transition(
        snapshot.clone(),
        json!({ "window_id": window_id }),
        events,
        effects,
    )
}

// ---------------------------------------------------------------------------
// v2:surface.refresh
// ---------------------------------------------------------------------------

/// Refreshes EVERY terminal surface in the resolved workspace (or resolved
/// window-Dock); selectors only influence routing
/// (ControlCommandCoordinator+Surface2.swift:75-101,
/// TerminalController+ControlSurfaceContext3.swift:77-110). The response
/// counts terminals only; 0 is success. The actual redraw is the xterm.js
/// renderer refresh on Windows (platform_equivalent) — modeled as
/// `TerminalRefresh` effects.
fn surface_refresh(
    snapshot: &AppSessionSnapshot,
    params: &Map<String, Value>,
    context: &WindowLifecycleContext,
) -> WindowLifecycleTransition {
    // Dock branch first (TerminalController+ControlSurfaceContext3.swift:79-93).
    if let Some(owner_id) = dock_owner_from_workspace_selector(snapshot, params) {
        if let Some(dock) = snapshot
            .windows
            .iter()
            .find(|window| window.window_id.as_deref() == Some(owner_id.as_str()))
            .and_then(|window| window.dock.as_ref())
        {
            let effects: Vec<_> = dock
                .surfaces
                .iter()
                .filter(|surface| is_terminal_kind(&surface.kind))
                .map(|surface| WindowLifecycleEffect::TerminalRefresh {
                    surface_id: surface.surface_id.clone(),
                    reason: "terminalController.v2SurfaceRefresh.windowDock",
                })
                .collect();
            return ok_transition(
                snapshot.clone(),
                json!({
                    "window_id": owner_id,
                    "workspace_id": dock.workspace_id,
                    "refreshed": effects.len(),
                }),
                vec![],
                effects,
            );
        }
    }
    let scope = match scope(snapshot, params, &routing_context(context)) {
        Ok(scope) => scope,
        Err((code, message)) => return error(snapshot, code, message, None),
    };
    let Ok(model) = SurfaceLifecycleModel::from_app_session(snapshot) else {
        return error(
            snapshot,
            "internal_error",
            "Invalid surface lifecycle state",
            None,
        );
    };
    let projected = model
        .to_app_session(snapshot)
        .unwrap_or_else(|_| snapshot.clone());
    let workspace =
        &projected.windows[scope.window_index].tab_manager.workspaces[scope.workspace_index];
    let effects: Vec<_> = workspace
        .surfaces
        .as_deref()
        .unwrap_or_default()
        .iter()
        .filter(|surface| is_terminal_kind(&surface.kind))
        .map(|surface| WindowLifecycleEffect::TerminalRefresh {
            surface_id: surface.surface_id.clone(),
            reason: "terminalController.v2SurfaceRefresh",
        })
        .collect();
    ok_transition(
        snapshot.clone(),
        json!({
            "window_id": scope.window_id,
            "workspace_id": scope.workspace_id,
            "refreshed": effects.len(),
        }),
        vec![],
        effects,
    )
}

// ---------------------------------------------------------------------------
// v2:surface.resume.set / get / clear
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResumeOp {
    Set,
    Get,
    Clear,
}

/// Fixed selector validation order (surfaceResumeTargetValidationError,
/// ControlCommandCoordinator+Surface3.swift:14-25): window_id -> workspace_id
/// -> surface_id -> terminal_id -> tab_id, each rejected when present-non-null
/// but unresolvable. Runs BEFORE routing (pinned:
/// AppDelegateIssue2907RoutingTests.swift:587-640).
fn resume_selector_validation_error(params: &Map<String, Value>) -> Option<String> {
    for key in [
        "window_id",
        "workspace_id",
        "surface_id",
        "terminal_id",
        "tab_id",
    ] {
        let Some(value) = params.get(key) else {
            continue;
        };
        if value.is_null() {
            continue;
        }
        if valid_selector_id(value).is_none() {
            return Some(format!("Missing or invalid {key}"));
        }
    }
    None
}

fn trimmed_string(params: &Map<String, Value>, key: &str) -> Option<String> {
    let text = params.get(key)?.as_str()?.trim();
    (!text.is_empty()).then(|| text.to_owned())
}

/// checkpoint_id (snake) then checkpointId (camel) — snake wins
/// (ControlCommandCoordinator+Surface3.swift:56-57).
fn checkpoint_param(params: &Map<String, Value>) -> Option<String> {
    trimmed_string(params, "checkpoint_id").or_else(|| trimmed_string(params, "checkpointId"))
}

struct ResumeTarget {
    window_index: usize,
    workspace_index: usize,
    window_id: String,
    workspace_id: String,
    pane_id: Option<String>,
    surface_id: String,
}

fn workspace_terminal(workspace: &SessionWorkspaceSnapshot, surface_id: &str) -> Option<bool> {
    workspace
        .surfaces
        .as_deref()
        .unwrap_or_default()
        .iter()
        .find(|record| record.surface_id == surface_id)
        .map(|record| is_terminal_kind(&record.kind))
}

/// resolveSurfaceResumeTarget (TerminalController+ControlSurfaceContext4.swift:22-70):
/// (1) explicit target + workspace_id -> terminal panel IN that workspace of
/// the routing-resolved TabManager; (2) explicit target + window_id -> only
/// that window's workspaces; (3) explicit target alone -> GLOBAL locate across
/// all windows; (4/5) fallback scans are subsumed by (3) in this model; no
/// explicit target -> resolved workspace's focusedPanelId, which must be a
/// terminal. Explicit target precedence: surface_id > terminal_id > tab_id.
fn resolve_resume_target(
    projected: &AppSessionSnapshot,
    model: &SurfaceLifecycleModel,
    params: &Map<String, Value>,
    window_index: usize,
) -> Option<ResumeTarget> {
    let explicit = params
        .get("surface_id")
        .or_else(|| params.get("terminal_id"))
        .or_else(|| params.get("tab_id"))
        .and_then(valid_selector_id);
    let workspace_selector = params.get("workspace_id").and_then(valid_selector_id);
    let has_window_selector = params
        .get("window_id")
        .and_then(valid_selector_id)
        .is_some();

    let target_in = |window_index: usize, workspace_index: usize, surface_id: &str| {
        let window = projected.windows.get(window_index)?;
        let workspace = window.tab_manager.workspaces.get(workspace_index)?;
        if workspace_terminal(workspace, surface_id) != Some(true) {
            return None;
        }
        Some(ResumeTarget {
            window_index,
            workspace_index,
            window_id: window
                .window_id
                .clone()
                .unwrap_or_else(|| format!("window:{window_index}")),
            workspace_id: workspace
                .workspace_id
                .clone()
                .unwrap_or_else(|| format!("workspace:{workspace_index}")),
            pane_id: model
                .owner_of_surface(surface_id)
                .map(|owner| owner.pane_id.clone()),
            surface_id: surface_id.to_owned(),
        })
    };

    if let Some(surface_id) = explicit {
        if let Some(workspace_id) = workspace_selector {
            // (1) strict workspace scope inside the routing-resolved window.
            let workspace_index = projected
                .windows
                .get(window_index)?
                .tab_manager
                .workspaces
                .iter()
                .position(|workspace| workspace.workspace_id.as_deref() == Some(&workspace_id))?;
            return target_in(window_index, workspace_index, &surface_id);
        }
        if has_window_selector {
            // (2) search only the resolved window's workspaces.
            let window = projected.windows.get(window_index)?;
            return (0..window.tab_manager.workspaces.len())
                .find_map(|index| target_in(window_index, index, &surface_id));
        }
        // (3) global cross-window locate.
        return projected
            .windows
            .iter()
            .enumerate()
            .find_map(|(wi, window)| {
                (0..window.tab_manager.workspaces.len())
                    .find_map(|index| target_in(wi, index, &surface_id))
            });
    }
    // No explicit target: the resolved workspace's focused panel, which must
    // be a terminal.
    let window = projected.windows.get(window_index)?;
    let workspace_index = if let Some(workspace_id) = workspace_selector {
        window
            .tab_manager
            .workspaces
            .iter()
            .position(|workspace| workspace.workspace_id.as_deref() == Some(&workspace_id))?
    } else {
        usize::try_from(window.tab_manager.selected_workspace_index.unwrap_or(0)).ok()?
    };
    let focused = window
        .tab_manager
        .workspaces
        .get(workspace_index)?
        .focused_panel_id
        .clone()?;
    target_in(window_index, workspace_index, &focused)
}

fn workspace_binding<'a>(
    snapshot: &'a AppSessionSnapshot,
    target: &ResumeTarget,
) -> Option<&'a SessionSurfaceResumeBindingSnapshot> {
    snapshot
        .windows
        .get(target.window_index)?
        .tab_manager
        .workspaces
        .get(target.workspace_index)?
        .surface_resume_bindings
        .as_deref()
        .unwrap_or_default()
        .iter()
        .find(|row| row.surface_id == target.surface_id)
        .map(|row| &row.binding)
}

/// Canonical `surfaceResumeBindingPayload` with explicit nulls
/// (ControlCommandCoordinator+Surface3.swift:131-144,147-167). ONE
/// implementation shared by surface.resume.* and surface.list rows.
pub(super) fn resume_binding_payload(
    binding: Option<&SessionSurfaceResumeBindingSnapshot>,
) -> Value {
    match binding {
        None => Value::Null,
        Some(binding) => json!({
            "name": binding.name,
            "kind": binding.kind,
            "command": binding.command,
            "cwd": binding.cwd,
            "checkpoint_id": binding.checkpoint_id,
            "source": binding.source,
            "environment": binding.environment,
            "auto_resume": binding.auto_resume,
            "approval_policy": binding.approval_policy,
            "approval_record_id": binding.approval_record_id,
            "updated_at": binding.updated_at,
        }),
    }
}

/// The shared 10-key resume result shape (window/workspace/pane/surface ids
/// get their `*_ref` twins from the wrapper's decoration pass).
fn resume_result_payload(
    target: &ResumeTarget,
    cleared: bool,
    binding: Option<&SessionSurfaceResumeBindingSnapshot>,
) -> Value {
    json!({
        "window_id": target.window_id,
        "workspace_id": target.workspace_id,
        "pane_id": target.pane_id,
        "surface_id": target.surface_id,
        "cleared": cleared,
        "resume_binding": resume_binding_payload(binding),
    })
}

fn surface_resume(
    snapshot: &AppSessionSnapshot,
    params: &Map<String, Value>,
    context: &WindowLifecycleContext,
    op: ResumeOp,
) -> WindowLifecycleTransition {
    // Error order 1: selector validation BEFORE routing.
    if let Some(message) = resume_selector_validation_error(params) {
        return error(snapshot, "invalid_params", &message, None);
    }
    // Error order 2: routing must resolve a TabManager — with the resume
    // family's DISTINCT unavailable message.
    let Ok(window_index) = resolve_window_index(snapshot, params, &routing_context(context)) else {
        return error(snapshot, "unavailable", RESUME_UNAVAILABLE, None);
    };
    // Error order 3 (set only): command required, trimmed non-empty
    // (ControlCommandCoordinator+Surface3.swift:42-46).
    let command = match op {
        ResumeOp::Set => match trimmed_string(params, "command") {
            Some(command) => Some(command),
            None => return error(snapshot, "invalid_params", "Missing command", None),
        },
        _ => None,
    };
    let Ok(model) = SurfaceLifecycleModel::from_app_session(snapshot) else {
        return error(
            snapshot,
            "internal_error",
            "Invalid surface lifecycle state",
            None,
        );
    };
    let projected = model
        .to_app_session(snapshot)
        .unwrap_or_else(|_| snapshot.clone());
    // Error order 4: target resolution.
    let Some(target) = resolve_resume_target(&projected, &model, params, window_index) else {
        return error(snapshot, "not_found", "Surface not found", None);
    };

    match op {
        ResumeOp::Get => {
            let binding = workspace_binding(snapshot, &target);
            ok_transition(
                snapshot.clone(),
                resume_result_payload(&target, false, binding),
                vec![],
                vec![],
            )
        }
        ResumeOp::Set => {
            let command = command.expect("validated above");
            // QUIRK: 'process-detected' is rewritten to 'manual' before
            // storage (publicResumeSource, ControlCommandCoordinator+Surface3.swift:28-32).
            let source = trimmed_string(params, "source").map(|source| {
                if source == "process-detected" {
                    "manual".to_owned()
                } else {
                    source
                }
            });
            let requested_auto_resume =
                super::bool_param(params, &["auto_resume"]).unwrap_or(false);
            // auto_resume is HONORED ONLY when source=='agent-hook', else
            // forced false (:60; pinned CannotEnableAutoResumeFromSocket).
            let auto_resume = requested_auto_resume && source.as_deref() == Some("agent-hook");
            let binding = SessionSurfaceResumeBindingSnapshot {
                name: trimmed_string(params, "name"),
                kind: trimmed_string(params, "kind"),
                command,
                cwd: trimmed_string(params, "cwd"),
                checkpoint_id: checkpoint_param(params),
                source: source.clone(),
                environment: params
                    .get("environment")
                    .and_then(Value::as_object)
                    .map(|object| {
                        object
                            .iter()
                            .filter_map(|(key, value)| {
                                value.as_str().map(|value| (key.clone(), value.to_owned()))
                            })
                            .collect()
                    }),
                auto_resume,
                // Promptless path: no stored approval record. The blocking
                // proposal alert is modeled as ResumeApprovalPrompt below.
                approval_policy: None,
                approval_record_id: None,
                updated_at: context.now_epoch_seconds,
            };
            let mut next = snapshot.clone();
            let rows = next.windows[target.window_index].tab_manager.workspaces
                [target.workspace_index]
                .surface_resume_bindings
                .get_or_insert_with(Vec::new);
            rows.retain(|row| row.surface_id != target.surface_id);
            rows.push(SessionSurfaceResumeBindingRecordSnapshot {
                surface_id: target.surface_id.clone(),
                binding: binding.clone(),
            });
            let mut effects = Vec::new();
            if source.as_deref() == Some("agent-hook") {
                effects.push(WindowLifecycleEffect::ResumeApprovalPrompt {
                    surface_id: target.surface_id.clone(),
                    source: source.clone(),
                    auto_resume: requested_auto_resume,
                });
            }
            effects.push(WindowLifecycleEffect::PersistSession);
            let payload = resume_result_payload(&target, false, Some(&binding));
            ok_transition(next, payload, vec![], effects)
        }
        ResumeOp::Clear => {
            let current = workspace_binding(snapshot, &target).cloned();
            // Guards: expected checkpoint first, then expected source; a miss
            // is SUCCESS with cleared=false and the UNTOUCHED binding
            // (TerminalController+ControlSurfaceContext4.swift:242-268).
            // Both guards produce the same miss response, so evaluation order
            // is observationally checkpoint-then-source.
            let guard_miss =
                |expected: Option<String>,
                 stored: fn(&SessionSurfaceResumeBindingSnapshot) -> Option<&str>| {
                    expected.is_some_and(|expected| {
                        current.as_ref().and_then(stored) != Some(expected.as_str())
                    })
                };
            if guard_miss(checkpoint_param(params), |binding| {
                binding.checkpoint_id.as_deref()
            }) || guard_miss(trimmed_string(params, "source"), |binding| {
                binding.source.as_deref()
            }) {
                return ok_transition(
                    snapshot.clone(),
                    resume_result_payload(&target, false, current.as_ref()),
                    vec![],
                    vec![],
                );
            }
            // Guard pass (or no guards): remove and report cleared=true even
            // when nothing existed (:269-288 `_ =`).
            let mut next = snapshot.clone();
            let workspace = &mut next.windows[target.window_index].tab_manager.workspaces
                [target.workspace_index];
            if let Some(rows) = workspace.surface_resume_bindings.as_mut() {
                rows.retain(|row| row.surface_id != target.surface_id);
                if rows.is_empty() {
                    workspace.surface_resume_bindings = None;
                }
            }
            let effects = if current.is_some() {
                vec![WindowLifecycleEffect::PersistSession]
            } else {
                vec![]
            };
            ok_transition(
                next,
                resume_result_payload(&target, true, None),
                vec![],
                effects,
            )
        }
    }
}

// ---------------------------------------------------------------------------
// v1 line protocol: new_window / focus_window / close_window
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
// Variant names deliberately mirror the canonical v1 wire commands
// new_window / focus_window / close_window.
#[allow(clippy::enum_variant_names)]
pub(super) enum V1WindowCommand {
    NewWindow,
    FocusWindow(Option<String>),
    CloseWindow(Option<String>),
}

/// Parse a v1 request line into one of the three window commands. Any other
/// line falls through (None) to the JSON/v2 pipeline. Trailing arguments after
/// `new_window` are ignored (no arg validation in the dispatch case,
/// CLI/cmux.swift:4294-4296); focus/close take the first argument only.
pub(super) fn parse_v1_window_command(line: &str) -> Option<V1WindowCommand> {
    let mut tokens = line.split_whitespace();
    match tokens.next()? {
        "new_window" => Some(V1WindowCommand::NewWindow),
        "focus_window" => Some(V1WindowCommand::FocusWindow(
            tokens.next().map(str::to_owned),
        )),
        "close_window" => Some(V1WindowCommand::CloseWindow(
            tokens.next().map(str::to_owned),
        )),
        _ => None,
    }
}

/// Map a parsed v1 command to its v2 method + params, or the early v1 error
/// reply when the argument is missing (`ERROR: Invalid window id`,
/// TerminalController.swift:11899-11903). v1 focus_window additionally sets
/// the active TabManager (TerminalController.swift:11899-11910); on this port
/// the active pointer follows OS focus, which the shared WindowFocus effect
/// already requests.
pub(super) fn v1_window_request(
    command: &V1WindowCommand,
) -> Result<(&'static str, Map<String, Value>), String> {
    let with_window_id = |method: &'static str, id: &Option<String>| {
        let Some(id) = id else {
            return Err("ERROR: Invalid window id".to_owned());
        };
        let mut params = Map::new();
        params.insert("window_id".into(), json!(id));
        Ok((method, params))
    };
    match command {
        V1WindowCommand::NewWindow => Ok(("window.create", Map::new())),
        V1WindowCommand::FocusWindow(id) => with_window_id("window.focus", id),
        V1WindowCommand::CloseWindow(id) => with_window_id("window.close", id),
    }
}

/// Format the v1 line reply for a dispatched command result: `OK <uuid>` for
/// new_window (TerminalController.swift:11919-11920), bare `OK` for
/// focus_window/close_window (:11910,:11922-11928), and the byte-frozen
/// `ERROR:` lines otherwise.
pub(super) fn v1_window_reply(command: &V1WindowCommand, result: &ControlCallResult) -> String {
    match (command, result) {
        (V1WindowCommand::NewWindow, ControlCallResult::Ok(payload)) => {
            let id = Value::from(payload.clone());
            match id.get("window_id").and_then(Value::as_str) {
                Some(id) => format!("OK {id}"),
                None => "ERROR: Failed to create window".to_owned(),
            }
        }
        (V1WindowCommand::NewWindow, ControlCallResult::Err { .. }) => {
            "ERROR: Failed to create window".to_owned()
        }
        (_, ControlCallResult::Ok(_)) => "OK".to_owned(),
        (_, ControlCallResult::Err { code, .. }) if code == "invalid_params" => {
            "ERROR: Invalid window id".to_owned()
        }
        (_, ControlCallResult::Err { .. }) => "ERROR: Window not found".to_owned(),
    }
}
