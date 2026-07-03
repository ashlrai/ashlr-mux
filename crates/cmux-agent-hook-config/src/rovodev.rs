//! Faithful 1:1 port of Swift `enum RovoDevHookConfig`
//! (`Packages/macOS/CMUXAgentLaunch/Sources/CMUXAgentLaunch/RovoDevHookConfig.swift`).
//!
//! DIVERGENCES (all sanctioned platform swaps):
//! - The namespace `enum` → a Rust module; `static func` → free `fn`.
//! - Foundation `.whitespaces` / `.whitespacesAndNewlines` trims are ASCII-only
//!   (see `common` module note).
//! - `NSRegularExpression` (ICU) → the `regex` crate.
//!
//! PARITY NOTE: unlike the Hermes port, `RovoDevHookConfig.normalizedLines`
//! does NOT normalize CRLF — it splits on `\n` only, so a `\r` at the end of a
//! line is preserved verbatim. This is faithful to the Swift source and is
//! covered by a dedicated test. There is also NO empty-events guard: installing
//! an empty event list still emits an (empty) marked block.

use std::sync::OnceLock;

use regex::Regex;

use crate::common::{
    leading_whitespace, serialized, splice_insert, trim_ws, trim_ws_nl, yaml_double_quoted,
};

/// Port of `RovoDevHookConfig.Event`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    pub name: String,
    pub command: String,
}

impl Event {
    pub fn new(name: impl Into<String>, command: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            command: command.into(),
        }
    }
}

const BEGIN_MARKER: &str = "# cmux hooks rovodev begin";
const END_MARKER: &str = "# cmux hooks rovodev end";

/// Port of `RovoDevHookConfig.installing(events:in:)` (Swift line 17).
pub fn installing(events: &[Event], existing: &str) -> String {
    let mut lines = normalized_lines(existing);
    lines = removing_marked_block(lines);

    if let Some(events_index) = events_line_index(&lines) {
        let event_indent = format!("{}  ", leading_whitespace(&lines[events_index]));
        let block = event_hooks_block(events, &event_indent, true);
        splice_insert(&mut lines, events_index + 1, block);
    } else if let Some(event_hooks_index) = event_hooks_line_index(&lines) {
        let child_indent = format!("{}  ", leading_whitespace(&lines[event_hooks_index]));
        let mut block = vec![
            format!("{child_indent}{BEGIN_MARKER}"),
            format!("{child_indent}events:"),
        ];
        let inner_indent = format!("{child_indent}  ");
        block.extend(event_hooks_block(events, &inner_indent, false));
        block.push(format!("{child_indent}{END_MARKER}"));
        splice_insert(&mut lines, event_hooks_index + 1, block);
    } else {
        if !lines.is_empty() && !trim_ws_nl(lines.last().unwrap()).is_empty() {
            lines.push(String::new());
        }
        lines.push(BEGIN_MARKER.to_string());
        lines.push("eventHooks:".to_string());
        lines.push("  events:".to_string());
        lines.extend(event_hooks_block(events, "    ", false));
        lines.push(END_MARKER.to_string());
    }

    serialized(&lines)
}

/// Port of `RovoDevHookConfig.uninstalling(from:)` (Swift line 48).
pub fn uninstalling(existing: &str) -> String {
    serialized(&removing_marked_block(normalized_lines(existing)))
}

/// Port of `normalizedLines(_:)` (Swift line 52). NOTE: no CRLF normalization.
fn normalized_lines(content: &str) -> Vec<String> {
    let mut lines: Vec<String> = content.split('\n').map(String::from).collect();
    if lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    lines
}

/// Port of `eventHooksBlock(events:itemIndent:includeMarkers:)` (Swift line 64).
fn event_hooks_block(events: &[Event], item_indent: &str, include_markers: bool) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    if include_markers {
        lines.push(format!("{item_indent}{BEGIN_MARKER}"));
    }
    for event in events {
        lines.push(format!("{item_indent}- name: {}", event.name));
        lines.push(format!("{item_indent}  commands:"));
        lines.push(format!(
            "{item_indent}    - command: {}",
            yaml_double_quoted(&event.command)
        ));
    }
    if include_markers {
        lines.push(format!("{item_indent}{END_MARKER}"));
    }
    lines
}

/// Port of `removingMarkedBlock(_:)` (Swift line 84).
fn removing_marked_block(lines: Vec<String>) -> Vec<String> {
    let mut result = lines;
    let mut index = 0usize;
    while index < result.len() {
        if trim_ws(&result[index]) != BEGIN_MARKER {
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

fn event_hooks_line_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^eventHooks:\s*(#.*)?$").unwrap())
}

fn non_whitespace_start_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^\S").unwrap())
}

fn events_suffix_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^events:\s*(#.*)?$").unwrap())
}

/// Port of `eventHooksLineIndex(in:)` (Swift line 110).
fn event_hooks_line_index(lines: &[String]) -> Option<usize> {
    lines
        .iter()
        .position(|line| event_hooks_line_regex().is_match(line))
}

/// Port of `eventsLineIndex(in:)` (Swift line 116).
fn events_line_index(lines: &[String]) -> Option<usize> {
    let event_hooks_index = event_hooks_line_index(lines)?;
    let events_indent = format!("{}  ", leading_whitespace(&lines[event_hooks_index]));
    for (index, line) in lines.iter().enumerate().skip(event_hooks_index + 1) {
        if non_whitespace_start_regex().is_match(line) {
            return None;
        }
        if !line.starts_with(events_indent.as_str()) {
            continue;
        }
        let suffix = &line[events_indent.len()..];
        if events_suffix_regex().is_match(suffix) {
            return Some(index);
        }
    }
    None
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
            "# cmux hooks rovodev begin\n\
             eventHooks:\n\
             \x20\x20events:\n\
             \x20\x20\x20\x20- name: PostToolUse\n\
             \x20\x20\x20\x20\x20\x20commands:\n\
             \x20\x20\x20\x20\x20\x20\x20\x20- command: \"cmux-hook\"\n\
             # cmux hooks rovodev end\n"
        );
    }

    #[test]
    fn install_into_empty_config_roundtrips() {
        let out = installing(&[event("PostToolUse", "cmux-hook")], "");
        assert_eq!(uninstalling(&out), "");
    }

    #[test]
    fn install_with_event_hooks_anchor_inserts_events_block() {
        let out = installing(&[event("PostToolUse", "cmux-hook")], "eventHooks:\n");
        assert_eq!(
            out,
            "eventHooks:\n\
             \x20\x20# cmux hooks rovodev begin\n\
             \x20\x20events:\n\
             \x20\x20\x20\x20- name: PostToolUse\n\
             \x20\x20\x20\x20\x20\x20commands:\n\
             \x20\x20\x20\x20\x20\x20\x20\x20- command: \"cmux-hook\"\n\
             \x20\x20# cmux hooks rovodev end\n"
        );
    }

    #[test]
    fn install_with_event_hooks_anchor_roundtrips() {
        let existing = "eventHooks:\n";
        let out = installing(&[event("PostToolUse", "cmux-hook")], existing);
        assert_eq!(uninstalling(&out), existing);
    }

    #[test]
    fn install_with_events_line_inserts_after_events() {
        let existing = "eventHooks:\n\
             \x20\x20events:\n\
             \x20\x20\x20\x20- name: Existing\n\
             \x20\x20\x20\x20\x20\x20commands:\n\
             \x20\x20\x20\x20\x20\x20\x20\x20- command: \"x\"\n";
        let out = installing(&[event("PostToolUse", "cmux-hook")], existing);
        assert_eq!(
            out,
            "eventHooks:\n\
             \x20\x20events:\n\
             \x20\x20\x20\x20# cmux hooks rovodev begin\n\
             \x20\x20\x20\x20- name: PostToolUse\n\
             \x20\x20\x20\x20\x20\x20commands:\n\
             \x20\x20\x20\x20\x20\x20\x20\x20- command: \"cmux-hook\"\n\
             \x20\x20\x20\x20# cmux hooks rovodev end\n\
             \x20\x20\x20\x20- name: Existing\n\
             \x20\x20\x20\x20\x20\x20commands:\n\
             \x20\x20\x20\x20\x20\x20\x20\x20- command: \"x\"\n"
        );
    }

    #[test]
    fn install_with_events_line_roundtrips() {
        let existing = "eventHooks:\n\
             \x20\x20events:\n\
             \x20\x20\x20\x20- name: Existing\n\
             \x20\x20\x20\x20\x20\x20commands:\n\
             \x20\x20\x20\x20\x20\x20\x20\x20- command: \"x\"\n";
        let out = installing(&[event("PostToolUse", "cmux-hook")], existing);
        assert_eq!(uninstalling(&out), existing);
    }

    #[test]
    fn install_is_idempotent() {
        let events = [event("PostToolUse", "cmux-hook")];
        let once = installing(&events, "");
        let twice = installing(&events, &once);
        assert_eq!(once, twice);
    }

    #[test]
    fn no_anchor_appends_blank_separator_before_block() {
        let out = installing(&[event("PostToolUse", "cmux-hook")], "root: 1\n");
        assert_eq!(
            out,
            "root: 1\n\
             \n\
             # cmux hooks rovodev begin\n\
             eventHooks:\n\
             \x20\x20events:\n\
             \x20\x20\x20\x20- name: PostToolUse\n\
             \x20\x20\x20\x20\x20\x20commands:\n\
             \x20\x20\x20\x20\x20\x20\x20\x20- command: \"cmux-hook\"\n\
             # cmux hooks rovodev end\n"
        );
    }

    #[test]
    fn no_anchor_roundtrips_consuming_blank_separator() {
        let existing = "root: 1\n";
        let out = installing(&[event("PostToolUse", "cmux-hook")], existing);
        assert_eq!(uninstalling(&out), existing);
    }

    #[test]
    fn empty_events_still_emits_block_no_guard() {
        // Unlike Hermes, RovoDev has no empty-events guard: an empty marked
        // block is emitted.
        let out = installing(&[], "");
        assert_eq!(
            out,
            "# cmux hooks rovodev begin\n\
             eventHooks:\n\
             \x20\x20events:\n\
             # cmux hooks rovodev end\n"
        );
        assert_eq!(uninstalling(&out), "");
    }

    #[test]
    fn crlf_is_not_normalized_carriage_return_preserved() {
        // RovoDev splits on \n only; the \r stays at the end of the anchor line.
        let out = installing(&[event("PostToolUse", "cmux-hook")], "eventHooks:\r\n");
        assert_eq!(
            out,
            "eventHooks:\r\n\
             \x20\x20# cmux hooks rovodev begin\n\
             \x20\x20events:\n\
             \x20\x20\x20\x20- name: PostToolUse\n\
             \x20\x20\x20\x20\x20\x20commands:\n\
             \x20\x20\x20\x20\x20\x20\x20\x20- command: \"cmux-hook\"\n\
             \x20\x20# cmux hooks rovodev end\n"
        );
    }

    #[test]
    fn event_name_is_emitted_raw_command_is_quoted() {
        let out = installing(&[event("Odd\"Name", "a\"b\\c")], "");
        // name emitted verbatim (not YAML-quoted)...
        assert!(out.contains("- name: Odd\"Name\n"));
        // ...command YAML double-quoted with escaping.
        assert!(out.contains("- command: \"a\\\"b\\\\c\"\n"));
    }

    #[test]
    fn events_line_ignored_when_top_level_line_intervenes() {
        // A dedent to column 0 before `events:` makes eventsLineIndex return
        // None, so installation falls back to the eventHooks anchor path.
        let existing = "eventHooks:\n\
             otherTop: 1\n\
             \x20\x20events:\n";
        let out = installing(&[event("PostToolUse", "cmux-hook")], existing);
        // Inserted right after the eventHooks line (index 1), producing its own
        // begin marker + events: block.
        assert_eq!(
            out,
            "eventHooks:\n\
             \x20\x20# cmux hooks rovodev begin\n\
             \x20\x20events:\n\
             \x20\x20\x20\x20- name: PostToolUse\n\
             \x20\x20\x20\x20\x20\x20commands:\n\
             \x20\x20\x20\x20\x20\x20\x20\x20- command: \"cmux-hook\"\n\
             \x20\x20# cmux hooks rovodev end\n\
             otherTop: 1\n\
             \x20\x20events:\n"
        );
        assert_eq!(uninstalling(&out), existing);
    }

    #[test]
    fn uninstall_without_markers_is_identity() {
        assert_eq!(uninstalling("a\nb\n"), "a\nb\n");
        assert_eq!(uninstalling(""), "");
    }

    #[test]
    fn multiple_events_emitted_in_order() {
        let out = installing(&[event("A", "one"), event("B", "two")], "");
        assert_eq!(
            out,
            "# cmux hooks rovodev begin\n\
             eventHooks:\n\
             \x20\x20events:\n\
             \x20\x20\x20\x20- name: A\n\
             \x20\x20\x20\x20\x20\x20commands:\n\
             \x20\x20\x20\x20\x20\x20\x20\x20- command: \"one\"\n\
             \x20\x20\x20\x20- name: B\n\
             \x20\x20\x20\x20\x20\x20commands:\n\
             \x20\x20\x20\x20\x20\x20\x20\x20- command: \"two\"\n\
             # cmux hooks rovodev end\n"
        );
    }
}
