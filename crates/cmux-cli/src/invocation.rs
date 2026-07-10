//! CLI invocation parsing: the global-option loop and presentation-option
//! parsing (M4 WS5).
//!
//! A verbatim port of the argv parsing in `CLI/cmux.swift` `run()`
//! (`CmuxCLI.run`, the global loop at ~3076-3142) and `parsePresentationOptions`
//! (~2929-2999). Pure and cross-platform, so it is exhaustively unit-testable on
//! any OS; the socket connect + command dispatch sit above it.

/// A CLI failure carrying the message to print and the process exit code.
/// Mirrors Swift `CLIError` (default `exitCode` 1). `Display` yields the bare
/// message; the top-level catch prepends `Error: ` and exits with `exit_code`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliError {
    pub message: String,
    pub exit_code: i32,
}

impl CliError {
    /// A failure with the default exit code 1.
    pub fn new(message: impl Into<String>) -> Self {
        Self::with_exit_code(message, 1)
    }

    /// A failure with an explicit exit code (e.g. 2 for "missing command").
    pub fn with_exit_code(message: impl Into<String>, exit_code: i32) -> Self {
        Self {
            message: message.into(),
            exit_code,
        }
    }
}

impl std::fmt::Display for CliError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for CliError {}

/// Global options parsed from the tokens before the command. Presentation
/// options (`json_output` / `id_format`) may be augmented by options appearing
/// after the command (see [`parse_global_options`]).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct GlobalOptions {
    /// `--socket <path>`.
    pub explicit_socket_path: Option<String>,
    /// `--json` (set by either the global loop or a post-command occurrence).
    pub json_output: bool,
    /// `--id-format <refs|uuids|both>`.
    pub id_format: Option<String>,
    /// `--window <id|ref|index>`.
    pub window_id: Option<String>,
    /// `--password <value>`.
    pub socket_password: Option<String>,
}

impl GlobalOptions {
    /// Fold post-command presentation options into these global options and
    /// return the leftover command arguments. `json_output` is monotonic-OR (a
    /// post-command `--json` only ever sets it true); `id_format` is overwritten
    /// when the post-command value is present. Single-sources the reconciliation
    /// rule that both parse sites depend on.
    fn merge_presentation(&mut self, presentation: PresentationOptions) -> Vec<String> {
        self.json_output |= presentation.json_output;
        if presentation.id_format.is_some() {
            self.id_format = presentation.id_format;
        }
        presentation.remaining
    }
}

/// What [`parse_global_options`] determined the invocation should do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseOutcome {
    /// `-v` / `--version`: print the version summary and exit 0.
    PrintVersion,
    /// `-h` / `--help`: print top-level usage and exit 0.
    PrintHelp,
    /// A named command with its resolved options and remaining arguments.
    Command {
        options: GlobalOptions,
        command: String,
        command_args: Vec<String>,
    },
}

/// Per-command options that take a following value. When such an option appears
/// in command arguments, its value is kept paired with it so the value is never
/// re-interpreted as a presentation flag. Verbatim port of Swift
/// `CmuxCLI.commandOptionsWithValues` (cmux.swift:2929).
const COMMAND_OPTIONS_WITH_VALUES: &[&str] = &[
    "-i",
    "-p",
    "--action",
    "--after-workspace",
    "--agent",
    "--amount",
    "--arch",
    "--attr",
    "--attribute",
    "--before-workspace",
    "--body",
    "--color",
    "--command",
    "--config",
    "--css",
    "--cwd",
    "--description",
    "--direction",
    "--domain",
    "--dx",
    "--dy",
    "--email",
    "--event",
    "--expression",
    "--expires",
    "--focus",
    "--function",
    "--id",
    "--image",
    "--index",
    "--key",
    "--kind",
    "--layout",
    "--loadState",
    "--lines",
    "--load-state",
    "--max-depth",
    "--name",
    "--os",
    "--order",
    "--out",
    "--pane",
    "--panel",
    "--path",
    "--profile",
    "--property",
    "--provider",
    "--relay-port",
    "--script",
    "--selector",
    "--session",
    "--shell",
    "--ssh-option",
    "--source",
    "--subtitle",
    "--surface",
    "--tab",
    "--target-pane",
    "--text",
    "--text-contains",
    "--timeout",
    "--timeout-ms",
    "--title",
    "--transcript",
    "--turn",
    "--type",
    "--url",
    "--url-contains",
    "--value",
    "--window",
    "--workspace",
    "--identity",
    "--checkpoint",
    "--checkpoint-id",
];

/// Parse the global options preceding the command, then the command and its
/// arguments. `args` is the full argv including `args[0]` (the program name),
/// which is skipped.
///
/// Recognized global flags are matched by exact, case-sensitive equality;
/// `-v`/`--version` and `-h`/`--help` short-circuit immediately. The first token
/// that is not a recognized global flag (including a bare `--`) ends the loop
/// and becomes the command. After the command, presentation options (`--json` /
/// `--id-format`) are merged: `json_output` is monotonic-OR (a post-command
/// `--json` only sets it true) and a post-command `--id-format` hard-overwrites.
pub fn parse_global_options(args: &[String]) -> Result<ParseOutcome, CliError> {
    let mut options = GlobalOptions::default();
    let mut index = 1; // skip argv[0]

    while index < args.len() {
        match args[index].as_str() {
            "--socket" => {
                options.explicit_socket_path =
                    Some(take_value(args, index, "--socket requires a path")?);
                index += 2;
            }
            "--json" => {
                options.json_output = true;
                index += 1;
            }
            "--id-format" => {
                options.id_format = Some(take_value(
                    args,
                    index,
                    "--id-format requires a value (refs|uuids|both)",
                )?);
                index += 2;
            }
            "--window" => {
                options.window_id = Some(take_value(args, index, "--window requires a window id")?);
                index += 2;
            }
            "--password" => {
                options.socket_password =
                    Some(take_value(args, index, "--password requires a value")?);
                index += 2;
            }
            "-v" | "--version" => return Ok(ParseOutcome::PrintVersion),
            "-h" | "--help" => return Ok(ParseOutcome::PrintHelp),
            _ => break,
        }
    }

    if index >= args.len() {
        return Err(CliError::with_exit_code(
            "Missing command. Usage: cmux <path>|<command> [options]. Run 'cmux --help' for the full command list.",
            2,
        ));
    }

    let command = args[index].clone();
    let presentation = parse_presentation_options(&args[index + 1..])?;
    let command_args = options.merge_presentation(presentation);

    Ok(ParseOutcome::Command {
        options,
        command,
        command_args,
    })
}

/// The value following a value-taking flag at `index`, or `error` (exit 1) when
/// the flag is the last token.
fn take_value(args: &[String], index: usize, error: &str) -> Result<String, CliError> {
    args.get(index + 1)
        .cloned()
        .ok_or_else(|| CliError::new(error))
}

/// Presentation options parsed from the arguments that follow the command.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct PresentationOptions {
    pub json_output: bool,
    pub id_format: Option<String>,
    /// All non-presentation arguments, in order. A `--` terminator is preserved
    /// in this list and everything after it is passed through verbatim.
    pub remaining: Vec<String>,
}

/// Parse `--json` / `--id-format` from command arguments, passing every other
/// argument through into `remaining`. A `--` terminator is itself preserved in
/// `remaining` and disables further presentation parsing (so e.g. an
/// `--id-format` after `--` is not validated). An unknown option that takes a
/// value (per [`COMMAND_OPTIONS_WITH_VALUES`]) keeps its value paired with it.
pub fn parse_presentation_options(args: &[String]) -> Result<PresentationOptions, CliError> {
    let mut out = PresentationOptions::default();
    let mut index = 0;

    while index < args.len() {
        match args[index].as_str() {
            "--" => {
                // Terminator: keep it, then copy the rest of the tail verbatim.
                out.remaining.push(args[index].clone());
                out.remaining.extend_from_slice(&args[index + 1..]);
                break;
            }
            "--json" => {
                out.json_output = true;
                index += 1;
            }
            "--id-format" => {
                out.id_format = Some(take_value(
                    args,
                    index,
                    "--id-format requires a value (refs|uuids|both)",
                )?);
                index += 2;
            }
            other => {
                out.remaining.push(args[index].clone());
                if COMMAND_OPTIONS_WITH_VALUES.contains(&other) && index + 1 < args.len() {
                    out.remaining.push(args[index + 1].clone());
                    index += 2;
                } else {
                    index += 1;
                }
            }
        }
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(tokens: &[&str]) -> Vec<String> {
        std::iter::once("cmux")
            .chain(tokens.iter().copied())
            .map(str::to_owned)
            .collect()
    }

    fn command_outcome(tokens: &[&str]) -> (GlobalOptions, String, Vec<String>) {
        match parse_global_options(&argv(tokens)).expect("parse") {
            ParseOutcome::Command {
                options,
                command,
                command_args,
            } => (options, command, command_args),
            other => panic!("expected Command, got {other:?}"),
        }
    }

    #[test]
    fn explicit_socket_is_captured_and_command_follows() {
        let (options, command, args) = command_outcome(&["--socket", "\\\\.\\pipe\\x", "list"]);
        assert_eq!(
            options.explicit_socket_path.as_deref(),
            Some("\\\\.\\pipe\\x")
        );
        assert_eq!(command, "list");
        assert!(args.is_empty());
    }

    #[test]
    fn version_and_help_flags_short_circuit() {
        assert_eq!(
            parse_global_options(&argv(&["--version"])).unwrap(),
            ParseOutcome::PrintVersion
        );
        assert_eq!(
            parse_global_options(&argv(&["-v"])).unwrap(),
            ParseOutcome::PrintVersion
        );
        assert_eq!(
            parse_global_options(&argv(&["--help"])).unwrap(),
            ParseOutcome::PrintHelp
        );
        assert_eq!(
            parse_global_options(&argv(&["-h"])).unwrap(),
            ParseOutcome::PrintHelp
        );
    }

    #[test]
    fn missing_command_is_exit_code_2() {
        let error = parse_global_options(&argv(&[])).unwrap_err();
        assert_eq!(error.exit_code, 2);
        assert!(error.message.starts_with("Missing command. Usage: cmux"));
        // A trailing global flag with no command also yields missing-command.
        assert_eq!(
            parse_global_options(&argv(&["--json"]))
                .unwrap_err()
                .exit_code,
            2
        );
    }

    #[test]
    fn value_flags_missing_value_are_exit_code_1_with_exact_messages() {
        for (tokens, message) in [
            (vec!["--socket"], "--socket requires a path"),
            (
                vec!["--id-format"],
                "--id-format requires a value (refs|uuids|both)",
            ),
            (vec!["--window"], "--window requires a window id"),
            (vec!["--password"], "--password requires a value"),
        ] {
            let error = parse_global_options(&argv(&tokens)).unwrap_err();
            assert_eq!(error.exit_code, 1, "{tokens:?} should be exit 1");
            assert_eq!(error.message, message);
        }
    }

    #[test]
    fn first_non_flag_token_becomes_command() {
        let (_, command, args) = command_outcome(&["list", "--foo", "bar"]);
        assert_eq!(command, "list");
        assert_eq!(args, vec!["--foo".to_owned(), "bar".to_owned()]);
    }

    #[test]
    fn bare_double_dash_token_becomes_command() {
        // `--` is not a recognized global flag, so it ends the loop as the command.
        let (_, command, _) = command_outcome(&["--", "rest"]);
        assert_eq!(command, "--");
    }

    #[test]
    fn presentation_json_is_monotonic_or_and_id_format_overwrites() {
        // Post-command --json sets true even with no pre-command --json.
        let (options, _, _) = command_outcome(&["list", "--json"]);
        assert!(options.json_output);
        // Pre-command --json stays true even without a post-command --json.
        let (options, _, _) = command_outcome(&["--json", "list"]);
        assert!(options.json_output);
        // Post-command --id-format overwrites a pre-command one.
        let (options, _, _) =
            command_outcome(&["--id-format", "refs", "list", "--id-format", "uuids"]);
        assert_eq!(options.id_format.as_deref(), Some("uuids"));
    }

    #[test]
    fn terminator_passes_everything_through_verbatim() {
        let pres = parse_presentation_options(&[
            "--".to_owned(),
            "--json".to_owned(),
            "--id-format".to_owned(),
        ])
        .unwrap();
        // --json after -- is NOT consumed; --id-format after -- does NOT throw.
        assert!(!pres.json_output);
        assert_eq!(
            pres.remaining,
            vec![
                "--".to_owned(),
                "--json".to_owned(),
                "--id-format".to_owned()
            ]
        );
    }

    #[test]
    fn presentation_id_format_missing_value_errors() {
        let error = parse_presentation_options(&["--id-format".to_owned()]).unwrap_err();
        assert_eq!(error.exit_code, 1);
        assert_eq!(
            error.message,
            "--id-format requires a value (refs|uuids|both)"
        );
    }

    #[test]
    fn command_value_option_keeps_its_value_paired() {
        // `--workspace ws1` survives together; a lone unknown flag is pushed singly.
        let pres = parse_presentation_options(&[
            "--workspace".to_owned(),
            "ws1".to_owned(),
            "--flag".to_owned(),
        ])
        .unwrap();
        assert_eq!(
            pres.remaining,
            vec![
                "--workspace".to_owned(),
                "ws1".to_owned(),
                "--flag".to_owned()
            ]
        );
    }

    #[test]
    fn ssh_value_options_keep_values_paired() {
        let pres = parse_presentation_options(&[
            "--ssh-option".to_owned(),
            "ControlPath=/tmp/cmux-%C".to_owned(),
            "--identity".to_owned(),
            "C:\\Users\\me\\id_ed25519".to_owned(),
            "-p".to_owned(),
            "2222".to_owned(),
            "--json".to_owned(),
        ])
        .unwrap();

        assert!(pres.json_output);
        assert_eq!(
            pres.remaining,
            vec![
                "--ssh-option".to_owned(),
                "ControlPath=/tmp/cmux-%C".to_owned(),
                "--identity".to_owned(),
                "C:\\Users\\me\\id_ed25519".to_owned(),
                "-p".to_owned(),
                "2222".to_owned(),
            ]
        );
    }
}
