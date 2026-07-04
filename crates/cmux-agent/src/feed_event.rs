//! The wire frame sent from hook subcommands and the OpenCode plugin to the
//! cmux socket, then materialized into a `WorkstreamItem` by the store.
//!
//! Swift parity source:
//! `Packages/macOS/CMUXAgentLaunch/Sources/CMUXAgentLaunch/Workstream/WorkstreamEvent.swift:1-153`
//! (`WorkstreamEvent` 10-146 and the `JSONDynamicKey` helper 148-153; the private
//! `AnyJSON` 155-283 is ported in [`crate::workstream_json`]).
//!
//! Field names mirror Vibe Island's hook payload format exactly so existing agent
//! payloads pass through untouched: `session_id`, `hook_event_name`,
//! `workspace_id`, `cwd`, `tool_name`, `tool_input`, `_source`, `_ppid`,
//! `_opencode_request_id`, `_received_at`. `context` is cmux-specific and
//! optional. Unknown keys are captured into `extra_fields_json` and re-emitted so
//! forward-compatible fields survive a decode/encode round-trip.
//!
//! JSON PARITY NOTES:
//! * `_received_at` is Swift's `Date`, which the default `JSONEncoder` writes as a
//!   `Double` of seconds since the 2001 reference date. It is modelled here as
//!   `f64`. SANCTIONED DIVERGENCE: Swift defaults a missing `_received_at` to the
//!   wall-clock `Date()`; this pure port has no clock and defaults it to `0.0`.
//!   The value is never asserted by the oracle tests.
//! * The extra dynamic fields are re-emitted in sorted (deterministic) key order
//!   because `extra_fields_json` is stored via [`crate::workstream_json`]'s sorted
//!   form. Swift iterates a `Dictionary` there, whose order is unspecified, so the
//!   deterministic Rust order is a valid instance of the Swift behaviour and
//!   round-trips identically.

use serde::de::Error as _;
use serde::ser::SerializeMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};

use crate::workstream_context::WorkstreamContext;
use crate::workstream_json::{json_from_string, json_to_string};

/// Hook event discriminator. Values match the strings Vibe Island and cmux hook
/// wrappers already emit on stdin so no translation layer is needed.
///
/// Swift `WorkstreamEvent.HookEventName` (`WorkstreamEvent.swift:55-74`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HookEventName {
    #[serde(rename = "SessionStart")]
    SessionStart,
    #[serde(rename = "SessionEnd")]
    SessionEnd,
    #[serde(rename = "UserPromptSubmit")]
    UserPromptSubmit,
    #[serde(rename = "PreToolUse")]
    PreToolUse,
    #[serde(rename = "PostToolUse")]
    PostToolUse,
    /// Codex compaction is about to start.
    #[serde(rename = "PreCompact")]
    PreCompact,
    /// Codex compaction completed.
    #[serde(rename = "PostCompact")]
    PostCompact,
    #[serde(rename = "PermissionRequest")]
    PermissionRequest,
    #[serde(rename = "AskUserQuestion")]
    AskUserQuestion,
    #[serde(rename = "ExitPlanMode")]
    ExitPlanMode,
    #[serde(rename = "TodoWrite")]
    TodoWrite,
    #[serde(rename = "Stop")]
    Stop,
    /// Codex started a subagent run.
    #[serde(rename = "SubagentStart")]
    SubagentStart,
    #[serde(rename = "SubagentStop")]
    SubagentStop,
    #[serde(rename = "Notification")]
    Notification,
}

impl HookEventName {
    /// The wire `hook_event_name` raw value — Swift `rawValue`.
    pub const fn wire_name(self) -> &'static str {
        match self {
            Self::SessionStart => "SessionStart",
            Self::SessionEnd => "SessionEnd",
            Self::UserPromptSubmit => "UserPromptSubmit",
            Self::PreToolUse => "PreToolUse",
            Self::PostToolUse => "PostToolUse",
            Self::PreCompact => "PreCompact",
            Self::PostCompact => "PostCompact",
            Self::PermissionRequest => "PermissionRequest",
            Self::AskUserQuestion => "AskUserQuestion",
            Self::ExitPlanMode => "ExitPlanMode",
            Self::TodoWrite => "TodoWrite",
            Self::Stop => "Stop",
            Self::SubagentStart => "SubagentStart",
            Self::SubagentStop => "SubagentStop",
            Self::Notification => "Notification",
        }
    }
}

/// The wire-frame keys the struct decodes into named fields — Swift
/// `CodingKeys` (`WorkstreamEvent.swift:76-88`). Any key NOT in this set is
/// captured into `extra_fields_json`.
const KNOWN_KEYS: [&str; 11] = [
    "session_id",
    "hook_event_name",
    "_source",
    "workspace_id",
    "cwd",
    "tool_name",
    "tool_input",
    "context",
    "_opencode_request_id",
    "_ppid",
    "_received_at",
];

/// Wire frame materialized into a `WorkstreamItem` by the store.
///
/// Swift `WorkstreamEvent` (`WorkstreamEvent.swift:10-146`).
#[derive(Debug, Clone, PartialEq)]
pub struct WorkstreamEvent {
    pub session_id: String,
    pub hook_event_name: HookEventName,
    pub source: String,
    pub workspace_id: Option<String>,
    pub cwd: Option<String>,
    pub tool_name: Option<String>,
    /// `tool_input` normalized to a JSON string. Incoming objects/arrays are
    /// re-serialized (sorted keys) via [`json_to_string`]; incoming strings are
    /// stored verbatim so consumers can parse agent-specific structure.
    pub tool_input_json: Option<String>,
    pub context: Option<WorkstreamContext>,
    pub request_id: Option<String>,
    pub ppid: Option<i64>,
    /// Seconds since the 2001 reference date (see module JSON parity note).
    pub received_at: f64,
    /// Unknown wire keys, preserved as a sorted JSON object string.
    pub extra_fields_json: Option<String>,
}

impl WorkstreamEvent {
    /// The three required fields; all optionals default to `None`/`0.0`, matching
    /// the defaulted parameters of Swift's `init(...)` (`WorkstreamEvent.swift:24-50`).
    pub fn new(
        session_id: impl Into<String>,
        hook_event_name: HookEventName,
        source: impl Into<String>,
    ) -> Self {
        Self {
            session_id: session_id.into(),
            hook_event_name,
            source: source.into(),
            workspace_id: None,
            cwd: None,
            tool_name: None,
            tool_input_json: None,
            context: None,
            request_id: None,
            ppid: None,
            received_at: 0.0,
            extra_fields_json: None,
        }
    }

    pub fn with_workspace_id(mut self, value: impl Into<String>) -> Self {
        self.workspace_id = Some(value.into());
        self
    }
    pub fn with_cwd(mut self, value: impl Into<String>) -> Self {
        self.cwd = Some(value.into());
        self
    }
    pub fn with_tool_name(mut self, value: impl Into<String>) -> Self {
        self.tool_name = Some(value.into());
        self
    }
    pub fn with_tool_input_json(mut self, value: impl Into<String>) -> Self {
        self.tool_input_json = Some(value.into());
        self
    }
    pub fn with_context(mut self, value: WorkstreamContext) -> Self {
        self.context = Some(value);
        self
    }
    pub fn with_request_id(mut self, value: impl Into<String>) -> Self {
        self.request_id = Some(value.into());
        self
    }
    pub fn with_ppid(mut self, value: i64) -> Self {
        self.ppid = Some(value);
        self
    }
    pub fn with_received_at(mut self, value: f64) -> Self {
        self.received_at = value;
        self
    }
}

impl Serialize for WorkstreamEvent {
    /// Swift `encode(to:)` (`WorkstreamEvent.swift:120-145`): the known scalar
    /// keys first (nil optionals omitted), then `_received_at`, then the dynamic
    /// extra fields, and finally `tool_input` re-parsed to real JSON.
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("session_id", &self.session_id)?;
        map.serialize_entry("hook_event_name", &self.hook_event_name)?;
        map.serialize_entry("_source", &self.source)?;
        if let Some(workspace_id) = &self.workspace_id {
            map.serialize_entry("workspace_id", workspace_id)?;
        }
        if let Some(cwd) = &self.cwd {
            map.serialize_entry("cwd", cwd)?;
        }
        if let Some(tool_name) = &self.tool_name {
            map.serialize_entry("tool_name", tool_name)?;
        }
        if let Some(context) = &self.context {
            map.serialize_entry("context", context)?;
        }
        if let Some(request_id) = &self.request_id {
            map.serialize_entry("_opencode_request_id", request_id)?;
        }
        if let Some(ppid) = &self.ppid {
            map.serialize_entry("_ppid", ppid)?;
        }
        map.serialize_entry("_received_at", &self.received_at)?;

        // Dynamic extra fields (skip any that collide with a known key).
        if let Some(Value::Object(extra)) = self
            .extra_fields_json
            .as_deref()
            .and_then(json_from_string)
        {
            for (key, value) in &extra {
                if !KNOWN_KEYS.contains(&key.as_str()) {
                    map.serialize_entry(key, value)?;
                }
            }
        }

        // tool_input re-parsed to real JSON, falling back to a JSON string.
        if let Some(tool_input_json) = &self.tool_input_json {
            let raw = json_from_string(tool_input_json)
                .unwrap_or_else(|| Value::String(tool_input_json.clone()));
            map.serialize_entry("tool_input", &raw)?;
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for WorkstreamEvent {
    /// Swift `init(from:)` (`WorkstreamEvent.swift:90-118`).
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        let Value::Object(obj) = value else {
            return Err(D::Error::custom("expected WorkstreamEvent object"));
        };

        let session_id = require_str(&obj, "session_id")?;
        let hook_event_name: HookEventName = obj
            .get("hook_event_name")
            .ok_or_else(|| D::Error::missing_field("hook_event_name"))
            .and_then(|v| serde_json::from_value(v.clone()).map_err(D::Error::custom))?;
        let source = require_str(&obj, "_source")?;

        let workspace_id = optional_str(&obj, "workspace_id")?;
        let cwd = optional_str(&obj, "cwd")?;
        let tool_name = optional_str(&obj, "tool_name")?;
        let request_id = optional_str(&obj, "_opencode_request_id")?;

        let context = match obj.get("context") {
            Some(v) if !v.is_null() => {
                Some(serde_json::from_value(v.clone()).map_err(D::Error::custom)?)
            }
            _ => None,
        };

        let ppid = optional_i64(&obj, "_ppid")?;
        let received_at = obj.get("_received_at").and_then(Value::as_f64).unwrap_or(0.0);

        // Capture unknown keys into a sorted extra-fields object string.
        let mut extra: Map<String, Value> = Map::new();
        for (key, val) in &obj {
            if !KNOWN_KEYS.contains(&key.as_str()) {
                extra.insert(key.clone(), val.clone());
            }
        }
        let extra_fields_json = if extra.is_empty() {
            None
        } else {
            Some(json_to_string(&Value::Object(extra)))
        };

        // tool_input: present (even null) → normalized string; absent → None.
        let tool_input_json = obj.get("tool_input").map(json_to_string);

        Ok(WorkstreamEvent {
            session_id,
            hook_event_name,
            source,
            workspace_id,
            cwd,
            tool_name,
            tool_input_json,
            context,
            request_id,
            ppid,
            received_at,
            extra_fields_json,
        })
    }
}

fn require_str<E: serde::de::Error>(obj: &Map<String, Value>, key: &'static str) -> Result<String, E> {
    obj.get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| E::missing_field(key))
}

/// Swift `try c.decodeIfPresent(String.self, forKey:)`
/// (`WorkstreamEvent.swift:95-99`): returns `None` only when the key is absent
/// or JSON `null`. A present, non-null value of the wrong JSON type is NOT
/// silently dropped — Swift's `decodeIfPresent` throws
/// `DecodingError.typeMismatch` and fails the whole decode, so we surface a
/// custom error here rather than coercing to `None`.
fn optional_str<E: serde::de::Error>(obj: &Map<String, Value>, key: &str) -> Result<Option<String>, E> {
    match obj.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.clone())),
        Some(_) => Err(E::custom(format!(
            "type mismatch decoding optional string field {key:?}"
        ))),
    }
}

/// Swift `try c.decodeIfPresent(Int.self, forKey: .ppid)`
/// (`WorkstreamEvent.swift:100`): mirrors [`optional_str`] for integers. A
/// present, non-null value that is not representable as an `Int` throws
/// `DecodingError.typeMismatch` in Swift, so we reject it instead of coercing to
/// `None`.
fn optional_i64<E: serde::de::Error>(obj: &Map<String, Value>, key: &str) -> Result<Option<i64>, E> {
    match obj.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v.as_i64().map(Some).ok_or_else(|| {
            E::custom(format!("type mismatch decoding optional int field {key:?}"))
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workstream_context::WorkstreamAllowedPrompt;

    // Ports of Tests/CMUXAgentLaunchTests/Workstream/WorkstreamEventTests.swift:1-165.

    /// `decodesHookPayload` (WorkstreamEventTests.swift:7-41).
    #[test]
    fn decodes_hook_payload() {
        let json = r#"
        {
          "session_id": "claude-abc",
          "hook_event_name": "PermissionRequest",
          "_source": "claude",
          "workspace_id": "11111111-1111-1111-1111-111111111111",
          "cwd": "/tmp/proj",
          "tool_name": "Write",
          "tool_input": {"file_path": "/etc/passwd", "content": "x"},
          "context": {"lastUserMessage": "write a file", "permissionMode": "plan"},
          "_opencode_request_id": "req-1",
          "_ppid": 1234
        }
        "#;
        let event: WorkstreamEvent = serde_json::from_str(json).unwrap();
        assert_eq!(event.session_id, "claude-abc");
        assert_eq!(event.hook_event_name, HookEventName::PermissionRequest);
        assert_eq!(event.source, "claude");
        assert_eq!(
            event.workspace_id.as_deref(),
            Some("11111111-1111-1111-1111-111111111111")
        );
        assert_eq!(event.tool_name.as_deref(), Some("Write"));
        assert_eq!(
            event.context.as_ref().and_then(|c| c.last_user_message.as_deref()),
            Some("write a file")
        );
        assert_eq!(
            event.context.as_ref().and_then(|c| c.permission_mode.as_deref()),
            Some("plan")
        );
        assert_eq!(event.request_id.as_deref(), Some("req-1"));
        assert_eq!(event.ppid, Some(1234));
        let raw = event.tool_input_json.as_deref().unwrap();
        let dict: Value = serde_json::from_str(raw).unwrap();
        assert_eq!(dict["file_path"], Value::String("/etc/passwd".into()));
        assert_eq!(dict["content"], Value::String("x".into()));
    }

    /// `roundTrip` (WorkstreamEventTests.swift:43-79).
    #[test]
    fn round_trip() {
        let event = WorkstreamEvent::new("opencode-xyz", HookEventName::ExitPlanMode, "opencode")
            .with_workspace_id("22222222-2222-2222-2222-222222222222")
            .with_cwd("/work")
            .with_tool_name("ExitPlanMode")
            .with_tool_input_json("{\"plan\":\"step1\\nstep2\"}")
            .with_context(WorkstreamContext::new(
                Some("make a plan".to_string()),
                Some("I will make a short plan.".to_string()),
                None,
                vec![WorkstreamAllowedPrompt::new("Bash", "run tests")],
                None,
                Some("plan".to_string()),
            ))
            .with_request_id("plan-1")
            .with_ppid(999);
        let data = serde_json::to_string(&event).unwrap();
        let back: WorkstreamEvent = serde_json::from_str(&data).unwrap();
        assert_eq!(back.session_id, event.session_id);
        assert_eq!(back.hook_event_name, event.hook_event_name);
        assert_eq!(back.workspace_id, event.workspace_id);
        assert_eq!(back.request_id, event.request_id);
        let plan: Value = serde_json::from_str(back.tool_input_json.as_deref().unwrap()).unwrap();
        assert!(plan["plan"].as_str().unwrap().contains("step1"));
        let ctx = back.context.unwrap();
        assert_eq!(ctx.last_user_message.as_deref(), Some("make a plan"));
        assert_eq!(ctx.assistant_preamble.as_deref(), Some("I will make a short plan."));
        assert_eq!(ctx.allowed_prompts.first().map(|p| p.prompt.as_str()), Some("run tests"));
        assert_eq!(ctx.permission_mode.as_deref(), Some("plan"));
    }

    /// `missingOptionals` (WorkstreamEventTests.swift:81-94).
    #[test]
    fn missing_optionals() {
        let json =
            r#"{"session_id": "s", "hook_event_name": "SessionStart", "_source": "claude"}"#;
        let event: WorkstreamEvent = serde_json::from_str(json).unwrap();
        assert_eq!(event.cwd, None);
        assert_eq!(event.tool_name, None);
        assert_eq!(event.tool_input_json, None);
        assert_eq!(event.context, None);
        assert_eq!(event.workspace_id, None);
        assert_eq!(event.request_id, None);
        assert_eq!(event.ppid, None);
    }

    /// Pinning oracle for the strict-optional parity fix: Swift
    /// `decodeIfPresent(String.self, forKey: .cwd)` throws
    /// `DecodingError.typeMismatch` when `cwd` is present but not a string, so the
    /// whole event must be rejected — not accepted with `cwd == None`
    /// (`WorkstreamEvent.swift:96`).
    #[test]
    fn rejects_wrong_typed_optional_string() {
        let json = r#"{"session_id":"s","hook_event_name":"SessionStart","_source":"claude","cwd":123}"#;
        let err = serde_json::from_str::<WorkstreamEvent>(json).unwrap_err();
        assert!(
            err.to_string().contains("type mismatch"),
            "expected type-mismatch rejection, got: {err}"
        );
    }

    /// Same class as [`rejects_wrong_typed_optional_string`], for `_ppid`, which
    /// Swift decodes via `decodeIfPresent(Int.self, forKey: .ppid)`
    /// (`WorkstreamEvent.swift:100`). A present string value must fail the decode.
    #[test]
    fn rejects_wrong_typed_optional_int() {
        let json =
            r#"{"session_id":"s","hook_event_name":"SessionStart","_source":"claude","_ppid":"1234"}"#;
        let err = serde_json::from_str::<WorkstreamEvent>(json).unwrap_err();
        assert!(
            err.to_string().contains("type mismatch"),
            "expected type-mismatch rejection, got: {err}"
        );
    }

    /// A JSON `null` for an optional field is still absent-equivalent and decodes
    /// to `None`, matching Swift `decodeIfPresent`'s null handling — the strict
    /// fix must NOT reject explicit nulls.
    #[test]
    fn accepts_null_optional_fields() {
        let json = r#"{"session_id":"s","hook_event_name":"SessionStart","_source":"claude","cwd":null,"_ppid":null}"#;
        let event: WorkstreamEvent = serde_json::from_str(json).unwrap();
        assert_eq!(event.cwd, None);
        assert_eq!(event.ppid, None);
    }

    /// `codexLifecycleFeedEventsDecode` (WorkstreamEventTests.swift:96-121).
    #[test]
    fn codex_lifecycle_feed_events_decode() {
        let cases = [
            ("PostToolUse", HookEventName::PostToolUse),
            ("PreCompact", HookEventName::PreCompact),
            ("PostCompact", HookEventName::PostCompact),
            ("SubagentStart", HookEventName::SubagentStart),
            ("SubagentStop", HookEventName::SubagentStop),
        ];
        for (wire_name, expected) in cases {
            let json = format!(
                r#"{{"session_id":"codex-session","hook_event_name":"{wire_name}","_source":"codex","cwd":"/tmp/project"}}"#
            );
            let event: WorkstreamEvent = serde_json::from_str(&json).unwrap();
            assert_eq!(event.hook_event_name, expected);
            assert_eq!(event.source, "codex");
            assert_eq!(event.session_id, "codex-session");
        }
    }

    /// `unknownFieldsRoundTrip` (WorkstreamEventTests.swift:123-149).
    #[test]
    fn unknown_fields_round_trip() {
        let json = r#"
        {
          "session_id": "s",
          "hook_event_name": "SessionStart",
          "_source": "claude",
          "future_field": {"enabled": true, "count": 2}
        }
        "#;
        let event: WorkstreamEvent = serde_json::from_str(json).unwrap();
        let extra: Value = serde_json::from_str(event.extra_fields_json.as_deref().unwrap()).unwrap();
        let future = &extra["future_field"];
        assert_eq!(future["enabled"], Value::Bool(true));
        assert_eq!(future["count"], Value::from(2));

        let encoded = serde_json::to_string(&event).unwrap();
        let object: Value = serde_json::from_str(&encoded).unwrap();
        let encoded_future = &object["future_field"];
        assert_eq!(encoded_future["enabled"], Value::Bool(true));
        assert_eq!(encoded_future["count"], Value::from(2));
    }

    /// `encodesRawToolInputString` (WorkstreamEventTests.swift:151-164).
    #[test]
    fn encodes_raw_tool_input_string() {
        let event = WorkstreamEvent::new("s", HookEventName::UserPromptSubmit, "claude")
            .with_tool_input_json("plain text");
        let data = serde_json::to_string(&event).unwrap();
        let object: Value = serde_json::from_str(&data).unwrap();
        assert_eq!(object["tool_input"], Value::String("plain text".into()));
    }
}
