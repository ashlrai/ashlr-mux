use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Condvar, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use cmux_agent::{
    append_workstream_item, load_recent_workstream_items, make_item, next_context,
    WorkstreamContext, WorkstreamDecision, WorkstreamEvent, WorkstreamExitPlanMode, WorkstreamItem,
    WorkstreamKind, WorkstreamPayload, WorkstreamPermissionMode, WorkstreamStatus,
};
use serde::Serialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, State};

pub const FEED_CHANGED_EVENT: &str = "cmux://feed-changed";
const FEED_ITEM_LIMIT: usize = 2_000;
const FEED_INITIAL_LOAD_LIMIT: usize = 300;

pub struct FeedState {
    inner: Mutex<FeedInner>,
    decisions_changed: Condvar,
}

#[derive(Default)]
struct FeedInner {
    items: Vec<WorkstreamItem>,
    last_context_by_workstream: HashMap<String, WorkstreamContext>,
    decisions_by_request: HashMap<String, WorkstreamDecision>,
    persistence_path: Option<PathBuf>,
    home_path: Option<String>,
}

impl Default for FeedState {
    fn default() -> Self {
        Self {
            inner: Mutex::new(FeedInner::default()),
            decisions_changed: Condvar::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct FeedListReply {
    pub items: Vec<FeedItemView>,
    pub pending_count: usize,
    pub total_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct FeedItemView {
    pub id: String,
    pub workstream_id: String,
    pub source: String,
    pub kind: String,
    pub status: String,
    pub title: Option<String>,
    pub cwd: Option<String>,
    pub request_id: Option<String>,
    pub tool_name: Option<String>,
    pub tool_input: Option<String>,
    pub plan: Option<String>,
    pub default_mode: Option<String>,
    pub questions: Vec<FeedQuestionView>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FeedQuestionView {
    pub id: String,
    pub header: Option<String>,
    pub prompt: String,
    pub multi_select: bool,
    pub options: Vec<FeedQuestionOptionView>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FeedQuestionOptionView {
    pub id: String,
    pub label: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FeedPushReply {
    pub status: &'static str,
    pub item_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decision: Option<Value>,
}

pub struct FeedInsert {
    pub item_id: String,
    pub request_id: Option<String>,
}

impl FeedState {
    pub fn configure_persistence(
        &self,
        path: PathBuf,
        home_path: Option<String>,
    ) -> Result<FeedListReply, String> {
        let loaded = load_recent_workstream_items(&path, FEED_INITIAL_LOAD_LIMIT);
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| "Feed state is unavailable".to_string())?;
        inner.persistence_path = Some(path);
        inner.home_path = home_path;
        let items = loaded.map_err(|error| error.to_string())?;
        inner.items = items;
        inner.last_context_by_workstream = rebuild_context_index(&inner.items);
        Ok(feed_list_reply(&inner.items))
    }

    pub fn list(&self) -> Result<FeedListReply, String> {
        let inner = self
            .inner
            .lock()
            .map_err(|_| "Feed state is unavailable".to_string())?;
        Ok(feed_list_reply(&inner.items))
    }

    pub fn ingest(&self, mut event: WorkstreamEvent) -> Result<FeedInsert, String> {
        if event.received_at == 0.0 {
            event.received_at = reference_date_now();
        }
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| "Feed state is unavailable".to_string())?;
        let previous_context = inner.last_context_by_workstream.get(&event.session_id);
        let item = make_item(&event, previous_context, &|_| None);
        if let Some(context) = next_context(&item, previous_context) {
            inner
                .last_context_by_workstream
                .insert(event.session_id.clone(), context);
        }
        let item_id = item.id.to_string();
        let request_id = request_id_for_item(&item).map(str::to_string);
        if let Some(request_id) = &request_id {
            inner.decisions_by_request.remove(request_id);
        }
        inner.items.push(item);
        if inner.items.len() > FEED_ITEM_LIMIT {
            let overflow = inner.items.len() - FEED_ITEM_LIMIT;
            inner.items.drain(..overflow);
        }
        if let (Some(path), Some(item)) = (inner.persistence_path.as_deref(), inner.items.last()) {
            let _ = append_workstream_item(path, item, inner.home_path.as_deref());
        }
        Ok(FeedInsert {
            item_id,
            request_id,
        })
    }

    pub fn wait_for_decision(
        &self,
        insert: &FeedInsert,
        timeout: Duration,
    ) -> Result<FeedPushReply, String> {
        let Some(request_id) = insert.request_id.as_deref() else {
            return Ok(FeedPushReply {
                status: "acknowledged",
                item_id: insert.item_id.clone(),
                decision: None,
            });
        };
        if timeout.is_zero() {
            return Ok(FeedPushReply {
                status: "acknowledged",
                item_id: insert.item_id.clone(),
                decision: None,
            });
        }

        let inner = self
            .inner
            .lock()
            .map_err(|_| "Feed state is unavailable".to_string())?;
        let (mut inner, wait) = self
            .decisions_changed
            .wait_timeout_while(inner, timeout, |inner| {
                !inner.decisions_by_request.contains_key(request_id)
            })
            .map_err(|_| "Feed state is unavailable".to_string())?;
        if let Some(decision) = inner.decisions_by_request.remove(request_id) {
            return Ok(FeedPushReply {
                status: "resolved",
                item_id: insert.item_id.clone(),
                decision: Some(decision_value(&decision)),
            });
        }
        if wait.timed_out() {
            mark_item_expired(&mut inner.items, &insert.item_id, reference_date_now());
        }
        Ok(FeedPushReply {
            status: "timed_out",
            item_id: insert.item_id.clone(),
            decision: None,
        })
    }

    pub fn resolve(
        &self,
        request_id: &str,
        decision: WorkstreamDecision,
    ) -> Result<FeedListReply, String> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| "Feed state is unavailable".to_string())?;
        let Some(item) =
            inner.items.iter_mut().rev().find(|item| {
                request_id_for_item(item) == Some(request_id) && item.status.is_pending()
            })
        else {
            return Err(format!("Feed request '{request_id}' was not found"));
        };
        let now = reference_date_now();
        item.status = WorkstreamStatus::Resolved {
            decision: decision.clone(),
            at: now,
        };
        item.updated_at = now;
        inner
            .decisions_by_request
            .insert(request_id.to_string(), decision);
        let reply = feed_list_reply(&inner.items);
        self.decisions_changed.notify_all();
        Ok(reply)
    }
}

pub fn bootstrap_feed_history(state: &FeedState) {
    let Some(home) = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")) else {
        eprintln!("[feed] home directory unavailable; history persistence disabled");
        return;
    };
    let home = PathBuf::from(home);
    let path = home.join(".cmuxterm").join("workstream.jsonl");
    if let Err(error) = state.configure_persistence(path, Some(home.to_string_lossy().into_owned()))
    {
        eprintln!("[feed] failed to restore history: {error}");
    }
}

pub fn permission_decision(mode: &str) -> Result<WorkstreamDecision, String> {
    let mode = match mode {
        "once" => WorkstreamPermissionMode::Once,
        "always" => WorkstreamPermissionMode::Always,
        "all" => WorkstreamPermissionMode::All,
        "bypass" => WorkstreamPermissionMode::Bypass,
        "deny" => WorkstreamPermissionMode::Deny,
        _ => return Err("mode must be once|always|all|bypass|deny".to_string()),
    };
    Ok(WorkstreamDecision::Permission { mode })
}

pub fn question_decision(selections: Vec<String>) -> WorkstreamDecision {
    WorkstreamDecision::Question { selections }
}

pub fn exit_plan_decision(
    mode: &str,
    feedback: Option<String>,
) -> Result<WorkstreamDecision, String> {
    let mode = match mode {
        "ultraplan" => WorkstreamExitPlanMode::Ultraplan,
        "bypassPermissions" => WorkstreamExitPlanMode::BypassPermissions,
        "autoAccept" => WorkstreamExitPlanMode::AutoAccept,
        "manual" => WorkstreamExitPlanMode::Manual,
        "deny" => WorkstreamExitPlanMode::Deny,
        _ => {
            return Err(
                "mode must be ultraplan|bypassPermissions|autoAccept|manual|deny".to_string(),
            )
        }
    };
    Ok(WorkstreamDecision::ExitPlan { mode, feedback })
}

fn feed_list_reply(items: &[WorkstreamItem]) -> FeedListReply {
    let pending_count = items.iter().filter(|item| item.status.is_pending()).count();
    FeedListReply {
        items: items.iter().rev().map(feed_item_view).collect(),
        pending_count,
        total_count: items.len(),
    }
}

fn rebuild_context_index(items: &[WorkstreamItem]) -> HashMap<String, WorkstreamContext> {
    let mut ordered = items.iter().collect::<Vec<_>>();
    ordered.sort_by(|left, right| left.created_at.total_cmp(&right.created_at));
    let mut contexts = HashMap::new();
    for item in ordered {
        let previous = contexts.get(&item.workstream_id);
        if let Some(context) = next_context(item, previous) {
            contexts.insert(item.workstream_id.clone(), context);
        }
    }
    contexts
}

fn feed_item_view(item: &WorkstreamItem) -> FeedItemView {
    let (request_id, tool_name, tool_input, plan, default_mode, questions) = match &item.payload {
        WorkstreamPayload::PermissionRequest {
            request_id,
            tool_name,
            tool_input_json,
            ..
        } => (
            Some(request_id.clone()),
            Some(tool_name.clone()),
            Some(tool_input_json.clone()),
            None,
            None,
            Vec::new(),
        ),
        WorkstreamPayload::ExitPlan {
            request_id,
            plan,
            default_mode,
        } => (
            Some(request_id.clone()),
            None,
            None,
            Some(plan.clone()),
            Some(exit_plan_mode_name(*default_mode).to_string()),
            Vec::new(),
        ),
        WorkstreamPayload::Question {
            request_id,
            questions,
        } => (
            Some(request_id.clone()),
            None,
            None,
            None,
            None,
            questions
                .iter()
                .map(|question| FeedQuestionView {
                    id: question.id.clone(),
                    header: question.header.clone(),
                    prompt: question.prompt.clone(),
                    multi_select: question.multi_select,
                    options: question
                        .options
                        .iter()
                        .map(|option| FeedQuestionOptionView {
                            id: option.id.clone(),
                            label: option.label.clone(),
                            description: option.description.clone(),
                        })
                        .collect(),
                })
                .collect(),
        ),
        WorkstreamPayload::ToolUse {
            tool_name,
            tool_input_json,
        } => (
            None,
            Some(tool_name.clone()),
            Some(tool_input_json.clone()),
            None,
            None,
            Vec::new(),
        ),
        _ => (None, None, None, None, None, Vec::new()),
    };
    FeedItemView {
        id: item.id.to_string(),
        workstream_id: item.workstream_id.clone(),
        source: item.source.wire_name().to_string(),
        kind: workstream_kind_name(item.kind).to_string(),
        status: workstream_status_name(&item.status).to_string(),
        title: item.title.clone(),
        cwd: item.cwd.clone(),
        request_id,
        tool_name,
        tool_input,
        plan,
        default_mode,
        questions,
    }
}

fn request_id_for_item(item: &WorkstreamItem) -> Option<&str> {
    match &item.payload {
        WorkstreamPayload::PermissionRequest { request_id, .. }
        | WorkstreamPayload::ExitPlan { request_id, .. }
        | WorkstreamPayload::Question { request_id, .. } => Some(request_id),
        _ => None,
    }
}

fn mark_item_expired(items: &mut [WorkstreamItem], item_id: &str, at: f64) {
    if let Some(item) = items
        .iter_mut()
        .find(|item| item.id.to_string() == item_id && item.status.is_pending())
    {
        item.status = WorkstreamStatus::Expired { at };
        item.updated_at = at;
    }
}

fn decision_value(decision: &WorkstreamDecision) -> Value {
    match decision {
        WorkstreamDecision::Permission { mode } => {
            json!({"kind": "permission", "mode": permission_mode_name(*mode)})
        }
        WorkstreamDecision::ExitPlan { mode, feedback } => {
            let mut value = json!({"kind": "exit_plan", "mode": exit_plan_mode_name(*mode)});
            if let Some(feedback) = feedback.as_ref().filter(|value| !value.is_empty()) {
                value["feedback"] = json!(feedback);
            }
            value
        }
        WorkstreamDecision::Question { selections } => {
            json!({"kind": "question", "selections": selections})
        }
    }
}

fn workstream_kind_name(kind: WorkstreamKind) -> &'static str {
    match kind {
        WorkstreamKind::PermissionRequest => "permissionRequest",
        WorkstreamKind::ExitPlan => "exitPlan",
        WorkstreamKind::Question => "question",
        WorkstreamKind::ToolUse => "toolUse",
        WorkstreamKind::ToolResult => "toolResult",
        WorkstreamKind::UserPrompt => "userPrompt",
        WorkstreamKind::AssistantMessage => "assistantMessage",
        WorkstreamKind::SessionStart => "sessionStart",
        WorkstreamKind::SessionEnd => "sessionEnd",
        WorkstreamKind::Stop => "stop",
        WorkstreamKind::Todos => "todos",
    }
}

fn workstream_status_name(status: &WorkstreamStatus) -> &'static str {
    match status {
        WorkstreamStatus::Pending {} => "pending",
        WorkstreamStatus::Resolved { .. } => "resolved",
        WorkstreamStatus::Expired { .. } => "expired",
        WorkstreamStatus::Telemetry {} => "telemetry",
    }
}

fn permission_mode_name(mode: WorkstreamPermissionMode) -> &'static str {
    match mode {
        WorkstreamPermissionMode::Once => "once",
        WorkstreamPermissionMode::Always => "always",
        WorkstreamPermissionMode::All => "all",
        WorkstreamPermissionMode::Bypass => "bypass",
        WorkstreamPermissionMode::Deny => "deny",
    }
}

fn exit_plan_mode_name(mode: WorkstreamExitPlanMode) -> &'static str {
    match mode {
        WorkstreamExitPlanMode::Ultraplan => "ultraplan",
        WorkstreamExitPlanMode::BypassPermissions => "bypassPermissions",
        WorkstreamExitPlanMode::AutoAccept => "autoAccept",
        WorkstreamExitPlanMode::Manual => "manual",
        WorkstreamExitPlanMode::Deny => "deny",
    }
}

fn reference_date_now() -> f64 {
    const APPLE_REFERENCE_UNIX_SECONDS: f64 = 978_307_200.0;
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs_f64() - APPLE_REFERENCE_UNIX_SECONDS)
        .unwrap_or(0.0)
}

#[tauri::command]
pub fn feed_list(state: State<'_, FeedState>) -> Result<FeedListReply, String> {
    state.list()
}

#[tauri::command]
pub fn feed_resolve(
    app: AppHandle,
    request_id: String,
    decision: Value,
    state: State<'_, FeedState>,
) -> Result<FeedListReply, String> {
    let decision = decision_from_value(&decision)?;
    let reply = state.resolve(&request_id, decision)?;
    app.emit(FEED_CHANGED_EVENT, &reply)
        .map_err(|error| error.to_string())?;
    Ok(reply)
}

pub fn decision_from_value(value: &Value) -> Result<WorkstreamDecision, String> {
    let kind = value
        .get("kind")
        .and_then(Value::as_str)
        .ok_or_else(|| "Feed decision requires kind".to_string())?;
    match kind {
        "permission" => permission_decision(
            value
                .get("mode")
                .and_then(Value::as_str)
                .ok_or_else(|| "Feed permission decision requires mode".to_string())?,
        ),
        "question" => Ok(question_decision(
            value
                .get("selections")
                .and_then(Value::as_array)
                .ok_or_else(|| "Feed question decision requires selections".to_string())?
                .iter()
                .map(|selection| {
                    selection
                        .as_str()
                        .map(str::to_string)
                        .ok_or_else(|| "Feed question selections must be strings".to_string())
                })
                .collect::<Result<Vec<_>, _>>()?,
        )),
        "exit_plan" => exit_plan_decision(
            value
                .get("mode")
                .and_then(Value::as_str)
                .ok_or_else(|| "Feed exit-plan decision requires mode".to_string())?,
            value
                .get("feedback")
                .and_then(Value::as_str)
                .map(str::to_string),
        ),
        _ => Err(format!("Unknown Feed decision kind '{kind}'")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cmux_agent::HookEventName;

    fn permission_event() -> WorkstreamEvent {
        WorkstreamEvent::new("claude-session", HookEventName::PermissionRequest, "claude")
            .with_request_id("request-1")
            .with_tool_name("Write")
            .with_tool_input_json(r#"{"file_path":"C:/repo/README.md"}"#)
            .with_cwd("C:/repo")
    }

    #[test]
    fn ingests_projects_and_resolves_canonical_permission_items() {
        let state = FeedState::default();
        let insert = state.ingest(permission_event()).unwrap();
        let list = state.list().unwrap();
        assert_eq!(list.pending_count, 1);
        assert_eq!(list.items[0].kind, "permissionRequest");
        assert_eq!(list.items[0].request_id.as_deref(), Some("request-1"));
        assert_eq!(list.items[0].tool_name.as_deref(), Some("Write"));

        state
            .resolve("request-1", permission_decision("once").unwrap())
            .unwrap();
        let reply = state
            .wait_for_decision(&insert, Duration::from_millis(1))
            .unwrap();
        assert_eq!(reply.status, "resolved");
        assert_eq!(
            reply.decision,
            Some(json!({"kind": "permission", "mode": "once"}))
        );
        assert_eq!(state.list().unwrap().items[0].status, "resolved");
    }

    #[test]
    fn telemetry_is_retained_without_becoming_pending() {
        let state = FeedState::default();
        let event = WorkstreamEvent::new("codex-session", HookEventName::PreToolUse, "codex")
            .with_tool_name("Shell")
            .with_tool_input_json("cargo test");
        state.ingest(event).unwrap();
        let list = state.list().unwrap();
        assert_eq!(list.pending_count, 0);
        assert_eq!(list.items[0].kind, "toolUse");
        assert_eq!(list.items[0].status, "telemetry");
    }

    #[test]
    fn invalid_decisions_are_rejected() {
        assert!(permission_decision("maybe").is_err());
        assert!(decision_from_value(&json!({"kind": "question", "selections": [1]})).is_err());
        assert!(exit_plan_decision("unknown", None).is_err());
    }
}
