//! Agent hook JSON to Feed control-socket bridge.

use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

use crate::invocation::CliError;

pub const FEED_HOOK_MAX_STDIN_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeedHookEnvironment {
    pub surface_id: Option<String>,
    pub workspace_id: Option<String>,
    pub agent_pid: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PreparedFeedHook {
    pub source: String,
    pub hook_event_name: String,
    pub tool_name: String,
    pub tool_input: Option<Value>,
    pub raw_object: Map<String, Value>,
    pub params: Value,
    pub actionable: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FeedEventSemantic {
    ApprovalRequest,
    ToolStart,
    ToolStartMaybeApproval,
    ToolEnd,
    PreCompact,
    PostCompact,
    PromptSubmit,
    SubagentStart,
    Response,
    SubagentResponse,
    SessionStart,
    SessionEnd,
    StatusNotification,
    Unknown,
}

pub fn prepare_feed_hook(
    args: &[String],
    stdin: &[u8],
    environment: &FeedHookEnvironment,
    fallback_request_id: &str,
) -> Result<Option<PreparedFeedHook>, CliError> {
    let source = option_value(args, "--source")
        .map(|source| source.trim().to_string())
        .filter(|source| !source.is_empty())
        .ok_or_else(|| CliError::new("cmux hooks feed requires --source <agent-name>"))?;
    if environment.surface_id.as_deref().is_none_or(str::is_empty) {
        return Ok(None);
    }
    if stdin.is_empty() || stdin.len() > FEED_HOOK_MAX_STDIN_BYTES {
        return Ok(None);
    }
    let Ok(raw_value) = serde_json::from_slice::<Value>(stdin) else {
        return Ok(None);
    };
    let Some(raw_object) = raw_value.as_object().cloned() else {
        return Ok(None);
    };

    let command_event = option_value(args, "--event");
    let raw_event = first_string(&raw_object, &["hook_event_name", "event"])
        .or(command_event)
        .unwrap_or_default();
    let tool_call = raw_object.get("toolCall").and_then(Value::as_object);
    let tool_name = first_string(&raw_object, &["tool_name", "toolName"])
        .or_else(|| tool_call.and_then(|call| first_string(call, &["name"])))
        .unwrap_or_default();
    let (hook_event_name, actionable) = classify_feed_event(&source, &raw_event, &tool_name);

    let session_id = first_string(
        &raw_object,
        &[
            "session_id",
            "sessionId",
            "conversation_id",
            "conversationId",
        ],
    )
    .unwrap_or_else(|| stable_fallback_session_id(&source, &raw_object, environment.agent_pid));
    let mut event = Map::new();
    event.insert(
        "session_id".to_string(),
        Value::String(format!("{source}-{session_id}")),
    );
    event.insert(
        "hook_event_name".to_string(),
        Value::String(hook_event_name.clone()),
    );
    event.insert("_source".to_string(), Value::String(source.clone()));
    event.insert("_ppid".to_string(), json!(environment.agent_pid));
    if let Some(workspace_id) = environment
        .workspace_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| {
            first_string(
                &raw_object,
                &[
                    "workspace_id",
                    "workspaceId",
                    "workspace_ref",
                    "workspaceRef",
                ],
            )
        })
    {
        event.insert("workspace_id".to_string(), Value::String(workspace_id));
    }
    if !tool_name.is_empty() {
        event.insert("tool_name".to_string(), Value::String(tool_name.clone()));
    }
    let tool_request_input = raw_object
        .get("tool_input")
        .or_else(|| raw_object.get("toolInput"))
        .or_else(|| tool_call.and_then(|call| call.get("args")))
        .cloned();
    if let Some(cwd) = extract_hook_cwd(&raw_object).or_else(|| {
        tool_request_input
            .as_ref()
            .and_then(Value::as_object)
            .and_then(|input| first_string(input, &["Cwd", "cwd"]))
    }) {
        event.insert("cwd".to_string(), Value::String(cwd));
    }
    let post_tool_response = raw_object
        .get("tool_response")
        .or_else(|| raw_object.get("toolResponse"))
        .or_else(|| raw_object.get("tool_result"))
        .or_else(|| raw_object.get("toolResult"));
    let tool_input =
        if source == "codex" && hook_event_name == "PostToolUse" && post_tool_response.is_some() {
            post_tool_response.map(sanitize_post_tool_use)
        } else {
            tool_request_input
        };
    if let Some(tool_input) = &tool_input {
        event.insert("tool_input".to_string(), tool_input.clone());
    }
    let request_id = first_string(
        &raw_object,
        &[
            "_opencode_request_id",
            "request_id",
            "tool_use_id",
            "toolUseID",
        ],
    )
    .unwrap_or_else(|| fallback_request_id.to_string());
    event.insert(
        "_opencode_request_id".to_string(),
        Value::String(request_id),
    );

    Ok(Some(PreparedFeedHook {
        source,
        hook_event_name,
        tool_name,
        tool_input,
        raw_object,
        params: json!({
            "event": Value::Object(event),
            "wait_timeout_seconds": if actionable { 120 } else { 0 },
        }),
        actionable,
    }))
}

pub fn classify_feed_event(source: &str, event: &str, tool_name: &str) -> (String, bool) {
    let semantic = match source {
        "claude" => match event {
            "PermissionRequest" => FeedEventSemantic::ApprovalRequest,
            "PreToolUse" => FeedEventSemantic::ToolStart,
            "PostToolUse" => FeedEventSemantic::ToolEnd,
            "PreCompact" => FeedEventSemantic::PreCompact,
            "PostCompact" => FeedEventSemantic::PostCompact,
            "UserPromptSubmit" => FeedEventSemantic::PromptSubmit,
            "SessionStart" => FeedEventSemantic::SessionStart,
            "SessionEnd" => FeedEventSemantic::SessionEnd,
            "Stop" => FeedEventSemantic::Response,
            "SubagentStart" => FeedEventSemantic::SubagentStart,
            "SubagentStop" => FeedEventSemantic::SubagentResponse,
            "Notification" => FeedEventSemantic::StatusNotification,
            _ => FeedEventSemantic::Unknown,
        },
        "codex" => match event {
            "PermissionRequest"
            | "permission_request"
            | "PreToolUse"
            | "pre_tool_use"
            | "beforeShellExecution" => FeedEventSemantic::ToolStart,
            "PostToolUse" | "post_tool_use" => FeedEventSemantic::ToolEnd,
            "PreCompact" | "pre_compact" => FeedEventSemantic::PreCompact,
            "PostCompact" | "post_compact" => FeedEventSemantic::PostCompact,
            "UserPromptSubmit" | "user_prompt_submit" => FeedEventSemantic::PromptSubmit,
            "SessionStart" | "session_start" => FeedEventSemantic::SessionStart,
            "SessionEnd" | "session_end" => FeedEventSemantic::SessionEnd,
            "Stop" | "stop" => FeedEventSemantic::Response,
            "SubagentStart" | "subagent_start" => FeedEventSemantic::SubagentStart,
            "SubagentStop" | "subagent_stop" => FeedEventSemantic::SubagentResponse,
            "Notification" | "notification" => FeedEventSemantic::StatusNotification,
            _ => FeedEventSemantic::Unknown,
        },
        "hermes-agent" => match event {
            "pre_tool_call" => FeedEventSemantic::ToolStart,
            "post_tool_call" => FeedEventSemantic::ToolEnd,
            "pre_approval_request" | "post_approval_response" => {
                FeedEventSemantic::StatusNotification
            }
            "pre_llm_call" => FeedEventSemantic::PromptSubmit,
            "post_llm_call" => FeedEventSemantic::Response,
            "on_session_start" | "on_session_reset" => FeedEventSemantic::SessionStart,
            "on_session_end" | "on_session_finalize" => FeedEventSemantic::SessionEnd,
            _ => FeedEventSemantic::Unknown,
        },
        "kiro" => match event {
            "preToolUse" => FeedEventSemantic::ToolStartMaybeApproval,
            "postToolUse" => FeedEventSemantic::ToolEnd,
            "userPromptSubmit" => FeedEventSemantic::PromptSubmit,
            "agentSpawn" => FeedEventSemantic::SessionStart,
            "stop" => FeedEventSemantic::Response,
            _ => FeedEventSemantic::Unknown,
        },
        _ => match event {
            "PreToolUse" | "beforeShellExecution" => FeedEventSemantic::ToolStartMaybeApproval,
            "PermissionRequest" => FeedEventSemantic::ApprovalRequest,
            "PostToolUse" => FeedEventSemantic::ToolEnd,
            "PreCompact" => FeedEventSemantic::PreCompact,
            "PostCompact" => FeedEventSemantic::PostCompact,
            "UserPromptSubmit" => FeedEventSemantic::PromptSubmit,
            "SessionStart" => FeedEventSemantic::SessionStart,
            "SessionEnd" => FeedEventSemantic::SessionEnd,
            "Stop" => FeedEventSemantic::Response,
            "SubagentStart" => FeedEventSemantic::SubagentStart,
            "SubagentStop" => FeedEventSemantic::SubagentResponse,
            "Notification" => FeedEventSemantic::StatusNotification,
            _ => FeedEventSemantic::Unknown,
        },
    };
    wire_mapping(semantic, source, tool_name)
}

fn wire_mapping(semantic: FeedEventSemantic, source: &str, tool_name: &str) -> (String, bool) {
    if matches!(
        semantic,
        FeedEventSemantic::ApprovalRequest | FeedEventSemantic::ToolStartMaybeApproval
    ) {
        if tool_name == "ExitPlanMode" || tool_name == "AskUserQuestion" {
            return (tool_name.to_string(), true);
        }
        if semantic == FeedEventSemantic::ApprovalRequest
            || is_side_effecting_tool(tool_name, source)
        {
            return ("PermissionRequest".to_string(), true);
        }
        return ("PreToolUse".to_string(), false);
    }
    let wire = match semantic {
        FeedEventSemantic::ToolStart => "PreToolUse",
        FeedEventSemantic::ToolEnd => "PostToolUse",
        FeedEventSemantic::PreCompact => "PreCompact",
        FeedEventSemantic::PostCompact => "PostCompact",
        FeedEventSemantic::PromptSubmit => "UserPromptSubmit",
        FeedEventSemantic::SubagentStart => "SubagentStart",
        FeedEventSemantic::Response => "Stop",
        FeedEventSemantic::SubagentResponse => "SubagentStop",
        FeedEventSemantic::SessionStart => "SessionStart",
        FeedEventSemantic::SessionEnd => "SessionEnd",
        FeedEventSemantic::StatusNotification => "Notification",
        FeedEventSemantic::Unknown => "PreToolUse",
        FeedEventSemantic::ApprovalRequest | FeedEventSemantic::ToolStartMaybeApproval => {
            unreachable!()
        }
    };
    (wire.to_string(), false)
}

fn is_side_effecting_tool(tool_name: &str, source: &str) -> bool {
    const TOOLS: &[&str] = &[
        "Bash",
        "Write",
        "Edit",
        "MultiEdit",
        "NotebookEdit",
        "apply_patch",
        "shell",
        "terminal",
        "run_command",
        "write_to_file",
        "replace_file_content",
        "multi_replace_file_content",
        "manage_task",
        "schedule",
        "ask_permission",
        "invoke_subagent",
        "define_subagent",
        "manage_subagents",
        "generate_image",
    ];
    if TOOLS.contains(&tool_name) {
        return true;
    }
    source == "kiro"
        && matches!(
            tool_name.to_ascii_lowercase().as_str(),
            "bash"
                | "write"
                | "edit"
                | "multiedit"
                | "notebookedit"
                | "apply_patch"
                | "shell"
                | "execute_bash"
                | "fs_write"
                | "use_aws"
                | "aws"
                | "terminal"
                | "run_command"
                | "write_to_file"
                | "replace_file_content"
                | "multi_replace_file_content"
                | "manage_task"
                | "schedule"
                | "ask_permission"
                | "invoke_subagent"
                | "define_subagent"
                | "manage_subagents"
                | "generate_image"
        )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentDecisionOutput {
    pub stdout: String,
    pub stderr: Option<String>,
    pub exit_code: u8,
}

pub fn render_agent_decision_output(
    prepared: &PreparedFeedHook,
    decision: &Value,
) -> AgentDecisionOutput {
    if prepared.source == "kiro"
        && decision.get("kind").and_then(Value::as_str) == Some("permission")
    {
        let mode = decision
            .get("mode")
            .and_then(Value::as_str)
            .map(str::trim)
            .map(str::to_ascii_lowercase);
        if mode
            .as_deref()
            .is_some_and(|mode| matches!(mode, "once" | "always" | "all" | "bypass"))
        {
            return AgentDecisionOutput {
                stdout: "{}".to_string(),
                stderr: None,
                exit_code: 0,
            };
        }
        return AgentDecisionOutput {
            stdout: String::new(),
            stderr: Some(if mode.as_deref() == Some("deny") {
                "User denied permission via cmux Feed.".to_string()
            } else {
                "cmux Feed returned an unrecognized Kiro permission decision; denying for safety."
                    .to_string()
            }),
            exit_code: 2,
        };
    }

    let kind = decision
        .get("kind")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let output = match kind {
        "permission" => permission_decision(prepared, decision),
        "exit_plan" => exit_plan_decision(prepared, decision),
        "question" => question_decision(prepared, decision),
        _ => json!({}),
    };
    AgentDecisionOutput {
        stdout: serde_json::to_string(&output).unwrap_or_else(|_| "{}".to_string()),
        stderr: None,
        exit_code: 0,
    }
}

fn permission_decision(prepared: &PreparedFeedHook, decision: &Value) -> Value {
    let mode = decision
        .get("mode")
        .and_then(Value::as_str)
        .unwrap_or("deny");
    if prepared.source == "claude" || prepared.source == "codex" {
        let updated_permissions = (prepared.source == "claude" && matches!(mode, "always" | "all"))
            .then(|| prepared.raw_object.get("permission_suggestions").cloned())
            .flatten();
        return claude_hook_decision(
            if mode == "deny" { "deny" } else { "allow" },
            (mode == "deny").then_some("User denied permission via cmux Feed."),
            None,
            updated_permissions,
        );
    }
    if prepared.source == "hermes-agent" {
        return if mode == "deny" {
            json!({"action":"block","message":"User denied permission via cmux Feed."})
        } else {
            json!({})
        };
    }
    if prepared.source == "antigravity" {
        return json!({
            "decision": if mode == "deny" { "deny" } else { "allow" },
            "reason": if mode == "deny" {
                "User denied permission via cmux Feed."
            } else {
                "User approved via cmux Feed."
            },
        });
    }
    let permission = if mode == "deny" { "deny" } else { "allow" };
    let reason = if mode == "deny" {
        "User denied permission via cmux Feed.".to_string()
    } else if matches!(mode, "always" | "all" | "bypass") {
        format!(
            "User granted {mode} permission via cmux Feed. Reduce subsequent approval prompts for similar calls."
        )
    } else {
        "User approved via cmux Feed.".to_string()
    };
    non_claude_pre_tool_decision(permission, &reason, None)
}

fn exit_plan_decision(prepared: &PreparedFeedHook, decision: &Value) -> Value {
    let mode = decision
        .get("mode")
        .and_then(Value::as_str)
        .unwrap_or("manual");
    let feedback = decision
        .get("feedback")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if prepared.source == "claude" {
        if let Some(feedback) = feedback {
            return claude_hook_decision(
                "deny",
                Some(&format!(
                    "User rejected the plan via cmux Feed and wants this change: {feedback}"
                )),
                None,
                None,
            );
        }
        if mode == "ultraplan" {
            return claude_hook_decision(
                "deny",
                Some(
                    "User chose Ultraplan via cmux Feed. Refine this plan with Ultraplan on Claude Code on the web.",
                ),
                None,
                None,
            );
        }
        if mode == "deny" {
            return claude_hook_decision(
                "deny",
                Some("User rejected the plan via cmux Feed."),
                None,
                None,
            );
        }
        let permissions = (mode == "autoAccept")
            .then(|| json!([{"type":"setMode","mode":"auto","destination":"session"}]));
        return claude_hook_decision("allow", None, prepared.tool_input.clone(), permissions);
    }
    if prepared.source == "hermes-agent" {
        return if let Some(feedback) = feedback {
            json!({"action":"block","message":format!("User rejected the plan via cmux Feed and wants this change: {feedback}")})
        } else if mode == "deny" {
            json!({"action":"block","message":"User rejected the plan via cmux Feed."})
        } else {
            json!({})
        };
    }
    if let Some(feedback) = feedback {
        let reason =
            format!("User rejected the plan via cmux Feed and wants this change: {feedback}");
        return non_claude_pre_tool_decision("deny", &reason, Some(&reason));
    }
    if mode == "deny" {
        return non_claude_pre_tool_decision("deny", "User rejected the plan via cmux Feed.", None);
    }
    if mode == "ultraplan" {
        let reason =
            "User chose Ultraplan via cmux Feed. Refine this plan with Ultraplan if available.";
        return non_claude_pre_tool_decision("deny", reason, Some(reason));
    }
    let mode_text = match mode {
        "bypassPermissions" => "bypass-permissions mode (no per-edit approval)",
        "autoAccept" => "auto mode",
        _ => "manual-approval mode (approve each edit)",
    };
    let context = format!(
        "User accepted this plan via cmux Feed with {mode_text}. Exit plan mode now and proceed to implement without re-entering ExitPlanMode. Do not ask again."
    );
    non_claude_pre_tool_decision("deny", &context, Some(&context))
}

fn question_decision(prepared: &PreparedFeedHook, decision: &Value) -> Value {
    let selections = decision
        .get("selections")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if selections
        == [Value::String(
            "Skip interview and plan immediately".to_string(),
        )]
    {
        let message = "User chose Skip interview and plan immediately via cmux Feed. Do not ask more interview questions. Write the plan now.";
        return if prepared.source == "claude" {
            claude_hook_decision("deny", Some(message), None, None)
        } else {
            non_claude_pre_tool_decision("deny", message, Some(message))
        };
    }
    if prepared.source == "hermes-agent" {
        let strings = selections
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>();
        let context = match strings.as_slice() {
            [] => "The user submitted an empty answer.".to_string(),
            [answer] => format!("The user answered: {answer}"),
            answers => format!("The user answered: {}", answers.join(", ")),
        };
        return json!({"context":context});
    }
    if prepared.source == "claude" {
        let mut updated_input = prepared
            .tool_input
            .as_ref()
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        let questions = updated_input
            .get("questions")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let mut answers = Map::new();
        for (index, selection) in selections.iter().enumerate() {
            let key = questions
                .get(index)
                .and_then(|question| question.get("question"))
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| format!("Answer {}", index + 1));
            answers.insert(key, selection.clone());
        }
        updated_input.insert("answers".to_string(), Value::Object(answers));
        return claude_hook_decision("allow", None, Some(Value::Object(updated_input)), None);
    }
    let strings = selections
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>();
    let body = match strings.as_slice() {
        [] => "The user submitted an empty answer.".to_string(),
        [answer] => format!("The user answered: {answer}"),
        answers => format!(
            "The user answered:\n{}",
            answers
                .iter()
                .enumerate()
                .map(|(index, answer)| format!("{}. {answer}", index + 1))
                .collect::<Vec<_>>()
                .join("\n")
        ),
    };
    let context = format!(
        "[cmux Feed] {body}. Treat these as the user's response to your AskUserQuestion prompt; do not call AskUserQuestion again for the same question."
    );
    non_claude_pre_tool_decision("deny", &context, Some(&context))
}

fn claude_hook_decision(
    behavior: &str,
    message: Option<&str>,
    updated_input: Option<Value>,
    updated_permissions: Option<Value>,
) -> Value {
    let mut decision = Map::new();
    decision.insert("behavior".to_string(), Value::String(behavior.to_string()));
    if let Some(message) = message {
        decision.insert("message".to_string(), Value::String(message.to_string()));
    }
    if let Some(input) =
        updated_input.filter(|value| value.as_object().is_some_and(|v| !v.is_empty()))
    {
        decision.insert("updatedInput".to_string(), input);
    }
    if let Some(permissions) = updated_permissions {
        decision.insert("updatedPermissions".to_string(), permissions);
    }
    json!({
        "hookSpecificOutput": {
            "hookEventName": "PermissionRequest",
            "decision": Value::Object(decision),
        }
    })
}

fn non_claude_pre_tool_decision(
    permission: &str,
    reason: &str,
    additional_context: Option<&str>,
) -> Value {
    let mut specific = Map::new();
    specific.insert(
        "hookEventName".to_string(),
        Value::String("PreToolUse".to_string()),
    );
    specific.insert(
        "permissionDecision".to_string(),
        Value::String(permission.to_string()),
    );
    if !reason.is_empty() {
        specific.insert(
            "permissionDecisionReason".to_string(),
            Value::String(reason.to_string()),
        );
    }
    if let Some(context) = additional_context.filter(|value| !value.is_empty()) {
        specific.insert(
            "additionalContext".to_string(),
            Value::String(context.to_string()),
        );
    }
    let mut output = Map::new();
    output.insert("hookSpecificOutput".to_string(), Value::Object(specific));
    if permission == "deny" {
        output.insert("decision".to_string(), Value::String("block".to_string()));
        if !reason.is_empty() {
            output.insert("reason".to_string(), Value::String(reason.to_string()));
        }
    } else if permission == "allow" {
        output.insert("decision".to_string(), Value::String("approve".to_string()));
        let message = additional_context.unwrap_or(reason);
        if !message.is_empty() {
            output.insert(
                "systemMessage".to_string(),
                Value::String(message.to_string()),
            );
        }
    }
    Value::Object(output)
}

fn stable_fallback_session_id(source: &str, raw: &Map<String, Value>, agent_pid: i64) -> String {
    let mut components = vec![
        format!("source={source}"),
        format!("pid={}", agent_pid.max(0)),
    ];
    if let Some(workspace_id) = first_string(
        raw,
        &[
            "workspace_id",
            "workspaceId",
            "workspace_ref",
            "workspaceRef",
        ],
    ) {
        components.push(format!("workspace={workspace_id}"));
    }
    if let Some(cwd) = extract_hook_cwd(raw) {
        components.push(format!("cwd={cwd}"));
    }
    if let Some(transcript) = extract_nested_string(raw, &["transcript_path", "transcriptPath"]) {
        components.push(format!("transcript={transcript}"));
    }
    let digest = Sha256::digest(components.join("\n").as_bytes());
    let prefix = digest[..8]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("fallback-{prefix}")
}

fn extract_hook_cwd(raw: &Map<String, Value>) -> Option<String> {
    const CWD_KEYS: &[&str] = &[
        "cwd",
        "working_directory",
        "workingDirectory",
        "project_dir",
        "projectDir",
        "project_path",
        "projectPath",
    ];
    first_string(raw, CWD_KEYS)
        .or_else(|| first_workspace_path(raw))
        .or_else(|| {
            ["notification", "data", "context"].iter().find_map(|key| {
                raw.get(*key).and_then(Value::as_object).and_then(|nested| {
                    first_string(nested, CWD_KEYS).or_else(|| first_workspace_path(nested))
                })
            })
        })
}

fn extract_nested_string(raw: &Map<String, Value>, keys: &[&str]) -> Option<String> {
    first_string(raw, keys).or_else(|| {
        ["notification", "data", "context"].iter().find_map(|key| {
            raw.get(*key)
                .and_then(Value::as_object)
                .and_then(|nested| first_string(nested, keys))
        })
    })
}

fn first_workspace_path(raw: &Map<String, Value>) -> Option<String> {
    raw.get("workspacePaths")
        .or_else(|| raw.get("workspace_paths"))
        .and_then(Value::as_array)
        .and_then(|paths| {
            paths.iter().find_map(|path| {
                path.as_str()
                    .map(str::trim)
                    .filter(|path| !path.is_empty())
                    .map(str::to_string)
            })
        })
}

fn sanitize_post_tool_use(value: &Value) -> Value {
    let mut summary = Map::new();
    summary.insert("_cmux_sanitized".to_string(), Value::Bool(true));
    match value {
        Value::Object(object) => {
            summary.insert("_cmux_original_key_count".to_string(), json!(object.len()));
            let mut summarized = 0usize;
            for (key, value) in object {
                let normalized = key
                    .chars()
                    .filter(|character| !matches!(character, '_' | '-'))
                    .collect::<String>()
                    .to_ascii_lowercase();
                if matches!(
                    normalized.as_str(),
                    "exitcode" | "status" | "signal" | "durationms" | "timedout" | "success"
                ) {
                    if let Some((value, truncated)) = bounded_scalar(value) {
                        summary.insert(key.clone(), value);
                        if truncated {
                            summary.insert(format!("{key}_truncated"), Value::Bool(true));
                        }
                        summarized += 1;
                        continue;
                    }
                }
                if matches!(
                    normalized.as_str(),
                    "stdout" | "stderr" | "output" | "text" | "result" | "message" | "error"
                ) {
                    summarize_omitted_field(&mut summary, key, value);
                    summarized += 1;
                }
            }
            let omitted = object.len().saturating_sub(summarized);
            if omitted > 0 {
                summary.insert("_cmux_omitted_key_count".to_string(), json!(omitted));
            }
        }
        Value::Array(values) => {
            summary.insert("_cmux_array_count".to_string(), json!(values.len()));
        }
        Value::String(_) => {
            summary.insert("_cmux_text_omitted".to_string(), Value::Bool(true));
        }
        scalar => {
            if let Some((value, truncated)) = bounded_scalar(scalar) {
                summary.insert("_cmux_value".to_string(), value);
                if truncated {
                    summary.insert("_cmux_value_truncated".to_string(), Value::Bool(true));
                }
            }
        }
    }
    Value::Object(summary)
}

fn summarize_omitted_field(summary: &mut Map<String, Value>, key: &str, value: &Value) {
    match value {
        Value::String(_) => {
            summary.insert(format!("{key}_text_omitted"), Value::Bool(true));
        }
        Value::Array(values) => {
            summary.insert(format!("{key}_array_count"), json!(values.len()));
            summary.insert(format!("{key}_omitted"), Value::Bool(true));
        }
        Value::Object(object) => {
            summary.insert(format!("{key}_object_key_count"), json!(object.len()));
            summary.insert(format!("{key}_omitted"), Value::Bool(true));
        }
        _ => {
            summary.insert(format!("{key}_omitted"), Value::Bool(true));
        }
    }
}

fn bounded_scalar(value: &Value) -> Option<(Value, bool)> {
    match value {
        Value::Null | Value::Bool(_) | Value::Number(_) => Some((value.clone(), false)),
        Value::String(value) => {
            let mut end = value.len();
            if value.len() > 512 {
                end = value
                    .char_indices()
                    .take_while(|(index, character)| index + character.len_utf8() <= 512)
                    .map(|(index, character)| index + character.len_utf8())
                    .last()
                    .unwrap_or(0);
            }
            Some((Value::String(value[..end].to_string()), end < value.len()))
        }
        Value::Array(_) | Value::Object(_) => None,
    }
}

fn option_value(args: &[String], name: &str) -> Option<String> {
    args.iter().enumerate().find_map(|(index, arg)| {
        if arg == name {
            return args.get(index + 1).cloned();
        }
        arg.strip_prefix(&format!("{name}=")).map(str::to_string)
    })
}

fn first_string(object: &Map<String, Value>, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        object
            .get(*key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render_agent_decision(prepared: &PreparedFeedHook, decision: &Value) -> String {
        render_agent_decision_output(prepared, decision).stdout
    }

    fn environment() -> FeedHookEnvironment {
        FeedHookEnvironment {
            surface_id: Some("surface-1".to_string()),
            workspace_id: Some("workspace-1".to_string()),
            agent_pid: 4242,
        }
    }

    #[test]
    fn classifier_keeps_dedicated_tool_hooks_telemetry_and_real_approvals_actionable() {
        assert_eq!(
            classify_feed_event("claude", "PreToolUse", "Write"),
            ("PreToolUse".to_string(), false)
        );
        assert_eq!(
            classify_feed_event("claude", "PermissionRequest", "ExitPlanMode"),
            ("ExitPlanMode".to_string(), true)
        );
        assert_eq!(
            classify_feed_event("codex", "PermissionRequest", "apply_patch"),
            ("PreToolUse".to_string(), false)
        );
    }

    #[test]
    fn classifier_escalates_generic_and_kiro_mutations_only() {
        assert!(classify_feed_event("gemini", "PreToolUse", "Write").1);
        assert!(!classify_feed_event("gemini", "PreToolUse", "Read").1);
        assert!(classify_feed_event("kiro", "preToolUse", "fs_write").1);
        assert!(!classify_feed_event("kiro", "preToolUse", "fs_read").1);
    }

    #[test]
    fn preparation_maps_agent_payload_and_wait_contract() {
        let prepared = prepare_feed_hook(
            &["--source".into(), "claude".into()],
            br#"{"hook_event_name":"PermissionRequest","session_id":"abc","cwd":"C:/repo","tool_name":"AskUserQuestion","tool_input":{"questions":[]},"request_id":"request-1"}"#,
            &environment(),
            "fallback-request",
        )
        .unwrap()
        .unwrap();
        assert!(prepared.actionable);
        assert_eq!(prepared.params["wait_timeout_seconds"], 120);
        assert_eq!(prepared.params["event"]["session_id"], "claude-abc");
        assert_eq!(prepared.params["event"]["workspace_id"], "workspace-1");
        assert_eq!(prepared.params["event"]["_ppid"], 4242);
        assert_eq!(
            prepared.params["event"]["_opencode_request_id"],
            "request-1"
        );
    }

    #[test]
    fn preparation_gracefully_noops_outside_surface_or_for_bad_stdin() {
        let mut env = environment();
        env.surface_id = None;
        assert!(prepare_feed_hook(
            &["--source=claude".into()],
            br#"{"hook_event_name":"Stop"}"#,
            &env,
            "request",
        )
        .unwrap()
        .is_none());
        assert!(prepare_feed_hook(
            &["--source=claude".into()],
            b"not-json",
            &environment(),
            "request",
        )
        .unwrap()
        .is_none());
    }

    #[test]
    fn missing_session_id_uses_stable_payload_identity_and_nested_paths() {
        let mut env = environment();
        env.workspace_id = None;
        env.agent_pid = 424242;
        let input = r#"{"hook_event_name":"PreToolUse","workspace_id":"workspace-payload","workspacePaths":["  C:/repo  "],"notification":{"transcript_path":"C:/logs/transcript-a.jsonl"},"toolCall":{"name":"read_file","args":{"path":"README.md"}}}"#;
        let first = prepare_feed_hook(
            &["--source=antigravity".into()],
            input.as_bytes(),
            &env,
            "request-one",
        )
        .unwrap()
        .unwrap();
        let second = prepare_feed_hook(
            &["--source=antigravity".into()],
            input.as_bytes(),
            &env,
            "request-two",
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            first.params["event"]["session_id"],
            second.params["event"]["session_id"]
        );
        assert!(first.params["event"]["session_id"]
            .as_str()
            .unwrap_or_default()
            .starts_with("antigravity-fallback-"));
        assert_eq!(first.params["event"]["workspace_id"], "workspace-payload");
        assert_eq!(first.params["event"]["cwd"], "C:/repo");

        let changed = prepare_feed_hook(
            &["--source=antigravity".into()],
            input.replace("transcript-a", "transcript-b").as_bytes(),
            &env,
            "request-three",
        )
        .unwrap()
        .unwrap();
        assert_ne!(
            first.params["event"]["session_id"],
            changed.params["event"]["session_id"]
        );
    }

    #[test]
    fn codex_post_tool_use_summarizes_response_without_retaining_output() {
        let prepared = prepare_feed_hook(
            &["--source=codex".into(), "--event=PostToolUse".into()],
            br#"{"session_id":"codex-1","tool_name":"shell","tool_input":{"command":"secret"},"tool_response":{"exitCode":0,"status":"ok","stdout":"sensitive output","details":{"token":"hidden"}}}"#,
            &environment(),
            "request",
        )
        .unwrap()
        .unwrap();
        let summary = &prepared.params["event"]["tool_input"];
        assert_eq!(summary["_cmux_sanitized"], true);
        assert_eq!(summary["exitCode"], 0);
        assert_eq!(summary["status"], "ok");
        assert_eq!(summary["stdout_text_omitted"], true);
        assert_eq!(summary["_cmux_original_key_count"], 4);
        assert_eq!(summary["_cmux_omitted_key_count"], 1);
        assert!(!summary.to_string().contains("sensitive output"));
        assert!(!summary.to_string().contains("hidden"));
    }

    #[test]
    fn claude_permission_and_question_decisions_match_hook_shapes() {
        let prepared = prepare_feed_hook(
            &["--source=claude".into()],
            br#"{"hook_event_name":"PermissionRequest","session_id":"abc","tool_name":"AskUserQuestion","tool_input":{"questions":[{"question":"Language?"},{"question":"Checks?"}]}}"#,
            &environment(),
            "request",
        )
        .unwrap()
        .unwrap();
        let denied: Value = serde_json::from_str(&render_agent_decision(
            &prepared,
            &json!({"kind":"permission","mode":"deny"}),
        ))
        .unwrap();
        assert_eq!(denied["hookSpecificOutput"]["decision"]["behavior"], "deny");
        let answered: Value = serde_json::from_str(&render_agent_decision(
            &prepared,
            &json!({"kind":"question","selections":["Rust","Unit, E2E"]}),
        ))
        .unwrap();
        assert_eq!(
            answered["hookSpecificOutput"]["decision"]["updatedInput"]["answers"],
            json!({"Language?":"Rust","Checks?":"Unit, E2E"})
        );
    }

    #[test]
    fn claude_always_and_plan_modes_preserve_canonical_updates() {
        let prepared = prepare_feed_hook(
            &["--source=claude".into()],
            br#"{"hook_event_name":"PermissionRequest","session_id":"abc","tool_name":"ExitPlanMode","tool_input":{"plan":"Ship it"},"permission_suggestions":[{"type":"addRules"}]}"#,
            &environment(),
            "request",
        )
        .unwrap()
        .unwrap();
        let always: Value = serde_json::from_str(&render_agent_decision(
            &prepared,
            &json!({"kind":"permission","mode":"always"}),
        ))
        .unwrap();
        assert_eq!(
            always["hookSpecificOutput"]["decision"]["updatedPermissions"],
            json!([{"type":"addRules"}])
        );
        let auto: Value = serde_json::from_str(&render_agent_decision(
            &prepared,
            &json!({"kind":"exit_plan","mode":"autoAccept"}),
        ))
        .unwrap();
        assert_eq!(
            auto["hookSpecificOutput"]["decision"]["updatedInput"],
            json!({"plan":"Ship it"})
        );
        assert_eq!(
            auto["hookSpecificOutput"]["decision"]["updatedPermissions"],
            json!([{"type":"setMode","mode":"auto","destination":"session"}])
        );
    }

    #[test]
    fn kiro_permission_adapter_allows_or_denies_with_native_exit_contract() {
        let prepared = prepare_feed_hook(
            &["--source=kiro".into(), "--event=preToolUse".into()],
            br#"{"session_id":"kiro-1","tool_name":"fs_write","tool_input":{}}"#,
            &environment(),
            "request",
        )
        .unwrap()
        .unwrap();
        let allowed =
            render_agent_decision_output(&prepared, &json!({"kind":"permission","mode":"once"}));
        assert_eq!(allowed.stdout, "{}");
        assert_eq!(allowed.stderr, None);
        assert_eq!(allowed.exit_code, 0);

        let denied =
            render_agent_decision_output(&prepared, &json!({"kind":"permission","mode":"deny"}));
        assert_eq!(denied.stdout, "");
        assert_eq!(
            denied.stderr.as_deref(),
            Some("User denied permission via cmux Feed.")
        );
        assert_eq!(denied.exit_code, 2);

        let malformed =
            render_agent_decision_output(&prepared, &json!({"kind":"permission","mode":"typo"}));
        assert_eq!(malformed.exit_code, 2);
        assert!(malformed
            .stderr
            .as_deref()
            .unwrap_or_default()
            .contains("denying for safety"));
    }

    #[test]
    fn hermes_and_antigravity_permission_adapters_match_native_shapes() {
        let mut prepared = prepare_feed_hook(
            &[
                "--source=hermes-agent".into(),
                "--event=pre_tool_call".into(),
            ],
            br#"{"session_id":"h1","tool_name":"terminal","tool_input":{}}"#,
            &environment(),
            "request",
        )
        .unwrap()
        .unwrap();
        let hermes: Value = serde_json::from_str(&render_agent_decision(
            &prepared,
            &json!({"kind":"permission","mode":"deny"}),
        ))
        .unwrap();
        assert_eq!(
            hermes,
            json!({"action":"block","message":"User denied permission via cmux Feed."})
        );

        prepared.source = "antigravity".to_string();
        let antigravity: Value = serde_json::from_str(&render_agent_decision(
            &prepared,
            &json!({"kind":"permission","mode":"always"}),
        ))
        .unwrap();
        assert_eq!(
            antigravity,
            json!({"decision":"allow","reason":"User approved via cmux Feed."})
        );
    }

    #[test]
    fn generic_plan_and_question_adapters_carry_blocking_context() {
        let mut prepared = prepare_feed_hook(
            &["--source=gemini".into(), "--event=PermissionRequest".into()],
            br#"{"session_id":"g1","tool_name":"ExitPlanMode","tool_input":{}}"#,
            &environment(),
            "request",
        )
        .unwrap()
        .unwrap();
        let plan: Value = serde_json::from_str(&render_agent_decision(
            &prepared,
            &json!({"kind":"exit_plan","mode":"autoAccept"}),
        ))
        .unwrap();
        assert_eq!(plan["decision"], "block");
        assert_eq!(plan["hookSpecificOutput"]["permissionDecision"], "deny");
        assert!(plan["hookSpecificOutput"]["additionalContext"]
            .as_str()
            .unwrap_or_default()
            .contains("auto mode"));

        prepared.tool_name = "AskUserQuestion".to_string();
        let question: Value = serde_json::from_str(&render_agent_decision(
            &prepared,
            &json!({"kind":"question","selections":["Rust","Unit, E2E"]}),
        ))
        .unwrap();
        assert!(question["reason"]
            .as_str()
            .unwrap_or_default()
            .contains("1. Rust\n2. Unit, E2E"));
        assert_eq!(
            question["hookSpecificOutput"]["additionalContext"],
            question["reason"]
        );
    }

    #[test]
    fn hermes_questions_return_context_and_approved_plans_return_empty_json() {
        let prepared = prepare_feed_hook(
            &[
                "--source=hermes-agent".into(),
                "--event=pre_tool_call".into(),
            ],
            br#"{"session_id":"h1","tool_name":"AskUserQuestion","tool_input":{}}"#,
            &environment(),
            "request",
        )
        .unwrap()
        .unwrap();
        let answer: Value = serde_json::from_str(&render_agent_decision(
            &prepared,
            &json!({"kind":"question","selections":["One","Two"]}),
        ))
        .unwrap();
        assert_eq!(answer, json!({"context":"The user answered: One, Two"}));
        assert_eq!(
            render_agent_decision(&prepared, &json!({"kind":"exit_plan","mode":"manual"})),
            "{}"
        );
    }
}
