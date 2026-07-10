//! Pure prompt/assistant-message extraction from a feed [`WorkstreamEvent`],
//! plus the conversation-message preview collapse/truncate helper.
//!
//! Swift parity source: `Sources/WorkspacePromptSubmit.swift`
//! - `WorkstreamEvent.submittedPromptMessage` (lines 67-73)
//! - `WorkstreamEvent.assistantFinalMessage` (lines 75-81)
//! - `messageText(fromJSON:keys:)` (lines 93-106)
//! - `messageText(from:keys:)` (lines 108-119)
//! - `firstMessageString(in:keys:)` (lines 121-128)
//! - `normalizedPromptText(_:)` (lines 130-136)
//! - `Workspace.conversationMessagePreview(from:maxLength:)` (lines 217-228)
//!
//! SCOPE: only the pure accessors are ported here. The stateful
//! `TabManager` reorder + `CmuxEventBus` publish and the `UserDefaults`-backed
//! `IMessageMode` settings that share the Swift file are GUI/state concerns and
//! are intentionally NOT part of this crate.
//!
//! SANCTIONED DIVERGENCE (whitespace collapse): Swift collapses runs with
//! `split(whereSeparator: { $0.isWhitespace })` (Unicode `Character.isWhitespace`)
//! and trims with `.whitespacesAndNewlines`. Rust's [`str::split_whitespace`]
//! splits on the Unicode `White_Space` property and drops empty runs, which
//! agrees with the Swift whitespace set on all realistic inputs — the same
//! convention already documented in [`crate::workstream_context`].

use serde_json::{Map, Value};
use unicode_segmentation::UnicodeSegmentation;

use crate::feed_event::{HookEventName, WorkstreamEvent};

/// Keys searched for a submitted user prompt, in priority order.
/// Swift `promptMessageKeys` (`WorkspacePromptSubmit.swift:83`).
const PROMPT_MESSAGE_KEYS: &[&str] = &["prompt", "text", "message", "body"];

/// Keys searched for the assistant's final message, in priority order.
/// Swift `assistantMessageKeys` (`WorkspacePromptSubmit.swift:84-91`).
const ASSISTANT_MESSAGE_KEYS: &[&str] = &[
    "last_assistant_message",
    "lastAssistantMessage",
    "assistantPreamble",
    "assistant_preamble",
    "last_agent_message",
    "lastAgentMessage",
];

impl WorkstreamEvent {
    /// The user prompt text for a `UserPromptSubmit` event, or `None` for any
    /// other hook. Priority: `tool_input` JSON → cleaned `context.lastUserMessage`
    /// → `extra_fields` JSON.
    ///
    /// Swift `submittedPromptMessage` (`WorkspacePromptSubmit.swift:67-73`).
    pub fn submitted_prompt_message(&self) -> Option<String> {
        if self.hook_event_name != HookEventName::UserPromptSubmit {
            return None;
        }
        // Swift precomputes `contextMessage` before the `??` chain, so eager
        // evaluation here is faithful.
        let context_message = self
            .context
            .as_ref()
            .and_then(|context| context.last_user_message.as_deref())
            .and_then(normalized_prompt_text);
        message_text_from_json(self.tool_input_json.as_deref(), PROMPT_MESSAGE_KEYS)
            .or(context_message)
            .or_else(|| {
                message_text_from_json(self.extra_fields_json.as_deref(), PROMPT_MESSAGE_KEYS)
            })
    }

    /// The assistant's final message for a `Stop` event, or `None` for any other
    /// hook (notably `SubagentStop`, which must NOT surface the parent
    /// assistant text). Priority: cleaned `context.assistantPreamble` →
    /// `extra_fields` JSON → `tool_input` JSON.
    ///
    /// Swift `assistantFinalMessage` (`WorkspacePromptSubmit.swift:75-81`).
    pub fn assistant_final_message(&self) -> Option<String> {
        if self.hook_event_name != HookEventName::Stop {
            return None;
        }
        let context_message = self
            .context
            .as_ref()
            .and_then(|context| context.assistant_preamble.as_deref())
            .and_then(normalized_prompt_text);
        context_message
            .or_else(|| {
                message_text_from_json(self.extra_fields_json.as_deref(), ASSISTANT_MESSAGE_KEYS)
            })
            .or_else(|| {
                message_text_from_json(self.tool_input_json.as_deref(), ASSISTANT_MESSAGE_KEYS)
            })
    }
}

/// Parse `json_string` (fragments allowed) and pull the first message string.
///
/// Swift `messageText(fromJSON:keys:)` (`WorkspacePromptSubmit.swift:93-106`):
/// * `None` in → `None`.
/// * Unparseable JSON → normalize the raw string itself (mirrors
///   `JSONSerialization` throwing, then falling back to `normalizedPromptText`).
/// * A bare JSON string (`.fragmentsAllowed`) → normalize its contents.
/// * A JSON object → search `keys` (with `notification`/`data` fallback).
/// * Anything else (number/array/bool/null) → `None`.
fn message_text_from_json(json_string: Option<&str>, keys: &[&str]) -> Option<String> {
    let json_string = json_string?;
    // serde_json parses top-level scalars, matching Foundation's
    // `JSONSerialization` `.fragmentsAllowed` option.
    let value: Value = match serde_json::from_str(json_string) {
        Ok(value) => value,
        Err(_) => return normalized_prompt_text(json_string),
    };
    match value {
        Value::String(string) => normalized_prompt_text(&string),
        Value::Object(dict) => message_text_from_dict(&dict, keys),
        _ => None,
    }
}

/// Search a decoded object for the first non-empty message string, first at the
/// top level then inside a nested `notification`/`data` object.
///
/// Swift `messageText(from:keys:)` (`WorkspacePromptSubmit.swift:108-119`).
fn message_text_from_dict(dict: &Map<String, Value>, keys: &[&str]) -> Option<String> {
    if let Some(direct) = first_message_string(dict, keys) {
        return Some(direct);
    }
    for nested_key in ["notification", "data"] {
        if let Some(Value::Object(nested)) = dict.get(nested_key) {
            if let Some(nested_message) = first_message_string(nested, keys) {
                return Some(nested_message);
            }
        }
    }
    None
}

/// Return the normalized value of the first `keys` entry that is a non-empty
/// string. A present-but-non-string value, or one that normalizes to empty, is
/// skipped (Swift's `guard ... else { continue }`).
///
/// Swift `firstMessageString(in:keys:)` (`WorkspacePromptSubmit.swift:121-128`).
fn first_message_string(dict: &Map<String, Value>, keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Some(Value::String(value)) = dict.get(*key) {
            if let Some(normalized) = normalized_prompt_text(value) {
                return Some(normalized);
            }
        }
    }
    None
}

/// Collapse whitespace runs to single spaces and trim; map empty to `None`.
///
/// Swift `normalizedPromptText(_:)` (`WorkspacePromptSubmit.swift:130-136`).
/// [`str::split_whitespace`] already trims, collapses runs, and drops empties,
/// so the trailing `trimmingCharacters(in:.whitespacesAndNewlines)` is a no-op.
fn normalized_prompt_text(value: &str) -> Option<String> {
    let normalized = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

/// Collapse whitespace and truncate to `max_length` graphemes with a trailing
/// `...`; `None` for a missing or blank message.
///
/// Swift `Workspace.conversationMessagePreview(from:maxLength:)`
/// (`WorkspacePromptSubmit.swift:217-228`), whose default `maxLength` is `240`.
///
/// Swift measures with `String.count` and slices with `String.prefix`, both of
/// which operate on `Character`s (extended grapheme clusters). We count and
/// slice with UAX #29 grapheme clusters ([`UnicodeSegmentation::graphemes`]
/// with `is_extended = true`) for byte-for-byte parity on multi-scalar
/// clusters — the same grapheme-accurate convention used in `cmux-window-title`.
pub fn conversation_message_preview(message: Option<&str>, max_length: usize) -> Option<String> {
    let collapsed = normalized_prompt_text(message?)?;
    if collapsed.graphemes(true).count() <= max_length {
        Some(collapsed)
    } else {
        let prefix: String = collapsed.graphemes(true).take(max_length).collect();
        Some(format!("{prefix}..."))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workstream_context::WorkstreamContext;

    /// Build a bare context carrying only `last_user_message`.
    fn context_with_user_message(message: &str) -> WorkstreamContext {
        WorkstreamContext::new(Some(message.to_string()), None, None, vec![], None, None)
    }

    /// Build a bare context carrying only `assistant_preamble`.
    fn context_with_assistant_preamble(message: &str) -> WorkstreamContext {
        WorkstreamContext::new(None, Some(message.to_string()), None, vec![], None, None)
    }

    /// `testFeedPromptSubmitEventExtractsToolInputMessage`
    /// (WorkspacePromptSubmitTests.swift:175-201): the tool-input `prompt`
    /// wins over the context fallback and is whitespace-collapsed.
    #[test]
    fn prompt_submit_extracts_tool_input_message() {
        let event = WorkstreamEvent::new(
            "opencode-session",
            HookEventName::UserPromptSubmit,
            "opencode",
        )
        .with_tool_input_json(r#"{"prompt":"  shipped from feed\npath  "}"#)
        .with_context(context_with_user_message("fallback message"));
        assert_eq!(
            event.submitted_prompt_message().as_deref(),
            Some("shipped from feed path")
        );
    }

    /// `testFeedPromptSubmitEventFallsBackToContextMessage`
    /// (WorkspacePromptSubmitTests.swift:203-213): no tool input, so the
    /// cleaned context message is used.
    #[test]
    fn prompt_submit_falls_back_to_context_message() {
        let event = WorkstreamEvent::new("agent-session", HookEventName::UserPromptSubmit, "codex")
            .with_context(context_with_user_message("from context"));
        assert_eq!(
            event.submitted_prompt_message().as_deref(),
            Some("from context")
        );
    }

    /// `testFeedPromptSubmitSkipsBlankContextBeforeExtraFields`
    /// (WorkspacePromptSubmitTests.swift:215-226): a context that cleans to
    /// empty is skipped and the extra-fields `message` is used instead.
    #[test]
    fn prompt_submit_skips_blank_context_before_extra_fields() {
        let mut event =
            WorkstreamEvent::new("agent-session", HookEventName::UserPromptSubmit, "codex")
                .with_context(context_with_user_message(" \n "));
        event.extra_fields_json = Some(r#"{"message":"from extra fields"}"#.to_string());
        assert_eq!(
            event.submitted_prompt_message().as_deref(),
            Some("from extra fields")
        );
    }

    /// `testFeedStopEventExtractsAssistantFinalMessageFromContext`
    /// (WorkspacePromptSubmitTests.swift:228-238).
    #[test]
    fn stop_extracts_assistant_final_message_from_context() {
        let event = WorkstreamEvent::new("agent-session", HookEventName::Stop, "codex")
            .with_context(context_with_assistant_preamble("  finished\n\nthis  "));
        assert_eq!(
            event.assistant_final_message().as_deref(),
            Some("finished this")
        );
    }

    /// `testFeedStopEventExtractsAssistantFinalMessageFromExtraFields`
    /// (WorkspacePromptSubmitTests.swift:240-250): no context, so the
    /// `last_assistant_message` extra field is used.
    #[test]
    fn stop_extracts_assistant_final_message_from_extra_fields() {
        let mut event = WorkstreamEvent::new("agent-session", HookEventName::Stop, "codex");
        event.extra_fields_json =
            Some(r#"{"last_assistant_message":"  done\nfrom extra fields  "}"#.to_string());
        assert_eq!(
            event.assistant_final_message().as_deref(),
            Some("done from extra fields")
        );
    }

    /// `testFeedSubagentStopDoesNotExtractParentAssistantFinalMessage`
    /// (WorkspacePromptSubmitTests.swift:252-262): a `SubagentStop` hook is not
    /// a `Stop`, so no parent assistant text is surfaced.
    #[test]
    fn subagent_stop_does_not_extract_parent_assistant_final_message() {
        let event = WorkstreamEvent::new("agent-session", HookEventName::SubagentStop, "codex")
            .with_context(context_with_assistant_preamble("subagent finished"));
        assert_eq!(event.assistant_final_message(), None);
    }

    // ---- parity-risk edges (beyond the cited oracle tests) ----

    /// A non-`UserPromptSubmit` hook returns `None` even with a tool-input
    /// prompt present (the `guard hookEventName == .userPromptSubmit`).
    #[test]
    fn prompt_submit_gated_on_hook_event_name() {
        let event = WorkstreamEvent::new("s", HookEventName::Stop, "codex")
            .with_tool_input_json(r#"{"prompt":"ignored"}"#);
        assert_eq!(event.submitted_prompt_message(), None);
    }

    /// Nested `notification`/`data` object fallback in `messageText(from:keys:)`
    /// (WorkspacePromptSubmit.swift:112-117): the top-level object has no key,
    /// but a nested `notification` object does.
    #[test]
    fn prompt_submit_extracts_from_nested_notification() {
        let event = WorkstreamEvent::new("s", HookEventName::UserPromptSubmit, "codex")
            .with_tool_input_json(r#"{"notification":{"text":"nested prompt"}}"#);
        assert_eq!(
            event.submitted_prompt_message().as_deref(),
            Some("nested prompt")
        );
    }

    /// Unparseable tool-input JSON falls back to normalizing the raw string
    /// (WorkspacePromptSubmit.swift:97-99).
    #[test]
    fn prompt_submit_normalizes_unparseable_tool_input() {
        let event = WorkstreamEvent::new("s", HookEventName::UserPromptSubmit, "codex")
            .with_tool_input_json("  plain   text  ");
        assert_eq!(
            event.submitted_prompt_message().as_deref(),
            Some("plain text")
        );
    }

    /// A bare JSON string tool input (`.fragmentsAllowed`) normalizes its
    /// contents (WorkspacePromptSubmit.swift:101-103).
    #[test]
    fn prompt_submit_normalizes_json_string_fragment() {
        let event = WorkstreamEvent::new("s", HookEventName::UserPromptSubmit, "codex")
            .with_tool_input_json(r#""  json  string  ""#);
        assert_eq!(
            event.submitted_prompt_message().as_deref(),
            Some("json string")
        );
    }

    /// A JSON number/array (neither string nor object) yields `None` from that
    /// source (WorkspacePromptSubmit.swift:104), letting later sources win.
    #[test]
    fn message_text_ignores_non_string_non_object_json() {
        let event = WorkstreamEvent::new("s", HookEventName::UserPromptSubmit, "codex")
            .with_tool_input_json("[1, 2, 3]")
            .with_context(context_with_user_message("context wins"));
        assert_eq!(
            event.submitted_prompt_message().as_deref(),
            Some("context wins")
        );
    }

    /// `conversationMessagePreview` collapses whitespace and returns the whole
    /// message when it is at or under `max_length`.
    #[test]
    fn conversation_preview_collapses_and_keeps_short() {
        assert_eq!(
            conversation_message_preview(Some("  hello \n\n world  "), 240).as_deref(),
            Some("hello world")
        );
    }

    /// `conversationMessagePreview` returns `None` for a missing or blank message.
    #[test]
    fn conversation_preview_none_for_missing_or_blank() {
        assert_eq!(conversation_message_preview(None, 240), None);
        assert_eq!(conversation_message_preview(Some(" \n "), 240), None);
    }

    /// `conversationMessagePreview` truncates to `max_length` graphemes and
    /// appends `...` (WorkspacePromptSubmit.swift:225-226).
    #[test]
    fn conversation_preview_truncates_with_ellipsis() {
        let long = "a".repeat(300);
        let preview = conversation_message_preview(Some(&long), 240).unwrap();
        assert_eq!(preview.chars().filter(|&c| c == 'a').count(), 240);
        assert!(preview.ends_with("..."));
        assert_eq!(preview.len(), 243);
    }
}
