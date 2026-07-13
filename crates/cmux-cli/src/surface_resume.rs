//! Pure parsing for `cmux surface-resume` / `cmux surface resume`
//! (CLI/cmux.swift:6563-6812 at pinned commit `e1825d40d`).
//!
//! Produces a [`SurfaceResumePlan`]: the v2 `surface.resume.*` method, the RAW
//! target selectors (`--workspace` / `--surface` / `--window` plus the ambient
//! `CMUX_*` env defaulting rules), and the fully built non-target params. The
//! executor resolves the raw selectors into ids via live `window.list` /
//! `workspace.list` / `surface.list` calls (`normalizeWindowHandle` /
//! `normalizeWorkspaceHandle` / `normalizeSurfaceHandle` parity) and sends the
//! request.
//!
//! Pinned canonical rules:
//! - default subcommand is `show`; `get` is an alias (CLI/cmux.swift:6570).
//! - every recognized value option must be followed by a real value BEFORE any
//!   socket work (`validateSurfaceResumeValueOptions`, :6737-6759).
//! - `--shell` wins over `-- <argv...>`; argv tokens are each single-quoted via
//!   `cliShellQuote` (:6597-6615, 6810-6812).
//! - `set` ALWAYS sends `source` (default `cli`) and `cwd` (default `$PWD`,
//!   else the process working directory) (:6594-6595).
//! - explicit `--window` (or the honored GLOBAL `--window` override) suppresses
//!   BOTH ambient `CMUX_SURFACE_ID` and `CMUX_WORKSPACE_ID` (:6778-6790).
//! - `--checkpoint-id` beats `--checkpoint` for set and clear (:6585, 6649).

use crate::invocation::CliError;

/// Which output convention the executor applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceResumeAction {
    /// `set` — prints `OK` (or the id-formatted payload with `--json`).
    Set,
    /// `show` / `get` — prints the binding command or `No resume binding`.
    Show,
    /// `clear` — prints `OK` (or the id-formatted payload with `--json`).
    Clear,
}

/// The fully parsed invocation: raw target selectors + built params.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurfaceResumePlan {
    /// `surface.resume.set` / `surface.resume.get` / `surface.resume.clear`.
    pub method: &'static str,
    pub action: SurfaceResumeAction,
    /// Raw `--window` value (per-command flag, else the global override).
    pub window: Option<String>,
    /// Raw workspace selector after ambient-env defaulting.
    pub workspace: Option<String>,
    /// Raw surface selector after ambient-env defaulting.
    pub surface: Option<String>,
    /// Non-target params (name/kind/checkpoint_id/source/cwd/command for set;
    /// checkpoint_id/source for clear; empty for show/get).
    pub extra: serde_json::Map<String, serde_json::Value>,
}

const TARGET_VALUE_OPTIONS: &[&str] = &["--workspace", "--surface", "--window"];
const SET_VALUE_OPTIONS: &[&str] = &[
    "--workspace",
    "--surface",
    "--window",
    "--name",
    "--kind",
    "--checkpoint",
    "--checkpoint-id",
    "--source",
    "--cwd",
    "--shell",
];
const CLEAR_VALUE_OPTIONS: &[&str] = &[
    "--workspace",
    "--surface",
    "--window",
    "--checkpoint",
    "--checkpoint-id",
    "--source",
];

/// Parse a `surface resume` invocation. `command_args` are the tokens after
/// `surface-resume` (or after `surface resume`); `window_override` is the
/// GLOBAL `--window` value (honored here, unlike focus/close-window); `env`
/// resolves `CMUX_SURFACE_ID` / `CMUX_WORKSPACE_ID` / `PWD`; `fallback_cwd` is
/// the process working directory used when `$PWD` is unset.
pub fn parse_surface_resume_command(
    command_args: &[String],
    window_override: Option<&str>,
    env: &dyn Fn(&str) -> Option<String>,
    fallback_cwd: &str,
) -> Result<SurfaceResumePlan, CliError> {
    let subcommand = command_args
        .first()
        .map(|argument| argument.to_lowercase())
        .unwrap_or_else(|| "show".to_owned());
    let rest: &[String] = if command_args.is_empty() {
        &[]
    } else {
        &command_args[1..]
    };

    match subcommand.as_str() {
        "set" => {
            validate_value_options(rest, SET_VALUE_OPTIONS, "surface resume set")?;
            let target = surface_resume_target(rest, window_override, env);
            let (options_part, argv) = split_at_argument_terminator(&target.remaining);
            let (name, rem1) = parse_option(&options_part, "--name");
            let (kind, rem2) = parse_option(&rem1, "--kind");
            let (checkpoint, rem3) = parse_option(&rem2, "--checkpoint");
            let (checkpoint_id, rem4) = parse_option(&rem3, "--checkpoint-id");
            let (source, rem5) = parse_option(&rem4, "--source");
            let (cwd, rem6) = parse_option(&rem5, "--cwd");
            let (shell_command, rem7) = parse_option(&rem6, "--shell");

            let mut extra = serde_json::Map::new();
            if let Some(name) = name {
                extra.insert("name".into(), serde_json::json!(name));
            }
            if let Some(kind) = kind {
                extra.insert("kind".into(), serde_json::json!(kind));
            }
            if let Some(checkpoint) = checkpoint_id.or(checkpoint) {
                extra.insert("checkpoint_id".into(), serde_json::json!(checkpoint));
            }
            extra.insert(
                "source".into(),
                serde_json::json!(source.unwrap_or_else(|| "cli".to_owned())),
            );
            extra.insert(
                "cwd".into(),
                serde_json::json!(cwd
                    .or_else(|| env("PWD"))
                    .unwrap_or_else(|| fallback_cwd.to_owned())),
            );

            let command_text = if let Some(shell_command) = shell_command {
                if let Some(unexpected) = rem7
                    .iter()
                    .chain(argv.as_deref().unwrap_or_default())
                    .next()
                {
                    return Err(CliError::new(format!(
                        "surface resume set: unexpected argument '{unexpected}' after --shell. \
                         Quote the full shell command or use -- <argv...>"
                    )));
                }
                shell_command.trim().to_owned()
            } else {
                if argv.is_some() {
                    if let Some(unexpected) = rem7.first() {
                        return Err(CliError::new(format!(
                            "surface resume set: unexpected argument '{unexpected}' before --"
                        )));
                    }
                }
                let argv = argv.unwrap_or(rem7);
                if argv.is_empty() {
                    return Err(CliError::new(
                        "surface resume set requires --shell <command> or -- <argv...>",
                    ));
                }
                argv.iter()
                    .map(|token| cli_shell_quote(token))
                    .collect::<Vec<_>>()
                    .join(" ")
            };
            if command_text.is_empty() {
                return Err(CliError::new(
                    "surface resume set requires a non-empty command",
                ));
            }
            extra.insert("command".into(), serde_json::json!(command_text));

            Ok(SurfaceResumePlan {
                method: "surface.resume.set",
                action: SurfaceResumeAction::Set,
                window: target.window,
                workspace: target.workspace,
                surface: target.surface,
                extra,
            })
        }
        "show" | "get" => {
            validate_value_options(
                rest,
                TARGET_VALUE_OPTIONS,
                &format!("surface resume {subcommand}"),
            )?;
            let target = surface_resume_target(rest, window_override, env);
            Ok(SurfaceResumePlan {
                method: "surface.resume.get",
                action: SurfaceResumeAction::Show,
                window: target.window,
                workspace: target.workspace,
                surface: target.surface,
                extra: serde_json::Map::new(),
            })
        }
        "clear" => {
            validate_value_options(rest, CLEAR_VALUE_OPTIONS, "surface resume clear")?;
            let target = surface_resume_target(rest, window_override, env);
            let (checkpoint, rem1) = parse_option(&target.remaining, "--checkpoint");
            let (checkpoint_id, rem2) = parse_option(&rem1, "--checkpoint-id");
            let (source, remaining) = parse_option(&rem2, "--source");
            if let Some(unexpected) = remaining.first() {
                return Err(CliError::new(format!(
                    "surface resume clear: unexpected argument '{unexpected}'"
                )));
            }
            let mut extra = serde_json::Map::new();
            if let Some(checkpoint) = checkpoint_id.or(checkpoint) {
                extra.insert("checkpoint_id".into(), serde_json::json!(checkpoint));
            }
            if let Some(source) = source {
                extra.insert("source".into(), serde_json::json!(source));
            }
            Ok(SurfaceResumePlan {
                method: "surface.resume.clear",
                action: SurfaceResumeAction::Clear,
                window: target.window,
                workspace: target.workspace,
                surface: target.surface,
                extra,
            })
        }
        _ => Err(CliError::new(format!(
            "Unsupported surface resume subcommand: {subcommand}"
        ))),
    }
}

/// The raw target selectors plus the tokens left for per-subcommand parsing.
struct SurfaceResumeTarget {
    window: Option<String>,
    workspace: Option<String>,
    surface: Option<String>,
    /// Leftover options followed by `--` + argv when a terminator was present.
    remaining: Vec<String>,
}

/// `surfaceResumeTarget` (CLI/cmux.swift:6773-6808) minus the socket
/// normalization: computes the RAW selectors after ambient-env defaulting.
fn surface_resume_target(
    args: &[String],
    window_override: Option<&str>,
    env: &dyn Fn(&str) -> Option<String>,
) -> SurfaceResumeTarget {
    let (options_part, argv) = split_at_argument_terminator(args);
    let (workspace_opt, rem1) = parse_option(&options_part, "--workspace");
    let (surface_opt, rem2) = parse_option(&rem1, "--surface");
    let (window_opt, remaining) = parse_option(&rem2, "--window");
    let window_raw = window_opt.or_else(|| window_override.map(str::to_owned));

    let uses_implicit_surface = surface_opt.is_none()
        && window_raw.is_none()
        && env("CMUX_SURFACE_ID").is_some_and(|value| !value.trim().is_empty());
    let should_use_env_workspace =
        surface_opt.is_none() && !uses_implicit_surface && window_raw.is_none();
    let workspace_raw = workspace_opt.clone().or_else(|| {
        if should_use_env_workspace {
            env("CMUX_WORKSPACE_ID")
        } else {
            None
        }
    });
    let surface_raw = surface_opt.or_else(|| {
        if workspace_opt.is_none() && window_raw.is_none() {
            env("CMUX_SURFACE_ID")
        } else {
            None
        }
    });

    let mut remaining_with_argv = remaining;
    if let Some(argv) = argv {
        remaining_with_argv.push("--".to_owned());
        remaining_with_argv.extend(argv);
    }
    SurfaceResumeTarget {
        window: window_raw,
        workspace: workspace_raw,
        surface: surface_raw,
        remaining: remaining_with_argv,
    }
}

/// `validateSurfaceResumeValueOptions` (CLI/cmux.swift:6737-6759): every
/// recognized option token before the `--` terminator must be followed by a
/// value that exists, is not `--`, is not another option name, and is not
/// blank.
fn validate_value_options(
    args: &[String],
    option_names: &[&str],
    context: &str,
) -> Result<(), CliError> {
    let mut past_terminator = false;
    for (index, arg) in args.iter().enumerate() {
        if past_terminator {
            continue;
        }
        if arg == "--" {
            past_terminator = true;
            continue;
        }
        if !option_names.contains(&arg.as_str()) {
            continue;
        }
        let value = args
            .get(index + 1)
            .ok_or_else(|| CliError::new(format!("{context}: {arg} requires a value")))?;
        if value == "--" || option_names.contains(&value.as_str()) || value.trim().is_empty() {
            return Err(CliError::new(format!("{context}: {arg} requires a value")));
        }
    }
    Ok(())
}

/// Swift `splitAtArgumentTerminator` (CLI/cmux.swift:6764-6771): split at the
/// FIRST `--`; the terminator itself is dropped from both halves.
fn split_at_argument_terminator(args: &[String]) -> (Vec<String>, Option<Vec<String>>) {
    match args.iter().position(|argument| argument == "--") {
        Some(index) => (args[..index].to_vec(), Some(args[index + 1..].to_vec())),
        None => (args.to_vec(), None),
    }
}

/// Swift `parseOption` (CLI/cmux.swift:17073-17100): extract `name value` /
/// `name=value` occurrences before the `--` terminator (last one wins), keep
/// every other token — including the terminator and everything after it — in
/// order.
fn parse_option(args: &[String], name: &str) -> (Option<String>, Vec<String>) {
    let inline_prefix = format!("{name}=");
    let mut remaining = Vec::new();
    let mut value = None;
    let mut skip_next = false;
    let mut past_terminator = false;
    for (index, arg) in args.iter().enumerate() {
        if skip_next {
            skip_next = false;
            continue;
        }
        if arg == "--" {
            past_terminator = true;
            remaining.push(arg.clone());
            continue;
        }
        if !past_terminator {
            if let Some(inline) = arg.strip_prefix(&inline_prefix) {
                value = Some(inline.to_owned());
                continue;
            }
            if arg == name && index + 1 < args.len() {
                value = Some(args[index + 1].clone());
                skip_next = true;
                continue;
            }
        }
        remaining.push(arg.clone());
    }
    (value, remaining)
}

/// Swift `cliShellQuote` (CLI/cmux.swift:6810-6812): unconditionally wrap in
/// single quotes, escaping embedded quotes as `'\''`.
pub fn cli_shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SURFACE: &str = "22222222-2222-4222-8222-222222222222";
    const WORKSPACE: &str = "11111111-1111-4111-8111-111111111111";
    const WINDOW: &str = "44444444-4444-4444-8444-444444444444";

    fn args(tokens: &[&str]) -> Vec<String> {
        tokens.iter().map(|token| token.to_string()).collect()
    }

    fn no_env(_name: &str) -> Option<String> {
        None
    }

    fn parse(
        tokens: &[&str],
        window_override: Option<&str>,
        env: &dyn Fn(&str) -> Option<String>,
    ) -> Result<SurfaceResumePlan, CliError> {
        parse_surface_resume_command(&args(tokens), window_override, env, "C:\\fallback")
    }

    #[test]
    fn default_subcommand_is_show_and_get_is_an_alias() {
        let bare = parse(&[], None, &no_env).unwrap();
        assert_eq!(bare.method, "surface.resume.get");
        assert_eq!(bare.action, SurfaceResumeAction::Show);
        assert!(bare.extra.is_empty());

        let get = parse(&["get"], None, &no_env).unwrap();
        assert_eq!(get.method, "surface.resume.get");
        // Uppercase subcommands are lowercased before matching.
        let upper = parse(&["SHOW"], None, &no_env).unwrap();
        assert_eq!(upper.method, "surface.resume.get");
    }

    #[test]
    fn unsupported_subcommand_reports_the_lowercased_name() {
        let error = parse(&["Frobnicate"], None, &no_env).unwrap_err();
        assert_eq!(
            error.message,
            "Unsupported surface resume subcommand: frobnicate"
        );
    }

    #[test]
    fn set_builds_quoted_argv_command_with_defaults() {
        let plan = parse(
            &[
                "set",
                "--surface",
                SURFACE,
                "--kind",
                "opencode",
                "--",
                "opencode",
                "--session",
                "it's",
            ],
            None,
            &no_env,
        )
        .unwrap();
        assert_eq!(plan.method, "surface.resume.set");
        assert_eq!(plan.surface.as_deref(), Some(SURFACE));
        assert_eq!(plan.extra["kind"], "opencode");
        assert_eq!(plan.extra["source"], "cli");
        assert_eq!(plan.extra["cwd"], "C:\\fallback");
        assert_eq!(plan.extra["command"], "'opencode' '--session' 'it'\\''s'");
    }

    #[test]
    fn set_prefers_pwd_env_and_explicit_cwd_and_checkpoint_id() {
        let env = |name: &str| (name == "PWD").then(|| "C:\\pwd".to_owned());
        let plan = parse(
            &[
                "set",
                "--checkpoint",
                "a",
                "--checkpoint-id",
                "b",
                "--",
                "x",
            ],
            None,
            &env,
        )
        .unwrap();
        assert_eq!(plan.extra["cwd"], "C:\\pwd");
        assert_eq!(plan.extra["checkpoint_id"], "b");

        let plan = parse(&["set", "--cwd", "D:\\explicit", "--", "x"], None, &env).unwrap();
        assert_eq!(plan.extra["cwd"], "D:\\explicit");
    }

    #[test]
    fn set_shell_wins_and_rejects_stray_tokens() {
        let plan = parse(&["set", "--shell", "  tmux attach  "], None, &no_env).unwrap();
        assert_eq!(plan.extra["command"], "tmux attach");

        let error = parse(&["set", "--shell", "tmux", "stray"], None, &no_env).unwrap_err();
        assert_eq!(
            error.message,
            "surface resume set: unexpected argument 'stray' after --shell. Quote the full shell command or use -- <argv...>"
        );
        let error = parse(&["set", "--shell", "tmux", "--", "x"], None, &no_env).unwrap_err();
        assert_eq!(
            error.message,
            "surface resume set: unexpected argument 'x' after --shell. Quote the full shell command or use -- <argv...>"
        );
        let error = parse(&["set", "stray", "--", "x"], None, &no_env).unwrap_err();
        assert_eq!(
            error.message,
            "surface resume set: unexpected argument 'stray' before --"
        );
        let error = parse(&["set"], None, &no_env).unwrap_err();
        assert_eq!(
            error.message,
            "surface resume set requires --shell <command> or -- <argv...>"
        );
        // Space-separated blank --shell is a value-option validation error;
        // the inline form reaches the non-empty-command check.
        let error = parse(&["set", "--shell", "   "], None, &no_env).unwrap_err();
        assert_eq!(
            error.message,
            "surface resume set: --shell requires a value"
        );
        let error = parse(&["set", "--shell=   "], None, &no_env).unwrap_err();
        assert_eq!(
            error.message,
            "surface resume set requires a non-empty command"
        );
    }

    #[test]
    fn bare_tokens_without_terminator_form_the_argv() {
        // Without `--`, leftover tokens ARE the argv (CLI/cmux.swift:6608).
        let plan = parse(&["set", "opencode", "run"], None, &no_env).unwrap();
        assert_eq!(plan.extra["command"], "'opencode' 'run'");
    }

    #[test]
    fn value_option_validation_is_exact() {
        for (tokens, expected) in [
            (
                vec!["set", "--surface"],
                "surface resume set: --surface requires a value",
            ),
            (
                vec!["set", "--name", "--kind", "agent", "--", "x"],
                "surface resume set: --name requires a value",
            ),
            (
                vec!["show", "--workspace", "  "],
                "surface resume show: --workspace requires a value",
            ),
            (
                vec!["get", "--window", "--"],
                "surface resume get: --window requires a value",
            ),
            (
                vec!["clear", "--source"],
                "surface resume clear: --source requires a value",
            ),
        ] {
            let error = parse(&tokens, None, &no_env).unwrap_err();
            assert_eq!(error.message, expected, "{tokens:?}");
        }
    }

    #[test]
    fn clear_rejects_unexpected_arguments_and_keeps_guards() {
        let plan = parse(
            &[
                "clear",
                "--checkpoint",
                "old",
                "--checkpoint-id",
                "new",
                "--source",
                "cli",
            ],
            None,
            &no_env,
        )
        .unwrap();
        assert_eq!(plan.method, "surface.resume.clear");
        assert_eq!(plan.extra["checkpoint_id"], "new");
        assert_eq!(plan.extra["source"], "cli");

        let error = parse(&["clear", "stray"], None, &no_env).unwrap_err();
        assert_eq!(
            error.message,
            "surface resume clear: unexpected argument 'stray'"
        );
        // A `--` terminator survives into clear's leftover scan.
        let error = parse(&["clear", "--", "x"], None, &no_env).unwrap_err();
        assert_eq!(
            error.message,
            "surface resume clear: unexpected argument '--'"
        );
    }

    #[test]
    fn ambient_env_rules_match_canonical_precedence() {
        let both = |name: &str| match name {
            "CMUX_SURFACE_ID" => Some(SURFACE.to_owned()),
            "CMUX_WORKSPACE_ID" => Some(WORKSPACE.to_owned()),
            _ => None,
        };
        // Surface env wins; workspace env is NOT used alongside it.
        let plan = parse(&["show"], None, &both).unwrap();
        assert_eq!(plan.surface.as_deref(), Some(SURFACE));
        assert_eq!(plan.workspace, None);

        // Workspace env applies only when the surface env is absent.
        let workspace_only = |name: &str| match name {
            "CMUX_WORKSPACE_ID" => Some(WORKSPACE.to_owned()),
            _ => None,
        };
        let plan = parse(&["show"], None, &workspace_only).unwrap();
        assert_eq!(plan.surface, None);
        assert_eq!(plan.workspace.as_deref(), Some(WORKSPACE));

        // Explicit --workspace suppresses the ambient surface.
        let plan = parse(&["show", "--workspace", WORKSPACE], None, &both).unwrap();
        assert_eq!(plan.surface, None);
        assert_eq!(plan.workspace.as_deref(), Some(WORKSPACE));

        // Explicit --window (per-command) suppresses both ambients.
        let plan = parse(&["show", "--window", WINDOW], None, &both).unwrap();
        assert_eq!(plan.window.as_deref(), Some(WINDOW));
        assert_eq!(plan.surface, None);
        assert_eq!(plan.workspace, None);

        // The GLOBAL --window override behaves like an explicit --window.
        let plan = parse(&["show"], Some(WINDOW), &both).unwrap();
        assert_eq!(plan.window.as_deref(), Some(WINDOW));
        assert_eq!(plan.surface, None);
        assert_eq!(plan.workspace, None);

        // A blank surface env does not count as an implicit surface, but is
        // still COPIED as the raw surface value (canonical surfaceRaw uses the
        // untrimmed env); the executor's normalization drops blanks.
        let blank_surface = |name: &str| match name {
            "CMUX_SURFACE_ID" => Some("  ".to_owned()),
            "CMUX_WORKSPACE_ID" => Some(WORKSPACE.to_owned()),
            _ => None,
        };
        let plan = parse(&["show"], None, &blank_surface).unwrap();
        assert_eq!(plan.surface.as_deref(), Some("  "));
        assert_eq!(plan.workspace.as_deref(), Some(WORKSPACE));
    }

    #[test]
    fn per_command_window_beats_the_global_override() {
        let plan = parse(&["show", "--window", WINDOW], Some("window:9"), &no_env).unwrap();
        assert_eq!(plan.window.as_deref(), Some(WINDOW));
    }

    #[test]
    fn set_options_may_appear_after_target_options_and_inline_form_works() {
        let plan = parse(
            &[
                "set",
                "--name=build",
                "--surface",
                SURFACE,
                "--shell",
                "make",
            ],
            None,
            &no_env,
        )
        .unwrap();
        assert_eq!(plan.extra["name"], "build");
        assert_eq!(plan.surface.as_deref(), Some(SURFACE));
        assert_eq!(plan.extra["command"], "make");
    }

    #[test]
    fn cli_shell_quote_wraps_every_token() {
        assert_eq!(cli_shell_quote("safe"), "'safe'");
        assert_eq!(cli_shell_quote("it's"), "'it'\\''s'");
        assert_eq!(cli_shell_quote(""), "''");
    }
}
