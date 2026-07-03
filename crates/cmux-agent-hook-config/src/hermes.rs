//! Faithful 1:1 port of Swift `enum HermesAgentHookConfig`
//! (`Packages/macOS/CMUXAgentLaunch/Sources/CMUXAgentLaunch/HermesAgentHookConfig.swift`).
//!
//! The Swift `enum` is used purely as a namespace of `static` functions; here it
//! becomes a module with free functions. Nested `HermesAgentHookConfig.Event`
//! becomes [`Event`].
//!
//! DIVERGENCES (all sanctioned platform swaps):
//! - The namespace `enum` → a Rust module; `static func` → free `fn`.
//! - Foundation `Data(_:).base64EncodedString()` / `Data(base64Encoded:)` →
//!   the `base64` crate's `STANDARD` engine (canonical padding, strict decode),
//!   which matches Foundation's default strict behavior.
//! - Foundation `.whitespaces` / `.whitespacesAndNewlines` trims are ASCII-only
//!   (see `common` module note).
//! - `NSRegularExpression` (ICU) → the `regex` crate. The patterns are anchored
//!   ASCII line patterns, so `^`/`$`/`\s`/`\S` semantics coincide.
//!
//! NOTE: The Swift file also declares `enum HermesAgentHookAllowlist` (a JSON
//! approvals transform). That type is out of scope for this crate and is not
//! ported here.

use std::collections::HashMap;
use std::sync::OnceLock;

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use regex::Regex;

use crate::common::{
    leading_whitespace, serialized, splice_insert, trim_ws, trim_ws_nl, yaml_double_quoted,
};

/// Port of `HermesAgentHookConfig.Event`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    pub name: String,
    pub command: String,
    pub timeout: i64,
    pub matcher: Option<String>,
}

impl Event {
    /// Mirrors the Swift `init(name:command:timeout:matcher:)` defaults
    /// (`timeout = 5`, `matcher = nil`).
    pub fn new(name: impl Into<String>, command: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            command: command.into(),
            timeout: 5,
            matcher: None,
        }
    }

    #[must_use]
    pub fn with_timeout(mut self, timeout: i64) -> Self {
        self.timeout = timeout;
        self
    }

    #[must_use]
    pub fn with_matcher(mut self, matcher: impl Into<String>) -> Self {
        self.matcher = Some(matcher.into());
        self
    }
}

/// Port of the private `EventGroup` struct.
struct EventGroup {
    name: String,
    events: Vec<Event>,
}

const BEGIN_MARKER: &str = "# cmux hooks hermes-agent begin";
const END_MARKER: &str = "# cmux hooks hermes-agent end";
const RESTORE_LINE_MARKER_PREFIX: &str = "# cmux hooks hermes-agent begin restore-line-base64:";

/// Port of `HermesAgentHookConfig.installing(events:in:)` (Swift line 27).
pub fn installing(events: &[Event], existing: &str) -> String {
    if events.is_empty() {
        return uninstalling(existing);
    }

    let mut lines = normalized_lines(existing);
    lines = removing_marked_blocks(lines);

    if let Some(hooks_index) = hooks_line_index(&lines) {
        let hooks_restore_line: Option<String> = if inline_empty_hooks_line(&lines[hooks_index]) {
            let original = lines[hooks_index].clone();
            let lw = leading_whitespace(&original).to_string();
            lines[hooks_index] = format!("{lw}hooks:");
            Some(original)
        } else {
            None
        };

        let child_indent = format!("{}  ", leading_whitespace(&lines[hooks_index]));
        let existing_events = direct_event_line_indexes(&lines, hooks_index);
        let mut missing_event_groups: Vec<EventGroup> = Vec::new();
        let mut matched_event_groups: Vec<(EventGroup, usize)> = Vec::new();

        for event_group in event_groups_by_name_preserving_order(events) {
            match existing_events.get(&event_group.name) {
                None => missing_event_groups.push(event_group),
                Some(&event_index) => matched_event_groups.push((event_group, event_index)),
            }
        }

        // `sorted(by: { $0.eventIndex > $1.eventIndex })` — descending, so
        // earlier (higher-index) insertions do not shift later lookups.
        // `Reverse` keeps this a stable descending sort (parity with Swift's
        // stable `sorted(by:)`); indexes are distinct in practice.
        matched_event_groups.sort_by_key(|(_, event_index)| std::cmp::Reverse(*event_index));

        for (event_group, event_index) in matched_event_groups {
            let event_restore_line: Option<String> = if inline_empty_event_line(&lines[event_index])
            {
                let original_line = lines[event_index].clone();
                let header_line = empty_event_header_line(&original_line);
                let restore = if original_line == header_line {
                    None
                } else {
                    Some(original_line)
                };
                lines[event_index] = header_line;
                restore
            } else {
                None
            };

            let entry_indent = format!("{}  ", leading_whitespace(&lines[event_index]));
            let block = hook_list_block(
                &event_group.events,
                &entry_indent,
                event_restore_line.as_deref(),
            );
            splice_insert(&mut lines, event_index + 1, block);
        }

        if !missing_event_groups.is_empty() {
            let block = event_sections_block_groups(
                &missing_event_groups,
                &child_indent,
                true,
                hooks_restore_line.as_deref(),
            );
            splice_insert(&mut lines, hooks_index + 1, block);
        }
    } else {
        if !lines.is_empty() && !trim_ws_nl(lines.last().unwrap()).is_empty() {
            lines.push(String::new());
        }
        lines.push(BEGIN_MARKER.to_string());
        lines.push("hooks:".to_string());
        lines.extend(event_sections_block_events(events, "  ", false, None));
        lines.push(END_MARKER.to_string());
    }

    serialized(&lines)
}

/// Port of `HermesAgentHookConfig.uninstalling(from:)` (Swift line 92).
pub fn uninstalling(existing: &str) -> String {
    serialized(&removing_marked_blocks(normalized_lines(existing)))
}

/// Port of `normalizedLines(_:)`. Normalizes `\r\n` then `\r` to `\n`, splits on
/// `\n` keeping empty subsequences, and drops a single trailing empty line.
fn normalized_lines(content: &str) -> Vec<String> {
    let replaced = content.replace("\r\n", "\n").replace('\r', "\n");
    let mut lines: Vec<String> = replaced.split('\n').map(String::from).collect();
    if lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    lines
}

/// Port of the `eventSectionsBlock(events:...)` overload (Swift line 112).
fn event_sections_block_events(
    events: &[Event],
    child_indent: &str,
    include_markers: bool,
    restore_line: Option<&str>,
) -> Vec<String> {
    let groups = event_groups_by_name_preserving_order(events);
    event_sections_block_groups(&groups, child_indent, include_markers, restore_line)
}

/// Port of the `eventSectionsBlock(eventGroups:...)` overload (Swift line 126).
fn event_sections_block_groups(
    event_groups: &[EventGroup],
    child_indent: &str,
    include_markers: bool,
    restore_line: Option<&str>,
) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    if include_markers {
        lines.push(format!("{child_indent}{}", begin_marker_line(restore_line)));
    }
    for event_group in event_groups {
        lines.push(format!("{child_indent}{}:", event_group.name));
        let item_indent = format!("{child_indent}  ");
        lines.extend(hook_entries(&event_group.events, &item_indent));
    }
    if include_markers {
        lines.push(format!("{child_indent}{END_MARKER}"));
    }
    lines
}

/// Port of `hookListBlock(events:itemIndent:restoreLine:)` (Swift line 146).
fn hook_list_block(events: &[Event], item_indent: &str, restore_line: Option<&str>) -> Vec<String> {
    let mut lines = vec![format!("{item_indent}{}", begin_marker_line(restore_line))];
    lines.extend(hook_entries(events, item_indent));
    lines.push(format!("{item_indent}{END_MARKER}"));
    lines
}

/// Port of `hookEntries(events:itemIndent:)` (Swift line 153).
fn hook_entries(events: &[Event], item_indent: &str) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for event in events {
        lines.push(format!(
            "{item_indent}- command: {}",
            yaml_double_quoted(&event.command)
        ));
        if let Some(matcher) = &event.matcher {
            let matcher = trim_ws_nl(matcher);
            if !matcher.is_empty() {
                lines.push(format!(
                    "{item_indent}  matcher: {}",
                    yaml_double_quoted(matcher)
                ));
            }
        }
        lines.push(format!("{item_indent}  timeout: {}", event.timeout));
    }
    lines
}

/// Port of `eventGroupsByNamePreservingOrder(_:)` (Swift line 165).
fn event_groups_by_name_preserving_order(events: &[Event]) -> Vec<EventGroup> {
    let mut event_groups: Vec<EventGroup> = Vec::new();
    let mut indexes_by_name: HashMap<String, usize> = HashMap::new();
    for event in events {
        if let Some(&index) = indexes_by_name.get(&event.name) {
            event_groups[index].events.push(event.clone());
        } else {
            indexes_by_name.insert(event.name.clone(), event_groups.len());
            event_groups.push(EventGroup {
                name: event.name.clone(),
                events: vec![event.clone()],
            });
        }
    }
    event_groups
}

/// Port of `removingMarkedBlocks(_:)` (Swift line 179), including the
/// begin+restore marker protocol.
fn removing_marked_blocks(lines: Vec<String>) -> Vec<String> {
    let mut result = lines;
    let mut index = 0usize;
    while index < result.len() {
        if !is_begin_marker_line(&result[index]) {
            index += 1;
            continue;
        }

        let Some(end_index) = result[index + 1..]
            .iter()
            .position(|line| trim_ws(line) == END_MARKER)
            .map(|pos| pos + index + 1)
        else {
            index += 1;
            continue;
        };

        // `if let restoreLine = ..., result.indices.contains(index - 1)` — BOTH
        // must hold; a restore marker at index 0 falls through to removal.
        if index >= 1 {
            if let Some(restore_line) = restore_line_from_begin_marker(&result[index]) {
                result[index - 1] = restore_line;
                result.drain(index..=end_index);
                continue;
            }
        }

        let removal_start = if index >= 1 && trim_ws_nl(&result[index - 1]).is_empty() {
            index - 1
        } else {
            index
        };
        result.drain(removal_start..=end_index);
        index = removal_start;
    }
    result
}

/// Port of `beginMarkerLine(restoreLine:)` (Swift line 209).
fn begin_marker_line(restore_line: Option<&str>) -> String {
    match restore_line {
        None => BEGIN_MARKER.to_string(),
        Some(restore_line) => {
            let encoded = STANDARD.encode(restore_line.as_bytes());
            format!("{RESTORE_LINE_MARKER_PREFIX} {encoded}")
        }
    }
}

/// Port of `isBeginMarkerLine(_:)` (Swift line 215).
fn is_begin_marker_line(line: &str) -> bool {
    let trimmed = trim_ws(line);
    trimmed == BEGIN_MARKER
        || trimmed.starts_with(&format!("{RESTORE_LINE_MARKER_PREFIX} "))
}

/// Port of `restoreLine(fromBeginMarkerLine:)` (Swift line 220).
fn restore_line_from_begin_marker(line: &str) -> Option<String> {
    let trimmed = trim_ws(line);
    let prefix_with_space = format!("{RESTORE_LINE_MARKER_PREFIX} ");
    if !trimmed.starts_with(&prefix_with_space) {
        return None;
    }
    // `dropFirst(restoreLineMarkerPrefix.count)` — the prefix WITHOUT the
    // trailing space — then `trimmingCharacters(in: .whitespaces)`.
    let encoded = trim_ws(&trimmed[RESTORE_LINE_MARKER_PREFIX.len()..]);
    let data = STANDARD.decode(encoded).ok()?;
    String::from_utf8(data).ok()
}

fn hooks_line_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^hooks:\s*((\{\}|\[\])\s*)?(#.*)?$").unwrap())
}

fn inline_empty_hooks_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^hooks:\s*(\{\}|\[\])\s*(#.*)?$").unwrap())
}

/// Port of `hooksLineIndex(in:)` (Swift line 229).
fn hooks_line_index(lines: &[String]) -> Option<usize> {
    lines
        .iter()
        .position(|line| leading_whitespace(line).is_empty() && hooks_line_regex().is_match(line))
}

/// Port of `inlineEmptyHooksLine(_:)` (Swift line 236).
fn inline_empty_hooks_line(line: &str) -> bool {
    inline_empty_hooks_regex().is_match(line)
}

/// Port of `inlineEmptyEventLine(_:)` (Swift line 240).
fn inline_empty_event_line(line: &str) -> bool {
    match line.find(':') {
        None => false,
        Some(colon) => suffix_is_inline_empty_map_or_list(&line[colon + 1..]),
    }
}

/// Port of `emptyEventHeaderLine(_:)` (Swift line 246): keep through the colon.
fn empty_event_header_line(line: &str) -> String {
    match line.find(':') {
        None => line.to_string(),
        Some(colon) => line[..=colon].to_string(),
    }
}

/// Port of `suffixIsInlineEmptyMapOrList(_:)` (Swift line 251).
fn suffix_is_inline_empty_map_or_list(suffix: &str) -> bool {
    let uncommented = suffix.split('#').next().unwrap_or("");
    let trimmed = trim_ws(uncommented);
    trimmed.is_empty() || trimmed == "{}" || trimmed == "[]"
}

/// Port of `directEventLineIndexes(in:hooksIndex:)` (Swift line 257).
fn direct_event_line_indexes(lines: &[String], hooks_index: usize) -> HashMap<String, usize> {
    let hooks_indent = leading_whitespace(&lines[hooks_index]);
    let child_indent = format!("{hooks_indent}  ");
    let mut indexes: HashMap<String, usize> = HashMap::new();

    let mut index = hooks_index + 1;
    while index < lines.len() {
        let line = &lines[index];
        let trimmed = trim_ws(line);
        if trimmed.is_empty() || trimmed.starts_with('#') {
            index += 1;
            continue;
        }
        if !line.starts_with(child_indent.as_str()) {
            break;
        }
        if leading_whitespace(line) != child_indent.as_str() {
            index += 1;
            continue;
        }
        let Some(colon) = trimmed.find(':') else {
            index += 1;
            continue;
        };
        let name = trimmed[..colon].to_string();
        let suffix = &trimmed[colon + 1..];
        if suffix_is_inline_empty_map_or_list(suffix) {
            indexes.insert(name, index);
        }
        index += 1;
    }
    indexes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(name: &str, command: &str) -> Event {
        Event::new(name, command)
    }

    #[test]
    fn install_into_empty_config_appends_full_block() {
        let out = installing(&[event("PostToolUse", "cmux-hook")], "");
        assert_eq!(
            out,
            "# cmux hooks hermes-agent begin\n\
             hooks:\n\
             \x20\x20PostToolUse:\n\
             \x20\x20\x20\x20- command: \"cmux-hook\"\n\
             \x20\x20\x20\x20\x20\x20timeout: 5\n\
             # cmux hooks hermes-agent end\n"
        );
    }

    #[test]
    fn install_is_idempotent() {
        let events = [event("PostToolUse", "cmux-hook")];
        let once = installing(&events, "");
        let twice = installing(&events, &once);
        assert_eq!(once, twice);
    }

    #[test]
    fn install_then_uninstall_empty_roundtrip() {
        let out = installing(&[event("PostToolUse", "cmux-hook")], "");
        assert_eq!(uninstalling(&out), "");
    }

    #[test]
    fn install_with_existing_hooks_anchor_inserts_after_hooks_line() {
        let existing = "version: 1\n\
             hooks:\n\
             \x20\x20PreToolUse:\n\
             \x20\x20\x20\x20- command: \"existing\"\n\
             \x20\x20\x20\x20\x20\x20timeout: 3\n";
        let out = installing(&[event("PostToolUse", "cmux-hook")], existing);
        assert_eq!(
            out,
            "version: 1\n\
             hooks:\n\
             \x20\x20# cmux hooks hermes-agent begin\n\
             \x20\x20PostToolUse:\n\
             \x20\x20\x20\x20- command: \"cmux-hook\"\n\
             \x20\x20\x20\x20\x20\x20timeout: 5\n\
             \x20\x20# cmux hooks hermes-agent end\n\
             \x20\x20PreToolUse:\n\
             \x20\x20\x20\x20- command: \"existing\"\n\
             \x20\x20\x20\x20\x20\x20timeout: 3\n"
        );
    }

    #[test]
    fn install_with_hooks_anchor_roundtrips_to_original() {
        let existing = "version: 1\n\
             hooks:\n\
             \x20\x20PreToolUse:\n\
             \x20\x20\x20\x20- command: \"existing\"\n\
             \x20\x20\x20\x20\x20\x20timeout: 3\n";
        let out = installing(&[event("PostToolUse", "cmux-hook")], existing);
        assert_eq!(uninstalling(&out), existing);
    }

    #[test]
    fn install_into_inline_empty_hooks_writes_restore_marker() {
        let out = installing(&[event("PostToolUse", "cmux-hook")], "hooks: {}\n");
        // base64("hooks: {}") == "aG9va3M6IHt9"
        assert_eq!(
            out,
            "hooks:\n\
             \x20\x20# cmux hooks hermes-agent begin restore-line-base64: aG9va3M6IHt9\n\
             \x20\x20PostToolUse:\n\
             \x20\x20\x20\x20- command: \"cmux-hook\"\n\
             \x20\x20\x20\x20\x20\x20timeout: 5\n\
             \x20\x20# cmux hooks hermes-agent end\n"
        );
    }

    #[test]
    fn inline_empty_hooks_restore_roundtrips() {
        let existing = "hooks: {}\n";
        let out = installing(&[event("PostToolUse", "cmux-hook")], existing);
        assert_eq!(uninstalling(&out), existing);
    }

    #[test]
    fn inline_empty_hooks_bracket_form_roundtrips() {
        let existing = "hooks: []\n";
        let out = installing(&[event("PostToolUse", "cmux-hook")], existing);
        assert_eq!(uninstalling(&out), existing);
    }

    #[test]
    fn matched_event_group_inserts_hook_list_under_existing_event() {
        let existing = "hooks:\n\x20\x20PostToolUse: {}\n";
        let out = installing(&[event("PostToolUse", "cmux-hook")], existing);
        assert_eq!(
            out,
            "hooks:\n\
             \x20\x20PostToolUse:\n\
             \x20\x20\x20\x20# cmux hooks hermes-agent begin restore-line-base64: \
             ICBQb3N0VG9vbFVzZToge30=\n\
             \x20\x20\x20\x20- command: \"cmux-hook\"\n\
             \x20\x20\x20\x20\x20\x20timeout: 5\n\
             \x20\x20\x20\x20# cmux hooks hermes-agent end\n"
        );
    }

    #[test]
    fn matched_event_group_roundtrips() {
        let existing = "hooks:\n\x20\x20PostToolUse: {}\n";
        let out = installing(&[event("PostToolUse", "cmux-hook")], existing);
        assert_eq!(uninstalling(&out), existing);
    }

    #[test]
    fn no_anchor_appends_blank_separator_before_block() {
        let out = installing(&[event("PostToolUse", "cmux-hook")], "version: 1\n");
        assert_eq!(
            out,
            "version: 1\n\
             \n\
             # cmux hooks hermes-agent begin\n\
             hooks:\n\
             \x20\x20PostToolUse:\n\
             \x20\x20\x20\x20- command: \"cmux-hook\"\n\
             \x20\x20\x20\x20\x20\x20timeout: 5\n\
             # cmux hooks hermes-agent end\n"
        );
    }

    #[test]
    fn no_anchor_roundtrips_to_original() {
        let existing = "version: 1\n";
        let out = installing(&[event("PostToolUse", "cmux-hook")], existing);
        assert_eq!(uninstalling(&out), existing);
    }

    #[test]
    fn empty_events_delegates_to_uninstalling() {
        let with_block = installing(&[event("PostToolUse", "cmux-hook")], "");
        // installing([]) must equal uninstalling of the same input.
        assert_eq!(installing(&[], &with_block), uninstalling(&with_block));
        assert_eq!(installing(&[], &with_block), "");
    }

    #[test]
    fn multiple_events_same_name_are_grouped_in_order() {
        let out = installing(
            &[event("PostToolUse", "a"), event("PostToolUse", "b")],
            "",
        );
        assert_eq!(
            out,
            "# cmux hooks hermes-agent begin\n\
             hooks:\n\
             \x20\x20PostToolUse:\n\
             \x20\x20\x20\x20- command: \"a\"\n\
             \x20\x20\x20\x20\x20\x20timeout: 5\n\
             \x20\x20\x20\x20- command: \"b\"\n\
             \x20\x20\x20\x20\x20\x20timeout: 5\n\
             # cmux hooks hermes-agent end\n"
        );
    }

    #[test]
    fn multiple_distinct_events_preserve_first_seen_order() {
        let out = installing(
            &[event("PostToolUse", "a"), event("PreToolUse", "b")],
            "",
        );
        assert_eq!(
            out,
            "# cmux hooks hermes-agent begin\n\
             hooks:\n\
             \x20\x20PostToolUse:\n\
             \x20\x20\x20\x20- command: \"a\"\n\
             \x20\x20\x20\x20\x20\x20timeout: 5\n\
             \x20\x20PreToolUse:\n\
             \x20\x20\x20\x20- command: \"b\"\n\
             \x20\x20\x20\x20\x20\x20timeout: 5\n\
             # cmux hooks hermes-agent end\n"
        );
    }

    #[test]
    fn matcher_present_is_emitted_trimmed_between_command_and_timeout() {
        let e = Event::new("PreToolUse", "cmd")
            .with_timeout(10)
            .with_matcher("  Bash  ");
        let out = installing(&[e], "");
        assert_eq!(
            out,
            "# cmux hooks hermes-agent begin\n\
             hooks:\n\
             \x20\x20PreToolUse:\n\
             \x20\x20\x20\x20- command: \"cmd\"\n\
             \x20\x20\x20\x20\x20\x20matcher: \"Bash\"\n\
             \x20\x20\x20\x20\x20\x20timeout: 10\n\
             # cmux hooks hermes-agent end\n"
        );
    }

    #[test]
    fn matcher_blank_or_none_is_skipped() {
        let blank = Event::new("PreToolUse", "cmd").with_matcher("   ");
        let out_blank = installing(&[blank], "");
        assert!(!out_blank.contains("matcher:"));

        let none = Event::new("PreToolUse", "cmd");
        let out_none = installing(&[none], "");
        assert!(!out_none.contains("matcher:"));
    }

    #[test]
    fn command_with_special_chars_is_yaml_escaped() {
        let e = Event::new("PostToolUse", "a\"b\\c");
        let out = installing(&[e], "");
        assert!(out.contains("- command: \"a\\\"b\\\\c\"\n"));
    }

    #[test]
    fn crlf_input_is_normalized_to_lf() {
        let out = installing(&[event("PostToolUse", "cmux-hook")], "a\r\nb\r\n");
        assert!(!out.contains('\r'));
        assert_eq!(
            out,
            "a\n\
             b\n\
             \n\
             # cmux hooks hermes-agent begin\n\
             hooks:\n\
             \x20\x20PostToolUse:\n\
             \x20\x20\x20\x20- command: \"cmux-hook\"\n\
             \x20\x20\x20\x20\x20\x20timeout: 5\n\
             # cmux hooks hermes-agent end\n"
        );
    }

    #[test]
    fn uninstall_removes_preceding_blank_line() {
        let existing = "version: 1\n";
        let installed = installing(&[event("PostToolUse", "cmux-hook")], existing);
        // The installed form has a blank separator before the begin marker;
        // uninstall must consume it so we get back exactly `version: 1\n`.
        assert_eq!(uninstalling(&installed), "version: 1\n");
    }

    #[test]
    fn uninstall_with_malformed_restore_base64_removes_block_without_restore() {
        // A begin+restore marker whose base64 payload is invalid: the block is
        // removed (falls through to the plain-removal branch), preceding blank
        // consumed, and no line is restored.
        let text = "keep\n\
             \n\
             \x20\x20# cmux hooks hermes-agent begin restore-line-base64: not*valid*base64\n\
             \x20\x20PostToolUse:\n\
             \x20\x20\x20\x20- command: \"x\"\n\
             \x20\x20# cmux hooks hermes-agent end\n\
             tail\n";
        assert_eq!(uninstalling(text), "keep\ntail\n");
    }

    #[test]
    fn uninstall_of_plain_string_without_markers_is_identity() {
        assert_eq!(uninstalling("hello\nworld\n"), "hello\nworld\n");
        assert_eq!(uninstalling(""), "");
    }

    #[test]
    fn direct_event_indexes_last_duplicate_name_wins() {
        // Two inline-empty PostToolUse lines under hooks: the later index wins,
        // matching the Swift dictionary-overwrite semantics. The matched insert
        // therefore lands under the SECOND occurrence.
        let existing = "hooks:\n\
             \x20\x20PostToolUse: {}\n\
             \x20\x20PreToolUse:\n\
             \x20\x20\x20\x20- command: \"e\"\n\
             \x20\x20\x20\x20\x20\x20timeout: 1\n\
             \x20\x20PostToolUse: []\n";
        let out = installing(&[event("PostToolUse", "new")], existing);
        // Restore of the SECOND ("[]") line means the "[]" was rewritten; the
        // first ("{}") stays intact.
        assert!(out.contains("\x20\x20PostToolUse: {}\n"));
        assert!(out.contains("- command: \"new\"\n"));
        assert_eq!(uninstalling(&out), existing);
    }
}
