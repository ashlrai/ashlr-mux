//! The transcript value types: one conversation item and its typed payload.
//!
//! Ports the `Model/*.swift` value types (`ChatMessage`, `ChatMessageKind`,
//! `ChatRole`, `ChatProse`, `ChatThought`, `ChatToolUse`,
//! `ChatTerminalCapture`, `ChatFileEdit`, `ChatQuestion`,
//! `ChatStatusTransition`, plus the never-parser-emitted `ChatPermissionRequest`
//! / `ChatAttachment` / `ChatUnsupportedPayload`).
//!
//! Swift nests the sub-enums (`ChatToolUse.Status`, `ChatFileEdit.Operation`,
//! ...); Rust flattens them to prefixed top-level types
//! (`ChatToolUseStatus`, `ChatFileEditOperation`, ...). Wire tags and
//! snake_case CodingKeys are preserved exactly, and the type-tagged
//! `ChatMessageKind` keeps the Swift fail-open decode: an unknown `type` (or a
//! payload that fails to decode) degrades to
//! [`ChatMessageKind::Unsupported`] instead of dropping the row.

use serde::de::Error as _;
use serde::ser::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};

use super::timestamp::Timestamp;

/// The author of a message within an agent or terminal conversation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChatRole {
    /// The human operating the session (prompts, answers, attachments).
    User,
    /// The coding agent (prose, tool runs, edits, questions).
    Agent,
    /// The session itself (lifecycle transitions, connection changes).
    System,
}

impl ChatRole {
    /// Maps a wire `role` string to a role, mirroring the Swift
    /// `ChatRole(rawValue:)` lookup used inside the fail-open message decode.
    fn from_wire(raw: &str) -> Option<ChatRole> {
        match raw {
            "user" => Some(ChatRole::User),
            "agent" => Some(ChatRole::Agent),
            "system" => Some(ChatRole::System),
            _ => None,
        }
    }
}

/// Conversational text content: a user prompt or agent prose.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatProse {
    /// The message text. Agent-authored text may contain markdown.
    pub text: String,
}

impl ChatProse {
    /// Creates prose content.
    pub fn new(text: impl Into<String>) -> Self {
        ChatProse { text: text.into() }
    }
}

/// A reasoning/thinking block the agent produced before responding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatThought {
    /// The reasoning text, possibly summarized by the agent runtime.
    pub text: String,
}

impl ChatThought {
    /// Creates a thought block.
    pub fn new(text: impl Into<String>) -> Self {
        ChatThought { text: text.into() }
    }
}

/// Lifecycle of a tool invocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChatToolUseStatus {
    /// The tool was invoked and no result has been observed yet.
    Running,
    /// The tool finished successfully.
    Succeeded,
    /// The tool finished with an error result.
    Failed,
}

/// A non-terminal tool invocation, paired with its result when one arrives.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatToolUse {
    /// The tool's machine name as the agent reported it (e.g. `Read`).
    #[serde(rename = "tool_name")]
    pub tool_name: String,
    /// A one-line human-readable summary of the invocation.
    pub summary: String,
    /// The full tool input rendered as text, for the expanded state.
    #[serde(
        rename = "input_detail",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub input_detail: Option<String>,
    /// The tool result rendered as text, when one has arrived.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    /// Current lifecycle state of the invocation.
    pub status: ChatToolUseStatus,
}

impl ChatToolUse {
    /// Creates a running tool invocation with no result yet, mirroring the
    /// Swift `status: Status = .running` default used by the parsers.
    pub fn running(
        tool_name: impl Into<String>,
        summary: impl Into<String>,
        input_detail: Option<String>,
    ) -> Self {
        ChatToolUse {
            tool_name: tool_name.into(),
            summary: summary.into(),
            input_detail,
            output: None,
            status: ChatToolUseStatus::Running,
        }
    }
}

/// A shell command and its captured output; renders as a terminal card.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatTerminalCapture {
    /// The command line as submitted to the shell.
    pub command: String,
    /// Captured output (stdout and stderr interleaved), possibly truncated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    /// The command's exit code, when known.
    #[serde(rename = "exit_code", default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i64>,
    /// Wall-clock duration in seconds, when known.
    #[serde(
        rename = "duration_seconds",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub duration_seconds: Option<f64>,
    /// Whether the command is still running (no result observed yet).
    #[serde(rename = "is_running")]
    pub is_running: bool,
}

impl ChatTerminalCapture {
    /// Creates a running terminal capture with no result yet, mirroring the
    /// Swift `ChatTerminalCapture(command:, isRunning: true)` the parsers use.
    pub fn running(command: impl Into<String>) -> Self {
        ChatTerminalCapture {
            command: command.into(),
            output: None,
            exit_code: None,
            duration_seconds: None,
            is_running: true,
        }
    }
}

/// The nature of a file modification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChatFileEditOperation {
    /// An in-place edit of an existing file.
    Edit,
    /// A whole-file write (create or overwrite).
    Write,
    /// A file deletion.
    Delete,
}

/// A file modification by the agent; renders as a diff card.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatFileEdit {
    /// Path of the modified file, as the agent reported it.
    #[serde(rename = "file_path")]
    pub file_path: String,
    /// The nature of the modification.
    pub operation: ChatFileEditOperation,
    /// Count of added lines, when computable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub additions: Option<i64>,
    /// Count of removed lines, when computable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deletions: Option<i64>,
    /// A unified-diff rendering of the change, possibly truncated.
    #[serde(
        rename = "unified_diff",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub unified_diff: Option<String>,
}

/// One selectable answer to a [`ChatQuestion`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatQuestionOption {
    /// The display text of the option.
    pub label: String,
    /// Optional longer explanation of the option.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl ChatQuestionOption {
    /// Creates an option.
    pub fn new(label: impl Into<String>, detail: Option<String>) -> Self {
        ChatQuestionOption {
            label: label.into(),
            detail,
        }
    }
}

/// A multiple-choice question the agent asked the user.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatQuestion {
    /// The question text.
    pub prompt: String,
    /// The selectable answers, in display order.
    pub options: Vec<ChatQuestionOption>,
    /// The label of the chosen option once answered, `None` while pending.
    #[serde(
        rename = "selected_option_label",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub selected_option_label: Option<String>,
}

/// A durable session lifecycle transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChatStatusEvent {
    /// The agent session started.
    #[serde(rename = "session_started")]
    SessionStarted,
    /// The agent session ended.
    #[serde(rename = "session_ended")]
    SessionEnded,
    /// The user interrupted the agent.
    #[serde(rename = "interrupted")]
    Interrupted,
    /// The agent compacted its context window.
    #[serde(rename = "context_compacted")]
    ContextCompacted,
}

/// A durable session lifecycle transition; renders as a centered caption.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatStatusTransition {
    /// The transition that occurred.
    pub event: ChatStatusEvent,
    /// Optional human-readable detail (e.g. an exit reason).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl ChatStatusTransition {
    /// Creates a status transition.
    pub fn new(event: ChatStatusEvent, detail: Option<String>) -> Self {
        ChatStatusTransition { event, detail }
    }
}

/// How a [`ChatPermissionRequest`] was answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChatPermissionResolution {
    /// The user approved the request.
    Approved,
    /// The user denied the request.
    Denied,
    /// The request lapsed (agent stopped or session ended unanswered).
    Expired,
}

/// An actionable permission request from the agent, awaiting a decision.
///
/// Never emitted by the transcript parsers (transcripts do not carry
/// permission prompts); ported for `ChatMessageKind` wire parity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatPermissionRequest {
    /// Short title for the card (e.g. "Claude wants to run:").
    pub title: String,
    /// The command or tool being gated, rendered as text.
    pub subject: String,
    /// The decision, or `None` while the request is pending.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolution: Option<ChatPermissionResolution>,
}

/// The media category of a [`ChatAttachment`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChatAttachmentMedia {
    /// A raster image (photo, screenshot).
    Image,
    /// Any other file.
    File,
}

/// An image or file the user attached to a prompt.
///
/// Never emitted by the transcript parsers; ported for wire parity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatAttachment {
    /// The attachment's media category.
    pub media: ChatAttachmentMedia,
    /// Display name, when one is known (e.g. the original filename).
    #[serde(
        rename = "display_name",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub display_name: Option<String>,
    /// Path on the host where the attachment was materialized, when known.
    #[serde(rename = "host_path", default, skip_serializing_if = "Option::is_none")]
    pub host_path: Option<String>,
}

/// Placeholder for a wire payload type this client does not understand.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatUnsupportedPayload {
    /// The unrecognized wire `type` discriminator.
    #[serde(rename = "raw_type")]
    pub raw_type: String,
}

impl ChatUnsupportedPayload {
    /// Creates an unsupported-payload placeholder.
    pub fn new(raw_type: impl Into<String>) -> Self {
        ChatUnsupportedPayload {
            raw_type: raw_type.into(),
        }
    }
}

/// The typed payload of a [`ChatMessage`]; renderers switch over this.
///
/// Encodes on the wire as `{"type": "<tag>", ...payload keys inlined...}`.
/// Unknown types (or payloads that fail to decode) decode as
/// [`ChatMessageKind::Unsupported`] so older clients fail open.
#[derive(Debug, Clone, PartialEq)]
pub enum ChatMessageKind {
    /// Conversational text, markdown-capable.
    Prose(ChatProse),
    /// A collapsed reasoning/thinking block produced by the agent.
    Thought(ChatThought),
    /// A non-terminal tool invocation.
    ToolUse(ChatToolUse),
    /// A shell command and its captured output.
    Terminal(ChatTerminalCapture),
    /// A file edit; renders as a diff card.
    FileEdit(ChatFileEdit),
    /// An actionable permission request awaiting the user's decision.
    PermissionRequest(ChatPermissionRequest),
    /// A multiple-choice question the agent asked the user.
    Question(ChatQuestion),
    /// A durable session lifecycle transition.
    Status(ChatStatusTransition),
    /// An image or file the user attached to a prompt.
    Attachment(ChatAttachment),
    /// A payload type this client does not understand.
    Unsupported(ChatUnsupportedPayload),
}

/// The wire `type` tag for each known [`ChatMessageKind`] case.
const TAG_PROSE: &str = "prose";
const TAG_THOUGHT: &str = "thought";
const TAG_TOOL_USE: &str = "tool_use";
const TAG_TERMINAL: &str = "terminal";
const TAG_FILE_EDIT: &str = "file_edit";
const TAG_PERMISSION_REQUEST: &str = "permission_request";
const TAG_QUESTION: &str = "question";
const TAG_STATUS: &str = "status";
const TAG_ATTACHMENT: &str = "attachment";

/// Serializes a payload into a `{"type": tag, ...payload}` JSON object.
fn tagged<T: Serialize>(tag: &str, payload: &T) -> Result<Value, serde_json::Error> {
    let mut map = match serde_json::to_value(payload)? {
        Value::Object(map) => map,
        _ => Map::new(),
    };
    map.insert("type".to_string(), Value::String(tag.to_string()));
    Ok(Value::Object(map))
}

impl ChatMessageKind {
    /// Renders the kind to its type-tagged JSON object.
    fn to_wire_value(&self) -> Result<Value, serde_json::Error> {
        match self {
            ChatMessageKind::Prose(p) => tagged(TAG_PROSE, p),
            ChatMessageKind::Thought(p) => tagged(TAG_THOUGHT, p),
            ChatMessageKind::ToolUse(p) => tagged(TAG_TOOL_USE, p),
            ChatMessageKind::Terminal(p) => tagged(TAG_TERMINAL, p),
            ChatMessageKind::FileEdit(p) => tagged(TAG_FILE_EDIT, p),
            ChatMessageKind::PermissionRequest(p) => tagged(TAG_PERMISSION_REQUEST, p),
            ChatMessageKind::Question(p) => tagged(TAG_QUESTION, p),
            ChatMessageKind::Status(p) => tagged(TAG_STATUS, p),
            ChatMessageKind::Attachment(p) => tagged(TAG_ATTACHMENT, p),
            // The unsupported case round-trips its raw tag as the `type` and
            // carries no other keys, exactly like the Swift encoder.
            ChatMessageKind::Unsupported(p) => {
                let mut map = Map::new();
                map.insert("type".to_string(), Value::String(p.raw_type.clone()));
                Ok(Value::Object(map))
            }
        }
    }

    /// Builds an [`ChatMessageKind::Unsupported`] for the given raw tag.
    fn unsupported(raw_type: &str) -> ChatMessageKind {
        ChatMessageKind::Unsupported(ChatUnsupportedPayload::new(raw_type))
    }
}

impl Serialize for ChatMessageKind {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.to_wire_value()
            .map_err(S::Error::custom)?
            .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ChatMessageKind {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = Value::deserialize(deserializer)?;
        let object = value
            .as_object()
            .ok_or_else(|| D::Error::custom("ChatMessageKind expects a JSON object"))?;
        let tag = object
            .get("type")
            .and_then(Value::as_str)
            .ok_or_else(|| D::Error::custom("ChatMessageKind is missing a string `type`"))?
            .to_string();

        // Payload decode failures fail OPEN to the unsupported case: a newer
        // producer adding a nested enum value degrades that single message
        // instead of throwing the whole page away.
        let decoded = |kind: Result<ChatMessageKind, serde_json::Error>| {
            kind.unwrap_or_else(|_| ChatMessageKind::unsupported(&tag))
        };
        Ok(match tag.as_str() {
            TAG_PROSE => decoded(serde_json::from_value(value.clone()).map(ChatMessageKind::Prose)),
            TAG_THOUGHT => {
                decoded(serde_json::from_value(value.clone()).map(ChatMessageKind::Thought))
            }
            TAG_TOOL_USE => {
                decoded(serde_json::from_value(value.clone()).map(ChatMessageKind::ToolUse))
            }
            TAG_TERMINAL => {
                decoded(serde_json::from_value(value.clone()).map(ChatMessageKind::Terminal))
            }
            TAG_FILE_EDIT => {
                decoded(serde_json::from_value(value.clone()).map(ChatMessageKind::FileEdit))
            }
            TAG_PERMISSION_REQUEST => decoded(
                serde_json::from_value(value.clone()).map(ChatMessageKind::PermissionRequest),
            ),
            TAG_QUESTION => {
                decoded(serde_json::from_value(value.clone()).map(ChatMessageKind::Question))
            }
            TAG_STATUS => {
                decoded(serde_json::from_value(value.clone()).map(ChatMessageKind::Status))
            }
            TAG_ATTACHMENT => {
                decoded(serde_json::from_value(value.clone()).map(ChatMessageKind::Attachment))
            }
            _ => ChatMessageKind::unsupported(&tag),
        })
    }
}

/// One item in a conversation transcript.
///
/// `id` and `seq` are load-bearing (identity, paging cursor) and their absence
/// is corruption; everything else fails open so one evolved field cannot sink
/// a whole page.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ChatMessage {
    /// Stable identity for the message, unique within its session.
    pub id: String,
    /// Monotonic position of the message within the session transcript.
    pub seq: i64,
    /// Who authored the message.
    pub role: ChatRole,
    /// When the message was produced.
    pub timestamp: Timestamp,
    /// The typed payload that decides how the message renders.
    pub kind: ChatMessageKind,
}

impl ChatMessage {
    /// Creates a chat message.
    pub fn new(
        id: impl Into<String>,
        seq: i64,
        role: ChatRole,
        timestamp: Timestamp,
        kind: ChatMessageKind,
    ) -> Self {
        ChatMessage {
            id: id.into(),
            seq,
            role,
            timestamp,
            kind,
        }
    }

    /// Copies the message with a different payload, keeping identity,
    /// position, author, and timestamp.
    pub fn replacing_kind(&self, kind: ChatMessageKind) -> ChatMessage {
        ChatMessage {
            id: self.id.clone(),
            seq: self.seq,
            role: self.role,
            timestamp: self.timestamp,
            kind,
        }
    }
}

impl<'de> Deserialize<'de> for ChatMessage {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = Value::deserialize(deserializer)?;
        let object = value
            .as_object()
            .ok_or_else(|| D::Error::custom("ChatMessage expects a JSON object"))?;

        // id and seq are genuinely load-bearing; their absence is corruption
        // and errors. Everything else fails open.
        let id = object
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| D::Error::custom("ChatMessage is missing `id`"))?
            .to_string();
        let seq = object
            .get("seq")
            .and_then(Value::as_i64)
            .ok_or_else(|| D::Error::custom("ChatMessage is missing `seq`"))?;
        let role = object
            .get("role")
            .and_then(Value::as_str)
            .and_then(ChatRole::from_wire)
            .unwrap_or(ChatRole::Agent);
        let timestamp = object
            .get("timestamp")
            .and_then(|v| serde_json::from_value::<Timestamp>(v.clone()).ok())
            .unwrap_or(Timestamp::EPOCH_ZERO);
        let kind = object
            .get("kind")
            .and_then(|v| serde_json::from_value::<ChatMessageKind>(v.clone()).ok())
            .unwrap_or_else(|| ChatMessageKind::unsupported("undecodable"));

        Ok(ChatMessage {
            id,
            seq,
            role,
            timestamp,
            kind,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(kind: ChatMessageKind) -> ChatMessageKind {
        let json = serde_json::to_string(&kind).unwrap();
        serde_json::from_str(&json).unwrap()
    }

    #[test]
    fn snake_case_tags_round_trip() {
        let cases = vec![
            ChatMessageKind::Prose(ChatProse::new("hi")),
            ChatMessageKind::Thought(ChatThought::new("thinking")),
            ChatMessageKind::ToolUse(ChatToolUse::running("Read", "Read x", None)),
            ChatMessageKind::FileEdit(ChatFileEdit {
                file_path: "/x".into(),
                operation: ChatFileEditOperation::Edit,
                additions: Some(1),
                deletions: Some(0),
                unified_diff: Some("+a".into()),
            }),
            ChatMessageKind::PermissionRequest(ChatPermissionRequest {
                title: "t".into(),
                subject: "s".into(),
                resolution: Some(ChatPermissionResolution::Approved),
            }),
            ChatMessageKind::Status(ChatStatusTransition::new(
                ChatStatusEvent::ContextCompacted,
                None,
            )),
        ];
        for kind in cases {
            assert_eq!(roundtrip(kind.clone()), kind);
        }
    }

    #[test]
    fn wire_tags_are_snake_case() {
        let value = serde_json::to_value(ChatMessageKind::ToolUse(ChatToolUse::running(
            "Read", "Read x", None,
        )))
        .unwrap();
        assert_eq!(value["type"], serde_json::json!("tool_use"));
        assert_eq!(value["tool_name"], serde_json::json!("Read"));

        let edit = serde_json::to_value(ChatMessageKind::FileEdit(ChatFileEdit {
            file_path: "/x".into(),
            operation: ChatFileEditOperation::Write,
            additions: None,
            deletions: None,
            unified_diff: None,
        }))
        .unwrap();
        assert_eq!(edit["type"], serde_json::json!("file_edit"));
        assert_eq!(edit["file_path"], serde_json::json!("/x"));
    }

    #[test]
    fn unknown_type_fails_open_to_unsupported() {
        let json = r#"{"type":"brand_new_kind","payload":42}"#;
        let kind: ChatMessageKind = serde_json::from_str(json).unwrap();
        assert_eq!(
            kind,
            ChatMessageKind::Unsupported(ChatUnsupportedPayload::new("brand_new_kind"))
        );
    }

    #[test]
    fn malformed_payload_fails_open_to_unsupported() {
        // `prose` requires a string `text`; a number must degrade, not throw.
        let json = r#"{"type":"prose","text":123}"#;
        let kind: ChatMessageKind = serde_json::from_str(json).unwrap();
        assert_eq!(
            kind,
            ChatMessageKind::Unsupported(ChatUnsupportedPayload::new("prose"))
        );
    }

    #[test]
    fn unsupported_round_trips_as_bare_type() {
        let kind = ChatMessageKind::Unsupported(ChatUnsupportedPayload::new("future"));
        let value = serde_json::to_value(&kind).unwrap();
        assert_eq!(value, serde_json::json!({ "type": "future" }));
        assert_eq!(roundtrip(kind.clone()), kind);
    }

    #[test]
    fn message_decode_is_fail_open_on_role_and_kind() {
        let json = r#"{"id":"m1","seq":3,"role":"martian","timestamp":1000,
            "kind":{"type":"prose","text":"hi"}}"#;
        let message: ChatMessage = serde_json::from_str(json).unwrap();
        assert_eq!(message.id, "m1");
        assert_eq!(message.seq, 3);
        assert_eq!(message.role, ChatRole::Agent); // unknown role -> agent
        assert_eq!(message.timestamp, Timestamp::from_millis(1000));
        assert_eq!(message.kind, ChatMessageKind::Prose(ChatProse::new("hi")));
    }

    #[test]
    fn message_round_trips_through_serde() {
        let message = ChatMessage::new(
            "m9",
            41,
            ChatRole::User,
            Timestamp::from_millis(1_781_000_000_000),
            ChatMessageKind::Prose(ChatProse::new("fix the bug")),
        );
        let json = serde_json::to_string(&message).unwrap();
        let decoded: ChatMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, message);
    }
}
