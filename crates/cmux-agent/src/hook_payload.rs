//! Kind-specific feed payloads, the feed item, and the pure event→item mapping.
//!
//! Swift parity sources:
//! - `Packages/macOS/CMUXAgentLaunch/Sources/CMUXAgentLaunch/Workstream/WorkstreamPayload.swift:1-296`
//!   (mode/option/prompt/todo value types + the `WorkstreamPayload` enum with its
//!   custom `Codable`),
//! - `.../Workstream/WorkstreamKind.swift:1-30`,
//! - `.../Workstream/WorkstreamItem.swift:1-81` (`WorkstreamDecision`,
//!   `WorkstreamStatus`, `WorkstreamItem`),
//! - the PURE mapping core extracted from `.../Workstream/WorkstreamStore.swift`
//!   (`makeItem`, `decode(event:source:)`, `defaultTitle`, `parseQuestions` /
//!   `makeQuestion`, `jsonObject` / `promptText`, `context(for:payload:)` /
//!   `updateContextIndex` / `carriedContext`, `stopReason`, `todos`).
//!
//! BOUNDARY: `WorkstreamStore` is a `@MainActor @Observable` actor holding a ring
//! buffer, JSONL persistence, and a mutable `lastContextByWorkstream` index. Only
//! its pure mapping functions are ported here; the I/O, ring-buffer, expiry, and
//! send/resolve machinery are out of scope. The store's mutable context index is
//! modelled as explicit `last_context` inputs and [`next_context`] outputs so the
//! mapping stays a pure function of its arguments.
//!
//! JSON PARITY: Swift enums-with-associated-values use the compiler-synthesized
//! `Codable` shape — `{"<case>":{ <labelled or _N fields> }}`, and a case with no
//! payload is `{"<case>":{}}`. [`WorkstreamStatus`] / [`WorkstreamDecision`]
//! reproduce that with externally-tagged serde enums whose payload-free cases are
//! empty struct variants (`Pending {}` ⇒ `{"pending":{}}`). [`WorkstreamPayload`]
//! keeps Swift's hand-written `Codable` verbatim (including the legacy flat
//! `question` decode fallback and the `todos` `_0` key) via a matching manual
//! `Deserialize` and a field-order-preserving derived `Serialize`.

use serde::de::Error as _;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use uuid::Uuid;

use crate::feed_event::{HookEventName, WorkstreamEvent};
use crate::workstream_context::{WorkstreamContext, WorkstreamExitPlanPreview};
use crate::workstream_json::json_from_string;
use crate::workstream_source::WorkstreamSource;

// ===========================================================================
// Value types — WorkstreamPayload.swift:1-82
// ===========================================================================

/// Inline permission decision modes the user can pick on a `.permissionRequest`
/// item. Wire format uses the lowercase raw values.
///
/// Swift `WorkstreamPermissionMode` (`WorkstreamPayload.swift:5-11`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WorkstreamPermissionMode {
    Once,
    Always,
    All,
    Bypass,
    Deny,
}

/// Inline plan-mode decision the user can pick on an `.exitPlan` item.
///
/// Swift `WorkstreamExitPlanMode` (`WorkstreamPayload.swift:14-20`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WorkstreamExitPlanMode {
    Ultraplan,
    BypassPermissions,
    AutoAccept,
    Manual,
    Deny,
}

/// Single option on a `.question` item.
///
/// Swift `WorkstreamQuestionOption` (`WorkstreamPayload.swift:23-36`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkstreamQuestionOption {
    pub id: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// One prompt inside a `.question` payload. Claude Code's `AskUserQuestion` tool
/// can include several questions in a single call, so a payload carries an array.
///
/// Swift `WorkstreamQuestionPrompt` (`WorkstreamPayload.swift:41-63`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkstreamQuestionPrompt {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub header: Option<String>,
    pub prompt: String,
    #[serde(rename = "multiSelect")]
    pub multi_select: bool,
    pub options: Vec<WorkstreamQuestionOption>,
}

/// Task-list entry state — Swift `WorkstreamTaskTodo.State`
/// (`WorkstreamPayload.swift:67-71`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WorkstreamTaskTodoState {
    Pending,
    InProgress,
    Completed,
}

/// Task-list entry reported by Claude's `TodoWrite` tool or equivalent.
///
/// Swift `WorkstreamTaskTodo` (`WorkstreamPayload.swift:66-82`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkstreamTaskTodo {
    pub id: String,
    pub content: String,
    pub state: WorkstreamTaskTodoState,
}

// ===========================================================================
// WorkstreamPayload — WorkstreamPayload.swift:84-296
// ===========================================================================

/// Kind-specific payload for a `WorkstreamItem`.
///
/// Swift `WorkstreamPayload` (`WorkstreamPayload.swift:85-296`). The derived
/// `Serialize` reproduces Swift's hand-written `encode(to:)` exactly: the single
/// case key wraps a nested object whose keys are in Swift's declared order, nil
/// `pattern`/`reason` are omitted, `question` always emits `questions`, the
/// payload-free `sessionStart`/`sessionEnd` become `{}`, and `todos` nests under
/// `_0`. `Deserialize` is hand-written to preserve the legacy flat `question`
/// fallback.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum WorkstreamPayload {
    PermissionRequest {
        #[serde(rename = "requestId")]
        request_id: String,
        #[serde(rename = "toolName")]
        tool_name: String,
        #[serde(rename = "toolInputJSON")]
        tool_input_json: String,
        #[serde(rename = "pattern", skip_serializing_if = "Option::is_none")]
        pattern: Option<String>,
    },
    ExitPlan {
        #[serde(rename = "requestId")]
        request_id: String,
        plan: String,
        #[serde(rename = "defaultMode")]
        default_mode: WorkstreamExitPlanMode,
    },
    Question {
        #[serde(rename = "requestId")]
        request_id: String,
        questions: Vec<WorkstreamQuestionPrompt>,
    },
    ToolUse {
        #[serde(rename = "toolName")]
        tool_name: String,
        #[serde(rename = "toolInputJSON")]
        tool_input_json: String,
    },
    ToolResult {
        #[serde(rename = "toolName")]
        tool_name: String,
        #[serde(rename = "resultJSON")]
        result_json: String,
        #[serde(rename = "isError")]
        is_error: bool,
    },
    UserPrompt {
        text: String,
    },
    AssistantMessage {
        text: String,
    },
    SessionStart {},
    SessionEnd {},
    Stop {
        #[serde(skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    Todos {
        #[serde(rename = "_0")]
        todos: Vec<WorkstreamTaskTodo>,
    },
}

/// Decode a nested `Value` into a payload field type, mapping errors to `E`.
fn payload_field<T: serde::de::DeserializeOwned, E: serde::de::Error>(v: &Value) -> Result<T, E> {
    serde_json::from_value(v.clone()).map_err(E::custom)
}

impl<'de> Deserialize<'de> for WorkstreamPayload {
    /// Swift `init(from:)` (`WorkstreamPayload.swift:168-249`).
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        let Value::Object(obj) = &value else {
            return Err(D::Error::custom("expected WorkstreamPayload object"));
        };
        // Swift decodes `c.allKeys.first`. A valid payload has exactly one key.
        let (key, inner) = obj
            .iter()
            .next()
            .ok_or_else(|| D::Error::custom("Expected WorkstreamPayload case"))?;
        let map = inner.as_object();

        let get_str = |m: Option<&Map<String, Value>>, k: &str| -> Option<String> {
            m.and_then(|m| m.get(k)).and_then(Value::as_str).map(str::to_string)
        };
        let req = |m: Option<&Map<String, Value>>, k: &'static str| -> Result<String, D::Error> {
            get_str(m, k).ok_or_else(|| D::Error::missing_field(k))
        };

        Ok(match key.as_str() {
            "permissionRequest" => WorkstreamPayload::PermissionRequest {
                request_id: req(map, "requestId")?,
                tool_name: req(map, "toolName")?,
                tool_input_json: req(map, "toolInputJSON")?,
                pattern: get_str(map, "pattern"),
            },
            "exitPlan" => WorkstreamPayload::ExitPlan {
                request_id: req(map, "requestId")?,
                plan: req(map, "plan")?,
                default_mode: payload_field(
                    map.and_then(|m| m.get("defaultMode"))
                        .ok_or_else(|| D::Error::missing_field("defaultMode"))?,
                )?,
            },
            "question" => {
                let request_id = req(map, "requestId")?;
                let questions = map
                    .and_then(|m| m.get("questions"))
                    .filter(|v| !v.is_null());
                if let Some(questions) = questions {
                    WorkstreamPayload::Question {
                        request_id,
                        questions: payload_field(questions)?,
                    }
                } else {
                    // Legacy flat shape: { requestId, prompt, options, multiSelect }.
                    // Swift `decodeIfPresent(String,...) ?? ""` /
                    // `decodeIfPresent(Bool,...) ?? false`
                    // (`WorkstreamPayload.swift:209-210`): a present, non-null value
                    // of the wrong JSON type throws `typeMismatch` and fails the
                    // decode — it is NOT coerced to the default.
                    let prompt = match map.and_then(|m| m.get("prompt")) {
                        None | Some(Value::Null) => String::new(),
                        Some(Value::String(s)) => s.clone(),
                        Some(_) => {
                            return Err(D::Error::custom(
                                "type mismatch decoding optional string field \"prompt\"",
                            ))
                        }
                    };
                    let multi_select = match map.and_then(|m| m.get("multiSelect")) {
                        None | Some(Value::Null) => false,
                        Some(Value::Bool(b)) => *b,
                        Some(_) => {
                            return Err(D::Error::custom(
                                "type mismatch decoding optional bool field \"multiSelect\"",
                            ))
                        }
                    };
                    let options = match map.and_then(|m| m.get("options")).filter(|v| !v.is_null()) {
                        Some(v) => payload_field(v)?,
                        None => Vec::new(),
                    };
                    WorkstreamPayload::Question {
                        request_id,
                        questions: vec![WorkstreamQuestionPrompt {
                            id: "q0".to_string(),
                            header: None,
                            prompt,
                            multi_select,
                            options,
                        }],
                    }
                }
            }
            "toolUse" => WorkstreamPayload::ToolUse {
                tool_name: req(map, "toolName")?,
                tool_input_json: req(map, "toolInputJSON")?,
            },
            "toolResult" => WorkstreamPayload::ToolResult {
                tool_name: req(map, "toolName")?,
                result_json: req(map, "resultJSON")?,
                is_error: map
                    .and_then(|m| m.get("isError"))
                    .and_then(Value::as_bool)
                    .ok_or_else(|| D::Error::missing_field("isError"))?,
            },
            "userPrompt" => WorkstreamPayload::UserPrompt {
                text: req(map, "text")?,
            },
            "assistantMessage" => WorkstreamPayload::AssistantMessage {
                text: req(map, "text")?,
            },
            "sessionStart" => WorkstreamPayload::SessionStart {},
            "sessionEnd" => WorkstreamPayload::SessionEnd {},
            "stop" => WorkstreamPayload::Stop {
                reason: get_str(map, "reason"),
            },
            "todos" => WorkstreamPayload::Todos {
                todos: payload_field(
                    map.and_then(|m| m.get("_0"))
                        .ok_or_else(|| D::Error::missing_field("_0"))?,
                )?,
            },
            other => {
                return Err(D::Error::custom(format!(
                    "unknown WorkstreamPayload case {other:?}"
                )))
            }
        })
    }
}

// ===========================================================================
// WorkstreamKind — WorkstreamKind.swift:1-30
// ===========================================================================

/// Classifies a `WorkstreamItem`. Actionable kinds surface in the default Feed
/// view; telemetry kinds are stored but hidden behind the "All" filter.
///
/// Swift `WorkstreamKind` (`WorkstreamKind.swift:6-30`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WorkstreamKind {
    PermissionRequest,
    ExitPlan,
    Question,
    ToolUse,
    ToolResult,
    UserPrompt,
    AssistantMessage,
    SessionStart,
    SessionEnd,
    Stop,
    Todos,
}

impl WorkstreamKind {
    /// Swift `isActionable` (`WorkstreamKind.swift:22-29`).
    pub const fn is_actionable(self) -> bool {
        matches!(
            self,
            Self::PermissionRequest | Self::ExitPlan | Self::Question
        )
    }
}

// ===========================================================================
// WorkstreamDecision / WorkstreamStatus / WorkstreamItem — WorkstreamItem.swift
// ===========================================================================

/// The user's decision on a resolved actionable item.
///
/// Swift `WorkstreamDecision` (`WorkstreamItem.swift:4-12`). Unlabelled
/// associated values map to the synthesized `_0` key; `feedback` is optional.
///
/// PARITY (synthesized-enum optional): `WorkstreamDecision` has NO hand-written
/// `Codable`, so Swift's *synthesized enum* coder encodes each associated value
/// with a plain `encode(_:forKey:)` — it does NOT apply the struct-synthesis
/// `encodeIfPresent` optimization to optional associated values. A nil
/// `feedback` therefore serializes as an explicit `"feedback":null`, not an
/// omitted key. (Contrast the sibling [`WorkstreamPayload`], which hand-writes
/// `encodeIfPresent` for `pattern`/`reason` precisely to *get* key-omission —
/// dead code unless synthesis emits null.) So `feedback` is emitted always:
/// `None` → `null`, `Some` → the string. `#[serde(default)]` keeps decode
/// tolerant of an absent OR null key (both → `None`), matching the synthesized
/// decoder. Not pinned by an execution oracle (no swiftc on the port host);
/// grounded in Swift's documented struct/enum synthesis asymmetry + the
/// in-repo `encodeIfPresent` workaround signal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WorkstreamDecision {
    Permission {
        #[serde(rename = "_0")]
        mode: WorkstreamPermissionMode,
    },
    ExitPlan {
        #[serde(rename = "_0")]
        mode: WorkstreamExitPlanMode,
        #[serde(default)]
        feedback: Option<String>,
    },
    Question {
        selections: Vec<String>,
    },
}

/// Lifecycle state of a `WorkstreamItem`.
///
/// Swift `WorkstreamStatus` (`WorkstreamItem.swift:15-30`). Payload-free cases
/// use empty struct variants so the externally-tagged form is `{"pending":{}}` /
/// `{"telemetry":{}}`, matching Swift's synthesized enum `Codable`. `at` is a
/// `Date` encoded as `f64` seconds-since-reference-date.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WorkstreamStatus {
    Pending {},
    Resolved {
        #[serde(rename = "_0")]
        decision: WorkstreamDecision,
        at: f64,
    },
    Expired {
        at: f64,
    },
    Telemetry {},
}

impl WorkstreamStatus {
    /// Swift `isPending` (`WorkstreamItem.swift:26-29`).
    pub fn is_pending(&self) -> bool {
        matches!(self, WorkstreamStatus::Pending {})
    }
}

/// A single feed entry. Workstream IDs group items belonging to the same agent
/// session (e.g. `claude-<sessionId>`, `opencode-<sessionId>`).
///
/// Swift `WorkstreamItem` (`WorkstreamItem.swift:34-81`). Uses the synthesized
/// `Codable` (no custom coder), so decode does NOT re-run the normalization that
/// [`WorkstreamItem::new`] applies — matching Swift, where only the memberwise
/// initializer normalizes.
///
/// `created_at` / `updated_at` are `Date`s encoded as `f64` seconds since the
/// 2001 reference date. SANCTIONED DIVERGENCE: `id` serializes lowercase
/// (uuid crate) whereas Swift's `UUID` `Codable` emits an uppercase string; both
/// parse back to the same UUID, so items round-trip identically. Swift also
/// defaults `createdAt` to the wall-clock `Date()`; this pure port defaults it to
/// `0.0`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkstreamItem {
    pub id: Uuid,
    pub workstream_id: String,
    pub source: WorkstreamSource,
    pub kind: WorkstreamKind,
    pub created_at: f64,
    pub updated_at: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub status: WorkstreamStatus,
    pub payload: WorkstreamPayload,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<WorkstreamContext>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ppid: Option<i64>,
}

impl WorkstreamItem {
    /// Swift memberwise `init(...)` (`WorkstreamItem.swift:53-80`): default id /
    /// timestamps, force telemetry kinds to `.telemetry` status, and drop an
    /// empty context to `None`.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: Option<Uuid>,
        workstream_id: impl Into<String>,
        source: WorkstreamSource,
        kind: WorkstreamKind,
        created_at: Option<f64>,
        updated_at: Option<f64>,
        cwd: Option<String>,
        title: Option<String>,
        status: Option<WorkstreamStatus>,
        payload: WorkstreamPayload,
        context: Option<WorkstreamContext>,
        ppid: Option<i64>,
    ) -> Self {
        let created_at = created_at.unwrap_or(0.0);
        let updated_at = updated_at.unwrap_or(created_at);
        let status = if kind.is_actionable() {
            status.unwrap_or(WorkstreamStatus::Pending {})
        } else {
            WorkstreamStatus::Telemetry {}
        };
        Self {
            id: id.unwrap_or_else(Uuid::new_v4),
            workstream_id: workstream_id.into(),
            source,
            kind,
            created_at,
            updated_at,
            cwd,
            title,
            status,
            payload,
            context: context.filter(|c| !c.is_empty()),
            ppid,
        }
    }
}

// ===========================================================================
// Pure mapping core — extracted from WorkstreamStore.swift
// ===========================================================================

/// The app-boundary hook the store calls `titleProvider`: an optional localized
/// display title for an event. Swift `titleProvider: (WorkstreamEvent) -> String?`.
pub type TitleProvider<'a> = dyn Fn(&WorkstreamEvent) -> Option<String> + 'a;

/// Materialize a wire frame into a `WorkstreamItem`.
///
/// Swift `makeItem(from:)` (`WorkstreamStore.swift:210-227`). The store's mutable
/// `lastContextByWorkstream[event.sessionId]` is passed explicitly as
/// `last_context`.
pub fn make_item(
    event: &WorkstreamEvent,
    last_context: Option<&WorkstreamContext>,
    title_provider: &TitleProvider<'_>,
) -> WorkstreamItem {
    let source = WorkstreamSource::from_wire(&event.source).unwrap_or(WorkstreamSource::Claude);
    let (kind, payload) = decode_event(event, title_provider);
    let status = if kind.is_actionable() {
        WorkstreamStatus::Pending {}
    } else {
        WorkstreamStatus::Telemetry {}
    };
    let context = context_for(event, &payload, last_context);
    WorkstreamItem::new(
        None,
        event.session_id.clone(),
        source,
        kind,
        Some(event.received_at),
        Some(event.received_at),
        event.cwd.clone(),
        default_title(event, title_provider),
        Some(status),
        payload,
        context,
        event.ppid,
    )
}

/// Swift `decode(event:source:)` (`WorkstreamStore.swift:276-348`). The `source`
/// parameter is unused in the Swift body and is dropped here.
pub fn decode_event(
    event: &WorkstreamEvent,
    title_provider: &TitleProvider<'_>,
) -> (WorkstreamKind, WorkstreamPayload) {
    let tool_input = event
        .tool_input_json
        .clone()
        .unwrap_or_else(|| "{}".to_string());
    let request_id = || event.request_id.clone().unwrap_or_else(|| event.session_id.clone());
    let titled = || {
        title_provider(event).unwrap_or_else(|| event.hook_event_name.wire_name().to_string())
    };

    match event.hook_event_name {
        HookEventName::PermissionRequest => (
            WorkstreamKind::PermissionRequest,
            WorkstreamPayload::PermissionRequest {
                request_id: request_id(),
                tool_name: event.tool_name.clone().unwrap_or_else(|| "unknown".to_string()),
                tool_input_json: tool_input,
                pattern: None,
            },
        ),
        HookEventName::AskUserQuestion => (
            WorkstreamKind::Question,
            WorkstreamPayload::Question {
                request_id: request_id(),
                questions: parse_questions(event.tool_input_json.as_deref()),
            },
        ),
        HookEventName::ExitPlanMode => (
            WorkstreamKind::ExitPlan,
            WorkstreamPayload::ExitPlan {
                request_id: request_id(),
                plan: tool_input,
                default_mode: WorkstreamExitPlanMode::Manual,
            },
        ),
        HookEventName::PreToolUse => (
            WorkstreamKind::ToolUse,
            WorkstreamPayload::ToolUse {
                tool_name: event.tool_name.clone().unwrap_or_default(),
                tool_input_json: tool_input,
            },
        ),
        HookEventName::PostToolUse => (
            WorkstreamKind::ToolResult,
            WorkstreamPayload::ToolResult {
                tool_name: event.tool_name.clone().unwrap_or_default(),
                result_json: tool_input,
                is_error: false,
            },
        ),
        HookEventName::PreCompact => (
            WorkstreamKind::ToolUse,
            WorkstreamPayload::ToolUse {
                tool_name: titled(),
                tool_input_json: tool_input,
            },
        ),
        HookEventName::PostCompact => (
            WorkstreamKind::ToolResult,
            WorkstreamPayload::ToolResult {
                tool_name: titled(),
                result_json: tool_input,
                is_error: false,
            },
        ),
        HookEventName::SubagentStart => (
            WorkstreamKind::ToolUse,
            WorkstreamPayload::ToolUse {
                tool_name: titled(),
                tool_input_json: tool_input,
            },
        ),
        HookEventName::SubagentStop => (
            WorkstreamKind::ToolResult,
            WorkstreamPayload::ToolResult {
                tool_name: titled(),
                result_json: tool_input,
                is_error: false,
            },
        ),
        HookEventName::UserPromptSubmit => {
            let prompt = prompt_text(event.tool_input_json.as_deref());
            let text = if prompt.is_empty() {
                event
                    .context
                    .as_ref()
                    .and_then(|c| c.last_user_message.clone())
                    .unwrap_or_default()
            } else {
                prompt
            };
            (WorkstreamKind::UserPrompt, WorkstreamPayload::UserPrompt { text })
        }
        HookEventName::SessionStart => {
            (WorkstreamKind::SessionStart, WorkstreamPayload::SessionStart {})
        }
        HookEventName::SessionEnd => {
            (WorkstreamKind::SessionEnd, WorkstreamPayload::SessionEnd {})
        }
        HookEventName::Stop => (
            WorkstreamKind::Stop,
            WorkstreamPayload::Stop {
                reason: stop_reason(event.tool_input_json.as_deref()),
            },
        ),
        HookEventName::TodoWrite => (
            WorkstreamKind::Todos,
            WorkstreamPayload::Todos {
                todos: todos(event.tool_input_json.as_deref()),
            },
        ),
        HookEventName::Notification => (
            WorkstreamKind::ToolResult,
            WorkstreamPayload::ToolResult {
                tool_name: "notification".to_string(),
                result_json: tool_input,
                is_error: false,
            },
        ),
    }
}

/// Swift `defaultTitle(for:)` (`WorkstreamStore.swift:350-355`).
pub fn default_title(event: &WorkstreamEvent, title_provider: &TitleProvider<'_>) -> Option<String> {
    if let Some(tool) = &event.tool_name {
        if !tool.is_empty() {
            return Some(tool.clone());
        }
    }
    title_provider(event)
}

/// Swift `parseQuestions(fromToolInput:)` (`WorkstreamStore.swift:362-374`).
pub fn parse_questions(json: Option<&str>) -> Vec<WorkstreamQuestionPrompt> {
    let Some(Value::Object(root)) = json.and_then(json_from_string) else {
        return Vec::new();
    };
    if let Some(Value::Array(arr)) = root.get("questions") {
        if arr.iter().all(Value::is_object) {
            return arr
                .iter()
                .enumerate()
                .map(|(idx, q)| make_question(q.as_object().unwrap(), &format!("q{idx}")))
                .collect();
        }
    }
    vec![make_question(&root, "q0")]
}

/// Swift `makeQuestion(from:fallbackId:)` (`WorkstreamStore.swift:376-406`).
fn make_question(dict: &Map<String, Value>, fallback_id: &str) -> WorkstreamQuestionPrompt {
    let str_of = |k: &str| dict.get(k).and_then(Value::as_str);
    let header = str_of("header").or_else(|| str_of("title")).map(str::to_string);
    let prompt = str_of("question")
        .or_else(|| str_of("prompt"))
        .unwrap_or("")
        .to_string();
    let multi = dict
        .get("multiSelect")
        .and_then(Value::as_bool)
        .or_else(|| dict.get("multi_select").and_then(Value::as_bool))
        .unwrap_or(false);
    let raw_options = dict.get("options").and_then(Value::as_array);
    let mut options = Vec::new();
    if let Some(raw_options) = raw_options {
        for (i, raw) in raw_options.iter().enumerate() {
            if let Some(s) = raw.as_str() {
                options.push(WorkstreamQuestionOption {
                    id: format!("opt{i}"),
                    label: s.to_string(),
                    description: None,
                });
            } else if let Some(d) = raw.as_object() {
                let id = d
                    .get("id")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .unwrap_or_else(|| format!("opt{i}"));
                let label = d
                    .get("label")
                    .and_then(Value::as_str)
                    .or_else(|| d.get("title").and_then(Value::as_str))
                    .map(str::to_string)
                    .unwrap_or_else(|| id.clone());
                let description = d
                    .get("description")
                    .and_then(Value::as_str)
                    .or_else(|| d.get("detail").and_then(Value::as_str))
                    .map(str::to_string);
                options.push(WorkstreamQuestionOption { id, label, description });
            }
        }
    }
    let id = dict
        .get("id")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| fallback_id.to_string());
    WorkstreamQuestionPrompt {
        id,
        header,
        prompt,
        multi_select: multi,
        options,
    }
}

/// Swift `promptText(from:)` (`WorkstreamStore.swift:413-421`).
fn prompt_text(json: Option<&str>) -> String {
    if let Some(Value::Object(dict)) = json.and_then(json_from_string) {
        return dict
            .get("prompt")
            .and_then(Value::as_str)
            .or_else(|| dict.get("text").and_then(Value::as_str))
            .or_else(|| dict.get("message").and_then(Value::as_str))
            .unwrap_or("")
            .to_string();
    }
    json.unwrap_or("").to_string()
}

/// Swift `stopReason(from:)` (`WorkstreamStore.swift:484-491`).
fn stop_reason(json: Option<&str>) -> Option<String> {
    if let Some(Value::Object(dict)) = json.and_then(json_from_string) {
        return dict
            .get("reason")
            .and_then(Value::as_str)
            .or_else(|| dict.get("message").and_then(Value::as_str))
            .or_else(|| dict.get("cause").and_then(Value::as_str))
            .map(str::to_string);
    }
    None
}

/// Swift `todos(from:)` (`WorkstreamStore.swift:493-525`).
fn todos(json: Option<&str>) -> Vec<WorkstreamTaskTodo> {
    let parsed = json.and_then(json_from_string);
    let raw: Vec<Value> = match &parsed {
        Some(Value::Object(m)) => m
            .get("todos")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default(),
        Some(Value::Array(a)) => a.clone(),
        _ => Vec::new(),
    };
    raw.iter()
        .enumerate()
        .filter_map(|(idx, raw)| {
            let dict = raw.as_object()?;
            let content = dict
                .get("content")
                .and_then(Value::as_str)
                .or_else(|| dict.get("text").and_then(Value::as_str))
                .or_else(|| dict.get("title").and_then(Value::as_str))
                .unwrap_or("");
            if content.is_empty() {
                return None;
            }
            let raw_state = dict
                .get("state")
                .and_then(Value::as_str)
                .or_else(|| dict.get("status").and_then(Value::as_str))
                .unwrap_or("pending");
            let state = match raw_state {
                "completed" | "done" => WorkstreamTaskTodoState::Completed,
                "inProgress" | "in_progress" | "active" => WorkstreamTaskTodoState::InProgress,
                _ => WorkstreamTaskTodoState::Pending,
            };
            let id = dict
                .get("id")
                .and_then(Value::as_str)
                .map(str::to_string)
                .unwrap_or_else(|| format!("todo{idx}"));
            Some(WorkstreamTaskTodo {
                id,
                content: content.to_string(),
                state,
            })
        })
        .collect()
}

/// Swift `context(for:payload:)` (`WorkstreamStore.swift:430-452`). `fallback`
/// is the store's `lastContextByWorkstream[event.sessionId]`.
pub fn context_for(
    event: &WorkstreamEvent,
    payload: &WorkstreamPayload,
    last_context: Option<&WorkstreamContext>,
) -> Option<WorkstreamContext> {
    let fallback = last_context.cloned();
    let mut context: Option<WorkstreamContext> = match &event.context {
        Some(c) => Some(c.merging_missing(fallback.as_ref())),
        None => fallback.clone(),
    };

    match payload {
        WorkstreamPayload::UserPrompt { text } => {
            context = Some(
                WorkstreamContext::new(Some(text.clone()), None, None, vec![], None, None)
                    .merging_missing(context.as_ref()),
            );
        }
        WorkstreamPayload::AssistantMessage { text } => {
            context = Some(
                WorkstreamContext::new(None, Some(text.clone()), None, vec![], None, None)
                    .merging_missing(context.as_ref()),
            );
        }
        WorkstreamPayload::ExitPlan { plan, .. } => {
            let preview = WorkstreamExitPlanPreview::new(plan);
            context = Some(
                WorkstreamContext::new(
                    None,
                    None,
                    preview.summary,
                    preview.allowed_prompts,
                    None,
                    None,
                )
                .merging_missing(context.as_ref()),
            );
        }
        _ => {}
    }

    context.filter(|c| !c.is_empty())
}

/// Swift `carriedContext(from:)` (`WorkstreamStore.swift:475-482`).
pub fn carried_context(context: &WorkstreamContext) -> Option<WorkstreamContext> {
    let carried = WorkstreamContext::new(
        context.last_user_message.clone(),
        context.assistant_preamble.clone(),
        None,
        vec![],
        None,
        context.permission_mode.clone(),
    );
    if carried.is_empty() {
        None
    } else {
        Some(carried)
    }
}

/// The store's `updateContextIndex(with:)` (`WorkstreamStore.swift:454-473`) as a
/// pure function: given the previously-indexed context (`current`) for the item's
/// workstream, return the new context to store, or `None` to leave it unchanged.
pub fn next_context(
    item: &WorkstreamItem,
    current: Option<&WorkstreamContext>,
) -> Option<WorkstreamContext> {
    let mut next: Option<WorkstreamContext> = None;

    if let Some(context) = &item.context {
        next = carried_context(context).map(|c| c.merging_missing(current));
    }

    match &item.payload {
        WorkstreamPayload::UserPrompt { text } => {
            let base = next.as_ref().or(current);
            next = Some(
                WorkstreamContext::new(Some(text.clone()), None, None, vec![], None, None)
                    .merging_missing(base),
            );
        }
        WorkstreamPayload::AssistantMessage { text } => {
            let base = next.as_ref().or(current);
            next = Some(
                WorkstreamContext::new(None, Some(text.clone()), None, vec![], None, None)
                    .merging_missing(base),
            );
        }
        _ => {}
    }

    next.filter(|c| !c.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `titleProvider` that returns `None` — the store default.
    fn no_titles(_: &WorkstreamEvent) -> Option<String> {
        None
    }

    // -----------------------------------------------------------------------
    // Ports of WorkstreamItemTests.swift:1-100
    // -----------------------------------------------------------------------

    /// `defaultStatusByKind` (WorkstreamItemTests.swift:7-28).
    #[test]
    fn default_status_by_kind() {
        let perm = WorkstreamItem::new(
            None,
            "claude-1",
            WorkstreamSource::Claude,
            WorkstreamKind::PermissionRequest,
            None,
            None,
            None,
            None,
            None,
            WorkstreamPayload::PermissionRequest {
                request_id: "r1".into(),
                tool_name: "Write".into(),
                tool_input_json: "{}".into(),
                pattern: None,
            },
            None,
            None,
        );
        assert!(perm.status.is_pending());

        let tool = WorkstreamItem::new(
            None,
            "claude-1",
            WorkstreamSource::Claude,
            WorkstreamKind::ToolUse,
            None,
            None,
            None,
            None,
            None,
            WorkstreamPayload::ToolUse {
                tool_name: "Read".into(),
                tool_input_json: "{}".into(),
            },
            None,
            None,
        );
        assert_eq!(tool.status, WorkstreamStatus::Telemetry {});
    }

    /// `codableRoundTrip` (WorkstreamItemTests.swift:30-48).
    #[test]
    fn codable_round_trip() {
        let original = WorkstreamItem::new(
            Some(Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap()),
            "codex-42",
            WorkstreamSource::Codex,
            WorkstreamKind::PermissionRequest,
            None,
            None,
            None,
            None,
            None,
            WorkstreamPayload::PermissionRequest {
                request_id: "req-7".into(),
                tool_name: "shell".into(),
                tool_input_json: "{\"cmd\":\"rm -rf /\"}".into(),
                pattern: Some("dangerous".into()),
            },
            Some(WorkstreamContext::new(
                Some("please clean up".into()),
                None,
                None,
                vec![],
                None,
                None,
            )),
            None,
        );
        let encoded = serde_json::to_string(&original).unwrap();
        let decoded: WorkstreamItem = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, original);
    }

    /// `legacyQuestionPayloadDecode` (WorkstreamItemTests.swift:50-73).
    #[test]
    fn legacy_question_payload_decode() {
        let json = r#"
        {
          "question": {
            "requestId": "req-q",
            "prompt": "Pick one",
            "multiSelect": false,
            "options": [
              {"id": "a", "label": "A"}
            ]
          }
        }
        "#;
        let decoded: WorkstreamPayload = serde_json::from_str(json).unwrap();
        let WorkstreamPayload::Question { request_id, questions } = decoded else {
            panic!("expected question payload");
        };
        assert_eq!(request_id, "req-q");
        assert_eq!(questions.len(), 1);
        assert_eq!(questions.first().map(|q| q.prompt.as_str()), Some("Pick one"));
        assert_eq!(
            questions.first().and_then(|q| q.options.first()).map(|o| o.label.as_str()),
            Some("A")
        );
    }

    /// Pinning oracle for the strict legacy-question parity fix: Swift decodes
    /// `prompt` via `decodeIfPresent(String.self,...) ?? ""`
    /// (`WorkstreamPayload.swift:209`), which throws `DecodingError.typeMismatch`
    /// on a present, non-null, non-string value. A numeric `prompt` must reject
    /// the whole payload rather than silently defaulting to `""`.
    #[test]
    fn legacy_question_rejects_wrong_typed_prompt() {
        let json = r#"{"question":{"requestId":"req-q","prompt":123}}"#;
        let err = serde_json::from_str::<WorkstreamPayload>(json).unwrap_err();
        assert!(
            err.to_string().contains("type mismatch"),
            "expected type-mismatch rejection, got: {err}"
        );
    }

    /// Same class as [`legacy_question_rejects_wrong_typed_prompt`], for
    /// `multiSelect`, decoded via `decodeIfPresent(Bool.self,...) ?? false`
    /// (`WorkstreamPayload.swift:210`). A string value must fail the decode.
    #[test]
    fn legacy_question_rejects_wrong_typed_multi_select() {
        let json = r#"{"question":{"requestId":"req-q","multiSelect":"yes"}}"#;
        let err = serde_json::from_str::<WorkstreamPayload>(json).unwrap_err();
        assert!(
            err.to_string().contains("type mismatch"),
            "expected type-mismatch rejection, got: {err}"
        );
    }

    /// The strict fix must still accept the legacy shape with absent/null
    /// `prompt` and `multiSelect`, defaulting to `""` / `false` exactly as Swift's
    /// `?? ""` / `?? false` fallbacks do.
    #[test]
    fn legacy_question_accepts_missing_and_null_scalars() {
        let json = r#"{"question":{"requestId":"req-q","prompt":null,"multiSelect":null}}"#;
        let decoded: WorkstreamPayload = serde_json::from_str(json).unwrap();
        let WorkstreamPayload::Question { request_id, questions } = decoded else {
            panic!("expected question payload");
        };
        assert_eq!(request_id, "req-q");
        assert_eq!(questions.first().map(|q| q.prompt.as_str()), Some(""));
        assert_eq!(questions.first().map(|q| q.multi_select), Some(false));
    }

    /// `nonActionableStatusNormalizesToTelemetry` (WorkstreamItemTests.swift:75-89).
    #[test]
    fn non_actionable_status_normalizes_to_telemetry() {
        let item = WorkstreamItem::new(
            None,
            "s",
            WorkstreamSource::Claude,
            WorkstreamKind::SessionStart,
            None,
            None,
            None,
            None,
            Some(WorkstreamStatus::Pending {}),
            WorkstreamPayload::SessionStart {},
            None,
            None,
        );
        assert_eq!(item.status, WorkstreamStatus::Telemetry {});
    }

    /// `isActionable` (WorkstreamItemTests.swift:91-99).
    #[test]
    fn is_actionable() {
        assert!(WorkstreamKind::PermissionRequest.is_actionable());
        assert!(WorkstreamKind::ExitPlan.is_actionable());
        assert!(WorkstreamKind::Question.is_actionable());
        assert!(!WorkstreamKind::ToolUse.is_actionable());
        assert!(!WorkstreamKind::SessionStart.is_actionable());
        assert!(!WorkstreamKind::Todos.is_actionable());
    }

    // -----------------------------------------------------------------------
    // Ports of the pure-mapping cases from WorkstreamStoreTests.swift
    // -----------------------------------------------------------------------

    /// `ingestPending` mapping half (WorkstreamStoreTests.swift:8-15): a
    /// permission event maps to a pending permissionRequest item.
    #[test]
    fn permission_event_maps_to_pending_item() {
        let event = WorkstreamEvent::new("s1", HookEventName::PermissionRequest, "claude")
            .with_cwd("/tmp")
            .with_tool_name("Write")
            .with_tool_input_json("{}")
            .with_request_id("r1");
        let item = make_item(&event, None, &no_titles);
        assert_eq!(item.kind, WorkstreamKind::PermissionRequest);
        assert!(item.status.is_pending());
    }

    /// `telemetryNeverPending` mapping half (WorkstreamStoreTests.swift:109-121).
    #[test]
    fn telemetry_never_pending() {
        let event = WorkstreamEvent::new("s1", HookEventName::PreToolUse, "claude")
            .with_tool_name("Read");
        let item = make_item(&event, None, &no_titles);
        assert_eq!(item.kind, WorkstreamKind::ToolUse);
        assert_eq!(item.status, WorkstreamStatus::Telemetry {});
    }

    /// `codexLifecycleFeedEventsStayTelemetry` (WorkstreamStoreTests.swift:123-199).
    #[test]
    fn codex_lifecycle_feed_events_stay_telemetry() {
        let titles = |event: &WorkstreamEvent| match event.hook_event_name {
            HookEventName::PreCompact | HookEventName::PostCompact => Some("Compaction".to_string()),
            HookEventName::SubagentStart | HookEventName::SubagentStop => {
                Some("Subagent".to_string())
            }
            _ => None,
        };
        let events = [
            HookEventName::PostToolUse,
            HookEventName::PreCompact,
            HookEventName::PostCompact,
            HookEventName::SubagentStart,
            HookEventName::SubagentStop,
        ];
        let items: Vec<WorkstreamItem> = events
            .into_iter()
            .map(|name| {
                let event = WorkstreamEvent::new("codex-session", name, "codex");
                make_item(&event, None, &titles)
            })
            .collect();

        assert_eq!(items.len(), events.len());
        assert!(items.iter().all(|i| i.status == WorkstreamStatus::Telemetry {}));
        let titles_present: Vec<Option<&str>> = items.iter().map(|i| i.title.as_deref()).collect();
        assert!(titles_present.contains(&Some("Compaction")));
        assert!(titles_present.contains(&Some("Subagent")));
        assert!(!titles_present.contains(&Some("PreCompact")));
        assert!(!titles_present.contains(&Some("SubagentStart")));
        assert!(!items.iter().any(|i| i.kind == WorkstreamKind::SessionStart));
        assert!(!items.iter().any(|i| i.kind == WorkstreamKind::Stop));

        let compaction_start = items
            .iter()
            .find(|i| i.title.as_deref() == Some("Compaction") && i.kind == WorkstreamKind::ToolUse)
            .expect("PreCompact item");
        let WorkstreamPayload::ToolUse { tool_name, .. } = &compaction_start.payload else {
            panic!("expected toolUse");
        };
        assert_eq!(tool_name, "Compaction");

        let subagent_stop = items
            .iter()
            .find(|i| i.title.as_deref() == Some("Subagent") && i.kind == WorkstreamKind::ToolResult)
            .expect("SubagentStop item");
        let WorkstreamPayload::ToolResult { tool_name, .. } = &subagent_stop.payload else {
            panic!("expected toolResult");
        };
        assert_eq!(tool_name, "Subagent");
    }

    /// `telemetryContent` (WorkstreamStoreTests.swift:201-239).
    #[test]
    fn telemetry_content() {
        let prompt = make_item(
            &WorkstreamEvent::new("s1", HookEventName::UserPromptSubmit, "claude")
                .with_tool_input_json(r#"{"prompt":"ship it"}"#),
            None,
            &no_titles,
        );
        let WorkstreamPayload::UserPrompt { text } = &prompt.payload else {
            panic!("expected user prompt");
        };
        assert_eq!(text, "ship it");

        let stop = make_item(
            &WorkstreamEvent::new("s1", HookEventName::Stop, "claude")
                .with_tool_input_json(r#"{"reason":"done"}"#),
            None,
            &no_titles,
        );
        let WorkstreamPayload::Stop { reason } = &stop.payload else {
            panic!("expected stop");
        };
        assert_eq!(reason.as_deref(), Some("done"));

        let todos_item = make_item(
            &WorkstreamEvent::new("s1", HookEventName::TodoWrite, "claude")
                .with_tool_input_json(r#"{"todos":[{"id":"t1","content":"test","status":"in_progress"}]}"#),
            None,
            &no_titles,
        );
        let WorkstreamPayload::Todos { todos } = &todos_item.payload else {
            panic!("expected todos");
        };
        assert_eq!(todos.first().map(|t| t.content.as_str()), Some("test"));
        assert_eq!(todos.first().map(|t| t.state), Some(WorkstreamTaskTodoState::InProgress));
    }

    /// `promptContextCarriesIntoPermission` (WorkstreamStoreTests.swift:241-262).
    /// Threads the store's context index (`next_context`) between the two events.
    #[test]
    fn prompt_context_carries_into_permission() {
        let event1 = WorkstreamEvent::new("s1", HookEventName::UserPromptSubmit, "claude")
            .with_tool_input_json(r#"{"prompt":"demo the permission UI"}"#)
            .with_context(WorkstreamContext::new(None, None, None, vec![], None, Some("plan".into())));
        let item1 = make_item(&event1, None, &no_titles);
        let index = next_context(&item1, None);

        let event2 = WorkstreamEvent::new("s1", HookEventName::PermissionRequest, "claude")
            .with_tool_name("Bash")
            .with_tool_input_json(r#"{"command":"echo hi"}"#)
            .with_request_id("r1");
        let item2 = make_item(&event2, index.as_ref(), &no_titles);

        let ctx = item2.context.expect("carried context");
        assert_eq!(ctx.last_user_message.as_deref(), Some("demo the permission UI"));
        assert_eq!(ctx.permission_mode.as_deref(), Some("plan"));
    }

    /// `exitPlanParsesContext` (WorkstreamStoreTests.swift:264-290).
    #[test]
    fn exit_plan_parses_context() {
        let plan = r##"
            {
              "plan": "# Demo Plan\n\n## Context\nShow the new feed UI.",
              "allowedPrompts": [
                {"tool": "Bash", "prompt": "run reload.sh --tag feedctx"}
              ],
              "planFilePath": "/tmp/demo.md"
            }
        "##;
        let event = WorkstreamEvent::new("s1", HookEventName::ExitPlanMode, "claude")
            .with_tool_name("ExitPlanMode")
            .with_tool_input_json(plan)
            .with_context(WorkstreamContext::new(
                Some("make a plan".into()),
                None,
                None,
                vec![],
                None,
                None,
            ))
            .with_request_id("plan-1");
        let item = make_item(&event, None, &no_titles);
        let ctx = item.context.expect("context");
        assert_eq!(ctx.last_user_message.as_deref(), Some("make a plan"));
        assert_eq!(ctx.plan_summary.as_deref(), Some("Show the new feed UI."));
        assert_eq!(ctx.allowed_prompts.first().map(|p| p.tool.as_str()), Some("Bash"));
        assert_eq!(
            ctx.allowed_prompts.first().map(|p| p.prompt.as_str()),
            Some("run reload.sh --tag feedctx")
        );
    }

    /// Status/decision JSON parity — Swift synthesized enum `Codable` shape.
    #[test]
    fn status_json_shapes_match_swift() {
        assert_eq!(
            serde_json::to_string(&WorkstreamStatus::Pending {}).unwrap(),
            r#"{"pending":{}}"#
        );
        assert_eq!(
            serde_json::to_string(&WorkstreamStatus::Resolved {
                decision: WorkstreamDecision::Permission {
                    mode: WorkstreamPermissionMode::Once
                },
                at: 0.0,
            })
            .unwrap(),
            r#"{"resolved":{"_0":{"permission":{"_0":"once"}},"at":0.0}}"#
        );
        let decoded: WorkstreamStatus = serde_json::from_str(r#"{"telemetry":{}}"#).unwrap();
        assert_eq!(decoded, WorkstreamStatus::Telemetry {});
    }

    /// Synthesized-enum optional parity: a nil `feedback` on the
    /// `exitPlan` decision emits an explicit `"feedback":null`, NOT an omitted
    /// key (Swift synthesizes `WorkstreamDecision`'s coder, which does not use
    /// `encodeIfPresent` for optional associated values). Decode stays tolerant
    /// of absent/null/string. See the `WorkstreamDecision` doc-comment.
    #[test]
    fn exit_plan_nil_feedback_emits_explicit_null() {
        let nil_fb = WorkstreamDecision::ExitPlan {
            mode: WorkstreamExitPlanMode::Manual,
            feedback: None,
        };
        assert_eq!(
            serde_json::to_string(&nil_fb).unwrap(),
            r#"{"exitPlan":{"_0":"manual","feedback":null}}"#
        );
        assert_eq!(
            serde_json::to_string(&WorkstreamDecision::ExitPlan {
                mode: WorkstreamExitPlanMode::Manual,
                feedback: Some("redo it".into()),
            })
            .unwrap(),
            r#"{"exitPlan":{"_0":"manual","feedback":"redo it"}}"#
        );
        // Decode tolerates the emitted null, an explicit null, and an absent key.
        for wire in [
            r#"{"exitPlan":{"_0":"manual","feedback":null}}"#,
            r#"{"exitPlan":{"_0":"manual"}}"#,
        ] {
            let decoded: WorkstreamDecision = serde_json::from_str(wire).unwrap();
            assert_eq!(decoded, nil_fb);
        }
    }

    /// Payload JSON parity for representative cases, incl. the `_0` todos key and
    /// the payload-free `sessionStart`.
    #[test]
    fn payload_json_shapes_match_swift() {
        assert_eq!(
            serde_json::to_string(&WorkstreamPayload::SessionStart {}).unwrap(),
            r#"{"sessionStart":{}}"#
        );
        assert_eq!(
            serde_json::to_string(&WorkstreamPayload::Stop { reason: None }).unwrap(),
            r#"{"stop":{}}"#
        );
        assert_eq!(
            serde_json::to_string(&WorkstreamPayload::Todos {
                todos: vec![WorkstreamTaskTodo {
                    id: "t1".into(),
                    content: "c".into(),
                    state: WorkstreamTaskTodoState::Pending,
                }],
            })
            .unwrap(),
            r#"{"todos":{"_0":[{"id":"t1","content":"c","state":"pending"}]}}"#
        );
        assert_eq!(
            serde_json::to_string(&WorkstreamPayload::PermissionRequest {
                request_id: "r".into(),
                tool_name: "Write".into(),
                tool_input_json: "{}".into(),
                pattern: None,
            })
            .unwrap(),
            r#"{"permissionRequest":{"requestId":"r","toolName":"Write","toolInputJSON":"{}"}}"#
        );
    }
}
