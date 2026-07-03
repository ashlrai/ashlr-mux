//! Port of `AgentLaunchPromptBoundaryOptions.swift`.
//!
//! Handles Claude Teams' `--tmux` prompt boundary: once the scanner reaches a
//! real `--tmux <prompt>` payload, option scanning stops, except that a small
//! allow-list of safe options (`--model` / `--fallback-model` / a limited
//! `--permission-mode`) can be *recovered* from after the prompt. A `--tmux
//! classic` launch-mode token is skipped without ending the scan.

use crate::policies::Policy;
use crate::{contains_whitespace_or_newline, option_before_equals};

/// `promptBoundaryOption(_:options:)` — Swift returns the matched option string;
/// every caller only needs the "is this a boundary option" bit, so this returns
/// `bool`. Matches both the bare token and the `option=value` form.
fn prompt_boundary_option(arg: &str, options: &std::collections::HashSet<&'static str>) -> bool {
    if options.contains(arg) {
        return true;
    }
    match option_before_equals(arg) {
        None => false,
        Some(option) => options.contains(option),
    }
}

/// `isOptionToken(_:)` — a leading `-` with no embedded whitespace/newline.
pub(crate) fn is_option_token(arg: &str) -> bool {
    arg.starts_with('-') && !contains_whitespace_or_newline(arg)
}

const KNOWN_TMUX_MODE_VALUES: [&str; 1] = ["classic"];
const SAFE_POST_BOUNDARY_PERMISSION_MODES: [&str; 1] = ["auto"];

fn post_boundary_recovery_start(args: &[String], index: usize) -> Option<usize> {
    let arg = args[index].as_str();
    if arg.starts_with("--tmux=") {
        return None;
    }
    if arg != "--tmux" || index + 1 >= args.len() {
        return None;
    }
    let value = args[index + 1].as_str();
    if !value.starts_with('-') && contains_whitespace_or_newline(value) {
        return Some(index + 2);
    }
    None
}

fn prompt_boundary_launch_mode_end(args: &[String], index: usize) -> Option<usize> {
    let arg = args[index].as_str();
    if let Some(value) = arg.strip_prefix("--tmux=") {
        return if KNOWN_TMUX_MODE_VALUES.contains(&value) {
            Some(index + 1)
        } else {
            None
        };
    }
    if arg != "--tmux" || index + 1 >= args.len() {
        return None;
    }
    if KNOWN_TMUX_MODE_VALUES.contains(&args[index + 1].as_str()) {
        Some(index + 2)
    } else {
        None
    }
}

fn recovered_post_boundary_option_end(args: &[String], index: usize) -> Option<usize> {
    if index >= args.len() {
        return None;
    }
    let arg = args[index].as_str();
    match arg {
        "--model" | "--fallback-model" => {
            if index + 1 < args.len() && !is_option_token(args[index + 1].as_str()) {
                Some(index + 2)
            } else {
                None
            }
        }
        _ if arg.starts_with("--model=") || arg.starts_with("--fallback-model=") => Some(index + 1),
        "--permission-mode" => {
            if index + 1 < args.len()
                && SAFE_POST_BOUNDARY_PERMISSION_MODES.contains(&args[index + 1].as_str())
            {
                Some(index + 2)
            } else {
                None
            }
        }
        _ if arg.starts_with("--permission-mode=") => {
            let value = &arg["--permission-mode=".len()..];
            if SAFE_POST_BOUNDARY_PERMISSION_MODES.contains(&value) {
                Some(index + 1)
            } else {
                None
            }
        }
        _ => None,
    }
}

/// `consumePromptBoundaryOption(...)`.
///
/// Returns `Some(false)` when `arg` is not a prompt-boundary option (caller
/// keeps normal handling), `Some(true)` when the boundary was consumed (caller
/// re-loops from the updated `index`). The Swift signature is `Bool?`; it never
/// actually returns `nil`, but the `Option` is preserved so the caller's
/// `guard let … else { return nil }` maps exactly.
pub(crate) fn consume_prompt_boundary_option(
    arg: &str,
    args: &[String],
    index: &mut usize,
    policy: &Policy,
    result: &mut Vec<String>,
) -> Option<bool> {
    if !prompt_boundary_option(arg, &policy.prompt_boundary_options) {
        return Some(false);
    }
    if let Some(mode_end) = prompt_boundary_launch_mode_end(args, *index) {
        *index = mode_end;
        return Some(true);
    }
    let Some(recovery_start) = post_boundary_recovery_start(args, *index) else {
        *index = args.len();
        return Some(true);
    };
    let mut scan = recovery_start;
    let mut recovered: Vec<String> = Vec::new();
    while scan < args.len() {
        let Some(end) = recovered_post_boundary_option_end(args, scan) else {
            break;
        };
        recovered.extend_from_slice(&args[scan..end]);
        scan = end;
    }
    result.extend(recovered);
    *index = args.len();
    Some(true)
}
