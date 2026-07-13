//! Pure parsing/classification for the v1 window-lifecycle CLI commands
//! (`new-window`, `focus-window`, `close-window`).
//!
//! Canonical: `CLI/cmux.swift` at `e1825d40d`:
//! - dispatch cases 4294-4310 (`new_window` takes no options; focus/close read
//!   the PER-COMMAND `--window` via `optionValue(commandArgs)` — the global
//!   `--window` override is deliberately NOT consulted),
//! - `normalizeWindowHandle` 6072-6101 (UUID passthrough; `kind:N` ref and bare
//!   integer index resolve CLIENT-side against a live v2 `window.list`; other
//!   text fails with the exact invalid-handle message),
//! - `optionValue` 17127-17138 (stops at `--`, supports `--window=value`),
//! - `isHandleRef` 6064-6070, `handlesMatch` 6300-6306, `intFromAny` 6016-6021.
//!
//! The classification here is pure; the `window.list` round-trips happen in the
//! executor (`main.rs`), which resolves a [`WindowHandle`] into the UUID/ref
//! line fragment for the v1 `focus_window <id>` / `close_window <id>` frame.

use crate::invocation::CliError;

/// A parsed window-lifecycle command, ready for the executor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WindowLifecycleCommand {
    /// `cmux new-window` — v1 `new_window`, prints the raw reply (`OK <uuid>`);
    /// `--json` and trailing arguments are ignored (CLI/cmux.swift:4294-4296).
    NewWindow,
    /// `cmux focus-window --window <id|ref|index>` — v1 `focus_window <id>`.
    FocusWindow(WindowHandle),
    /// `cmux close-window --window <id|ref|index>` — v1 `close_window <id>`.
    CloseWindow(WindowHandle),
}

impl WindowLifecycleCommand {
    /// The v1 socket command name this maps to.
    pub fn v1_command(&self) -> &'static str {
        match self {
            Self::NewWindow => "new_window",
            Self::FocusWindow(_) => "focus_window",
            Self::CloseWindow(_) => "close_window",
        }
    }
}

/// The `--window` selector, pre-classified so the executor knows whether a
/// `window.list` resolution round-trip is required (`normalizeWindowHandle`,
/// CLI/cmux.swift:6072-6101).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WindowHandle {
    /// A canonical UUID string: passed through verbatim, no socket resolution.
    Uuid(String),
    /// A `kind:N` handle ref: matched against `window.list` ids/refs.
    Ref(String),
    /// A bare integer: matched against `window.list` row indexes.
    Index(i64),
}

/// Parse a window-lifecycle command. Returns `None` when `command` is not one
/// of the three lifecycle spellings (so the caller falls through to the next
/// mapping layer).
pub fn window_lifecycle_command_for(
    command: &str,
    args: &[String],
) -> Option<Result<WindowLifecycleCommand, CliError>> {
    match command {
        "new-window" => Some(Ok(WindowLifecycleCommand::NewWindow)),
        "focus-window" => {
            Some(required_window_handle(command, args).map(WindowLifecycleCommand::FocusWindow))
        }
        "close-window" => {
            Some(required_window_handle(command, args).map(WindowLifecycleCommand::CloseWindow))
        }
        _ => None,
    }
}

/// The per-command `--window` value, classified. Missing or blank →
/// `"<command> requires --window"` (Swift's `guard let target = optionValue(…),
/// let windowID = try normalizeWindowHandle(target, …)` — a blank value makes
/// `normalizeWindowHandle` return nil, failing the same guard,
/// CLI/cmux.swift:4298-4310).
fn required_window_handle(command: &str, args: &[String]) -> Result<WindowHandle, CliError> {
    let target = option_value(args, "--window")
        .ok_or_else(|| CliError::new(format!("{command} requires --window")))?;
    classify_window_handle(&target)?
        .ok_or_else(|| CliError::new(format!("{command} requires --window")))
}

/// Swift `optionValue` (CLI/cmux.swift:17127-17138): scan stops at the first
/// `--`; `name value` and `name=value` are both accepted; a trailing `name`
/// with no following token yields `None`.
pub fn option_value(args: &[String], name: &str) -> Option<String> {
    let inline_prefix = format!("{name}=");
    for (index, arg) in args.iter().enumerate() {
        if arg == "--" {
            return None;
        }
        if arg == name {
            if index + 1 < args.len() {
                return Some(args[index + 1].clone());
            }
            continue;
        }
        if let Some(value) = arg.strip_prefix(&inline_prefix) {
            return Some(value.to_owned());
        }
    }
    None
}

/// Classify a raw window selector per `normalizeWindowHandle`
/// (CLI/cmux.swift:6084-6094): trim → blank is `None`; UUID passthrough;
/// `kind:N` handle ref; bare integer index; anything else is the exact
/// invalid-handle error.
pub fn classify_window_handle(raw: &str) -> Result<Option<WindowHandle>, CliError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    if is_uuid_handle(trimmed) {
        return Ok(Some(WindowHandle::Uuid(trimmed.to_owned())));
    }
    if is_handle_ref(trimmed) {
        return Ok(Some(WindowHandle::Ref(trimmed.to_owned())));
    }
    if let Ok(index) = trimmed.parse::<i64>() {
        return Ok(Some(WindowHandle::Index(index)));
    }
    Err(CliError::new(format!(
        "Invalid window handle: {trimmed} (expected UUID, ref like window:1, or index)"
    )))
}

/// Foundation `UUID(uuidString:)` parity: the canonical hyphenated
/// 8-4-4-4-12 form only (case-insensitive). `uuid::Uuid::parse_str` also
/// accepts simple/braced/urn forms, so gate on the 36-char hyphenated length.
pub fn is_uuid_handle(value: &str) -> bool {
    value.len() == 36 && uuid::Uuid::parse_str(value).is_ok()
}

/// Swift `isHandleRef` (CLI/cmux.swift:6064-6070): exactly two `:`-separated
/// pieces, kind ∈ {window, workspace, pane, surface} (case-insensitive), and an
/// integer second piece.
pub fn is_handle_ref(value: &str) -> bool {
    let mut pieces = value.splitn(3, ':');
    let (Some(kind), Some(index), None) = (pieces.next(), pieces.next(), pieces.next()) else {
        return false;
    };
    let kind = kind.to_lowercase();
    matches!(kind.as_str(), "window" | "workspace" | "pane" | "surface")
        && index.parse::<i64>().is_ok()
}

/// Swift `handlesMatch` (CLI/cmux.swift:6300-6306): when both sides parse as
/// UUIDs compare UUID equality (case-insensitive); otherwise lowercase string
/// equality.
pub fn handles_match(lhs: &str, rhs: &str) -> bool {
    if let (Ok(lhs_uuid), Ok(rhs_uuid)) = (uuid::Uuid::parse_str(lhs), uuid::Uuid::parse_str(rhs)) {
        return lhs_uuid == rhs_uuid;
    }
    lhs.to_lowercase() == rhs.to_lowercase()
}

/// Swift `intFromAny` (CLI/cmux.swift:6016-6021): an integer, a number, or a
/// numeric string.
pub fn int_from_any(value: Option<&serde_json::Value>) -> Option<i64> {
    match value? {
        serde_json::Value::Number(number) => number.as_i64(),
        serde_json::Value::String(text) => text.parse::<i64>().ok(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(tokens: &[&str]) -> Vec<String> {
        tokens.iter().map(|token| token.to_string()).collect()
    }

    const UUID: &str = "44444444-4444-4444-8444-444444444444";

    #[test]
    fn new_window_maps_regardless_of_arguments() {
        assert_eq!(
            window_lifecycle_command_for("new-window", &args(&["--json", "extra"])),
            Some(Ok(WindowLifecycleCommand::NewWindow))
        );
    }

    #[test]
    fn focus_and_close_require_the_per_command_window_flag() {
        for command in ["focus-window", "close-window"] {
            let missing = window_lifecycle_command_for(command, &[]).unwrap();
            assert_eq!(
                missing.unwrap_err().message,
                format!("{command} requires --window")
            );
            let blank = window_lifecycle_command_for(command, &args(&["--window", "  "])).unwrap();
            assert_eq!(
                blank.unwrap_err().message,
                format!("{command} requires --window")
            );
            // optionValue stops at `--`.
            let terminated =
                window_lifecycle_command_for(command, &args(&["--", "--window", UUID])).unwrap();
            assert_eq!(
                terminated.unwrap_err().message,
                format!("{command} requires --window")
            );
        }
    }

    #[test]
    fn window_flag_supports_equals_form_and_classifies_targets() {
        let uuid =
            window_lifecycle_command_for("focus-window", &args(&[&format!("--window={UUID}")]))
                .unwrap()
                .unwrap();
        assert_eq!(
            uuid,
            WindowLifecycleCommand::FocusWindow(WindowHandle::Uuid(UUID.to_owned()))
        );

        let reference =
            window_lifecycle_command_for("close-window", &args(&["--window", "window:2"]))
                .unwrap()
                .unwrap();
        assert_eq!(
            reference,
            WindowLifecycleCommand::CloseWindow(WindowHandle::Ref("window:2".to_owned()))
        );

        let index = window_lifecycle_command_for("focus-window", &args(&["--window", "-1"]))
            .unwrap()
            .unwrap();
        assert_eq!(
            index,
            WindowLifecycleCommand::FocusWindow(WindowHandle::Index(-1))
        );
    }

    #[test]
    fn invalid_window_handles_fail_with_the_exact_message() {
        let error = window_lifecycle_command_for("focus-window", &args(&["--window", "bogus"]))
            .unwrap()
            .unwrap_err();
        assert_eq!(
            error.message,
            "Invalid window handle: bogus (expected UUID, ref like window:1, or index)"
        );
    }

    #[test]
    fn handle_refs_accept_all_four_kinds_and_reject_others() {
        for good in ["window:1", "Workspace:0", "pane:12", "surface:-1"] {
            assert!(is_handle_ref(good), "{good}");
        }
        for bad in ["window:", "window:1:2", "tab:1", "window:x", "window"] {
            assert!(!is_handle_ref(bad), "{bad}");
        }
    }

    #[test]
    fn uuid_handles_require_the_hyphenated_form() {
        assert!(is_uuid_handle(UUID));
        assert!(is_uuid_handle(&UUID.to_uppercase()));
        assert!(!is_uuid_handle(&UUID.replace('-', "")));
        assert!(!is_uuid_handle("not-a-uuid"));
    }

    #[test]
    fn handles_match_is_uuid_aware_and_case_insensitive() {
        assert!(handles_match(UUID, &UUID.to_uppercase()));
        assert!(handles_match("window:1", "WINDOW:1"));
        assert!(!handles_match("window:1", "window:2"));
    }

    #[test]
    fn int_from_any_accepts_numbers_and_numeric_strings() {
        assert_eq!(int_from_any(Some(&serde_json::json!(3))), Some(3));
        assert_eq!(int_from_any(Some(&serde_json::json!("4"))), Some(4));
        assert_eq!(int_from_any(Some(&serde_json::json!(true))), None);
        assert_eq!(int_from_any(None), None);
    }

    #[test]
    fn unrelated_commands_fall_through() {
        assert_eq!(window_lifecycle_command_for("window", &[]), None);
    }
}
