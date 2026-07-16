//! Canonical Agent Hibernation policy and live tracking state.
//!
//! Process, terminal, timer, socket, and focus adapters intentionally live in
//! the follow-up runtime slice. Keeping this module side-effect free makes the
//! destructive hibernation decision deterministic and independently testable.
#![cfg_attr(not(test), allow(dead_code))]

use std::collections::{BTreeMap, BTreeSet};

use cmux_core::session::{
    AppSessionSnapshot, SessionRestorableAgentSnapshot, SessionSurfaceKindSnapshot,
    SessionSurfaceSnapshot, SessionWorkspaceLayoutSnapshot, SessionWorkspaceSnapshot,
};

pub(crate) const INITIAL_EVALUATION_DELAY_SECONDS: u64 = 5;
pub(crate) const EVALUATION_INTERVAL_SECONDS: u64 = 30;

const DEFAULT_IDLE_SECONDS: f64 = 5.0;
const DEFAULT_MAX_LIVE_TERMINALS: usize = 12;
const DEFAULT_CONFIRMATION_SECONDS: f64 = 60.0;
const MAX_IDLE_SECONDS: f64 = 7.0 * 24.0 * 60.0 * 60.0;
const MAX_CONFIRMATION_SECONDS: f64 = 60.0 * 60.0;

const ALLOWED_AGENT_LIFECYCLE_KEYS: &[&str] = &[
    "amp",
    "antigravity",
    "claude_code",
    "codebuddy",
    "codex",
    "copilot",
    "cursor",
    "factory",
    "gemini",
    "grok",
    "hermes-agent",
    "kiro",
    "omp",
    "opencode",
    "pi",
    "qoder",
    "rovodev",
];

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct AgentHibernationSettingsValues {
    pub(crate) enabled: bool,
    pub(crate) idle_seconds: f64,
    pub(crate) max_live_terminals: usize,
    pub(crate) confirmation_seconds: f64,
}

impl Default for AgentHibernationSettingsValues {
    fn default() -> Self {
        Self {
            enabled: false,
            idle_seconds: DEFAULT_IDLE_SECONDS,
            max_live_terminals: DEFAULT_MAX_LIVE_TERMINALS,
            confirmation_seconds: DEFAULT_CONFIRMATION_SECONDS,
        }
    }
}

pub(crate) fn sanitized_idle_seconds(value: f64) -> f64 {
    if value.is_finite() {
        value.round().clamp(DEFAULT_IDLE_SECONDS, MAX_IDLE_SECONDS)
    } else {
        DEFAULT_IDLE_SECONDS
    }
}

pub(crate) fn sanitized_max_live_terminals(value: i64) -> usize {
    value.clamp(1, 256) as usize
}

pub(crate) fn sanitized_confirmation_seconds(value: f64) -> f64 {
    if value.is_finite() {
        value
            .round()
            .clamp(DEFAULT_IDLE_SECONDS, MAX_CONFIRMATION_SECONDS)
    } else {
        DEFAULT_CONFIRMATION_SECONDS
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AgentLifecycleState {
    Unknown,
    Running,
    Idle,
    NeedsInput,
}

impl AgentLifecycleState {
    pub(crate) fn parse_cli(raw_value: &str) -> Option<Self> {
        match raw_value
            .trim()
            .to_ascii_lowercase()
            .replace('_', "-")
            .as_str()
        {
            "unknown" => Some(Self::Unknown),
            "running" => Some(Self::Running),
            "idle" => Some(Self::Idle),
            "needsinput" | "needs-input" => Some(Self::NeedsInput),
            _ => None,
        }
    }

    pub(crate) fn allows_hibernation(self) -> bool {
        self == Self::Idle
    }

    fn priority(self) -> u8 {
        match self {
            Self::Idle => 0,
            Self::Unknown => 1,
            Self::NeedsInput => 2,
            Self::Running => 3,
        }
    }
}

pub(crate) fn is_manual_lifecycle_key(key: &str) -> bool {
    key == "manual" || key.starts_with("manual:")
}

pub(crate) fn is_allowed_lifecycle_key(key: &str) -> bool {
    ALLOWED_AGENT_LIFECYCLE_KEYS.contains(&key.trim())
}

pub(crate) fn aggregate_lifecycle(
    states: impl IntoIterator<Item = AgentLifecycleState>,
    fallback: AgentLifecycleState,
) -> AgentLifecycleState {
    states
        .into_iter()
        .max_by_key(|state| state.priority())
        .unwrap_or(fallback)
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct PanelKey {
    pub(crate) workspace_id: String,
    pub(crate) panel_id: String,
}

impl PanelKey {
    pub(crate) fn new(workspace_id: impl Into<String>, panel_id: impl Into<String>) -> Self {
        Self {
            workspace_id: workspace_id.into(),
            panel_id: panel_id.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct AgentHibernationCandidate {
    pub(crate) key: PanelKey,
    pub(crate) has_restorable_agent: bool,
    pub(crate) is_live: bool,
    pub(crate) is_protected: bool,
    pub(crate) lifecycle: AgentLifecycleState,
    pub(crate) has_unconfirmed_terminal_input: bool,
    pub(crate) last_activity_at: f64,
}

pub(crate) fn selected_panel_keys(
    inputs: &[AgentHibernationCandidate],
    settings: AgentHibernationSettingsValues,
    now: f64,
) -> BTreeSet<PanelKey> {
    if !settings.enabled {
        return BTreeSet::new();
    }

    let live_restorable_count = inputs
        .iter()
        .filter(|input| input.has_restorable_agent && input.is_live)
        .count();
    let excess = live_restorable_count.saturating_sub(settings.max_live_terminals);
    if excess == 0 {
        return BTreeSet::new();
    }

    let mut eligible = inputs
        .iter()
        .filter(|input| {
            input.has_restorable_agent
                && input.is_live
                && !input.is_protected
                && input.lifecycle.allows_hibernation()
                && !input.has_unconfirmed_terminal_input
                && now - input.last_activity_at >= settings.idle_seconds
        })
        .collect::<Vec<_>>();
    eligible.sort_by(|left, right| {
        left.last_activity_at
            .total_cmp(&right.last_activity_at)
            .then_with(|| left.key.panel_id.cmp(&right.key.panel_id))
    });
    eligible
        .into_iter()
        .take(excess)
        .map(|candidate| candidate.key.clone())
        .collect()
}

#[derive(Debug, Clone)]
struct TailFingerprintSample {
    fingerprint: String,
    stable_since: f64,
}

#[derive(Debug, Clone, Copy)]
struct Confirmation {
    sampled_at: f64,
    due_at: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConfirmationDecision {
    Waiting,
    Ready,
    Reset,
}

#[derive(Debug, Default)]
pub(crate) struct AgentHibernationPolicy {
    lifecycle_by_panel: BTreeMap<PanelKey, BTreeMap<String, AgentLifecycleState>>,
    activity_by_panel: BTreeMap<PanelKey, f64>,
    terminal_input_by_panel: BTreeMap<PanelKey, f64>,
    lifecycle_change_by_panel: BTreeMap<PanelKey, f64>,
    confirmations: BTreeMap<PanelKey, (String, Confirmation)>,
    tail_fingerprints: BTreeMap<PanelKey, TailFingerprintSample>,
}

impl AgentHibernationPolicy {
    fn record_activity(&mut self, key: PanelKey, recorded_at: f64) {
        self.activity_by_panel.insert(key.clone(), recorded_at);
        self.confirmations.remove(&key);
    }

    pub(crate) fn record_focus(&mut self, key: PanelKey, recorded_at: f64) {
        self.record_activity(key, recorded_at);
    }

    pub(crate) fn record_terminal_input(&mut self, key: PanelKey, recorded_at: f64) {
        self.record_activity(key.clone(), recorded_at);
        self.terminal_input_by_panel.insert(key, recorded_at);
    }

    pub(crate) fn record_lifecycle(
        &mut self,
        key: PanelKey,
        status_key: impl Into<String>,
        state: AgentLifecycleState,
        recorded_at: f64,
    ) {
        self.record_activity(key.clone(), recorded_at);
        self.lifecycle_by_panel
            .entry(key.clone())
            .or_default()
            .insert(status_key.into(), state);
        self.lifecycle_change_by_panel.insert(key, recorded_at);
    }

    pub(crate) fn lifecycle(
        &self,
        key: &PanelKey,
        fallback: AgentLifecycleState,
    ) -> AgentLifecycleState {
        aggregate_lifecycle(
            self.lifecycle_by_panel
                .get(key)
                .into_iter()
                .flat_map(|states| states.values().copied()),
            fallback,
        )
    }

    pub(crate) fn has_unconfirmed_terminal_input(&self, key: &PanelKey) -> bool {
        self.terminal_input_by_panel
            .get(key)
            .is_some_and(|input_at| {
                *input_at
                    > self
                        .lifecycle_change_by_panel
                        .get(key)
                        .copied()
                        .unwrap_or_default()
            })
    }

    pub(crate) fn effective_last_activity_at(
        &self,
        key: &PanelKey,
        indexed_activity_at: f64,
        runtime_created_at: f64,
    ) -> f64 {
        indexed_activity_at
            .max(runtime_created_at)
            .max(self.activity_by_panel.get(key).copied().unwrap_or_default())
    }

    pub(crate) fn observe_tail_fingerprint(
        &mut self,
        key: &PanelKey,
        fingerprint: Option<&str>,
        is_live_eligible: bool,
        last_activity_at: f64,
        now: f64,
    ) -> Option<f64> {
        let Some(fingerprint) = fingerprint.filter(|_| is_live_eligible) else {
            self.tail_fingerprints.remove(key);
            self.confirmations.remove(key);
            return None;
        };
        let previous = self.tail_fingerprints.get(key);
        if previous.is_some_and(|sample| sample.fingerprint == fingerprint) {
            return previous.map(|sample| sample.stable_since);
        }
        let stable_since = tail_fingerprint_stable_since(
            previous.map(|sample| sample.fingerprint.as_str()),
            previous.map(|sample| sample.stable_since),
            fingerprint,
            last_activity_at,
            now,
        );
        self.tail_fingerprints.insert(
            key.clone(),
            TailFingerprintSample {
                fingerprint: fingerprint.to_owned(),
                stable_since,
            },
        );
        self.confirmations.remove(key);
        Some(stable_since)
    }

    pub(crate) fn confirm(
        &mut self,
        key: &PanelKey,
        fingerprint: Option<&str>,
        effective_last_activity_at: f64,
        confirmation_seconds: f64,
        now: f64,
    ) -> ConfirmationDecision {
        if let Some((expected_fingerprint, confirmation)) = self.confirmations.get(key).cloned() {
            if now < confirmation.due_at {
                return ConfirmationDecision::Waiting;
            }
            self.confirmations.remove(key);
            if effective_last_activity_at > confirmation.sampled_at
                || fingerprint != Some(expected_fingerprint.as_str())
            {
                return ConfirmationDecision::Reset;
            }
            return ConfirmationDecision::Ready;
        }

        let Some(fingerprint) = fingerprint else {
            return ConfirmationDecision::Reset;
        };
        self.confirmations.insert(
            key.clone(),
            (
                fingerprint.to_owned(),
                Confirmation {
                    sampled_at: now,
                    due_at: now + confirmation_seconds,
                },
            ),
        );
        ConfirmationDecision::Waiting
    }

    pub(crate) fn has_pending_confirmation(&self, key: &PanelKey) -> bool {
        self.confirmations.contains_key(key)
    }

    pub(crate) fn clear_panel(&mut self, key: &PanelKey) {
        self.lifecycle_by_panel.remove(key);
        self.activity_by_panel.remove(key);
        self.terminal_input_by_panel.remove(key);
        self.lifecycle_change_by_panel.remove(key);
        self.confirmations.remove(key);
        self.tail_fingerprints.remove(key);
    }

    pub(crate) fn prune(
        &mut self,
        current_keys: &BTreeSet<PanelKey>,
        selected_keys: &BTreeSet<PanelKey>,
    ) {
        self.lifecycle_by_panel
            .retain(|key, _| current_keys.contains(key));
        self.activity_by_panel
            .retain(|key, _| current_keys.contains(key));
        self.terminal_input_by_panel
            .retain(|key, _| current_keys.contains(key));
        self.lifecycle_change_by_panel
            .retain(|key, _| current_keys.contains(key));
        self.tail_fingerprints
            .retain(|key, _| current_keys.contains(key));
        self.confirmations
            .retain(|key, _| current_keys.contains(key) && selected_keys.contains(key));
    }

    pub(crate) fn has_panel(&self, key: &PanelKey) -> bool {
        self.lifecycle_by_panel.contains_key(key)
            || self.activity_by_panel.contains_key(key)
            || self.terminal_input_by_panel.contains_key(key)
            || self.lifecycle_change_by_panel.contains_key(key)
            || self.confirmations.contains_key(key)
            || self.tail_fingerprints.contains_key(key)
    }

    pub(crate) fn latest_lifecycle_change_at(&self, key: &PanelKey) -> Option<f64> {
        self.lifecycle_change_by_panel.get(key).copied()
    }
}

pub(crate) fn process_fallback_fingerprint(
    kind: &str,
    session_id: &str,
    process_ids: impl IntoIterator<Item = i32>,
) -> String {
    format!(
        "process:{kind}:{session_id}:{}",
        process_identity_fingerprint(process_ids)
    )
}

pub(crate) fn scrollback_fingerprint(
    tail: &str,
    process_ids: impl IntoIterator<Item = i32>,
) -> String {
    format!(
        "scrollback:{}:{tail}",
        process_identity_fingerprint(process_ids)
    )
}

fn process_identity_fingerprint(process_ids: impl IntoIterator<Item = i32>) -> String {
    let mut process_ids = process_ids.into_iter().collect::<Vec<_>>();
    process_ids.sort_unstable();
    process_ids
        .into_iter()
        .map(|process_id| process_id.to_string())
        .collect::<Vec<_>>()
        .join(",")
}

pub(crate) fn tail_fingerprint_stable_since(
    previous_fingerprint: Option<&str>,
    previous_stable_since: Option<f64>,
    current_fingerprint: &str,
    last_activity_at: f64,
    now: f64,
) -> f64 {
    if previous_fingerprint == Some(current_fingerprint) {
        previous_stable_since.unwrap_or(last_activity_at)
    } else {
        now
    }
}

fn layout_selected_panel_ids(
    layout: &SessionWorkspaceLayoutSnapshot,
    selected: &mut BTreeSet<String>,
) {
    match layout {
        SessionWorkspaceLayoutSnapshot::Pane(pane) => {
            if let Some(panel_id) = pane
                .selected_panel_id
                .as_ref()
                .or_else(|| pane.panel_ids.first())
            {
                selected.insert(panel_id.clone());
            }
        }
        SessionWorkspaceLayoutSnapshot::Split(split) => {
            layout_selected_panel_ids(&split.first, selected);
            layout_selected_panel_ids(&split.second, selected);
        }
    }
}

fn rendered_panel_ids(workspace: &SessionWorkspaceSnapshot) -> BTreeSet<String> {
    if let Some(panel_id) = workspace.zoomed_panel_id.as_ref() {
        return BTreeSet::from([panel_id.clone()]);
    }
    if workspace.layout_mode.as_deref() == Some("canvas") {
        return workspace
            .canvas_panes
            .as_deref()
            .unwrap_or_default()
            .iter()
            .filter_map(|pane| {
                pane.selected_panel_id
                    .as_ref()
                    .or_else(|| pane.panel_ids.as_ref().and_then(|ids| ids.first()))
                    .cloned()
                    .or_else(|| Some(pane.panel_id.clone()))
            })
            .collect();
    }
    let mut selected = BTreeSet::new();
    if let Some(layout) = workspace.layout.as_ref() {
        layout_selected_panel_ids(layout, &mut selected);
    }
    selected
}

pub(crate) fn protected_panel_ids(
    snapshot: &AppSessionSnapshot,
    visible_window_ids: &BTreeSet<String>,
) -> BTreeSet<String> {
    snapshot
        .windows
        .iter()
        .filter(|window| {
            window
                .window_id
                .as_ref()
                .is_some_and(|window_id| visible_window_ids.contains(window_id))
        })
        .filter_map(|window| {
            window
                .selected_workspace_id
                .as_deref()
                .and_then(|selected_id| {
                    window
                        .tab_manager
                        .workspaces
                        .iter()
                        .find(|workspace| workspace.workspace_id.as_deref() == Some(selected_id))
                })
                .or_else(|| {
                    usize::try_from(window.tab_manager.selected_workspace_index?)
                        .ok()
                        .and_then(|index| window.tab_manager.workspaces.get(index))
                })
        })
        .flat_map(rendered_panel_ids)
        .collect()
}

fn normalized_value(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

fn restorable_agents_are_compatible(
    left: &SessionRestorableAgentSnapshot,
    right: &SessionRestorableAgentSnapshot,
) -> bool {
    normalized_value(Some(&left.kind)) == normalized_value(Some(&right.kind))
        && normalized_value(Some(&left.session_id)) == normalized_value(Some(&right.session_id))
}

fn dormant_surface_binding_is_valid(
    workspace: &SessionWorkspaceSnapshot,
    surface: &SessionSurfaceSnapshot,
) -> bool {
    if !matches!(surface.kind, SessionSurfaceKindSnapshot::Terminal) {
        return false;
    }
    let Some(startup) = surface
        .terminal_startup
        .as_ref()
        .filter(|startup| startup.hibernation.is_some())
    else {
        return true;
    };
    let Some(agent) = startup.resume_binding.as_deref() else {
        return false;
    };
    if normalized_value(Some(&agent.kind)).is_none()
        || normalized_value(Some(&agent.session_id)).is_none()
        || normalized_value(agent.resume_command.as_deref()).is_none()
    {
        return false;
    }
    if workspace
        .restorable_agent_snapshots
        .as_deref()
        .and_then(|rows| rows.iter().find(|row| row.panel_id == surface.surface_id))
        .is_some_and(|indexed| !restorable_agents_are_compatible(agent, &indexed.snapshot))
    {
        return false;
    }
    workspace
        .surface_resume_bindings
        .as_deref()
        .unwrap_or_default()
        .iter()
        .filter(|row| {
            row.surface_id == surface.surface_id
                && row.binding.source.as_deref() == Some("agent-hook")
        })
        .all(|row| {
            normalized_value(row.binding.checkpoint_id.as_deref())
                .is_none_or(|checkpoint| checkpoint == agent.session_id.trim())
                && normalized_value(row.binding.kind.as_deref())
                    .is_none_or(|kind| kind == agent.kind.trim())
        })
}

pub(crate) fn sanitize_invalid_hibernation(snapshot: &mut AppSessionSnapshot) -> Vec<String> {
    let mut sanitized = BTreeSet::new();
    for window in &mut snapshot.windows {
        for workspace in &mut window.tab_manager.workspaces {
            let invalid = workspace
                .surfaces
                .as_deref()
                .unwrap_or_default()
                .iter()
                .filter(|surface| {
                    surface
                        .terminal_startup
                        .as_ref()
                        .is_some_and(|startup| startup.hibernation.is_some())
                        && !dormant_surface_binding_is_valid(workspace, surface)
                })
                .map(|surface| surface.surface_id.clone())
                .collect::<BTreeSet<_>>();
            if invalid.is_empty() {
                continue;
            }

            if let Some(surfaces) = workspace.surfaces.as_mut() {
                for surface in surfaces
                    .iter_mut()
                    .filter(|surface| invalid.contains(&surface.surface_id))
                {
                    if let Some(startup) = surface.terminal_startup.as_mut() {
                        startup.hibernation = None;
                        startup.resume_binding = None;
                        startup.command = None;
                        startup.tmux_start_command = None;
                        startup.initial_input = None;
                    }
                }
            }
            if let Some(rows) = workspace.restorable_agent_snapshots.as_mut() {
                rows.retain(|row| !invalid.contains(&row.panel_id));
                if rows.is_empty() {
                    workspace.restorable_agent_snapshots = None;
                }
            }
            if let Some(rows) = workspace.surface_resume_bindings.as_mut() {
                rows.retain(|row| {
                    !invalid.contains(&row.surface_id)
                        || row.binding.source.as_deref() != Some("agent-hook")
                });
                if rows.is_empty() {
                    workspace.surface_resume_bindings = None;
                }
            }
            sanitized.extend(invalid);
        }
    }
    sanitized.into_iter().collect()
}
