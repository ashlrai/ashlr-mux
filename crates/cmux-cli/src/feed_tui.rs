use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::invocation::CliError;

pub const OPEN_TUI_CORE_VERSION: &str = "0.1.106";
pub const FEED_TUI_USAGE: &str = "Usage: cmux feed tui [--opentui|--legacy]";
const OPEN_TUI_SOURCE: &str = include_str!("../../../Resources/feed-tui/index.ts");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeedTuiImplementation {
    Automatic,
    OpenTui,
    Legacy,
    Help,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeedTuiLaunchInputs {
    pub interactive: bool,
    pub bun_path: String,
    pub source_path: String,
    pub cwd: String,
    pub socket_path: String,
    pub socket_password: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeedTuiLaunchPlan {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: String,
    pub environment: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyFeedOption {
    pub id: String,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyFeedQuestion {
    pub multi_select: bool,
    pub options: Vec<LegacyFeedOption>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyFeedItem {
    pub id: String,
    pub request_id: String,
    pub source: String,
    pub kind: String,
    pub title: String,
    pub default_mode: Option<String>,
    pub questions: Vec<LegacyFeedQuestion>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LegacyFeedKey {
    Enter,
    Once,
    Always,
    All,
    Bypass,
    Deny,
    AutoAccept,
    Manual,
    Ultraplan,
    Feedback(String),
    Number(usize),
}

#[derive(Debug, Clone, PartialEq)]
pub enum LegacyFeedAction {
    Request {
        method: &'static str,
        params: serde_json::Value,
    },
}

pub fn legacy_feed_item(value: &serde_json::Value) -> Option<LegacyFeedItem> {
    let object = value.as_object()?;
    if object.get("status")?.as_str()? != "pending" {
        return None;
    }
    let questions = object
        .get("questions")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|question| {
            let question = question.as_object()?;
            let options = question
                .get("options")
                .and_then(serde_json::Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|option| {
                    let option = option.as_object()?;
                    Some(LegacyFeedOption {
                        id: option.get("id")?.as_str()?.to_string(),
                        label: option.get("label")?.as_str()?.to_string(),
                    })
                })
                .collect();
            Some(LegacyFeedQuestion {
                multi_select: question
                    .get("multi_select")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false),
                options,
            })
        })
        .collect();
    Some(LegacyFeedItem {
        id: object.get("id")?.as_str()?.to_string(),
        request_id: object.get("request_id")?.as_str()?.to_string(),
        source: object.get("source")?.as_str()?.to_string(),
        kind: object.get("kind")?.as_str()?.to_string(),
        title: object
            .get("title")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_else(|| {
                object
                    .get("kind")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("Feed")
            })
            .to_string(),
        default_mode: object
            .get("default_mode")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string),
        questions,
    })
}

pub fn legacy_action(
    item: &LegacyFeedItem,
    key: LegacyFeedKey,
    selected_labels: &[String],
) -> Option<LegacyFeedAction> {
    let (method, params) = match item.kind.as_str() {
        "permissionRequest" => {
            let mode = match key {
                LegacyFeedKey::Enter | LegacyFeedKey::Once => "once",
                LegacyFeedKey::Always => "always",
                LegacyFeedKey::All if item.source != "hermes-agent" => "all",
                LegacyFeedKey::Bypass
                    if !matches!(item.source.as_str(), "codex" | "claude" | "hermes-agent") =>
                {
                    "bypass"
                }
                LegacyFeedKey::Deny => "deny",
                _ => return None,
            };
            (
                "feed.permission.reply",
                serde_json::json!({"request_id":item.request_id,"mode":mode}),
            )
        }
        "exitPlan" => {
            if let LegacyFeedKey::Feedback(feedback) = key {
                return Some(LegacyFeedAction::Request {
                    method: "feed.exit_plan.reply",
                    params: serde_json::json!({
                        "request_id":item.request_id,
                        "mode":"deny",
                        "feedback":feedback
                    }),
                });
            }
            let mode = match key {
                LegacyFeedKey::Enter => item.default_mode.as_deref().unwrap_or("manual"),
                LegacyFeedKey::Always | LegacyFeedKey::AutoAccept => "autoAccept",
                LegacyFeedKey::Manual => "manual",
                LegacyFeedKey::Ultraplan => "ultraplan",
                LegacyFeedKey::Bypass
                    if !matches!(item.source.as_str(), "codex" | "claude" | "hermes-agent") =>
                {
                    "bypassPermissions"
                }
                LegacyFeedKey::Deny => "deny",
                _ => return None,
            };
            (
                "feed.exit_plan.reply",
                serde_json::json!({"request_id":item.request_id,"mode":mode}),
            )
        }
        "question" => {
            let selections = if !selected_labels.is_empty() {
                selected_labels.to_vec()
            } else if let LegacyFeedKey::Number(index) = key {
                vec![item
                    .questions
                    .first()?
                    .options
                    .get(index.checked_sub(1)?)?
                    .label
                    .clone()]
            } else if key == LegacyFeedKey::Enter {
                item.questions
                    .iter()
                    .filter_map(|question| {
                        question.options.first().map(|option| option.label.clone())
                    })
                    .collect()
            } else {
                return None;
            };
            (
                "feed.question.reply",
                serde_json::json!({"request_id":item.request_id,"selections":selections}),
            )
        }
        _ => return None,
    };
    Some(LegacyFeedAction::Request { method, params })
}

pub fn parse_feed_tui_args(args: &[String]) -> Result<FeedTuiImplementation, CliError> {
    let mut implementation = FeedTuiImplementation::Automatic;
    for argument in args {
        match argument.as_str() {
            "--opentui" => {
                if implementation == FeedTuiImplementation::Legacy {
                    return Err(CliError::new(
                        "cmux feed tui: choose only one TUI implementation",
                    ));
                }
                implementation = FeedTuiImplementation::OpenTui;
            }
            "--legacy" => {
                if implementation == FeedTuiImplementation::OpenTui {
                    return Err(CliError::new(
                        "cmux feed tui: choose only one TUI implementation",
                    ));
                }
                implementation = FeedTuiImplementation::Legacy;
            }
            "--help" | "-h" => return Ok(FeedTuiImplementation::Help),
            argument => {
                return Err(CliError::new(format!(
                    "cmux feed tui: unknown argument {argument}"
                )))
            }
        }
    }
    Ok(implementation)
}

pub fn build_open_tui_launch_plan(
    inputs: &FeedTuiLaunchInputs,
) -> Result<FeedTuiLaunchPlan, CliError> {
    if !inputs.interactive {
        return Err(CliError::new(
            "cmux feed tui requires an interactive terminal",
        ));
    }
    let mut environment = BTreeMap::from([
        ("CMUX_SOCKET_PATH".to_string(), inputs.socket_path.clone()),
        ("CMUX_FEED_TUI_PATH".to_string(), "opentui".to_string()),
        ("OTUI_USE_CONSOLE".to_string(), "0".to_string()),
        ("OTUI_USE_ALTERNATE_SCREEN".to_string(), "1".to_string()),
    ]);
    if let Some(password) = &inputs.socket_password {
        environment.insert("CMUX_SOCKET_PASSWORD".to_string(), password.clone());
    }
    Ok(FeedTuiLaunchPlan {
        program: inputs.bun_path.clone(),
        args: vec![inputs.source_path.clone()],
        cwd: inputs.cwd.clone(),
        environment,
    })
}

pub fn resolve_bun_executable(override_path: Option<&str>, home: &Path) -> Option<PathBuf> {
    if let Some(path) = override_path.map(str::trim).filter(|path| !path.is_empty()) {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Some(path);
        }
    }
    if let Some(path) = std::env::var_os("PATH") {
        for directory in std::env::split_paths(&path) {
            for name in ["bun.exe", "bun"] {
                let candidate = directory.join(name);
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
    }
    [
        home.join(".bun/bin/bun.exe"),
        home.join(".bun/bin/bun"),
        home.join(".local/bin/bun.exe"),
        home.join(".local/bin/bun"),
    ]
    .into_iter()
    .find(|path| path.is_file())
}

pub fn prepare_open_tui_app(home: &Path, bun_path: &Path) -> Result<PathBuf, CliError> {
    let app_directory = home.join(".cmuxterm").join("feed-tui-opentui");
    fs::create_dir_all(&app_directory)
        .map_err(|error| CliError::new(format!("failed to prepare OpenTUI Feed: {error}")))?;
    let package_path = app_directory.join("package.json");
    let source_path = app_directory.join("index.ts");
    let package = format!(
        "{{\n  \"private\": true,\n  \"type\": \"module\",\n  \"dependencies\": {{\n    \"@opentui/core\": \"{OPEN_TUI_CORE_VERSION}\"\n  }}\n}}\n"
    );
    write_if_changed(&package_path, package.as_bytes())?;
    write_if_changed(&source_path, OPEN_TUI_SOURCE.as_bytes())?;

    let installed = app_directory
        .join("node_modules")
        .join("@opentui")
        .join("core")
        .join("package.json");
    if installed_open_tui_version(&installed).as_deref() != Some(OPEN_TUI_CORE_VERSION) {
        let output = Command::new(bun_path)
            .args(["install", "--silent"])
            .current_dir(&app_directory)
            .output()
            .map_err(|error| CliError::new(format!("failed to run bun install: {error}")))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
            return Err(CliError::new(if !stderr.is_empty() {
                stderr
            } else if !stdout.is_empty() {
                stdout
            } else {
                "bun install failed".to_string()
            }));
        }
    }
    Ok(source_path)
}

pub fn run_open_tui_launch_plan(plan: &FeedTuiLaunchPlan) -> Result<(), CliError> {
    let mut command = Command::new(&plan.program);
    command.args(&plan.args).current_dir(&plan.cwd);
    command.env_remove("CMUX_SOCKET");
    command.envs(&plan.environment);
    let status = command
        .status()
        .map_err(|error| CliError::new(format!("failed to start OpenTUI Feed: {error}")))?;
    if status.success() || status.code() == Some(130) {
        return Ok(());
    }
    Err(CliError::new(format!(
        "OpenTUI Feed exited with status {}",
        status.code().unwrap_or(1)
    )))
}

fn write_if_changed(path: &Path, contents: &[u8]) -> Result<(), CliError> {
    if fs::read(path).ok().as_deref() == Some(contents) {
        return Ok(());
    }
    fs::write(path, contents)
        .map_err(|error| CliError::new(format!("failed to write {}: {error}", path.display())))
}

fn installed_open_tui_version(path: &Path) -> Option<String> {
    let value = serde_json::from_slice::<serde_json::Value>(&fs::read(path).ok()?).ok()?;
    value.get("version")?.as_str().map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_canonical_feed_tui_implementation_flags() {
        assert_eq!(
            parse_feed_tui_args(&[]).unwrap(),
            FeedTuiImplementation::Automatic
        );
        assert_eq!(
            parse_feed_tui_args(&["--opentui".into()]).unwrap(),
            FeedTuiImplementation::OpenTui
        );
        assert_eq!(
            parse_feed_tui_args(&["--legacy".into()]).unwrap(),
            FeedTuiImplementation::Legacy
        );
        assert_eq!(
            parse_feed_tui_args(&["--help".into()]).unwrap(),
            FeedTuiImplementation::Help
        );
        assert_eq!(
            parse_feed_tui_args(&["--legacy".into(), "--opentui".into()])
                .unwrap_err()
                .message,
            "cmux feed tui: choose only one TUI implementation"
        );
        assert_eq!(
            parse_feed_tui_args(&["--wat".into()]).unwrap_err().message,
            "cmux feed tui: unknown argument --wat"
        );
    }

    #[test]
    fn launch_plan_requires_interactive_terminal_and_carries_socket_environment() {
        let error = build_open_tui_launch_plan(&FeedTuiLaunchInputs {
            interactive: false,
            bun_path: "C:/bun.exe".into(),
            source_path: "C:/feed/index.ts".into(),
            cwd: "C:/repo".into(),
            socket_path: r"\\.\pipe\cmux.sock".into(),
            socket_password: Some("secret".into()),
        })
        .unwrap_err();
        assert_eq!(
            error.message,
            "cmux feed tui requires an interactive terminal"
        );

        let plan = build_open_tui_launch_plan(&FeedTuiLaunchInputs {
            interactive: true,
            bun_path: "C:/bun.exe".into(),
            source_path: "C:/feed/index.ts".into(),
            cwd: "C:/repo".into(),
            socket_path: r"\\.\pipe\cmux.sock".into(),
            socket_password: Some("secret".into()),
        })
        .unwrap();
        assert_eq!(plan.program, "C:/bun.exe");
        assert_eq!(plan.args, ["C:/feed/index.ts"]);
        assert_eq!(plan.environment["CMUX_SOCKET_PATH"], r"\\.\pipe\cmux.sock");
        assert_eq!(plan.environment["CMUX_SOCKET_PASSWORD"], "secret");
        assert_eq!(plan.environment["CMUX_FEED_TUI_PATH"], "opentui");
    }

    #[test]
    fn stages_the_embedded_source_without_reinstalling_the_pinned_dependency() {
        let home = std::env::temp_dir().join(format!("cmux-feed-tui-{}", uuid::Uuid::new_v4()));
        let installed =
            home.join(".cmuxterm/feed-tui-opentui/node_modules/@opentui/core/package.json");
        fs::create_dir_all(installed.parent().unwrap()).unwrap();
        fs::write(
            &installed,
            format!(r#"{{"version":"{OPEN_TUI_CORE_VERSION}"}}"#),
        )
        .unwrap();

        let source = prepare_open_tui_app(&home, Path::new("missing-bun.exe")).unwrap();
        assert_eq!(source, home.join(".cmuxterm/feed-tui-opentui/index.ts"));
        assert_eq!(fs::read_to_string(source).unwrap(), OPEN_TUI_SOURCE);
        let _ = fs::remove_dir_all(home);
    }

    #[test]
    fn legacy_actions_map_permission_plan_and_question_replies() {
        let permission = legacy_feed_item(&serde_json::json!({
            "id":"p1","request_id":"r1","workstream_id":"w1","source":"claude",
            "kind":"permissionRequest","status":"pending","title":"Write"
        }))
        .unwrap();
        assert_eq!(
            legacy_action(&permission, LegacyFeedKey::Enter, &[]).unwrap(),
            LegacyFeedAction::Request {
                method: "feed.permission.reply",
                params: serde_json::json!({"request_id":"r1","mode":"once"}),
            }
        );

        let plan = legacy_feed_item(&serde_json::json!({
            "id":"p2","request_id":"r2","workstream_id":"w2","source":"claude",
            "kind":"exitPlan","status":"pending","default_mode":"manual","title":"Plan"
        }))
        .unwrap();
        assert_eq!(
            legacy_action(&plan, LegacyFeedKey::AutoAccept, &[]).unwrap(),
            LegacyFeedAction::Request {
                method: "feed.exit_plan.reply",
                params: serde_json::json!({"request_id":"r2","mode":"autoAccept"}),
            }
        );
        assert_eq!(
            legacy_action(
                &plan,
                LegacyFeedKey::Feedback("Use fewer steps".into()),
                &[],
            )
            .unwrap(),
            LegacyFeedAction::Request {
                method: "feed.exit_plan.reply",
                params: serde_json::json!({
                    "request_id":"r2","mode":"deny","feedback":"Use fewer steps"
                }),
            }
        );

        let question = legacy_feed_item(&serde_json::json!({
            "id":"p3","request_id":"r3","workstream_id":"w3","source":"claude",
            "kind":"question","status":"pending","title":"Question",
            "questions":[{"id":"q1","prompt":"Choose","multi_select":false,
                "options":[{"id":"o1","label":"First"},{"id":"o2","label":"Second"}]}]
        }))
        .unwrap();
        assert_eq!(
            legacy_action(&question, LegacyFeedKey::Enter, &[]).unwrap(),
            LegacyFeedAction::Request {
                method: "feed.question.reply",
                params: serde_json::json!({"request_id":"r3","selections":["First"]}),
            }
        );
    }
}
