//! `cmux` CLI entry point. A thin shell over [`cmux_cli`]: parse the
//! invocation, classify the command into its pre-socket action, and execute the
//! resulting [`DispatchPlan`] — printing version/help, running the `rpc`
//! control-socket round-trip, or failing with the correct exit code.

use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;
use std::{collections::HashMap, io};
use std::{
    io::{IsTerminal, Read, Write},
    thread,
};

use cmux_cli::{
    classify_command, parse_global_options, plan_with_args, ClassifyEnv, CliError, DispatchPlan,
    GlobalOptions, ParseOutcome, CMUX_SURFACE_ID_ENV, CMUX_WORKSPACE_ID_ENV,
};

macro_rules! print {
    ($($argument:tt)*) => {{
        safe_stdout(format_args!($($argument)*), false)
    }};
}

macro_rules! println {
    () => {{ safe_stdout(format_args!(""), true) }};
    ($($argument:tt)*) => {{
        safe_stdout(format_args!($($argument)*), true)
    }};
}

macro_rules! eprintln {
    () => {{ safe_stderr(format_args!(""), true) }};
    ($($argument:tt)*) => {{
        safe_stderr(format_args!($($argument)*), true)
    }};
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Error: {}", error.message);
            ExitCode::from(error.exit_code.clamp(0, 255) as u8)
        }
    }
}

fn run(args: &[String]) -> Result<(), CliError> {
    match parse_global_options(args)? {
        ParseOutcome::PrintVersion => {
            print_version();
            Ok(())
        }
        ParseOutcome::PrintHelp => {
            print_top_level_help();
            Ok(())
        }
        ParseOutcome::Command {
            options,
            command,
            command_args,
        } => dispatch(&options, &command, &command_args),
    }
}

/// Classify the command into its pre-socket action, then execute the resulting
/// [`DispatchPlan`]. Classification and planning are pure (and unit-tested in
/// the library); this function is the thin I/O executor — it reads the current
/// directory and path existence for path-open classification, prints, or hands
/// off to the `rpc` round-trip.
fn dispatch(
    options: &GlobalOptions,
    command: &str,
    command_args: &[String],
) -> Result<(), CliError> {
    let cwd = std::env::current_dir().unwrap_or_default();
    let path_exists = |path: &Path| path.exists();
    let env = ClassifyEnv {
        cwd: &cwd,
        path_exists: &path_exists,
    };
    let action = classify_command(
        command,
        command_args,
        options.explicit_socket_path.as_deref(),
        &env,
    );

    match plan_with_args(&action, command, command_args) {
        DispatchPlan::PrintVersion => {
            print_version();
            Ok(())
        }
        DispatchPlan::PrintTopLevelHelp => {
            print_top_level_help();
            Ok(())
        }
        DispatchPlan::PrintLine(line) => {
            println!("{line}");
            Ok(())
        }
        DispatchPlan::RunRpc => run_rpc_command(options, command_args),
        DispatchPlan::RunSsh(args) => run_ssh_command(options, &args),
        DispatchPlan::RunRemoteDaemonStatus(args) => {
            let output = cmux_cli::remote_daemon_status::run_remote_daemon_status(
                &args,
                options.json_output,
            )?;
            println!("{output}");
            Ok(())
        }
        DispatchPlan::RunVmPtyConnect(args) => cmux_cli::vm_pty_connect::run_vm_pty_connect(&args),
        DispatchPlan::RunWindowLifecycle(lifecycle) => {
            run_window_lifecycle_command(options, &lifecycle)
        }
        DispatchPlan::RunSurfaceResume(args) => run_surface_resume_command(options, &args),
        DispatchPlan::RunControl(control) => {
            if command == "window" {
                // Canonical `window display` honors the GLOBAL --window override
                // with client-side normalizeWindowHandle resolution
                // (CLI/cmux.swift:8087-8091) — opposite of focus/close-window.
                return run_window_namespace_command(options, &control.method, &control.params);
            }
            let ambient_workspace_id = std::env::var(CMUX_WORKSPACE_ID_ENV).ok();
            let ambient_surface_id = if command == "tab-action" {
                std::env::var("CMUX_TAB_ID")
                    .ok()
                    .or_else(|| std::env::var(CMUX_SURFACE_ID_ENV).ok())
            } else if command == "respawn-pane" {
                None
            } else {
                std::env::var(CMUX_SURFACE_ID_ENV).ok()
            };
            let control = control.with_window_id(options.window_id.as_deref());
            let has_window_scope = ["window_id", "window_ref", "window_index"]
                .iter()
                .any(|key| control.params.get(*key).is_some());
            let suppress_ambient_window = control.params.get("suppress_ambient_window").is_some();
            let has_global_window = options
                .window_id
                .as_deref()
                .is_some_and(|window| !window.trim().is_empty());
            let control = if has_global_window || has_window_scope {
                control
            } else if suppress_ambient_window {
                control.with_ambient_workspace_id(ambient_workspace_id.as_deref())
            } else {
                control
                    .with_ambient_surface_id(ambient_surface_id.as_deref())
                    .with_ambient_workspace_id(ambient_workspace_id.as_deref())
            };
            if matches!(
                command,
                "list-workspaces"
                    | "current-workspace"
                    | "new-workspace"
                    | "close-workspace"
                    | "select-workspace"
                    | "rename-workspace"
                    | "rename-window"
            ) {
                run_legacy_workspace_command(options, command, &control.method, &control.params)
            } else if matches!(command, "tab-action" | "respawn-pane") {
                run_lifecycle_command(options, &control.method, &control.params)
            } else {
                run_control_command(options, &control.method, &control.params)
            }
        }
        DispatchPlan::RunTmuxCompat(args) => run_tmux_compat_command(options, &args),
        DispatchPlan::RunEvents(args) => run_events_command(options, &args),
        DispatchPlan::RunDiffViewerRefs(args) => {
            let output = cmux_cli::diff_viewer_cli::run_diff_viewer_refs_command(&args, &cwd)?;
            println!("{output}");
            Ok(())
        }
        DispatchPlan::RunDiffViewerBranch(args) => {
            let output = cmux_cli::diff_viewer_cli::run_diff_viewer_branch_command(&args, &cwd)?;
            println!("{output}");
            Ok(())
        }
        DispatchPlan::RunDiffViewerServer(args) => {
            let server = cmux_cli::diff_viewer_server::DiffViewerServer::prepare(&args)?;
            println!("{}", server.port());
            server.run()
        }
        DispatchPlan::RunDocs(args) => {
            let output = cmux_cli::docs::run_docs_command(&args, options.json_output)?;
            println!("{output}");
            Ok(())
        }
        DispatchPlan::RunWelcome => {
            print!("{}", cmux_cli::welcome::render_welcome(false));
            Ok(())
        }
        DispatchPlan::RunSettings(args) => {
            let output = cmux_cli::settings::run_settings_no_socket(&args, options.json_output)?;
            println!("{output}");
            Ok(())
        }
        DispatchPlan::RunConfig(args) => {
            let result = cmux_cli::config::run_config_no_socket(&args, options.json_output)?;
            println!("{}", result.output);
            result.failure.map_or(Ok(()), Err)
        }
        DispatchPlan::RunConfigMutation(args) => run_config_mutation_command(options, &args),
        DispatchPlan::RunWindowDefaultDisplay(args) => {
            let output = cmux_cli::window_default_display::run_window_default_display(
                &args,
                options.json_output,
            )?;
            println!("{output}");
            Ok(())
        }
        DispatchPlan::RunOpenPath(path) => {
            let output = cmux_cli::path_open::run_open_path(&path, &cwd)?;
            println!("{output}");
            Ok(())
        }
        DispatchPlan::RunSessions(args) => {
            let environment = std::env::vars().collect();
            let output =
                cmux_cli::sessions::run_sessions_command(&args, options.json_output, &environment)?;
            println!("{output}");
            Ok(())
        }
        DispatchPlan::RunSigpipeProbe(args) => {
            if let Some(output) = cmux_cli::sigpipe::run_sigpipe_probe(&args)? {
                println!("{output}");
            }
            Ok(())
        }
        DispatchPlan::RunSigpipeStdinPipeProbe => {
            println!("{}", cmux_cli::sigpipe::run_sigpipe_stdin_pipe_probe()?);
            Ok(())
        }
        DispatchPlan::RunSigpipeInspect(args) => {
            if let Some(output) = cmux_cli::sigpipe::run_sigpipe_inspect(&args)? {
                println!("{output}");
            }
            Ok(())
        }
        DispatchPlan::RunHooksInstaller { command, args } => {
            let output = cmux_cli::hooks_installer::run_hooks_command(&command, &args)?;
            print!("{output}");
            Ok(())
        }
        DispatchPlan::RunFeedHook(args) => run_feed_hook_command(options, &args),
        DispatchPlan::RunFeed(args) => run_feed_command(options, &args),
        DispatchPlan::Fail(error) => Err(error),
    }
}

fn safe_stdout(arguments: std::fmt::Arguments<'_>, newline: bool) {
    let mut stdout = std::io::stdout().lock();
    write_cli_output(&mut stdout, arguments, newline);
}

fn safe_stderr(arguments: std::fmt::Arguments<'_>, newline: bool) {
    let mut stderr = std::io::stderr().lock();
    write_cli_output(&mut stderr, arguments, newline);
}

fn write_cli_output(writer: &mut dyn Write, arguments: std::fmt::Arguments<'_>, newline: bool) {
    if writer.write_fmt(arguments).is_ok() && newline {
        let _ = writer.write_all(b"\n");
    }
    let _ = writer.flush();
}

#[cfg(test)]
mod cli_output_tests {
    use super::write_cli_output;
    use std::io::{self, Write};

    struct BrokenPipeWriter;

    impl Write for BrokenPipeWriter {
        fn write(&mut self, _buffer: &[u8]) -> io::Result<usize> {
            Err(io::Error::new(io::ErrorKind::BrokenPipe, "closed pipe"))
        }

        fn flush(&mut self) -> io::Result<()> {
            Err(io::Error::new(io::ErrorKind::BrokenPipe, "closed pipe"))
        }
    }

    #[test]
    fn top_level_output_ignores_closed_pipes() {
        write_cli_output(&mut BrokenPipeWriter, format_args!("version"), true);
        write_cli_output(&mut BrokenPipeWriter, format_args!("error"), false);
    }
}

fn run_feed_command(options: &GlobalOptions, args: &[String]) -> Result<(), CliError> {
    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .ok_or_else(|| CliError::new("home directory is unavailable"))?;
    if args
        .first()
        .is_some_and(|argument| argument.eq_ignore_ascii_case("tui"))
    {
        let implementation = cmux_cli::feed_tui::parse_feed_tui_args(&args[1..])?;
        if implementation == cmux_cli::feed_tui::FeedTuiImplementation::Help {
            println!("{}", cmux_cli::feed_tui::FEED_TUI_USAGE);
            return Ok(());
        }
        let interactive = std::io::stdin().is_terminal() && std::io::stdout().is_terminal();
        if !interactive {
            return Err(CliError::new(
                "cmux feed tui requires an interactive terminal",
            ));
        }
        let force_legacy = implementation == cmux_cli::feed_tui::FeedTuiImplementation::Legacy
            || std::env::var("CMUX_FEED_TUI_LEGACY").as_deref() == Ok("1");
        if force_legacy {
            return run_legacy_feed_tui(options);
        }
        let open_tui = run_open_feed_tui(options, &home, interactive);
        if implementation == cmux_cli::feed_tui::FeedTuiImplementation::OpenTui {
            return open_tui;
        }
        return open_tui.or_else(|error| {
            eprintln!("cmux feed tui: OpenTUI unavailable ({error}); falling back to legacy TUI.");
            run_legacy_feed_tui(options)
        });
    }
    let output = cmux_cli::feed_clear::run_feed_command(args, &home, |prompt| {
        print!("{prompt}");
        std::io::stdout()
            .flush()
            .map_err(|error| CliError::new(format!("failed to write Feed prompt: {error}")))?;
        let mut answer = String::new();
        std::io::stdin()
            .read_line(&mut answer)
            .map_err(|error| CliError::new(format!("failed to read Feed confirmation: {error}")))?;
        Ok(answer.to_ascii_lowercase().starts_with('y'))
    })?;
    println!("{output}");
    Ok(())
}

fn run_open_feed_tui(
    options: &GlobalOptions,
    home: &Path,
    interactive: bool,
) -> Result<(), CliError> {
    let bun = cmux_cli::feed_tui::resolve_bun_executable(
        std::env::var("CMUX_FEED_TUI_BUN_PATH").ok().as_deref(),
        home,
    )
    .ok_or_else(|| CliError::new("Bun is required for the OpenTUI Feed"))?;
    eprintln!("cmux feed tui: preparing OpenTUI Feed...");
    let source = cmux_cli::feed_tui::prepare_open_tui_app(home, &bun)?;
    let (socket_path, socket_password) = resolved_control_connection(options)?;
    let cwd = std::env::current_dir().unwrap_or_default();
    let plan =
        cmux_cli::feed_tui::build_open_tui_launch_plan(&cmux_cli::feed_tui::FeedTuiLaunchInputs {
            interactive,
            bun_path: bun.to_string_lossy().into_owned(),
            source_path: source.to_string_lossy().into_owned(),
            cwd: cwd.to_string_lossy().into_owned(),
            socket_path,
            socket_password,
        })?;
    eprintln!("cmux feed tui: starting OpenTUI Feed.");
    cmux_cli::feed_tui::run_open_tui_launch_plan(&plan)
}

fn run_legacy_feed_tui(options: &GlobalOptions) -> Result<(), CliError> {
    use crossterm::cursor::{Hide, Show};
    use crossterm::event::{self, Event, KeyCode};
    use crossterm::execute;
    use crossterm::terminal::{
        disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
    };

    struct TerminalGuard;
    impl Drop for TerminalGuard {
        fn drop(&mut self) {
            let _ = disable_raw_mode();
            let _ = execute!(io::stdout(), Show, LeaveAlternateScreen);
        }
    }

    enable_raw_mode()
        .map_err(|error| CliError::new(format!("Failed to enter terminal raw mode: {error}")))?;
    let _guard = TerminalGuard;
    execute!(io::stdout(), EnterAlternateScreen, Hide)
        .map_err(|error| CliError::new(format!("Failed to initialize Feed TUI: {error}")))?;
    let mut selected = 0usize;
    let mut selected_answers = HashMap::<String, Vec<String>>::new();
    let mut status = "Loaded Feed.".to_string();

    loop {
        let result = call_control_command(
            options,
            "feed.list",
            &serde_json::json!({"pending_only":true}),
        )?;
        let items = result
            .get("items")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(cmux_cli::feed_tui::legacy_feed_item)
            .collect::<Vec<_>>();
        selected = selected.min(items.len().saturating_sub(1));
        render_legacy_feed_tui(&items, selected, &status, &selected_answers)?;

        let Event::Key(key) = event::read()
            .map_err(|error| CliError::new(format!("Failed to read Feed TUI input: {error}")))?
        else {
            continue;
        };
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
            KeyCode::Down | KeyCode::Char('j') => {
                selected = (selected + 1).min(items.len().saturating_sub(1));
                continue;
            }
            KeyCode::Up | KeyCode::Char('k') => {
                selected = selected.saturating_sub(1);
                continue;
            }
            KeyCode::Char('r') => {
                status = "Refreshed Feed.".to_string();
                continue;
            }
            _ => {}
        }
        let Some(item) = items.get(selected) else {
            status = "No selection".to_string();
            continue;
        };
        let action_key = match key.code {
            KeyCode::Enter => cmux_cli::feed_tui::LegacyFeedKey::Enter,
            KeyCode::Char('o') => cmux_cli::feed_tui::LegacyFeedKey::Once,
            KeyCode::Char('a') => cmux_cli::feed_tui::LegacyFeedKey::Always,
            KeyCode::Char('l') => cmux_cli::feed_tui::LegacyFeedKey::All,
            KeyCode::Char('b') => cmux_cli::feed_tui::LegacyFeedKey::Bypass,
            KeyCode::Char('d') => cmux_cli::feed_tui::LegacyFeedKey::Deny,
            KeyCode::Char('m') => cmux_cli::feed_tui::LegacyFeedKey::Manual,
            KeyCode::Char('u') => cmux_cli::feed_tui::LegacyFeedKey::Ultraplan,
            KeyCode::Char('f') if item.kind == "exitPlan" => {
                let feedback = read_legacy_feed_feedback()?;
                if feedback.is_empty() {
                    status = "Replan cancelled".to_string();
                    continue;
                }
                cmux_cli::feed_tui::LegacyFeedKey::Feedback(feedback)
            }
            KeyCode::Char(digit @ '1'..='9') => {
                let number = digit.to_digit(10).unwrap_or(1) as usize;
                if item
                    .questions
                    .first()
                    .is_some_and(|question| question.multi_select)
                {
                    if let Some(option) = item
                        .questions
                        .first()
                        .and_then(|question| question.options.get(number - 1))
                    {
                        let answers = selected_answers.entry(item.request_id.clone()).or_default();
                        if let Some(index) = answers.iter().position(|label| label == &option.label)
                        {
                            answers.remove(index);
                        } else {
                            answers.push(option.label.clone());
                        }
                        status = format!("Updated selections for {}", item.title);
                    }
                    continue;
                }
                cmux_cli::feed_tui::LegacyFeedKey::Number(number)
            }
            _ => {
                status = "Key is not available for this item".to_string();
                continue;
            }
        };
        let answers = selected_answers
            .get(&item.request_id)
            .cloned()
            .unwrap_or_default();
        let Some(cmux_cli::feed_tui::LegacyFeedAction::Request { method, params }) =
            cmux_cli::feed_tui::legacy_action(item, action_key, &answers)
        else {
            status = "Key is not available for this item".to_string();
            continue;
        };
        call_control_command(options, method, &params)?;
        selected_answers.remove(&item.request_id);
        status = format!("Sent action for {}", item.title);
    }
}

fn read_legacy_feed_feedback() -> Result<String, CliError> {
    use crossterm::cursor::{Hide, Show};
    use crossterm::execute;
    use crossterm::terminal::{disable_raw_mode, enable_raw_mode};

    disable_raw_mode()
        .map_err(|error| CliError::new(format!("Failed to pause terminal raw mode: {error}")))?;
    execute!(io::stdout(), Show)
        .map_err(|error| CliError::new(format!("Failed to show terminal cursor: {error}")))?;
    print!("\r\nReplan feedback: ");
    io::stdout()
        .flush()
        .map_err(|error| CliError::new(error.to_string()))?;
    let mut feedback = String::new();
    let read_result = io::stdin().read_line(&mut feedback);
    enable_raw_mode()
        .map_err(|error| CliError::new(format!("Failed to resume terminal raw mode: {error}")))?;
    execute!(io::stdout(), Hide)
        .map_err(|error| CliError::new(format!("Failed to hide terminal cursor: {error}")))?;
    read_result
        .map_err(|error| CliError::new(format!("Failed to read replan feedback: {error}")))?;
    Ok(feedback.trim().to_string())
}

fn render_legacy_feed_tui(
    items: &[cmux_cli::feed_tui::LegacyFeedItem],
    selected: usize,
    status: &str,
    selected_answers: &HashMap<String, Vec<String>>,
) -> Result<(), CliError> {
    use crossterm::cursor::MoveTo;
    use crossterm::execute;
    use crossterm::terminal::{Clear, ClearType};

    let mut stdout = io::stdout();
    execute!(stdout, MoveTo(0, 0), Clear(ClearType::All))
        .map_err(|error| CliError::new(format!("Failed to render Feed TUI: {error}")))?;
    stdout
        .write_all(legacy_feed_tui_text(items, selected, status, selected_answers).as_bytes())
        .and_then(|()| stdout.flush())
        .map_err(|error| CliError::new(error.to_string()))
}

fn legacy_feed_tui_text(
    items: &[cmux_cli::feed_tui::LegacyFeedItem],
    selected: usize,
    status: &str,
    selected_answers: &HashMap<String, Vec<String>>,
) -> String {
    use std::fmt::Write as _;

    let mut output = format!(
        "cmux feed — {} pending\n────────────────────────────────────────\n",
        items.len()
    );
    if items.is_empty() {
        let _ = writeln!(output, "No Feed items yet.");
    }
    for (index, item) in items.iter().enumerate() {
        let marker = if index == selected { ">" } else { " " };
        let _ = writeln!(
            output,
            "{marker} [{}] {} · {}",
            item.source, item.title, item.kind
        );
        if let Some(question) = item.questions.first() {
            for (option_index, option) in question.options.iter().enumerate() {
                let checked = selected_answers
                    .get(&item.request_id)
                    .is_some_and(|answers| answers.contains(&option.label));
                let _ = writeln!(
                    output,
                    "    {} {}. {}",
                    if checked { "[x]" } else { "   " },
                    option_index + 1,
                    option.label
                );
            }
        }
    }
    let _ = writeln!(output, "────────────────────────────────────────");
    let _ = writeln!(output, "{status}");
    let _ = writeln!(
        output,
        "j/k move · Enter default · o/a/l/b/d actions · f replan · r refresh · q quit"
    );
    output
}

#[cfg(test)]
mod legacy_feed_tui_tests {
    use super::*;

    #[test]
    fn text_projection_marks_selection_and_question_choices() {
        let item = cmux_cli::feed_tui::legacy_feed_item(&serde_json::json!({
            "id":"i1","request_id":"r1","workstream_id":"w1","source":"claude",
            "kind":"question","status":"pending","title":"Choose",
            "questions":[{"multi_select":true,"options":[{"id":"o1","label":"Tests"}]}]
        }))
        .unwrap();
        let answers = HashMap::from([("r1".to_string(), vec!["Tests".to_string()])]);
        let text = legacy_feed_tui_text(&[item], 0, "Ready", &answers);
        assert!(text.contains("> [claude] Choose · question"));
        assert!(text.contains("[x] 1. Tests"));
        assert!(text.contains("f replan"));
    }
}

#[cfg(windows)]
fn run_feed_hook_command(options: &GlobalOptions, args: &[String]) -> Result<(), CliError> {
    let mut stdin =
        std::io::stdin().take((cmux_cli::feed_hook::FEED_HOOK_MAX_STDIN_BYTES + 1) as u64);
    let mut input = Vec::new();
    stdin
        .read_to_end(&mut input)
        .map_err(|error| CliError::new(format!("failed to read Feed hook stdin: {error}")))?;
    let environment = cmux_cli::feed_hook::FeedHookEnvironment {
        surface_id: std::env::var("CMUX_SURFACE_ID").ok(),
        workspace_id: std::env::var(CMUX_WORKSPACE_ID_ENV).ok(),
        agent_pid: feed_hook_agent_pid(args),
    };
    let Some(prepared) = cmux_cli::feed_hook::prepare_feed_hook(
        args,
        &input,
        &environment,
        &uuid::Uuid::new_v4().to_string(),
    )?
    else {
        println!("{{}}");
        return Ok(());
    };
    let result = match call_control_command(options, "feed.push", &prepared.params) {
        Ok(result) => result,
        Err(_) => {
            println!("{{}}");
            return Ok(());
        }
    };
    let output = (result.get("status").and_then(serde_json::Value::as_str) == Some("resolved"))
        .then(|| result.get("decision"))
        .flatten()
        .map(|decision| cmux_cli::feed_hook::render_agent_decision_output(&prepared, decision));
    let Some(output) = output else {
        println!("{{}}");
        return Ok(());
    };
    if let Some(stderr) = output.stderr {
        eprintln!("{stderr}");
    }
    if !output.stdout.is_empty() {
        println!("{}", output.stdout);
    }
    if output.exit_code != 0 {
        std::process::exit(output.exit_code.into());
    }
    Ok(())
}

#[cfg(windows)]
fn feed_hook_agent_pid(args: &[String]) -> i64 {
    let source = args.iter().enumerate().find_map(|(index, arg)| {
        (arg == "--source")
            .then(|| args.get(index + 1).cloned())
            .flatten()
            .or_else(|| arg.strip_prefix("--source=").map(str::to_string))
    });
    let key = source.map(|source| {
        format!(
            "CMUX_{}_PID",
            source
                .chars()
                .map(|character| if character.is_ascii_alphanumeric() {
                    character.to_ascii_uppercase()
                } else {
                    '_'
                })
                .collect::<String>()
        )
    });
    key.and_then(|key| std::env::var(key).ok())
        .and_then(|value| value.parse::<i64>().ok())
        .unwrap_or_else(|| i64::from(std::process::id()))
}

/// Print the version summary to stdout. The standalone Rust CLI reports the
/// crate version; the macOS CLI's bundle/commit suffix has no analogue here yet.
fn print_version() {
    println!("cmux {}", env!("CARGO_PKG_VERSION"));
}

/// Print the top-level help to stdout. The full macOS `usage()` block (the
/// 150-command listing, with its macOS-specific paths) is a later, platform-
/// adapted slice; for now this is the one-line synopsis.
fn print_top_level_help() {
    println!("Usage: cmux <path>|<command> [options]");
}

#[cfg(windows)]
fn run_config_mutation_command(
    options: &GlobalOptions,
    command_args: &[String],
) -> Result<(), CliError> {
    let result = cmux_cli::config::run_config_mutation(command_args, options.json_output, || {
        call_control_command(options, "config.reload", &serde_json::json!({}))
            .map(|value| format_control_result("config.reload", &value))
    })?;
    println!("{}", result.output);
    result.failure.map_or(Ok(()), Err)
}

#[cfg(not(windows))]
fn run_config_mutation_command(
    options: &GlobalOptions,
    command_args: &[String],
) -> Result<(), CliError> {
    let result = cmux_cli::config::run_config_mutation(command_args, options.json_output, || {
        Err(CliError::new(
            "socket commands are only supported on Windows in this build",
        ))
    })?;
    println!("{}", result.output);
    result.failure.map_or(Ok(()), Err)
}

/// `cmux rpc <method> [json-params]` — resolve the socket address and password
/// from flags + environment, then round-trip a v2 request over the control pipe
/// and print the result. The address/password resolution and the transport are
/// each unit-tested; this glue only reads the ambient env.
#[cfg(windows)]
fn run_rpc_command(options: &GlobalOptions, command_args: &[String]) -> Result<(), CliError> {
    let method = command_args
        .first()
        .map(|arg| arg.trim())
        .filter(|arg| !arg.is_empty())
        .ok_or_else(|| CliError::new("Usage: cmux rpc <method> [json-params]"))?;
    let params = cmux_cli::parse_rpc_params(&command_args[1..])?;

    run_control_command(options, method, &params)
}

#[cfg(windows)]
fn run_control_command(
    options: &GlobalOptions,
    method: &str,
    params: &serde_json::Value,
) -> Result<(), CliError> {
    let result = call_control_command(options, method, params)?;
    if options.json_output {
        println!("{}", serde_json::to_string(&result).unwrap_or_default());
    } else {
        println!("{}", format_control_result(method, &result));
    }
    Ok(())
}

#[cfg(not(windows))]
fn run_lifecycle_command(
    _options: &GlobalOptions,
    _method: &str,
    _params: &serde_json::Value,
) -> Result<(), CliError> {
    Err(CliError::new(
        "socket commands are only supported on Windows in this build",
    ))
}

#[cfg(windows)]
fn run_lifecycle_command(
    options: &GlobalOptions,
    method: &str,
    params: &serde_json::Value,
) -> Result<(), CliError> {
    let mut request_params = params.clone();
    if method == "surface.respawn" {
        if let Some(object) = request_params.as_object_mut() {
            let has_window = ["window_id", "window_ref", "window_index"]
                .iter()
                .any(|key| object.get(*key).is_some());
            let has_workspace = object.get("workspace_id").is_some()
                || object.get("workspace_ref").is_some()
                || object.get("workspace_index").is_some();
            if has_window && !has_workspace {
                object.remove("resolve_current_workspace");
            }
        }
    }
    normalize_workspace_params(options, method, &mut request_params)?;
    normalize_lifecycle_surface_params(options, method, &mut request_params)?;
    let result = call_control_command(options, method, &request_params)?;
    let id_format = options.id_format.as_deref().unwrap_or("refs");
    if options.json_output {
        let mut formatted = result;
        filter_id_format(&mut formatted, id_format);
        println!("{}", serde_json::to_string(&formatted).unwrap_or_default());
    } else {
        let requested_action = request_params
            .get("action")
            .and_then(serde_json::Value::as_str);
        println!(
            "{}",
            format_lifecycle_text(method, &result, id_format, requested_action)
        );
    }
    Ok(())
}

/// Execute a v1 window-lifecycle command: resolve the `--window` selector
/// client-side (UUID passthrough; ref/index via a live `window.list`), send the
/// v1 text frame, and print the raw reply (`OK` / `OK <uuid>`). `--json` has no
/// effect (CLI/cmux.swift:4294-4310).
#[cfg(windows)]
fn run_window_lifecycle_command(
    options: &GlobalOptions,
    lifecycle: &cmux_cli::WindowLifecycleCommand,
) -> Result<(), CliError> {
    use cmux_cli::WindowLifecycleCommand as Lifecycle;
    let line = match lifecycle {
        Lifecycle::NewWindow => lifecycle.v1_command().to_owned(),
        Lifecycle::FocusWindow(handle) | Lifecycle::CloseWindow(handle) => format!(
            "{} {}",
            lifecycle.v1_command(),
            resolve_window_handle_value(options, handle)?
        ),
    };
    let response = call_v1_command(options, &line)?;
    println!("{response}");
    Ok(())
}

#[cfg(not(windows))]
fn run_window_lifecycle_command(
    _options: &GlobalOptions,
    _lifecycle: &cmux_cli::WindowLifecycleCommand,
) -> Result<(), CliError> {
    Err(CliError::new(
        "socket commands are only supported on Windows in this build",
    ))
}

/// Resolve a classified `--window` selector into the string sent on the v1
/// wire / as `window_id`, per `normalizeWindowHandle`
/// (CLI/cmux.swift:6084-6112): UUIDs pass through; a `kind:N` ref must match a
/// `window.list` row's id or ref (`Window not found: <ref>`); a bare integer
/// must match a row's `index` (`Window index not found`).
#[cfg(windows)]
fn resolve_window_handle_value(
    options: &GlobalOptions,
    handle: &cmux_cli::WindowHandle,
) -> Result<String, CliError> {
    use cmux_cli::WindowHandle as Handle;
    match handle {
        Handle::Uuid(value) => Ok(value.clone()),
        Handle::Ref(reference) => {
            for window in listed_windows(options)? {
                if item_matches_handle(&window, reference) {
                    return Ok(canonical_id_or_ref(&window).unwrap_or_else(|| reference.clone()));
                }
            }
            Err(CliError::new(format!("Window not found: {reference}")))
        }
        Handle::Index(index) => {
            for window in listed_windows(options)? {
                if cmux_cli::int_from_any(window.get("index")) == Some(*index) {
                    if let Some(resolved) = canonical_id_or_ref(&window) {
                        return Ok(resolved);
                    }
                }
            }
            Err(CliError::new("Window index not found"))
        }
    }
}

/// Swift `windowHandleMatches` / `surfaceHandleMatches` shape: a list row
/// matches when its `id` or `ref` handles-matches the target (UUID-aware,
/// else case-insensitive; blank candidates never match).
#[cfg(windows)]
fn item_matches_handle(item: &serde_json::Value, handle: &str) -> bool {
    ["id", "ref"].iter().any(|key| {
        item.get(*key)
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|candidate| !candidate.is_empty())
            .is_some_and(|candidate| cmux_cli::handles_match(handle, candidate))
    })
}

/// Resolve the raw GLOBAL/explicit `--window` value like the canonical helpers
/// do before scoping a request: classify (blank → `None`), then resolve
/// refs/indexes through `window.list`.
#[cfg(windows)]
fn normalize_window_selector(
    options: &GlobalOptions,
    raw: &str,
) -> Result<Option<String>, CliError> {
    match cmux_cli::classify_window_handle(raw)? {
        Some(handle) => resolve_window_handle_value(options, &handle).map(Some),
        None => Ok(None),
    }
}

/// The canonical CLI reads plain `id` / `ref` keys from v2 list rows
/// (CLI/cmux.swift:6096-6101, 6109-6111).
#[cfg(windows)]
fn canonical_id_or_ref(item: &serde_json::Value) -> Option<String> {
    item.get("id")
        .or_else(|| item.get("ref"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
}

#[cfg(windows)]
fn listed_windows(options: &GlobalOptions) -> Result<Vec<serde_json::Value>, CliError> {
    let listed = call_control_command(options, "window.list", &serde_json::json!({}))?;
    Ok(listed
        .get("windows")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default())
}

/// One blocking v1 text round-trip over the control pipe.
#[cfg(windows)]
fn call_v1_command(options: &GlobalOptions, line: &str) -> Result<String, CliError> {
    let (socket_path, password) = resolved_control_connection(options)?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| CliError::new(format!("failed to start async runtime: {error}")))?;
    runtime.block_on(cmux_cli::transport::run_v1(
        &socket_path,
        password.as_deref(),
        line,
    ))
}

/// The `cmux window <displays|display>` namespace runner. `window.display`
/// normalizes the GLOBAL --window override client-side and id-formats its
/// `--json` payload (CLI/cmux.swift:8072-8110); `window.displays` keeps the
/// plain control-command path (raw `--json`, canonical text rows).
#[cfg(windows)]
fn run_window_namespace_command(
    options: &GlobalOptions,
    method: &str,
    params: &serde_json::Value,
) -> Result<(), CliError> {
    if method != "window.display" {
        return run_control_command(options, method, params);
    }
    let mut request = params.clone();
    if let Some(window_override) = options.window_id.as_deref() {
        // Canonical: normalizeWindowHandle(windowOverride) ?? windowOverride —
        // a blank override falls back to the raw string (CLI/cmux.swift:8088).
        let normalized = normalize_window_selector(options, window_override)?
            .unwrap_or_else(|| window_override.to_owned());
        if let Some(object) = request.as_object_mut() {
            object.insert("window_id".into(), serde_json::json!(normalized));
        }
    }
    let result = call_control_command(options, method, &request)?;
    if options.json_output {
        let mut formatted = result;
        filter_id_format(
            &mut formatted,
            options.id_format.as_deref().unwrap_or("refs"),
        );
        println!("{}", serde_json::to_string(&formatted).unwrap_or_default());
    } else {
        println!("{}", format_window_display_result(&result));
    }
    Ok(())
}

#[cfg(not(windows))]
fn run_window_namespace_command(
    _options: &GlobalOptions,
    _method: &str,
    _params: &serde_json::Value,
) -> Result<(), CliError> {
    Err(CliError::new(
        "socket commands are only supported on Windows in this build",
    ))
}

/// Execute a parsed `surface resume` plan: resolve the raw target selectors
/// (`normalizeWindowHandle` / `normalizeWorkspaceHandle` /
/// `normalizeSurfaceHandle` parity), send the v2 `surface.resume.*` request,
/// and print per canonical output rules (CLI/cmux.swift:6618-6657).
#[cfg(windows)]
fn run_surface_resume_command(options: &GlobalOptions, args: &[String]) -> Result<(), CliError> {
    let env = |name: &str| std::env::var(name).ok();
    let fallback_cwd = std::env::current_dir()
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_default();
    let plan = cmux_cli::parse_surface_resume_command(
        args,
        options.window_id.as_deref(),
        &env,
        &fallback_cwd,
    )?;

    let window_handle = match plan.window.as_deref() {
        Some(raw) => normalize_window_selector(options, raw)?,
        None => None,
    };
    let workspace_id = resolve_resume_workspace_handle(
        options,
        plan.workspace.as_deref(),
        window_handle.as_deref(),
    )?;
    let surface_id = resolve_resume_surface_handle(
        options,
        plan.surface.as_deref(),
        workspace_id.as_deref(),
        window_handle.as_deref(),
    )?;

    let mut params = serde_json::Map::new();
    if let Some(window_handle) = window_handle {
        params.insert("window_id".into(), serde_json::json!(window_handle));
    }
    if let Some(workspace_id) = workspace_id {
        params.insert("workspace_id".into(), serde_json::json!(workspace_id));
    }
    if let Some(surface_id) = surface_id {
        params.insert("surface_id".into(), serde_json::json!(surface_id));
    }
    params.extend(plan.extra.clone());

    let result = call_control_command(options, plan.method, &serde_json::Value::Object(params))?;
    let id_format = options.id_format.as_deref().unwrap_or("refs");
    if options.json_output {
        let mut formatted = result;
        filter_id_format(&mut formatted, id_format);
        println!("{}", serde_json::to_string(&formatted).unwrap_or_default());
        return Ok(());
    }
    match plan.action {
        cmux_cli::SurfaceResumeAction::Set | cmux_cli::SurfaceResumeAction::Clear => {
            println!("OK");
        }
        cmux_cli::SurfaceResumeAction::Show => {
            let command = result
                .get("resume_binding")
                .and_then(|binding| binding.get("command"))
                .and_then(serde_json::Value::as_str)
                .filter(|command| !command.is_empty());
            match command {
                Some(command) => println!("{command}"),
                None => println!("No resume binding"),
            }
        }
    }
    Ok(())
}

#[cfg(not(windows))]
fn run_surface_resume_command(_options: &GlobalOptions, _args: &[String]) -> Result<(), CliError> {
    Err(CliError::new(
        "socket commands are only supported on Windows in this build",
    ))
}

/// `normalizeWorkspaceHandle` without the allow-current path
/// (CLI/cmux.swift:6137-6183): UUID passthrough; a ref passes through verbatim
/// UNLESS a window handle scopes it (then `resolveWorkspaceId` matches the
/// window's `workspace.list` refs exactly and requires an id); a bare integer
/// resolves against `workspace.list` row indexes.
#[cfg(windows)]
fn resolve_resume_workspace_handle(
    options: &GlobalOptions,
    raw: Option<&str>,
    window_handle: Option<&str>,
) -> Result<Option<String>, CliError> {
    let Some(raw) = raw else {
        return Ok(None);
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    if cmux_cli::is_uuid_handle(trimmed) {
        return Ok(Some(trimmed.to_owned()));
    }
    if cmux_cli::window_lifecycle::is_handle_ref(trimmed) {
        let Some(window_handle) = window_handle else {
            return Ok(Some(trimmed.to_owned()));
        };
        // resolveWorkspaceId (CLI/cmux.swift:14863-14880): exact `ref` match
        // within the window; only an `id` resolves it.
        let listed = call_control_command(
            options,
            "workspace.list",
            &serde_json::json!({"window_id": window_handle}),
        )?;
        for workspace in listed
            .get("workspaces")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
        {
            if workspace.get("ref").and_then(serde_json::Value::as_str) == Some(trimmed) {
                if let Some(id) = workspace.get("id").and_then(serde_json::Value::as_str) {
                    return Ok(Some(id.to_owned()));
                }
            }
        }
        return Err(CliError::new(format!("Workspace ref not found: {trimmed}")));
    }
    if let Ok(index) = trimmed.parse::<i64>() {
        let mut params = serde_json::Map::new();
        if let Some(window_handle) = window_handle {
            params.insert("window_id".into(), serde_json::json!(window_handle));
        }
        let listed = call_control_command(
            options,
            "workspace.list",
            &serde_json::Value::Object(params),
        )?;
        for workspace in listed
            .get("workspaces")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
        {
            if cmux_cli::int_from_any(workspace.get("index")) == Some(index) {
                if let Some(resolved) = canonical_id_or_ref(workspace) {
                    return Ok(Some(resolved));
                }
            }
        }
        return Err(CliError::new("Workspace index not found"));
    }
    Err(CliError::new(format!(
        "Invalid workspace handle: {trimmed} (expected UUID, ref like workspace:1, or index)"
    )))
}

/// `normalizeSurfaceHandle` without the allow-focused path
/// (CLI/cmux.swift:6308-6355): UUIDs/refs pass through verbatim unless a
/// window handle scopes them (then the surface must exist in that window);
/// bare integers resolve against `surface.list` row indexes.
#[cfg(windows)]
fn resolve_resume_surface_handle(
    options: &GlobalOptions,
    raw: Option<&str>,
    workspace_handle: Option<&str>,
    window_handle: Option<&str>,
) -> Result<Option<String>, CliError> {
    let Some(raw) = raw else {
        return Ok(None);
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    if cmux_cli::is_uuid_handle(trimmed) || cmux_cli::window_lifecycle::is_handle_ref(trimmed) {
        let Some(window_handle) = window_handle else {
            return Ok(Some(trimmed.to_owned()));
        };
        return validate_surface_handle_in_window(
            options,
            trimmed,
            workspace_handle,
            window_handle,
        )
        .map(Some);
    }
    if let Ok(index) = trimmed.parse::<i64>() {
        let mut params = serde_json::Map::new();
        if let Some(window_handle) = window_handle {
            params.insert("window_id".into(), serde_json::json!(window_handle));
        }
        if let Some(workspace_handle) = workspace_handle {
            params.insert("workspace_id".into(), serde_json::json!(workspace_handle));
        }
        let listed =
            call_control_command(options, "surface.list", &serde_json::Value::Object(params))?;
        for surface in listed
            .get("surfaces")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
        {
            if cmux_cli::int_from_any(surface.get("index")) == Some(index) {
                if let Some(resolved) = canonical_id_or_ref(surface) {
                    return Ok(Some(resolved));
                }
            }
        }
        return Err(CliError::new("Surface index not found"));
    }
    Err(CliError::new(format!(
        "Invalid surface handle: {trimmed} (expected UUID, ref like surface:1, or index)"
    )))
}

/// `validateSurfaceHandleInWindow` (CLI/cmux.swift:6360-6395): scoped to the
/// given workspace when present, otherwise every workspace of the window.
#[cfg(windows)]
fn validate_surface_handle_in_window(
    options: &GlobalOptions,
    surface_handle: &str,
    workspace_handle: Option<&str>,
    window_handle: &str,
) -> Result<String, CliError> {
    if let Some(workspace_handle) = workspace_handle {
        if let Some(matched) =
            matching_surface_in_workspace(options, surface_handle, workspace_handle, window_handle)?
        {
            return Ok(matched);
        }
        return Err(CliError::new("Surface not found in window"));
    }
    let listed = call_control_command(
        options,
        "workspace.list",
        &serde_json::json!({"window_id": window_handle}),
    )?;
    for workspace in listed
        .get("workspaces")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
    {
        let Some(workspace_handle) = canonical_id_or_ref(workspace) else {
            continue;
        };
        if let Some(matched) = matching_surface_in_workspace(
            options,
            surface_handle,
            &workspace_handle,
            window_handle,
        )? {
            return Ok(matched);
        }
    }
    Err(CliError::new("Surface not found in window"))
}

/// `matchingSurfaceHandleInWorkspace` (CLI/cmux.swift:6397-6420): match the
/// handle against each row's `id`/`ref` (UUID-aware, else case-insensitive).
#[cfg(windows)]
fn matching_surface_in_workspace(
    options: &GlobalOptions,
    surface_handle: &str,
    workspace_handle: &str,
    window_handle: &str,
) -> Result<Option<String>, CliError> {
    let listed = call_control_command(
        options,
        "surface.list",
        &serde_json::json!({"workspace_id": workspace_handle, "window_id": window_handle}),
    )?;
    for surface in listed
        .get("surfaces")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
    {
        if item_matches_handle(surface, surface_handle) {
            return Ok(Some(
                canonical_id_or_ref(surface).unwrap_or_else(|| surface_handle.to_owned()),
            ));
        }
    }
    Ok(None)
}

#[cfg(windows)]
fn run_legacy_workspace_command(
    options: &GlobalOptions,
    command: &str,
    method: &str,
    params: &serde_json::Value,
) -> Result<(), CliError> {
    if command != "current-workspace"
        && command != "rename-window"
        && std::env::var_os("CMUX_QUIET").is_none()
    {
        let replacement = match command {
            "list-workspaces" => "workspace list",
            "new-workspace" => "workspace create",
            "close-workspace" => "workspace close",
            "select-workspace" => "workspace select",
            "rename-workspace" => "workspace rename",
            _ => command,
        };
        eprintln!("Warning: `{command}` is deprecated; use `cmux {replacement}` instead.");
    }
    let mut request_params = params.clone();
    let post_create_command = request_params
        .as_object_mut()
        .and_then(|params| params.remove("__post_create_command"))
        .and_then(|value| value.as_str().map(str::to_owned));
    normalize_workspace_params(options, method, &mut request_params)?;
    let has_layout = request_params.get("layout").is_some();
    let result = call_control_command(options, method, &request_params)?;
    if command == "close-workspace" {
        if let Some(workspace_id) = result
            .get("workspace_id")
            .and_then(serde_json::Value::as_str)
        {
            let _ = prune_tmux_compat_workspace_state(workspace_id);
        }
    }
    if command == "new-workspace" && !has_layout {
        if let Some(text) = post_create_command {
            let mut send_params = serde_json::Map::new();
            if let Some(workspace_id) = result.get("workspace_id") {
                send_params.insert("workspace_id".into(), workspace_id.clone());
            } else if let Some(workspace_ref) = result.get("workspace_ref") {
                send_params.insert("workspace_ref".into(), workspace_ref.clone());
            }
            if let Some(surface_id) = result.get("surface_id") {
                send_params.insert("surface_id".into(), surface_id.clone());
            } else if let Some(surface_ref) = result.get("surface_ref") {
                send_params.insert("surface_ref".into(), surface_ref.clone());
            }
            send_params.insert("text".into(), serde_json::json!(format!("{text}\r")));
            call_control_command(
                options,
                "surface.send_text",
                &serde_json::Value::Object(send_params),
            )?;
        }
    }
    if options.json_output && command != "new-workspace" {
        let mut formatted = result.clone();
        filter_id_format(
            &mut formatted,
            options.id_format.as_deref().unwrap_or("refs"),
        );
        println!("{}", serde_json::to_string(&formatted).unwrap_or_default());
    } else {
        println!(
            "{}",
            format_legacy_workspace_text(
                method,
                &result,
                options.id_format.as_deref().unwrap_or("refs"),
            )
        );
    }
    Ok(())
}

fn filter_id_format(value: &mut serde_json::Value, id_format: &str) {
    match value {
        serde_json::Value::Array(items) => {
            for item in items {
                filter_id_format(item, id_format);
            }
        }
        serde_json::Value::Object(object) => {
            for child in object.values_mut() {
                filter_id_format(child, id_format);
            }
            let keys = object.keys().cloned().collect::<Vec<_>>();
            let (plain_remove, singular_remove, singular_keep, plural_remove, plural_keep) =
                match id_format {
                    "refs" => ("id", "_id", "_ref", "_ids", "_refs"),
                    "uuids" => ("ref", "_ref", "_id", "_refs", "_ids"),
                    _ => return,
                };
            let plain_keep = if plain_remove == "id" { "ref" } else { "id" };
            if object.contains_key(plain_remove) && object.contains_key(plain_keep) {
                object.remove(plain_remove);
            }
            for key in keys {
                let paired = key
                    .strip_suffix(plural_remove)
                    .map(|prefix| format!("{prefix}{plural_keep}"))
                    .or_else(|| {
                        key.strip_suffix(singular_remove)
                            .map(|prefix| format!("{prefix}{singular_keep}"))
                    });
                if paired.is_some_and(|paired| object.contains_key(&paired)) {
                    object.remove(&key);
                }
            }
        }
        _ => {}
    }
}

#[cfg(windows)]
fn selected_id_or_ref<'a>(
    value: &'a serde_json::Value,
    id_key: &str,
    ref_key: &str,
) -> Option<&'a str> {
    value
        .get(id_key)
        .or_else(|| value.get("id"))
        .or_else(|| value.get(ref_key))
        .or_else(|| value.get("ref"))
        .and_then(serde_json::Value::as_str)
}

#[cfg(windows)]
fn resolve_respawn_workspace_ref(
    options: &GlobalOptions,
    object: &mut serde_json::Map<String, serde_json::Value>,
    workspace_ref: &str,
) -> Result<(), CliError> {
    let window_ids =
        if let Some(window_id) = object.get("window_id").and_then(serde_json::Value::as_str) {
            vec![window_id.to_string()]
        } else {
            call_control_command(options, "window.list", &serde_json::json!({}))?
                .get("windows")
                .and_then(serde_json::Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|window| {
                    window
                        .get("window_id")
                        .or_else(|| window.get("id"))
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_owned)
                })
                .collect()
        };
    for window_id in window_ids {
        let listed = call_control_command(
            options,
            "workspace.list",
            &serde_json::json!({"window_id":window_id}),
        )?;
        if let Some(id) = listed
            .get("workspaces")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .find(|workspace| {
                workspace
                    .get("workspace_ref")
                    .or_else(|| workspace.get("ref"))
                    .and_then(serde_json::Value::as_str)
                    == Some(workspace_ref)
            })
            .and_then(|workspace| {
                workspace
                    .get("workspace_id")
                    .or_else(|| workspace.get("id"))
                    .and_then(serde_json::Value::as_str)
            })
        {
            object.remove("workspace_ref");
            object.insert("workspace_id".into(), serde_json::json!(id));
            return Ok(());
        }
    }
    Err(CliError::new(format!(
        "Workspace ref not found: {workspace_ref}"
    )))
}

#[cfg(windows)]
fn normalize_workspace_params(
    options: &GlobalOptions,
    method: &str,
    params: &mut serde_json::Value,
) -> Result<(), CliError> {
    let Some(object) = params.as_object_mut() else {
        return Ok(());
    };
    object.remove("suppress_ambient_workspace");
    object.remove("suppress_ambient_window");
    if let Some(raw) = object
        .get("window_id")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .map(str::to_owned)
    {
        if uuid::Uuid::parse_str(&raw).is_ok() {
            object.insert("window_id".into(), serde_json::json!(raw));
        } else if let Ok(index) = raw.parse::<i64>() {
            object.remove("window_id");
            object.insert("window_index".into(), serde_json::json!(index));
        } else if raw.split_once(':').is_some_and(|(kind, index)| {
            kind.eq_ignore_ascii_case("window") && index.parse::<i64>().is_ok()
        }) {
            object.remove("window_id");
            object.insert("window_ref".into(), serde_json::json!(raw));
        } else {
            return Err(CliError::new(format!(
                "Invalid window handle: {raw} (expected UUID, ref like window:1, or index)"
            )));
        }
    }
    let window_index = object
        .remove("window_index")
        .and_then(|value| value.as_i64());
    let window_ref = object
        .get("window_ref")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned);
    if window_index.is_some() || window_ref.is_some() {
        let result = call_control_command(options, "window.list", &serde_json::json!({}))?;
        let windows = result
            .get("windows")
            .and_then(serde_json::Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let matched = windows
            .iter()
            .find(|window| {
                window_index.is_some_and(|index| {
                    window.get("index").and_then(serde_json::Value::as_i64) == Some(index)
                }) || window_ref.as_deref().is_some_and(|reference| {
                    window
                        .get("window_ref")
                        .or_else(|| window.get("ref"))
                        .and_then(serde_json::Value::as_str)
                        .is_some_and(|candidate| candidate.eq_ignore_ascii_case(reference))
                })
            })
            .ok_or_else(|| {
                CliError::new(if let Some(window_ref) = window_ref.as_deref() {
                    format!("Window not found: {window_ref}")
                } else {
                    "Window index not found".to_string()
                })
            })?;
        let handle = selected_id_or_ref(matched, "window_id", "window_ref")
            .ok_or_else(|| CliError::new("Window not found"))?;
        object.remove("window_ref");
        object.insert("window_id".into(), serde_json::json!(handle));
    }

    if !matches!(
        method,
        "workspace.close"
            | "workspace.select"
            | "workspace.rename"
            | "tab.action"
            | "surface.respawn"
    ) {
        return Ok(());
    }
    if let Some(raw) = object
        .get("workspace_id")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .map(str::to_owned)
    {
        if uuid::Uuid::parse_str(&raw).is_ok() {
            object.insert("workspace_id".into(), serde_json::json!(raw));
        } else if let Ok(index) = raw.parse::<i64>() {
            object.remove("workspace_id");
            object.insert("workspace_index".into(), serde_json::json!(index));
        } else if raw.split_once(':').is_some_and(|(kind, index)| {
            kind.eq_ignore_ascii_case("workspace") && index.parse::<i64>().is_ok()
        }) {
            object.remove("workspace_id");
            object.insert("workspace_ref".into(), serde_json::json!(raw));
        } else {
            return Err(CliError::new(format!(
                "Invalid workspace handle: {raw} (expected UUID, ref like workspace:1, or index)"
            )));
        }
    }
    let workspace_index = object
        .remove("workspace_index")
        .and_then(|value| value.as_i64());
    let workspace_ref = object
        .get("workspace_ref")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned);
    let resolve_current = object
        .remove("resolve_current_workspace")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    if object.get("workspace_id").is_some() && workspace_index.is_none() && workspace_ref.is_none()
    {
        return Ok(());
    }
    if resolve_current && workspace_index.is_none() && workspace_ref.is_none() {
        let mut current_params = serde_json::Map::new();
        if let Some(window_id) = object.get("window_id") {
            current_params.insert("window_id".into(), window_id.clone());
        }
        let current = call_control_command(
            options,
            "workspace.current",
            &serde_json::Value::Object(current_params),
        )?;
        let handle = selected_id_or_ref(&current, "workspace_id", "workspace_ref")
            .ok_or_else(|| CliError::new("No workspace selected"))?;
        if method == "surface.respawn" && uuid::Uuid::parse_str(handle).is_err() {
            let workspace_ref = handle.to_string();
            object.insert("workspace_ref".into(), serde_json::json!(workspace_ref));
            return resolve_respawn_workspace_ref(options, object, &workspace_ref);
        }
        object.insert("workspace_id".into(), serde_json::json!(handle));
        return Ok(());
    }
    if workspace_index.is_none() && workspace_ref.is_none() {
        return Ok(());
    }
    if workspace_index.is_none() {
        if let Some(workspace_ref) = workspace_ref.as_deref() {
            if method == "surface.respawn" {
                return resolve_respawn_workspace_ref(options, object, workspace_ref);
            }
            if object.get("window_id").is_none() && object.get("window_ref").is_none() {
                object.remove("workspace_ref");
                object.insert("workspace_id".into(), serde_json::json!(workspace_ref));
                return Ok(());
            }
        }
    }
    let mut list_params = serde_json::Map::new();
    if let Some(window_id) = object.get("window_id") {
        list_params.insert("window_id".into(), window_id.clone());
    }
    let result = call_control_command(
        options,
        "workspace.list",
        &serde_json::Value::Object(list_params),
    )?;
    let workspaces = result
        .get("workspaces")
        .and_then(serde_json::Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let matched = workspaces
        .iter()
        .find(|workspace| {
            workspace_index.is_some_and(|index| {
                workspace.get("index").and_then(serde_json::Value::as_i64) == Some(index)
            }) || workspace_ref.as_deref().is_some_and(|reference| {
                workspace
                    .get("workspace_ref")
                    .or_else(|| workspace.get("ref"))
                    .and_then(serde_json::Value::as_str)
                    == Some(reference)
            })
        })
        .ok_or_else(|| {
            let message = if workspace_index.is_some() {
                "Workspace index not found".to_string()
            } else if matches!(method, "tab.action" | "surface.respawn") {
                format!(
                    "Workspace ref not found: {}",
                    workspace_ref.as_deref().unwrap_or_default()
                )
            } else {
                "Workspace not found".to_string()
            };
            CliError::new(message)
        })?;
    let handle = selected_id_or_ref(matched, "workspace_id", "workspace_ref")
        .ok_or_else(|| CliError::new("Workspace not found"))?;
    if method == "surface.respawn" && uuid::Uuid::parse_str(handle).is_err() {
        let workspace_ref = handle.to_string();
        object.insert("workspace_ref".into(), serde_json::json!(workspace_ref));
        return resolve_respawn_workspace_ref(options, object, &workspace_ref);
    }
    object.remove("workspace_ref");
    object.insert("workspace_id".into(), serde_json::json!(handle));
    Ok(())
}

#[cfg(windows)]
fn normalize_lifecycle_surface_params(
    options: &GlobalOptions,
    method: &str,
    params: &mut serde_json::Value,
) -> Result<(), CliError> {
    let Some(object) = params.as_object_mut() else {
        return Ok(());
    };
    let suppress_surface = object
        .remove("suppress_ambient_surface")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    if let Some(raw) = object
        .get("surface_id")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .map(str::to_owned)
    {
        if uuid::Uuid::parse_str(&raw).is_ok() {
            object.insert("surface_id".into(), serde_json::json!(raw));
        } else if let Ok(index) = raw.parse::<i64>() {
            object.remove("surface_id");
            object.insert("surface_index".into(), serde_json::json!(index));
        } else if let Some((_, index)) = raw.split_once(':').filter(|(kind, index)| {
            (kind.eq_ignore_ascii_case("surface") || kind.eq_ignore_ascii_case("tab"))
                && index.parse::<i64>().is_ok()
        }) {
            object.remove("surface_id");
            object.insert(
                "surface_ref".into(),
                serde_json::json!(format!("surface:{index}")),
            );
        } else {
            return Err(CliError::new(format!(
                "Invalid surface handle: {raw} (expected UUID, ref like surface:1, or index)"
            )));
        }
    }
    let surface_index = object
        .remove("surface_index")
        .and_then(|value| value.as_i64());
    let surface_ref = object
        .get("surface_ref")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned);
    let surface_id = object
        .get("surface_id")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned);
    let target = surface_ref.as_deref().or(surface_id.as_deref());
    if surface_index.is_none() && target.is_none() {
        if suppress_surface {
            return Ok(());
        }
        if matches!(method, "tab.action" | "surface.respawn")
            && object.get("workspace_id").is_none()
        {
            let mut identify_params = serde_json::Map::new();
            if let Some(window_id) = object.get("window_id") {
                identify_params.insert("window_id".into(), window_id.clone());
            }
            let identified = call_control_command(
                options,
                "system.identify",
                &serde_json::Value::Object(identify_params),
            )?;
            let surface_id = identified
                .get("focused")
                .and_then(|focused| {
                    focused
                        .get("surface_id")
                        .or_else(|| focused.get("surface_ref"))
                })
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| CliError::new("Surface not found in window"))?;
            object.insert("surface_id".into(), serde_json::json!(surface_id));
        }
        return Ok(());
    }

    let window_scoped = object.get("window_id").is_some() || object.get("window_ref").is_some();
    let respawn_ref_requires_id = method == "surface.respawn"
        && surface_ref.is_some()
        && object.get("workspace_id").is_some();
    if surface_index.is_none() && !window_scoped && !respawn_ref_requires_id {
        if let Some(surface_ref) = surface_ref {
            object.remove("surface_ref");
            object.insert("surface_id".into(), serde_json::json!(surface_ref));
        }
        return Ok(());
    }

    let mut list_params = serde_json::Map::new();
    for key in ["workspace_id", "window_id"] {
        if let Some(value) = object.get(key) {
            list_params.insert(key.into(), value.clone());
        }
    }
    let listed = call_control_command(
        options,
        "surface.list",
        &serde_json::Value::Object(list_params),
    )?;
    let surfaces = listed
        .get("surfaces")
        .and_then(serde_json::Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let exact_ref_match = method == "surface.respawn" && !window_scoped && surface_ref.is_some();
    let matched = surfaces.iter().find(|surface| {
        surface_index.is_some_and(|index| {
            surface.get("index").and_then(serde_json::Value::as_i64) == Some(index)
        }) || target.is_some_and(|target| {
            ["surface_id", "id", "surface_ref", "ref"]
                .iter()
                .filter_map(|key| surface.get(*key).and_then(serde_json::Value::as_str))
                .any(|candidate| {
                    if exact_ref_match {
                        candidate == target
                    } else {
                        candidate.eq_ignore_ascii_case(target)
                    }
                })
        })
    });
    let matched = matched.ok_or_else(|| {
        let message = if surface_index.is_some() {
            "Surface index not found".to_string()
        } else if exact_ref_match {
            format!(
                "Surface ref not found: {}",
                surface_ref.as_deref().unwrap_or_default()
            )
        } else {
            "Surface not found in window".to_string()
        };
        CliError::new(message)
    })?;
    let handle = matched
        .get("surface_id")
        .or_else(|| matched.get("id"))
        .or_else(|| matched.get("surface_ref"))
        .or_else(|| matched.get("ref"))
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| CliError::new("Surface not found"))?;
    object.remove("surface_ref");
    object.insert("surface_id".into(), serde_json::json!(handle));
    Ok(())
}

#[cfg(windows)]
fn run_tmux_compat_command(options: &GlobalOptions, args: &[String]) -> Result<(), CliError> {
    let workspace_id = std::env::var(CMUX_WORKSPACE_ID_ENV).ok();
    let pane_id = std::env::var("TMUX_PANE")
        .ok()
        .or_else(|| std::env::var("CMUX_PANE_ID").ok());
    let launched_through_omx = std::env::var_os("CMUX_OMX_CMUX_BIN").is_some()
        || std::env::var("CMUX_AGENT_LAUNCH_KIND").as_deref() == Ok("omx");
    let result = cmux_cli::tmux_compat::run_tmux_compat(
        args,
        &cmux_cli::tmux_compat::TmuxCompatEnvironment {
            workspace_id: workspace_id.as_deref(),
            pane_id: pane_id.as_deref(),
            launched_through_omx,
        },
        |method, params| call_control_command(options, method, params),
    )?;
    if let Some(output) = result.output {
        println!("{output}");
    }
    Ok(())
}

#[cfg(windows)]
fn run_events_command(options: &GlobalOptions, command_args: &[String]) -> Result<(), CliError> {
    let mut command = events_stream_options(command_args)?;
    let mut printed_events = 0usize;
    let mut last_seq = command
        .params
        .get("after_seq")
        .and_then(serde_json::Value::as_u64);

    loop {
        let mut stopped_by_limit = false;
        let mut saw_frame = false;
        stream_control_command(options, "events.stream", &command.params, |frame| {
            saw_frame = true;
            let keep_reading =
                handle_events_frame(frame, &command, &mut printed_events, &mut last_seq)?;
            if !keep_reading {
                stopped_by_limit = true;
            }
            Ok(keep_reading)
        })?;

        if stopped_by_limit || !command.reconnect {
            break;
        }
        if let Some(seq) = last_seq {
            command
                .params
                .as_object_mut()
                .expect("events params are an object")
                .insert("after_seq".to_string(), serde_json::json!(seq));
        }
        if !saw_frame {
            thread::sleep(Duration::from_millis(500));
        }
    }
    Ok(())
}

#[cfg(windows)]
struct EventsCommandOptions {
    params: serde_json::Value,
    print_ack: bool,
    cursor_file: Option<PathBuf>,
    event_limit: Option<usize>,
    reconnect: bool,
}

#[cfg(windows)]
fn events_stream_options(command_args: &[String]) -> Result<EventsCommandOptions, CliError> {
    let mut params = serde_json::Map::new();
    let mut names = Vec::new();
    let mut categories = Vec::new();
    let mut print_ack = true;
    let mut cursor_file = None;
    let mut event_limit = None;
    let mut reconnect = false;
    let mut index = 0;
    while index < command_args.len() {
        let arg = command_args[index].as_str();
        match arg {
            "--after" | "--after-seq" => {
                index += 1;
                let value = command_args
                    .get(index)
                    .ok_or_else(|| CliError::new(format!("{arg} requires a value")))?;
                let seq = value
                    .parse::<u64>()
                    .map_err(|_| CliError::new(format!("{arg} requires an integer")))?;
                params.insert("after_seq".to_string(), serde_json::json!(seq));
            }
            "--cursor-file" => {
                index += 1;
                let path = command_args
                    .get(index)
                    .ok_or_else(|| CliError::new("--cursor-file requires a path"))?;
                cursor_file = Some(PathBuf::from(path));
                if let Ok(raw) = std::fs::read_to_string(path) {
                    if let Ok(seq) = raw.trim().parse::<u64>() {
                        params.insert("after_seq".to_string(), serde_json::json!(seq));
                    }
                }
            }
            "--name" => {
                index += 1;
                let value = command_args
                    .get(index)
                    .ok_or_else(|| CliError::new("--name requires a value"))?;
                names.push(value.clone());
            }
            "--category" => {
                index += 1;
                let value = command_args
                    .get(index)
                    .ok_or_else(|| CliError::new("--category requires a value"))?;
                categories.push(value.clone());
            }
            "--limit" => {
                index += 1;
                let value = command_args
                    .get(index)
                    .ok_or_else(|| CliError::new("--limit requires a value"))?;
                let limit = value
                    .parse::<usize>()
                    .map_err(|_| CliError::new("--limit requires an integer"))?;
                if limit == 0 {
                    return Err(CliError::new("--limit must be greater than zero"));
                }
                event_limit = Some(limit);
                params.insert("limit".to_string(), serde_json::json!(limit));
            }
            "--no-ack" => {
                print_ack = false;
            }
            "--no-heartbeat" | "--no-heartbeats" => {
                params.insert("include_heartbeats".to_string(), serde_json::json!(false));
            }
            "--reconnect" => {
                reconnect = true;
            }
            other => {
                return Err(CliError::new(format!("unknown events option: {other}")));
            }
        }
        index += 1;
    }
    if !names.is_empty() {
        params.insert("names".to_string(), serde_json::json!(names));
    }
    if !categories.is_empty() {
        params.insert("categories".to_string(), serde_json::json!(categories));
    }
    Ok(EventsCommandOptions {
        params: serde_json::Value::Object(params),
        print_ack,
        cursor_file,
        event_limit,
        reconnect,
    })
}

#[cfg(windows)]
fn handle_events_frame(
    frame: &str,
    command: &EventsCommandOptions,
    printed_events: &mut usize,
    last_seq: &mut Option<u64>,
) -> Result<bool, CliError> {
    let value = serde_json::from_str::<serde_json::Value>(frame).ok();
    let frame_type = value
        .as_ref()
        .and_then(|value| value.get("type"))
        .and_then(serde_json::Value::as_str);
    let is_event = frame_type == Some("event");
    if is_event
        && command
            .event_limit
            .is_some_and(|limit| *printed_events >= limit)
    {
        return Ok(false);
    }
    if frame_type != Some("ack") || command.print_ack {
        println!("{frame}");
        std::io::stdout()
            .flush()
            .map_err(|error| CliError::new(format!("failed to flush stdout: {error}")))?;
    }
    if is_event {
        *printed_events = (*printed_events).saturating_add(1);
        if let Some(seq) = value
            .as_ref()
            .and_then(|value| value.get("seq"))
            .and_then(serde_json::Value::as_u64)
        {
            *last_seq = Some(seq);
            if let Some(path) = command.cursor_file.as_ref() {
                write_event_cursor(path, seq)?;
            }
        }
        if command
            .event_limit
            .is_some_and(|limit| *printed_events >= limit)
        {
            return Ok(false);
        }
    }
    Ok(true)
}

#[cfg(windows)]
fn write_event_cursor(path: &Path, seq: u64) -> Result<(), CliError> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|error| {
                CliError::new(format!(
                    "failed to create cursor-file directory {}: {error}",
                    parent.display()
                ))
            })?;
        }
    }
    std::fs::write(path, seq.to_string()).map_err(|error| {
        CliError::new(format!(
            "failed to write cursor file {}: {error}",
            path.display()
        ))
    })
}

fn format_control_result(method: &str, result: &serde_json::Value) -> String {
    match method {
        "workspace.set_progress"
        | "workspace.clear_progress"
        | "workspace.set_status"
        | "workspace.clear_status"
        | "workspace.set_agent_pid"
        | "workspace.clear_agent_pid"
        | "workspace.report_pr"
        | "workspace.report_review"
        | "workspace.clear_pr"
        | "workspace.report_meta"
        | "workspace.clear_meta"
        | "workspace.report_meta_block"
        | "workspace.clear_meta_block"
        | "workspace.reset_sidebar"
        | "workspace.log"
        | "workspace.clear_log" => "OK".to_string(),
        "surface.report_tty" | "surface.report_shell_state" => "OK".to_string(),
        "surface.read_text" => result
            .get("text")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_string(),
        "browser.url.get" => result
            .get("url")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_string(),
        "browser.is_webview_focused" => result
            .get("focused")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false)
            .to_string(),
        "browser.focus_webview"
        | "browser.reload"
        | "config.reload"
        | "notification.dismiss"
        | "notification.mark_read"
        | "notification.clear"
        | "notification.create"
        | "session.restore_previous"
        | "surface.clear_history"
        | "surface.refresh_all"
        | "surface.trigger_flash"
        | "pane.swap"
        | "pane.break"
        | "pane.join" => "OK".to_string(),
        "window.list" => format_window_entries(result),
        "workspace.list" => format_workspace_entries(result),
        "workspace.current" => control_handle(result, "workspace").to_string(),
        "workspace.create" | "workspace.close" | "workspace.select" | "workspace.rename" => {
            format!("OK {}", control_handle(result, "workspace"))
        }
        "pane.list" => format_pane_entries(result),
        "pane.surfaces" => format_pane_surface_entries(result),
        "window.displays" => format_display_entries(result),
        "window.display" => format_window_display_result(result),
        "window.current" => result
            .get("window_id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_string(),
        "notification.list" => format_notification_entries(result),
        "notification.open" | "notification.jump_to_unread" => {
            format_notification_navigation(result)
        }
        "right_sidebar" => {
            if result.get("mode").is_some() {
                serde_json::to_string(result).unwrap_or_default()
            } else {
                "OK".to_string()
            }
        }
        "workspace.list_status" => format_status_entries(result),
        "workspace.list_meta" => format_metadata_entries(result),
        "workspace.list_meta_blocks" => format_metadata_blocks(result),
        "workspace.list_log" => format_log_entries(result),
        "workspace.sidebar_state" => format_sidebar_state(result),
        "workspace.reorder" => format_workspace_reorder(result),
        "workspace.reorder_many" => format_workspace_reorder_items(result),
        "workspace.move_to_window" => format!(
            "OK workspace={} window={}",
            control_handle(result, "workspace"),
            control_handle(result, "window"),
        ),
        "surface.reorder" => format!(
            "OK surface={} pane={} workspace={}",
            control_handle(result, "surface"),
            control_handle(result, "pane"),
            control_handle(result, "workspace"),
        ),
        "surface.move" => format!(
            "OK surface={} pane={} workspace={} window={}",
            control_handle(result, "surface"),
            control_handle(result, "pane"),
            control_handle(result, "workspace"),
            control_handle(result, "window"),
        ),
        "surface.split_off" | "surface.drag_to_split" => format!(
            "OK surface={} pane={} workspace={} window={}",
            control_handle(result, "surface"),
            control_handle(result, "pane"),
            control_handle(result, "workspace"),
            control_handle(result, "window"),
        ),
        "pane.last" => format!("OK {}", control_handle(result, "pane")),
        "pane.focus" => format!("OK {}", control_handle(result, "pane")),
        "workspace.last" => format!("OK {}", control_handle(result, "workspace")),
        "pane.resize" => format!("OK {}", control_handle(result, "pane")),
        _ => serde_json::to_string(result).unwrap_or_default(),
    }
}

fn format_lifecycle_text(
    method: &str,
    result: &serde_json::Value,
    id_format: &str,
    requested_action: Option<&str>,
) -> String {
    if method == "surface.respawn" {
        return "OK".to_string();
    }
    let mut fields = vec![format!(
        "action={}",
        requested_action
            .or_else(|| result.get("action").and_then(serde_json::Value::as_str))
            .unwrap_or_default()
    )];
    push_lifecycle_id_alias_field(
        &mut fields,
        result,
        "tab",
        ["tab_id", "surface_id"],
        ["tab_ref", "surface_ref"],
        id_format,
    );
    push_lifecycle_id_field(
        &mut fields,
        result,
        "workspace",
        "workspace_id",
        "workspace_ref",
        id_format,
    );
    for key in ["closed", "full_width_tab_mode"] {
        if let Some(value) = result.get(key) {
            fields.push(format!("{key}={value}"));
        }
    }
    push_lifecycle_id_alias_field(
        &mut fields,
        result,
        "created",
        ["created_tab_id", "created_surface_id"],
        ["created_tab_ref", "created_surface_ref"],
        id_format,
    );
    push_lifecycle_id_field(
        &mut fields,
        result,
        "created_workspace",
        "created_workspace_id",
        "created_workspace_ref",
        id_format,
    );
    format!("OK {}", fields.join(" "))
}

fn push_lifecycle_id_alias_field(
    fields: &mut Vec<String>,
    result: &serde_json::Value,
    label: &str,
    id_keys: [&str; 2],
    ref_keys: [&str; 2],
    id_format: &str,
) {
    let id = id_keys
        .iter()
        .find_map(|key| result.get(*key).and_then(serde_json::Value::as_str));
    let reference = ref_keys
        .iter()
        .find_map(|key| result.get(*key).and_then(serde_json::Value::as_str));
    let Some(mut handle) = format_id_pair(id, reference, id_format) else {
        return;
    };
    if matches!(label, "tab" | "created")
        && handle
            .get(.."surface:".len())
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("surface:"))
    {
        handle = format!("tab:{}", &handle["surface:".len()..]);
    }
    fields.push(format!("{label}={handle}"));
}

fn push_lifecycle_id_field(
    fields: &mut Vec<String>,
    result: &serde_json::Value,
    label: &str,
    id_key: &str,
    ref_key: &str,
    id_format: &str,
) {
    push_lifecycle_id_alias_field(
        fields,
        result,
        label,
        [id_key, id_key],
        [ref_key, ref_key],
        id_format,
    );
}

fn format_workspace_entries(result: &serde_json::Value) -> String {
    format_workspace_entries_with_mode(result, "refs")
}

fn format_workspace_entries_with_mode(result: &serde_json::Value, id_format: &str) -> String {
    let Some(workspaces) = result
        .get("workspaces")
        .and_then(serde_json::Value::as_array)
    else {
        return "No workspaces".to_string();
    };
    if workspaces.is_empty() {
        return "No workspaces".to_string();
    }
    workspaces
        .iter()
        .map(|workspace| {
            let selected = workspace
                .get("selected")
                .and_then(serde_json::Value::as_bool)
                == Some(true);
            let prefix = if selected { "* " } else { "  " };
            let mut line = format!("{prefix}{}", workspace_row_handle(workspace, id_format));
            if let Some(title) = workspace
                .get("title")
                .and_then(serde_json::Value::as_str)
                .filter(|title| !title.is_empty())
            {
                line.push_str("  ");
                line.push_str(title);
            }
            if let Some(remote) = workspace
                .get("remote")
                .and_then(serde_json::Value::as_object)
                .filter(|remote| {
                    remote.get("enabled").and_then(serde_json::Value::as_bool) == Some(true)
                })
            {
                let transport = remote
                    .get("transport")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("remote");
                let state = remote
                    .get("state")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("unknown");
                line.push_str(&format!("  [{transport}:{state}]"));
            }
            if selected {
                line.push_str("  [selected]");
            }
            line
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn workspace_row_handle(workspace: &serde_json::Value, id_format: &str) -> String {
    format_id_pair(
        workspace.get("id").and_then(serde_json::Value::as_str),
        workspace.get("ref").and_then(serde_json::Value::as_str),
        id_format,
    )
    .unwrap_or_else(|| "unknown".to_string())
}

fn workspace_handle(result: &serde_json::Value, id_format: &str) -> String {
    format_id_pair(
        result
            .get("workspace_id")
            .and_then(serde_json::Value::as_str),
        result
            .get("workspace_ref")
            .and_then(serde_json::Value::as_str),
        id_format,
    )
    .unwrap_or_else(|| "unknown".to_string())
}

fn format_id_pair(id: Option<&str>, reference: Option<&str>, id_format: &str) -> Option<String> {
    match id_format {
        "uuids" => id.or(reference).map(str::to_owned),
        "both" => match (reference, id) {
            (Some(reference), Some(id)) => Some(format!("{reference} ({id})")),
            (Some(reference), None) => Some(reference.to_string()),
            (None, Some(id)) => Some(id.to_string()),
            (None, None) => None,
        },
        _ => reference.or(id).map(str::to_owned),
    }
}

fn format_legacy_workspace_text(
    method: &str,
    result: &serde_json::Value,
    id_format: &str,
) -> String {
    match method {
        "workspace.list" => format_workspace_entries_with_mode(result, id_format),
        "workspace.current" => workspace_handle(result, id_format),
        "workspace.create" | "workspace.close" | "workspace.select" | "workspace.rename" => {
            format!("OK {}", workspace_handle(result, id_format))
        }
        _ => format_control_result(method, result),
    }
}

fn prune_tmux_compat_workspace_value(store: &mut serde_json::Value, workspace_id: &str) -> bool {
    let mut changed = false;
    for key in ["mainVerticalLayouts", "lastSplitSurface"] {
        if let Some(map) = store
            .get_mut(key)
            .and_then(serde_json::Value::as_object_mut)
        {
            changed |= map.remove(workspace_id).is_some();
        }
    }
    changed
}

fn prune_tmux_compat_workspace_state(workspace_id: &str) -> Result<(), std::io::Error> {
    let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) else {
        return Ok(());
    };
    let path = PathBuf::from(home)
        .join(".cmuxterm")
        .join("tmux-compat-store.json");
    prune_tmux_compat_workspace_state_at(&path, workspace_id)
}

fn prune_tmux_compat_workspace_state_at(
    path: &Path,
    workspace_id: &str,
) -> Result<(), std::io::Error> {
    let Ok(contents) = std::fs::read_to_string(path) else {
        return Ok(());
    };
    let Ok(mut store) = serde_json::from_str::<serde_json::Value>(&contents) else {
        return Ok(());
    };
    if prune_tmux_compat_workspace_value(&mut store, workspace_id) {
        std::fs::write(
            path,
            serde_json::to_vec(&store).map_err(std::io::Error::other)?,
        )?;
    }
    Ok(())
}

fn format_workspace_reorder(result: &serde_json::Value) -> String {
    if result.get("dry_run").and_then(serde_json::Value::as_bool) == Some(true) {
        return format_workspace_reorder_items(result);
    }
    format!(
        "OK workspace={} window={} index={}",
        control_handle(result, "workspace"),
        control_handle(result, "window"),
        control_index(result),
    )
}

fn format_workspace_reorder_items(result: &serde_json::Value) -> String {
    let prefix = if result
        .get("dry_run")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
    {
        "OK plan"
    } else {
        "OK"
    };
    let plan = result.get("plan").and_then(serde_json::Value::as_array);
    let items: Vec<&serde_json::Value> = plan
        .map(|items| items.iter().collect())
        .unwrap_or_else(|| vec![result]);
    items
        .into_iter()
        .map(|item| {
            format!(
                "{prefix} workspace={} window={} index={}",
                control_handle(item, "workspace"),
                control_handle(item, "window"),
                control_index(item),
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn control_handle<'a>(result: &'a serde_json::Value, kind: &str) -> &'a str {
    result
        .get(format!("{kind}_ref"))
        .and_then(serde_json::Value::as_str)
        .or_else(|| {
            result
                .get(format!("{kind}_id"))
                .and_then(serde_json::Value::as_str)
        })
        .unwrap_or("unknown")
}

fn control_index(result: &serde_json::Value) -> String {
    result
        .get("to_index")
        .or_else(|| result.get("index"))
        .map(|value| match value {
            serde_json::Value::String(value) => value.clone(),
            value => value.to_string(),
        })
        .unwrap_or_else(|| "?".to_string())
}

fn entry_handle(entry: &serde_json::Value) -> &str {
    entry
        .get("ref")
        .and_then(serde_json::Value::as_str)
        .or_else(|| entry.get("id").and_then(serde_json::Value::as_str))
        .unwrap_or("unknown")
}

fn format_pane_entries(result: &serde_json::Value) -> String {
    let Some(panes) = result
        .get("panes")
        .and_then(serde_json::Value::as_array)
        .filter(|panes| !panes.is_empty())
    else {
        return "No panes".to_string();
    };
    panes
        .iter()
        .map(|pane| {
            let focused = pane.get("focused").and_then(serde_json::Value::as_bool) == Some(true);
            let count = pane
                .get("surface_count")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0);
            format!(
                "{}{}  [{} surface{}]{}",
                if focused { "* " } else { "  " },
                entry_handle(pane),
                count,
                if count == 1 { "" } else { "s" },
                if focused { "  [focused]" } else { "" },
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn format_pane_surface_entries(result: &serde_json::Value) -> String {
    let Some(surfaces) = result
        .get("surfaces")
        .and_then(serde_json::Value::as_array)
        .filter(|surfaces| !surfaces.is_empty())
    else {
        return "No surfaces in pane".to_string();
    };
    surfaces
        .iter()
        .map(|surface| {
            let selected =
                surface.get("selected").and_then(serde_json::Value::as_bool) == Some(true);
            let title = surface
                .get("title")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            format!(
                "{}{}  {}{}",
                if selected { "* " } else { "  " },
                entry_handle(surface),
                title,
                if selected { "  [selected]" } else { "" },
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn format_display_entries(result: &serde_json::Value) -> String {
    let displays = result.get("displays").and_then(serde_json::Value::as_array);
    let Some(displays) = displays.filter(|displays| !displays.is_empty()) else {
        return "No displays found.".into();
    };
    displays
        .iter()
        .map(|display| {
            let index = display
                .get("index")
                .and_then(serde_json::Value::as_i64)
                .unwrap_or(-1);
            let name = display
                .get("name")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("(unknown)");
            let main = if display.get("main").and_then(serde_json::Value::as_bool) == Some(true) {
                "  (main)"
            } else {
                ""
            };
            format!("{index}: {name}{main}")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn format_window_display_result(result: &serde_json::Value) -> String {
    let display = result
        .get("display")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    let moved = result
        .get("moved")
        .and_then(serde_json::Value::as_array)
        .map_or(0, Vec::len);
    let suffix = if moved == 1 { "" } else { "s" };
    format!("Moved {moved} window{suffix} to {display}.")
}

fn format_window_entries(result: &serde_json::Value) -> String {
    let windows = result
        .as_array()
        .or_else(|| result.get("windows").and_then(serde_json::Value::as_array));
    let Some(windows) = windows else {
        return "No windows".to_string();
    };
    if windows.is_empty() {
        return "No windows".to_string();
    }
    windows
        .iter()
        .map(|window| {
            let selected = if window
                .get("key")
                .or_else(|| window.get("selected"))
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false)
            {
                "*"
            } else {
                " "
            };
            let index = window
                .get("index")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or_default();
            let id = string_field(window, "id");
            let selected_workspace = window
                .get("selected_workspace_id")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("none");
            let workspace_count = window
                .get("workspace_count")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or_default();
            format!(
                "{selected} {index}: {id} selected_workspace={selected_workspace} workspaces={workspace_count}"
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn format_notification_entries(result: &serde_json::Value) -> String {
    let Some(rows) = result.as_array() else {
        return "No notifications".to_string();
    };
    if rows.is_empty() {
        return "No notifications".to_string();
    }
    rows.iter()
        .enumerate()
        .map(|(index, row)| {
            let surface = row
                .get("surface_id")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("none");
            let read = if row
                .get("is_read")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false)
            {
                "read"
            } else {
                "unread"
            };
            let created_at = row
                .get("created_at")
                .map(|value| {
                    value
                        .as_str()
                        .map(str::to_owned)
                        .unwrap_or_else(|| value.to_string())
                })
                .unwrap_or_default();
            format!(
                "{index}:{}|{}|{surface}|{read}|{}|{}|{}|{created_at}|{}",
                string_field(row, "id"),
                string_field(row, "workspace_id"),
                string_field(row, "title"),
                string_field(row, "subtitle"),
                string_field(row, "body"),
                string_field(row, "tab_title")
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn format_notification_navigation(result: &serde_json::Value) -> String {
    if result.get("opened").and_then(serde_json::Value::as_bool) != Some(true) {
        return "OK".to_string();
    }
    let mut parts = vec!["OK".to_string()];
    for (reference, id) in [
        ("workspace_ref", "workspace_id"),
        ("surface_ref", "surface_id"),
    ] {
        if let Some(handle) = result
            .get(reference)
            .or_else(|| result.get(id))
            .and_then(serde_json::Value::as_str)
        {
            parts.push(handle.to_string());
        }
    }
    parts.join(" ")
}

#[cfg(test)]
mod control_result_tests {
    use super::*;

    #[test]
    fn terminal_text_result_prints_plain_text() {
        let result = serde_json::json!({"text": "first\nsecond", "surface_ref": "surface:1"});
        assert_eq!(
            format_control_result("surface.read_text", &result),
            "first\nsecond"
        );
    }

    #[test]
    fn legacy_workspace_results_use_canonical_plain_output() {
        let list = serde_json::json!({"workspaces":[
            {"id":"uuid-a","ref":"workspace:1","title":"Build","selected":true,"remote":{"enabled":false}},
            {"id":"uuid-b","ref":"workspace:2","title":"Remote","selected":false,"remote":{"enabled":true}}
        ]});
        assert_eq!(
            format_control_result("workspace.list", &list),
            "* workspace:1  Build  [selected]\n  workspace:2  Remote  [remote:unknown]"
        );
        assert_eq!(
            format_control_result("workspace.list", &serde_json::json!({"workspaces":[]})),
            "No workspaces"
        );
        let current = serde_json::json!({"workspace_ref":"workspace:2","workspace_id":"uuid"});
        assert_eq!(
            format_control_result("workspace.current", &current),
            "workspace:2"
        );
        for method in [
            "workspace.create",
            "workspace.close",
            "workspace.select",
            "workspace.rename",
        ] {
            assert_eq!(format_control_result(method, &current), "OK workspace:2");
        }
    }

    #[test]
    fn legacy_workspace_json_honors_recursive_id_format() {
        let original = serde_json::json!({"workspace_id":"uuid","workspace_ref":"workspace:1","workspace":{"surface_id":"surface-uuid","surface_ref":"surface:1"}});
        let mut refs = original.clone();
        filter_id_format(&mut refs, "refs");
        assert_eq!(
            refs,
            serde_json::json!({"workspace_ref":"workspace:1","workspace":{"surface_ref":"surface:1"}})
        );
        let mut uuids = original;
        filter_id_format(&mut uuids, "uuids");
        assert_eq!(
            uuids,
            serde_json::json!({"workspace_id":"uuid","workspace":{"surface_id":"surface-uuid"}})
        );
    }

    #[test]
    fn verifier_real_workspace_schema_and_text_id_modes_are_canonical() {
        let list = serde_json::json!({"workspaces":[
            {"id":"uuid-a","ref":"workspace:1","title":"Local","selected":true,
             "remote":{"enabled":false,"transport":null,"state":"disconnected"}},
            {"id":"uuid-b","ref":"workspace:2","title":"Remote","selected":false,
             "remote":{"enabled":true,"transport":"ssh","state":"connected"}}
        ]});
        assert_eq!(
            format_workspace_entries(&list),
            "* workspace:1  Local  [selected]\n  workspace:2  Remote  [ssh:connected]"
        );

        let response = serde_json::json!({"workspace_id":"uuid-a","workspace_ref":"workspace:1"});
        assert_eq!(workspace_handle(&response, "refs"), "workspace:1");
        assert_eq!(workspace_handle(&response, "uuids"), "uuid-a");
        assert_eq!(workspace_handle(&response, "both"), "workspace:1 (uuid-a)");
        for method in [
            "workspace.current",
            "workspace.close",
            "workspace.select",
            "workspace.rename",
        ] {
            assert_eq!(
                format_legacy_workspace_text(method, &response, "both"),
                if method == "workspace.current" {
                    "workspace:1 (uuid-a)"
                } else {
                    "OK workspace:1 (uuid-a)"
                }
            );
        }
    }

    #[test]
    fn verifier_tmux_workspace_prune_removes_only_closed_workspace_state() {
        let mut store = serde_json::json!({
            "buffers":{"default":"keep"}, "hooks":{"after":"keep"},
            "mainVerticalLayouts":{"closed":{"main":"x"},"keep":{"main":"y"}},
            "lastSplitSurface":{"closed":"surface-a","keep":"surface-b"}
        });
        prune_tmux_compat_workspace_value(&mut store, "closed");
        assert!(store["mainVerticalLayouts"].get("closed").is_none());
        assert!(store["lastSplitSurface"].get("closed").is_none());
        assert_eq!(store["buffers"]["default"], "keep");
        assert_eq!(store["mainVerticalLayouts"]["keep"]["main"], "y");

        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("tmux-compat-store.json");
        std::fs::write(&path, serde_json::to_vec(&store).unwrap()).unwrap();
        prune_tmux_compat_workspace_state_at(&path, "keep").unwrap();
        let persisted: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert!(persisted["mainVerticalLayouts"].get("keep").is_none());
        assert!(persisted["lastSplitSurface"].get("keep").is_none());
        assert_eq!(persisted["buffers"]["default"], "keep");
    }

    #[test]
    fn verifier_json_id_filter_matches_frozen_plain_and_plural_pairs_recursively() {
        let original = serde_json::json!({
            "id":"root-id", "ref":"root:1",
            "workspace_id":"workspace-id", "workspace_ref":"workspace:1",
            "surface_ids":["surface-a"], "surface_refs":["surface:1"],
            "nested":[{"id":"child-id","ref":"child:1","pane_ids":["pane-a"],"pane_refs":["pane:1"]}]
        });
        let mut refs = original.clone();
        filter_id_format(&mut refs, "refs");
        assert_eq!(
            refs,
            serde_json::json!({
                "ref":"root:1", "workspace_ref":"workspace:1", "surface_refs":["surface:1"],
                "nested":[{"ref":"child:1","pane_refs":["pane:1"]}]
            })
        );
        let mut uuids = original.clone();
        filter_id_format(&mut uuids, "uuids");
        assert_eq!(
            uuids,
            serde_json::json!({
                "id":"root-id", "workspace_id":"workspace-id", "surface_ids":["surface-a"],
                "nested":[{"id":"child-id","pane_ids":["pane-a"]}]
            })
        );
        let mut both = original.clone();
        filter_id_format(&mut both, "both");
        assert_eq!(both, original);
    }

    #[test]
    fn legacy_browser_aliases_keep_their_plain_output_contracts() {
        let result = serde_json::json!({"url": "https://example.com", "focused": true});
        assert_eq!(
            format_control_result("browser.url.get", &result),
            "https://example.com"
        );
        assert_eq!(
            format_control_result("browser.is_webview_focused", &result),
            "true"
        );
        assert_eq!(
            format_control_result("browser.focus_webview", &result),
            "OK"
        );
        assert_eq!(format_control_result("browser.reload", &result), "OK");
    }

    #[test]
    fn reorder_workspace_keeps_canonical_plain_output() {
        let result = serde_json::json!({
            "workspace_id": "ws-2",
            "workspace_ref": "workspace:1",
            "window_id": "win-1",
            "window_ref": "window:1",
            "index": 0,
            "dry_run": false,
            "plan": [{
                "workspace_id": "ws-2",
                "workspace_ref": "workspace:1",
                "window_id": "win-1",
                "window_ref": "window:1",
                "from_index": 1,
                "to_index": 0,
            }],
        });
        assert_eq!(
            format_control_result("workspace.reorder", &result),
            "OK workspace=workspace:1 window=window:1 index=0"
        );

        let dry_run = serde_json::json!({
            "dry_run": true,
            "plan": [{
                "workspace_ref": "workspace:1",
                "window_ref": "window:1",
                "to_index": 0,
            }],
        });
        assert_eq!(
            format_control_result("workspace.reorder", &dry_run),
            "OK plan workspace=workspace:1 window=window:1 index=0"
        );

        let many = serde_json::json!({
            "dry_run": false,
            "plan": [
                {"workspace_ref": "workspace:1", "window_ref": "window:1", "to_index": 0},
                {"workspace_ref": "workspace:2", "window_ref": "window:1", "to_index": 1},
            ],
        });
        assert_eq!(
            format_control_result("workspace.reorder_many", &many),
            "OK workspace=workspace:1 window=window:1 index=0\nOK workspace=workspace:2 window=window:1 index=1"
        );
    }

    #[test]
    fn reorder_surface_keeps_canonical_plain_output() {
        let result = serde_json::json!({
            "surface_ref": "surface:1",
            "pane_ref": "pane:2",
            "workspace_ref": "workspace:1",
        });
        assert_eq!(
            format_control_result("surface.reorder", &result),
            "OK surface=surface:1 pane=pane:2 workspace=workspace:1"
        );
    }

    #[test]
    fn move_surface_keeps_canonical_plain_output() {
        let result = serde_json::json!({
            "surface_ref": "surface:1",
            "pane_ref": "pane:2",
            "workspace_ref": "workspace:3",
            "window_ref": "window:1",
        });
        assert_eq!(
            format_control_result("surface.move", &result),
            "OK surface=surface:1 pane=pane:2 workspace=workspace:3 window=window:1"
        );
    }

    #[test]
    fn move_workspace_to_window_keeps_canonical_plain_output() {
        let result = serde_json::json!({
            "workspace_ref": "workspace:2",
            "window_ref": "window:3",
        });
        assert_eq!(
            format_control_result("workspace.move_to_window", &result),
            "OK workspace=workspace:2 window=window:3"
        );
    }

    #[test]
    fn split_off_keeps_canonical_plain_output() {
        let result = serde_json::json!({
            "surface_ref": "surface:2",
            "pane_ref": "pane:2",
            "workspace_ref": "workspace:1",
            "window_ref": "window:1",
        });
        assert_eq!(
            format_control_result("surface.split_off", &result),
            "OK surface=surface:2 pane=pane:2 workspace=workspace:1 window=window:1"
        );
        assert_eq!(
            format_control_result("surface.drag_to_split", &result),
            "OK surface=surface:2 pane=pane:2 workspace=workspace:1 window=window:1"
        );
    }

    #[test]
    fn swap_pane_keeps_canonical_plain_output() {
        assert_eq!(
            format_control_result("pane.swap", &serde_json::json!({"pane_ref": "pane:1"})),
            "OK"
        );
    }

    #[test]
    fn break_pane_keeps_canonical_plain_output() {
        assert_eq!(
            format_control_result(
                "pane.break",
                &serde_json::json!({"surface_ref": "surface:1"})
            ),
            "OK"
        );
    }

    #[test]
    fn join_pane_keeps_canonical_plain_output() {
        assert_eq!(
            format_control_result(
                "pane.join",
                &serde_json::json!({"surface_ref": "surface:1"})
            ),
            "OK"
        );
    }

    #[test]
    fn last_pane_keeps_canonical_handle_summary() {
        assert_eq!(
            format_control_result("pane.last", &serde_json::json!({"pane_ref": "pane:2"})),
            "OK pane:2"
        );
    }

    #[test]
    fn focus_pane_keeps_canonical_handle_summary() {
        assert_eq!(
            format_control_result("pane.focus", &serde_json::json!({"pane_ref": "pane:2"})),
            "OK pane:2"
        );
    }

    #[test]
    fn last_window_keeps_canonical_handle_summary() {
        assert_eq!(
            format_control_result(
                "workspace.last",
                &serde_json::json!({"workspace_ref":"workspace:2"}),
            ),
            "OK workspace:2"
        );
    }

    #[test]
    fn pane_list_outputs_match_canonical_text_rows() {
        assert_eq!(
            format_control_result(
                "pane.list",
                &serde_json::json!({"panes":[
                    {"id":"pane-a", "ref":"pane:1", "surface_count":1, "focused":true},
                    {"id":"pane-b", "ref":"pane:2", "surface_count":2, "focused":false}
                ]})
            ),
            "* pane:1  [1 surface]  [focused]\n  pane:2  [2 surfaces]"
        );
        assert_eq!(
            format_control_result(
                "pane.surfaces",
                &serde_json::json!({"surfaces":[
                    {"id":"surface-a", "ref":"surface:1", "title":"Shell", "selected":true},
                    {"id":"surface-b", "ref":"surface:2", "title":"Logs", "selected":false}
                ]})
            ),
            "* surface:1  Shell  [selected]\n  surface:2  Logs"
        );
        assert_eq!(
            format_control_result("pane.list", &serde_json::json!({"panes":[]})),
            "No panes"
        );
        assert_eq!(
            format_control_result("pane.surfaces", &serde_json::json!({"surfaces":[]})),
            "No surfaces in pane"
        );
    }

    #[test]
    fn resize_pane_keeps_canonical_handle_summary() {
        assert_eq!(
            format_control_result("pane.resize", &serde_json::json!({"pane_ref": "pane:3"})),
            "OK pane:3"
        );
    }

    #[test]
    fn restore_session_prints_plain_ok() {
        assert_eq!(
            format_control_result("session.restore_previous", &serde_json::json!({})),
            "OK"
        );
    }

    #[test]
    fn clear_terminal_history_prints_plain_ok() {
        assert_eq!(
            format_control_result("surface.clear_history", &serde_json::json!({})),
            "OK"
        );
    }

    #[test]
    fn trigger_flash_prints_plain_ok() {
        assert_eq!(
            format_control_result("surface.trigger_flash", &serde_json::json!({})),
            "OK"
        );
    }

    #[test]
    fn reload_config_prints_plain_ok() {
        assert_eq!(
            format_control_result("config.reload", &serde_json::json!({})),
            "OK"
        );
    }

    #[test]
    fn window_introspection_keeps_canonical_plain_text() {
        let result = serde_json::json!({
            "windows": [{
                "index": 0,
                "id": "window-id",
                "key": true,
                "visible": true,
                "selected_workspace_id": "workspace-id",
                "workspace_count": 2
            }]
        });
        assert_eq!(
            format_control_result("window.list", &result),
            "* 0: window-id selected_workspace=workspace-id workspaces=2"
        );
        assert_eq!(
            format_control_result(
                "window.current",
                &serde_json::json!({"window_id": "window-id"})
            ),
            "window-id"
        );
    }

    #[test]
    fn window_display_commands_keep_canonical_plain_text() {
        let displays = serde_json::json!({"displays":[
            {"index":0,"name":"LG HDR 4K","main":true},
            {"index":1,"name":"Sidecar","main":false}
        ]});
        assert_eq!(
            format_control_result("window.displays", &displays),
            "0: LG HDR 4K  (main)\n1: Sidecar"
        );
        assert_eq!(
            format_control_result(
                "window.display",
                &serde_json::json!({"display":"LG HDR 4K","moved":["window-1"]})
            ),
            "Moved 1 window to LG HDR 4K."
        );
    }

    #[test]
    fn notification_list_keeps_canonical_plain_text() {
        let result = serde_json::json!([{
            "id": "notification-1",
            "workspace_id": "workspace-1",
            "surface_id": "surface-1",
            "is_read": false,
            "title": "Build",
            "subtitle": "Agent",
            "body": "Needs input",
            "created_at": 42,
            "tab_title": "API"
        }]);
        assert_eq!(
            format_control_result("notification.list", &result),
            "0:notification-1|workspace-1|surface-1|unread|Build|Agent|Needs input|42|API"
        );
    }

    #[test]
    fn notification_mutations_print_plain_ok() {
        for method in [
            "notification.dismiss",
            "notification.mark_read",
            "notification.clear",
            "notification.create",
            "surface.refresh_all",
        ] {
            assert_eq!(format_control_result(method, &serde_json::json!({})), "OK");
        }
    }

    #[test]
    fn notification_navigation_prints_canonical_summary() {
        let opened = serde_json::json!({
            "opened": true,
            "workspace_ref": "workspace:2",
            "surface_ref": "surface:3"
        });
        assert_eq!(
            format_control_result("notification.open", &opened),
            "OK workspace:2 surface:3"
        );
        assert_eq!(
            format_control_result(
                "notification.jump_to_unread",
                &serde_json::json!({"opened": false})
            ),
            "OK"
        );
    }

    #[test]
    fn right_sidebar_prints_state_only_for_mode_queries() {
        assert_eq!(
            format_control_result(
                "right_sidebar",
                &serde_json::json!({"visible": true, "mode": "sessions"})
            ),
            r#"{"mode":"sessions","visible":true}"#
        );
        assert_eq!(
            format_control_result("right_sidebar", &serde_json::json!({"ok": true})),
            "OK"
        );
    }
}

fn format_status_entries(result: &serde_json::Value) -> String {
    result
        .get("status_entries")
        .and_then(serde_json::Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .map(|entry| {
                    let mut line = format!(
                        "{}={}",
                        string_field(entry, "key"),
                        string_field(entry, "value")
                    );
                    append_i64_field(&mut line, entry, "priority");
                    line
                })
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}

fn format_metadata_entries(result: &serde_json::Value) -> String {
    result
        .get("metadata_entries")
        .and_then(serde_json::Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .map(|entry| {
                    let mut line = format!(
                        "{}={}",
                        string_field(entry, "key"),
                        string_field(entry, "value")
                    );
                    append_string_field(&mut line, entry, "icon");
                    append_string_field(&mut line, entry, "color");
                    append_string_field(&mut line, entry, "url");
                    append_i64_field(&mut line, entry, "priority");
                    append_string_field(&mut line, entry, "format");
                    line
                })
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}

fn format_metadata_blocks(result: &serde_json::Value) -> String {
    result
        .get("metadata_blocks")
        .and_then(serde_json::Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .map(|entry| {
                    let mut line = format!(
                        "{}={}",
                        string_field(entry, "key"),
                        string_field(entry, "markdown")
                    );
                    append_i64_field(&mut line, entry, "priority");
                    line
                })
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}

fn format_log_entries(result: &serde_json::Value) -> String {
    result
        .get("log_entries")
        .and_then(serde_json::Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .map(|entry| {
                    format!(
                        "[{}] {}",
                        string_field(entry, "level"),
                        string_field(entry, "message")
                    )
                })
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}

fn format_sidebar_state(result: &serde_json::Value) -> String {
    let mut lines = Vec::new();
    if let Some(workspace_id) = result
        .get("workspace_id")
        .and_then(serde_json::Value::as_str)
    {
        lines.push(format!("workspace_id={workspace_id}"));
    }
    if let Some(workspace_ref) = result
        .get("workspace_ref")
        .and_then(serde_json::Value::as_str)
    {
        lines.push(format!("workspace_ref={workspace_ref}"));
    }
    lines.push(format!("ports={}", format_ports(result.get("ports"))));
    lines.push(format!(
        "agent_pid_count={}",
        usize_field(result, "agent_pid_count")
    ));
    lines.push(format!(
        "status_count={}",
        usize_field(result, "status_count")
    ));
    lines.push(format!(
        "metadata_count={}",
        usize_field(result, "metadata_count")
    ));
    lines.push(format!(
        "metadata_block_count={}",
        usize_field(result, "metadata_block_count")
    ));
    lines.push(format!("log_count={}", usize_field(result, "log_count")));
    lines.push(format!(
        "progress={}",
        format_progress(result.get("progress"))
    ));

    let status_lines = format_status_entries(result);
    if !status_lines.is_empty() {
        lines.push(status_lines);
    }
    let metadata_lines = format_metadata_entries(result);
    if !metadata_lines.is_empty() {
        lines.push(metadata_lines);
    }
    let block_lines = format_metadata_blocks(result);
    if !block_lines.is_empty() {
        lines.push(block_lines);
    }
    let log_lines = format_log_entries(result);
    if !log_lines.is_empty() {
        lines.push(log_lines);
    }
    lines.join("\n")
}

fn format_ports(ports: Option<&serde_json::Value>) -> String {
    let Some(values) = ports.and_then(serde_json::Value::as_array) else {
        return "none".to_string();
    };
    let ports: Vec<String> = values
        .iter()
        .filter_map(serde_json::Value::as_u64)
        .map(|value| value.to_string())
        .collect();
    if ports.is_empty() {
        "none".to_string()
    } else {
        ports.join(",")
    }
}

fn format_progress(progress: Option<&serde_json::Value>) -> String {
    let Some(progress) = progress else {
        return "none".to_string();
    };
    if progress.is_null() {
        return "none".to_string();
    }
    let value = progress
        .get("value")
        .and_then(serde_json::Value::as_f64)
        .unwrap_or(0.0)
        .clamp(0.0, 1.0);
    let label = progress
        .get("label")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    match label {
        Some(label) => format!("{value:.2} {label}"),
        None => format!("{value:.2}"),
    }
}

fn string_field<'a>(entry: &'a serde_json::Value, key: &str) -> &'a str {
    entry
        .get(key)
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
}

fn usize_field(entry: &serde_json::Value, key: &str) -> usize {
    entry
        .get(key)
        .and_then(serde_json::Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .unwrap_or(0)
}

fn append_string_field(line: &mut String, entry: &serde_json::Value, key: &str) {
    if let Some(value) = entry
        .get(key)
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        line.push_str(&format!(" {key}={value}"));
    }
}

fn append_i64_field(line: &mut String, entry: &serde_json::Value, key: &str) {
    if let Some(value) = entry.get(key).and_then(serde_json::Value::as_i64) {
        line.push_str(&format!(" {key}={value}"));
    }
}

#[cfg(windows)]
fn run_ssh_command(options: &GlobalOptions, command_args: &[String]) -> Result<(), CliError> {
    let unique_id = uuid::Uuid::new_v4().to_string();
    let remote_relay_port = generated_relay_port(&unique_id);
    let local_proxy_port = reserve_loopback_port()?;
    let startup_script_path = startup_script_path(&unique_id);
    let plan = cmux_cli::build_ssh_command_plan(
        command_args,
        cmux_cli::SshCommandBuildOptions {
            unique_id: unique_id.clone(),
            remote_relay_port,
            startup_script_path: startup_script_path.to_string_lossy().to_string(),
            existing_ghostty_shell_features: std::env::var("GHOSTTY_SHELL_FEATURES").ok(),
        },
    )?;
    if let Some(parent) = startup_script_path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| {
            CliError::new(format!(
                "failed to create cmux ssh startup directory: {error}"
            ))
        })?;
    }
    std::fs::write(&startup_script_path, &plan.ssh_startup_script).map_err(|error| {
        CliError::new(format!("failed to write cmux ssh startup script: {error}"))
    })?;

    let workspace_create_params = serde_json::json!({
        "initial_terminal_command": plan.ssh_terminal_command,
        "initial_terminal_environment": plan.ssh_env_overrides,
    });
    let created = call_control_command(options, "workspace.create", &workspace_create_params)?;
    let workspace_id = created
        .get("workspace_id")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_string();
    let workspace_ref = created
        .get("workspace_ref")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_string();
    let workspace_selector = if workspace_id.trim().is_empty() {
        serde_json::json!({ "workspace_ref": workspace_ref })
    } else {
        serde_json::json!({ "workspace_id": workspace_id })
    };
    let mut remote_params = serde_json::Map::new();
    if let Some(object) = workspace_selector.as_object() {
        remote_params.extend(object.clone());
    }
    remote_params.insert("transport".to_string(), serde_json::json!("ssh"));
    remote_params.insert(
        "destination".to_string(),
        serde_json::json!(plan.destination),
    );
    remote_params.insert(
        "port".to_string(),
        plan.port
            .map_or(serde_json::Value::Null, serde_json::Value::from),
    );
    remote_params.insert(
        "local_proxy_port".to_string(),
        serde_json::json!(local_proxy_port),
    );
    remote_params.insert(
        "persistent_daemon_slot".to_string(),
        serde_json::json!(plan.persistent_daemon_slot),
    );
    remote_params.insert(
        "remoteDaemonRelayPort".to_string(),
        serde_json::json!(plan.remote_relay_port),
    );
    if let Some(identity_file) = plan.identity_file.as_ref() {
        remote_params.insert(
            "identity_file".to_string(),
            serde_json::json!(identity_file),
        );
    }
    remote_params.insert(
        "ssh_options".to_string(),
        serde_json::json!(plan.effective_ssh_options),
    );
    remote_params.insert("auto_connect".to_string(), serde_json::json!(true));
    let configured = call_control_command(
        options,
        "workspace.remote.configure",
        &serde_json::Value::Object(remote_params),
    )?;

    let mut output = match created {
        serde_json::Value::Object(object) => object,
        _ => serde_json::Map::new(),
    };
    output.insert(
        "remote_relay_port".to_string(),
        serde_json::json!(plan.remote_relay_port),
    );
    output.insert(
        "local_proxy_port".to_string(),
        serde_json::json!(local_proxy_port),
    );
    output.insert(
        "ssh_command".to_string(),
        serde_json::json!(plan.ssh_command),
    );
    output.insert(
        "ssh_terminal_command".to_string(),
        serde_json::json!(plan.ssh_terminal_command),
    );
    output.insert(
        "ssh_startup_command".to_string(),
        serde_json::json!(plan.ssh_startup_command),
    );
    output.insert(
        "ssh_startup_command_text".to_string(),
        serde_json::json!(plan.ssh_startup_command_text),
    );
    output.insert(
        "ssh_env_overrides".to_string(),
        serde_json::json!(plan.ssh_env_overrides),
    );
    if let Some(remote) = configured.get("remote") {
        output.insert("remote".to_string(), remote.clone());
    }
    println!(
        "{}",
        serde_json::to_string(&serde_json::Value::Object(output)).unwrap_or_default()
    );
    Ok(())
}

#[cfg(windows)]
fn call_control_command(
    options: &GlobalOptions,
    method: &str,
    params: &serde_json::Value,
) -> Result<serde_json::Value, CliError> {
    let (socket_path, password) = resolved_control_connection(options)?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| CliError::new(format!("failed to start async runtime: {error}")))?;
    let result = runtime.block_on(cmux_cli::transport::run_rpc(
        &socket_path,
        password.as_deref(),
        method,
        params,
    ))?;
    Ok(result)
}

#[cfg(windows)]
fn resolved_control_connection(
    options: &GlobalOptions,
) -> Result<(String, Option<String>), CliError> {
    let default_addr = cmux_ipc::control_pipe_path("cmux")
        .map_err(|error| CliError::new(format!("invalid default socket name: {error}")))?;
    let env_socket_path = std::env::var("CMUX_SOCKET_PATH").ok();
    let env_socket = std::env::var("CMUX_SOCKET").ok();
    let resolution = cmux_cli::resolve_socket_path(
        options.explicit_socket_path.as_deref(),
        cmux_cli::EnvView {
            socket_path: env_socket_path.as_deref(),
            socket: env_socket.as_deref(),
        },
        &default_addr,
    )?;

    let env_password = std::env::var("CMUX_SOCKET_PASSWORD").ok();
    let local_app_data = std::env::var("LOCALAPPDATA").ok();
    let file_password = cmux_cli::read_password_file(local_app_data.as_deref());
    let password = cmux_ipc::resolve_password(cmux_ipc::PasswordSources {
        explicit: options.socket_password.as_deref(),
        env: env_password.as_deref(),
        file: file_password.as_deref(),
        keychain: None,
    });
    Ok((resolution.path, password))
}

#[cfg(windows)]
fn stream_control_command(
    options: &GlobalOptions,
    method: &str,
    params: &serde_json::Value,
    on_frame: impl FnMut(&str) -> Result<bool, CliError>,
) -> Result<(), CliError> {
    let (socket_path, password) = resolved_control_connection(options)?;

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| CliError::new(format!("failed to start async runtime: {error}")))?;
    runtime.block_on(cmux_cli::transport::stream_rpc_with_handler(
        &socket_path,
        password.as_deref(),
        method,
        params,
        on_frame,
    ))
}

#[cfg(windows)]
fn reserve_loopback_port() -> Result<u16, CliError> {
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .map_err(|error| CliError::new(format!("failed to reserve local proxy port: {error}")))?;
    listener
        .local_addr()
        .map(|addr| addr.port())
        .map_err(|error| CliError::new(format!("failed to inspect local proxy port: {error}")))
}

#[cfg(windows)]
fn startup_script_path(unique_id: &str) -> PathBuf {
    std::env::temp_dir().join(format!("cmux-ssh-startup-{unique_id}.sh"))
}

#[cfg(windows)]
fn generated_relay_port(unique_id: &str) -> u16 {
    let mut hash = 0u32;
    for byte in unique_id.as_bytes() {
        hash = hash.wrapping_mul(33).wrapping_add(u32::from(*byte));
    }
    20_000 + (hash % 30_000) as u16
}

/// On non-Windows targets the named-pipe transport is unavailable, so socket
/// commands cannot run. (The CLI is only shipped on Windows for this port; this
/// keeps the crate buildable on other CI targets.)
#[cfg(not(windows))]
fn run_rpc_command(_options: &GlobalOptions, _command_args: &[String]) -> Result<(), CliError> {
    Err(CliError::new(
        "socket commands are only supported on Windows in this build",
    ))
}

#[cfg(not(windows))]
fn run_feed_hook_command(_options: &GlobalOptions, _args: &[String]) -> Result<(), CliError> {
    Err(CliError::new(
        "Feed hook socket bridging is only supported on Windows in this build",
    ))
}

#[cfg(not(windows))]
fn run_control_command(
    _options: &GlobalOptions,
    _method: &str,
    _params: &serde_json::Value,
) -> Result<(), CliError> {
    Err(CliError::new(
        "socket commands are only supported on Windows in this build",
    ))
}

#[cfg(not(windows))]
fn run_tmux_compat_command(_options: &GlobalOptions, _args: &[String]) -> Result<(), CliError> {
    Err(CliError::new(
        "tmux compatibility commands are only supported on Windows in this build",
    ))
}

#[cfg(not(windows))]
fn run_events_command(_options: &GlobalOptions, _command_args: &[String]) -> Result<(), CliError> {
    Err(CliError::new(
        "socket commands are only supported on Windows in this build",
    ))
}

#[cfg(not(windows))]
fn run_ssh_command(_options: &GlobalOptions, _command_args: &[String]) -> Result<(), CliError> {
    Err(CliError::new(
        "ssh workspace bootstrap is only supported on Windows in this build",
    ))
}

#[cfg(all(test, windows))]
mod events_command_tests {
    use super::*;

    #[test]
    fn events_stream_options_reads_cursor_and_tracks_client_semantics() {
        let cursor = std::env::temp_dir().join(format!(
            "cmux-events-cursor-test-{}.seq",
            uuid::Uuid::new_v4()
        ));
        std::fs::write(&cursor, "41").expect("cursor seed");
        let args = vec![
            "--cursor-file".to_string(),
            cursor.to_string_lossy().to_string(),
            "--limit".to_string(),
            "2".to_string(),
            "--category".to_string(),
            "workspace".to_string(),
            "--no-ack".to_string(),
            "--no-heartbeats".to_string(),
            "--reconnect".to_string(),
        ];

        let options = events_stream_options(&args).expect("options");

        assert_eq!(options.params["after_seq"], serde_json::json!(41));
        assert_eq!(options.params["limit"], serde_json::json!(2));
        assert_eq!(
            options.params["categories"],
            serde_json::json!(["workspace"])
        );
        assert_eq!(
            options.params["include_heartbeats"],
            serde_json::json!(false)
        );
        assert_eq!(options.event_limit, Some(2));
        assert!(!options.print_ack);
        assert!(options.reconnect);
        assert_eq!(options.cursor_file, Some(cursor.clone()));
        let _ = std::fs::remove_file(cursor);
    }

    #[test]
    fn handle_events_frame_updates_cursor_and_stops_at_event_limit() {
        let cursor = std::env::temp_dir().join(format!(
            "cmux-events-cursor-write-test-{}.seq",
            uuid::Uuid::new_v4()
        ));
        let command = EventsCommandOptions {
            params: serde_json::json!({}),
            print_ack: false,
            cursor_file: Some(cursor.clone()),
            event_limit: Some(1),
            reconnect: false,
        };
        let mut printed_events = 0usize;
        let mut last_seq = None;

        assert!(handle_events_frame(
            r#"{"type":"ack","resume":{"latest_seq":9}}"#,
            &command,
            &mut printed_events,
            &mut last_seq,
        )
        .expect("ack"));
        assert_eq!(printed_events, 0);
        assert_eq!(last_seq, None);

        assert!(!handle_events_frame(
            r#"{"type":"event","seq":42,"name":"workspace.selected"}"#,
            &command,
            &mut printed_events,
            &mut last_seq,
        )
        .expect("event"));
        assert_eq!(printed_events, 1);
        assert_eq!(last_seq, Some(42));
        assert_eq!(std::fs::read_to_string(&cursor).expect("cursor"), "42");
        let _ = std::fs::remove_file(cursor);
    }
}
