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
    GlobalOptions, ParseOutcome, CMUX_WORKSPACE_ID_ENV,
};

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
        DispatchPlan::RunControl(control) => {
            let ambient_workspace_id = std::env::var(CMUX_WORKSPACE_ID_ENV).ok();
            let control = control
                .with_ambient_workspace_id(ambient_workspace_id.as_deref())
                .with_window_id(options.window_id.as_deref());
            run_control_command(options, &control.method, &control.params)
        }
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
    if is_event {
        if command
            .event_limit
            .is_some_and(|limit| *printed_events >= limit)
        {
            return Ok(false);
        }
    }
    if !(frame_type == Some("ack") && !command.print_ack) {
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
        | "surface.trigger_flash" => "OK".to_string(),
        "window.list" => format_window_entries(result),
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
        _ => serde_json::to_string(result).unwrap_or_default(),
    }
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
