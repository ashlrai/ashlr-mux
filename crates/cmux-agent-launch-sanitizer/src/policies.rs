//! Per-kind option policy tables, ported byte-for-byte from the Swift sources:
//!
//! - `AgentLaunchSanitizerPrimaryPolicies.swift`
//! - `AgentLaunchSanitizerAdditionalPolicies.swift`
//! - `AgentLaunchSanitizerClaudeTeamsPolicy.swift`
//!
//! Every option `Set` is transcribed verbatim. The order within a set never
//! affects behavior (all are membership-tested), except `dropped_option_prefixes`
//! which is a `Vec` matched by prefix; the individual prefixes are disjoint so
//! iteration order is immaterial there too, but insertion order is preserved to
//! mirror the Swift array exactly.

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

/// Mirror of the Swift `AgentLaunchSanitizer.Policy` struct. Sets hold
/// `&'static str` because every option token is a compile-time literal; this
/// avoids per-call `String` allocation while keeping `contains(&str)` lookups.
#[derive(Clone, Default)]
pub(crate) struct Policy {
    pub value_options: HashSet<&'static str>,
    pub optional_value_options: HashSet<&'static str>,
    pub optional_value_choices: HashMap<&'static str, HashSet<&'static str>>,
    pub greedy_optional_value_options: HashSet<&'static str>,
    pub variadic_options: HashSet<&'static str>,
    pub non_restorable_commands: HashSet<&'static str>,
    pub dropped_options: HashSet<&'static str>,
    pub dropped_option_prefixes: Vec<&'static str>,
    pub reject_options: HashSet<&'static str>,
    pub prompt_boundary_options: HashSet<&'static str>,
    pub resume_subcommand: Option<&'static str>,
    pub preserve_first_positional: bool,
    pub skip_claude_hook_settings: bool,
}

fn hset(items: &[&'static str]) -> HashSet<&'static str> {
    items.iter().copied().collect()
}

pub(crate) fn claude_policy() -> Policy {
    Policy {
        value_options: hset(&[
            "--add-dir",
            "--agent",
            "--agents",
            "--allowedTools",
            "--allowed-tools",
            "--append-system-prompt",
            "--append-system-prompt-file",
            "--betas",
            "--dangerously-load-development-channels",
            "--debug-file",
            "--disallowedTools",
            "--disallowed-tools",
            "--effort",
            "--fallback-model",
            "--file",
            "--from-pr",
            "--input-format",
            "--json-schema",
            "--max-budget-usd",
            "--mcp-config",
            "--model",
            "--name",
            "-n",
            "--output-format",
            "--permission-mode",
            "--plugin-dir",
            "--remote-control-session-name-prefix",
            "--resume",
            "-r",
            "--session-id",
            "--setting-sources",
            "--settings",
            "--system-prompt",
            "--system-prompt-file",
            "--teammate-mode",
            "--tmux",
            "--tools",
            "--worktree",
            "-w",
        ]),
        optional_value_options: hset(&["--debug"]),
        variadic_options: hset(&[
            "--add-dir",
            "--allowedTools",
            "--allowed-tools",
            "--betas",
            "--disallowedTools",
            "--disallowed-tools",
            "--file",
            "--mcp-config",
            "--tools",
        ]),
        non_restorable_commands: hset(&[
            "agents",
            "auth",
            "auto-mode",
            "api-key",
            "config",
            "doctor",
            "install",
            "mcp",
            "plugin",
            "plugins",
            "rc",
            "remote-control",
            "setup-token",
            "update",
            "upgrade",
        ]),
        dropped_options: hset(&[
            "--continue",
            "-c",
            "--file",
            "--fork-session",
            "--from-pr",
            "--resume",
            "-r",
            "--session-id",
            "--tmux",
            "--worktree",
            "-w",
        ]),
        dropped_option_prefixes: vec![
            "--file=",
            "--fork-session=",
            "--from-pr=",
            "--resume=",
            "--session-id=",
            "--tmux=",
            "--worktree=",
        ],
        reject_options: hset(&["--print", "-p", "--no-session-persistence"]),
        skip_claude_hook_settings: true,
        ..Default::default()
    }
}

/// Returns a shared static instance: the codex policy is consulted repeatedly
/// per sanitize call (fork detection, positional dropping, option
/// preservation), so it is built once. The tables are immutable, mirroring the
/// Swift `static let` policy.
pub(crate) fn codex_policy() -> &'static Policy {
    static POLICY: OnceLock<Policy> = OnceLock::new();
    POLICY.get_or_init(|| Policy {
        value_options: hset(&[
            "--config",
            "-c",
            "--remote",
            "--remote-auth-token-env",
            "--image",
            "-i",
            "--model",
            "-m",
            "--local-provider",
            "--profile",
            "-p",
            "--sandbox",
            "-s",
            "--ask-for-approval",
            "-a",
            "--cd",
            "-C",
            "--add-dir",
            "--enable",
            "--disable",
        ]),
        variadic_options: hset(&["--image", "-i"]),
        non_restorable_commands: hset(&[
            "exec",
            "e",
            "review",
            "login",
            "logout",
            "mcp",
            "mcp-server",
            "app-server",
            "app",
            "completion",
            "sandbox",
            "debug",
            "apply",
            "a",
            "fork",
            "cloud",
            "exec-server",
            "features",
            "help",
        ]),
        dropped_options: hset(&[
            "--last",
            "--image",
            "-i",
            "--remote",
            "--remote-auth-token-env",
            "--all",
        ]),
        dropped_option_prefixes: vec!["--remote=", "--remote-auth-token-env="],
        resume_subcommand: Some("resume"),
        ..Default::default()
    })
}

pub(crate) fn grok_policy() -> Policy {
    Policy {
        value_options: hset(&[
            "--agent",
            "--agents",
            "--allow",
            "--cwd",
            "--deny",
            "--disallowed-tools",
            "--effort",
            "--max-turns",
            "--model",
            "-m",
            "--permission-mode",
            "--reasoning-effort",
            "--resume",
            "-r",
            "--rules",
            "--sandbox",
            "--system-prompt-override",
            "--tools",
            "--worktree",
            "-w",
        ]),
        optional_value_options: hset(&["--resume", "-r", "--worktree", "-w"]),
        non_restorable_commands: hset(&[
            "agent", "help", "import", "inspect", "leader", "login", "mcp", "memory", "models",
            "sessions", "setup", "share", "ssh", "trace", "update", "version", "v", "worktree",
        ]),
        dropped_options: hset(&[
            "--continue",
            "-c",
            "--restore-code",
            "--resume",
            "-r",
            "--worktree",
            "-w",
        ]),
        dropped_option_prefixes: vec!["--resume=", "-r=", "--worktree=", "-w="],
        reject_options: hset(&[
            "--best-of-n",
            "--output-format",
            "--prompt-file",
            "--prompt-json",
            "--single",
            "-p",
        ]),
        ..Default::default()
    }
}

pub(crate) fn pi_policy() -> Policy {
    Policy {
        value_options: hset(&[
            "--append-system-prompt",
            "--api-key",
            "--extension",
            "--fork",
            "--model",
            "--models",
            "--prompt-template",
            "--provider",
            "--resume",
            "--session",
            "--session-dir",
            "--skill",
            "--system-prompt",
            "--theme",
            "--thinking",
            "--tools",
            "-e",
            "-r",
            "-t",
        ]),
        non_restorable_commands: hset(&[
            "config", "help", "install", "list", "login", "logout", "remove", "uninstall",
            "update",
        ]),
        dropped_options: hset(&[
            "--api-key",
            "--continue",
            "--fork",
            "--resume",
            "--session",
            "-c",
            "-r",
        ]),
        dropped_option_prefixes: vec!["--api-key=", "--fork=", "--resume=", "--session="],
        reject_options: hset(&[
            "--export",
            "--list-models",
            "--mode",
            "--no-session",
            "--print",
            "--prompt",
            "--version",
            "-h",
            "-p",
            "-v",
        ]),
        ..Default::default()
    }
}

pub(crate) fn amp_policy() -> Policy {
    Policy {
        value_options: hset(&[
            "--effort",
            "--label",
            "--log-file",
            "--log-level",
            "--mcp-config",
            "--mode",
            "--settings-file",
            "--visibility",
            "-l",
            "-m",
        ]),
        non_restorable_commands: hset(&[
            "login",
            "logout",
            "mcp",
            "permissions",
            "permission",
            "review",
            "skill",
            "skills",
            "tool",
            "tools",
            "update",
            "up",
            "usage",
            "version",
        ]),
        dropped_options: hset(&[
            "--archive",
            "--label",
            "-l",
            "--stream-json",
            "--stream-json-input",
            "--stream-json-thinking",
        ]),
        reject_options: hset(&["--execute", "--print", "-V", "-x"]),
        ..Default::default()
    }
}

pub(crate) fn gemini_policy() -> Policy {
    Policy {
        value_options: hset(&[
            "--model",
            "-m",
            "--sandbox",
            "-s",
            "--approval-mode",
            "--policy",
            "--admin-policy",
            "--allowed-mcp-server-names",
            "--allowed-tools",
            "--extensions",
            "-e",
            "--include-directories",
            "--resume",
            "-r",
            "--session-id",
            "--worktree",
            "-w",
            "--prompt",
            "-p",
            "--prompt-interactive",
            "-i",
            "--delete-session",
            "--output-format",
            "-o",
        ]),
        optional_value_options: hset(&["--resume", "-r"]),
        variadic_options: hset(&[
            "--policy",
            "--admin-policy",
            "--allowed-mcp-server-names",
            "--allowed-tools",
            "--extensions",
            "-e",
            "--include-directories",
        ]),
        non_restorable_commands: hset(&["mcp", "extensions", "skills", "hooks", "gemma", "help"]),
        dropped_options: hset(&["--resume", "-r", "--session-id", "--worktree", "-w"]),
        dropped_option_prefixes: vec!["--resume=", "--session-id=", "--worktree="],
        reject_options: hset(&[
            "--prompt",
            "-p",
            "--prompt-interactive",
            "-i",
            "--list-sessions",
            "--delete-session",
            "--output-format",
            "-o",
            "--raw-output",
            "--accept-raw-output-risk",
            "--acp",
            "--experimental-acp",
            "--list-extensions",
        ]),
        ..Default::default()
    }
}

pub(crate) fn antigravity_policy() -> Policy {
    Policy {
        value_options: hset(&[
            "--add-dir",
            "--conversation",
            "--log-file",
            "--print-timeout",
            "--prompt",
            "-p",
            "--sandbox",
        ]),
        optional_value_options: hset(&["--continue", "-c"]),
        non_restorable_commands: hset(&[
            "changelog", "help", "install", "plugin", "plugins", "update",
        ]),
        dropped_options: hset(&["--continue", "-c", "--conversation"]),
        dropped_option_prefixes: vec!["--conversation="],
        reject_options: hset(&[
            "--prompt",
            "-p",
            "--prompt-interactive",
            "-i",
            "--print",
        ]),
        ..Default::default()
    }
}

pub(crate) fn cursor_policy() -> Policy {
    Policy {
        value_options: hset(&[
            "--api-key",
            "-H",
            "--header",
            "--mode",
            "--model",
            "--output-format",
            "--resume",
            "--sandbox",
            "--workspace",
            "-w",
            "--worktree",
            "--worktree-base",
        ]),
        optional_value_options: hset(&["-w", "--resume", "--worktree"]),
        non_restorable_commands: hset(&[
            "about",
            "create-chat",
            "generate-rule",
            "help",
            "install-shell-integration",
            "login",
            "logout",
            "ls",
            "mcp",
            "models",
            "rule",
            "status",
            "uninstall-shell-integration",
            "update",
            "whoami",
        ]),
        dropped_options: hset(&[
            "--api-key",
            "-H",
            "--header",
            "--continue",
            "--resume",
            "--workspace",
            "-w",
            "--worktree",
            "--worktree-base",
            "--skip-worktree-setup",
        ]),
        dropped_option_prefixes: vec![
            "--api-key=",
            "--header=",
            "-H=",
            "--resume=",
            "--workspace=",
            "--worktree=",
            "--worktree-base=",
        ],
        reject_options: hset(&[
            "--cloud",
            "--output-format",
            "--print",
            "-p",
            "--stream-partial-output",
        ]),
        resume_subcommand: Some("resume"),
        ..Default::default()
    }
}

pub(crate) fn open_code_policy() -> Policy {
    Policy {
        value_options: hset(&[
            "--log-level",
            "--port",
            "--hostname",
            "--mdns-domain",
            "--cors",
            "--file",
            "-f",
            "--model",
            "-m",
            "--session",
            "-s",
            "--prompt",
            "--agent",
        ]),
        variadic_options: hset(&["--cors"]),
        non_restorable_commands: hset(&[
            "completion",
            "acp",
            "mcp",
            "attach",
            "run",
            "debug",
            "providers",
            "auth",
            "agent",
            "upgrade",
            "uninstall",
            "serve",
            "web",
            "models",
            "stats",
            "export",
            "import",
            "pr",
            "github",
            "session",
            "plugin",
            "plug",
            "db",
        ]),
        dropped_options: hset(&[
            "--continue",
            "-c",
            "--file",
            "-f",
            "--fork",
            "--session",
            "-s",
            "--prompt",
        ]),
        dropped_option_prefixes: vec!["--file=", "-f=", "--fork=", "--session=", "--prompt="],
        preserve_first_positional: true,
        ..Default::default()
    }
}

pub(crate) fn copilot_policy() -> Policy {
    Policy {
        value_options: hset(&[
            "--add-dir",
            "--add-github-mcp-tool",
            "--add-github-mcp-toolset",
            "--additional-mcp-config",
            "--agent",
            "--allow-tool",
            "--allow-url",
            "--available-tools",
            "--bash-env",
            "--connect",
            "--deny-tool",
            "--deny-url",
            "--disable-mcp-server",
            "--effort",
            "--excluded-tools",
            "--interactive",
            "-i",
            "--log-dir",
            "--log-level",
            "--max-autopilot-continues",
            "--mode",
            "--model",
            "-n",
            "--name",
            "--output-format",
            "--plugin-dir",
            "--prompt",
            "-p",
            "--reasoning-effort",
            "--resume",
            "--secret-env-vars",
            "--share",
            "--stream",
        ]),
        optional_value_options: hset(&[
            "--allow-tool",
            "--allow-url",
            "--available-tools",
            "--bash-env",
            "--connect",
            "--deny-tool",
            "--deny-url",
            "--excluded-tools",
            "--mouse",
            "--resume",
            "--secret-env-vars",
            "--share",
        ]),
        variadic_options: hset(&[
            "--add-dir",
            "--add-github-mcp-tool",
            "--add-github-mcp-toolset",
            "--additional-mcp-config",
            "--allow-tool",
            "--allow-url",
            "--available-tools",
            "--deny-tool",
            "--deny-url",
            "--disable-mcp-server",
            "--excluded-tools",
            "--plugin-dir",
            "--secret-env-vars",
        ]),
        non_restorable_commands: hset(&[
            "completion", "help", "init", "login", "mcp", "plugin", "update", "version",
        ]),
        dropped_options: hset(&["--connect", "--continue", "--interactive", "-i", "--resume"]),
        dropped_option_prefixes: vec!["--connect=", "--interactive=", "-i=", "--resume="],
        reject_options: hset(&[
            "--acp",
            "--output-format",
            "--prompt",
            "-p",
            "--share",
            "--share-gist",
            "--silent",
            "-s",
        ]),
        ..Default::default()
    }
}

pub(crate) fn code_buddy_policy() -> Policy {
    Policy {
        value_options: hset(&[
            "--add-dir",
            "--agent",
            "--agents",
            "--allowedTools",
            "--append-system-prompt",
            "--channels",
            "--dangerously-load-development-channels",
            "--disallowedTools",
            "--fallback-model",
            "-H",
            "--header",
            "--image-to-image-model",
            "--input-format",
            "--json-schema",
            "--max-turns",
            "--mcp-config",
            "--model",
            "--name",
            "--output-format",
            "--permission-mode",
            "--plugin-dir",
            "--port",
            "--resume",
            "-r",
            "--sandbox",
            "--sandbox-id",
            "--setting-sources",
            "--settings",
            "--session-id",
            "--subagent-permission-mode",
            "--system-prompt",
            "--system-prompt-file",
            "--teleport",
            "--text-to-image-model",
            "--tools",
            "--worktree",
            "-w",
            "--worktree-branch",
        ]),
        optional_value_options: hset(&["--debug", "--resume", "-r", "--sandbox", "--worktree", "-w"]),
        variadic_options: hset(&[
            "--add-dir",
            "--allowedTools",
            "--disallowedTools",
            "--mcp-config",
            "--plugin-dir",
        ]),
        non_restorable_commands: hset(&[
            "attach", "config", "daemon", "doctor", "help", "install", "kill", "logs", "mcp",
            "plugin", "ps", "sandbox", "update",
        ]),
        dropped_options: hset(&[
            "--continue",
            "-c",
            "-H",
            "--header",
            "--fork-session",
            "--name",
            "--resume",
            "-r",
            "--session-id",
            "--tmux",
            "--tmux-classic",
            "--worktree",
            "-w",
            "--worktree-branch",
        ]),
        dropped_option_prefixes: vec![
            "--header=",
            "-H=",
            "--name=",
            "--resume=",
            "-r=",
            "--session-id=",
            "--worktree=",
            "-w=",
            "--worktree-branch=",
        ],
        reject_options: hset(&[
            "--acp",
            "--background",
            "--bg",
            "--input-format",
            "--output-format",
            "--print",
            "-p",
            "--serve",
        ]),
        ..Default::default()
    }
}

pub(crate) fn factory_policy() -> Policy {
    Policy {
        value_options: hset(&[
            "--append-system-prompt",
            "--append-system-prompt-file",
            "--cwd",
            "--fork",
            "--resume",
            "-r",
            "--settings",
            "--worktree",
            "-w",
            "--worktree-dir",
        ]),
        optional_value_options: hset(&["--resume", "-r", "--worktree", "-w"]),
        non_restorable_commands: hset(&[
            "computer", "daemon", "exec", "find", "help", "mcp", "plugin", "search", "update",
        ]),
        dropped_options: hset(&[
            "--fork",
            "--resume",
            "-r",
            "--worktree",
            "-w",
            "--worktree-dir",
        ]),
        dropped_option_prefixes: vec![
            "--fork=",
            "--resume=",
            "-r=",
            "--worktree=",
            "-w=",
            "--worktree-dir=",
        ],
        ..Default::default()
    }
}

pub(crate) fn qoder_policy() -> Policy {
    Policy {
        value_options: hset(&[
            "--agent",
            "--agents",
            "--allowed-mcp-server-names",
            "--allowed-tools",
            "--append-system-prompt",
            "--attachment",
            "--cwd",
            "--delete-session",
            "--disallowed-tools",
            "--input-format",
            "--max-output-tokens",
            "--mcp-config",
            "--model",
            "-m",
            "--name",
            "-n",
            "--output-format",
            "-o",
            "-f",
            "--permission-mode",
            "--plugin-dir",
            "--prompt-interactive",
            "-i",
            "--resume",
            "-r",
            "--session-id",
            "--setting-sources",
            "--settings",
            "--system-prompt",
            "--tools",
            "--workspace",
            "-w",
        ]),
        variadic_options: hset(&[
            "--allowed-mcp-server-names",
            "--allowed-tools",
            "--attachment",
            "--disallowed-tools",
            "--mcp-config",
            "--plugin-dir",
            "--setting-sources",
            "--tools",
        ]),
        non_restorable_commands: hset(&[
            "agent", "agents", "feedback", "help", "hook", "hooks", "login", "mcp", "plugin",
            "plugins", "skill", "skills", "update",
        ]),
        dropped_options: hset(&[
            "--continue",
            "-c",
            "--fork-session",
            "--resume",
            "-r",
            "--session-id",
        ]),
        dropped_option_prefixes: vec!["--resume=", "-r=", "--session-id="],
        reject_options: hset(&[
            "--acp",
            "--delete-session",
            "--input-format",
            "--list-sessions",
            "--output-format",
            "-o",
            "-f",
            "--print",
            "-p",
            "--prompt-interactive",
            "-i",
        ]),
        ..Default::default()
    }
}

pub(crate) fn kiro_policy() -> Policy {
    Policy {
        value_options: hset(&[
            "--agent",
            "--delete-session",
            "--format",
            "-f",
            "--resume-id",
            "--trust-tools",
            "--wrap",
        ]),
        non_restorable_commands: hset(&[
            "agent",
            "diagnostic",
            "doctor",
            "inline",
            "integrations",
            "issue",
            "login",
            "logout",
            "mcp",
            "settings",
            "theme",
            "translate",
            "update",
            "version",
            "whoami",
        ]),
        dropped_options: hset(&[
            "--delete-session",
            "--format",
            "-f",
            "--resume",
            "-r",
            "--resume-id",
        ]),
        dropped_option_prefixes: vec!["--delete-session=", "--format=", "-f=", "--resume-id="],
        reject_options: hset(&[
            "--list-models",
            "--list-sessions",
            "--no-interactive",
            "--resume-picker",
        ]),
        ..Default::default()
    }
}

pub(crate) fn rovo_dev_policy() -> Policy {
    Policy {
        value_options: hset(&["--config", "--config-file", "--model", "--model-id", "--restore"]),
        optional_value_options: hset(&["--restore"]),
        non_restorable_commands: hset(&[
            "auth", "config", "help", "mcp", "server", "update", "upgrade", "version",
        ]),
        dropped_options: hset(&["--restore"]),
        dropped_option_prefixes: vec!["--restore="],
        reject_options: hset(&[
            "--prompt",
            "-p",
            "--prompt-interactive",
            "-i",
            "--print",
            "--input-format",
            "--output-format",
            "-o",
        ]),
        ..Default::default()
    }
}

pub(crate) fn hermes_agent_policy() -> Policy {
    Policy {
        value_options: hset(&[
            "--api-key",
            "--base-url",
            "--image",
            "--max-turns",
            "--model",
            "-m",
            "--profile",
            "-p",
            "--provider",
            "--resume",
            "-r",
            "--skills",
            "-s",
            "--source",
            "--toolsets",
            "-t",
            "--worktree",
            "-w",
        ]),
        optional_value_options: hset(&["--continue", "-c"]),
        non_restorable_commands: HashSet::new(),
        dropped_options: hset(&[
            "--api-key",
            "--continue",
            "-c",
            "--image",
            "--resume",
            "-r",
            "--source",
            "--verbose",
            "-v",
            "--worktree",
            "-w",
        ]),
        dropped_option_prefixes: vec![
            "--api-key=",
            "--continue=",
            "-c=",
            "--image=",
            "--resume=",
            "-r=",
            "--source=",
            "--worktree=",
            "-w=",
        ],
        reject_options: hset(&[
            "--oneshot",
            "-z",
            "--query",
            "-q",
            "--quiet",
            "-Q",
            "--list-tools",
            "--list-toolsets",
        ]),
        ..Default::default()
    }
}

/// `AgentLaunchSanitizerClaudeTeamsPolicy.swift`: derived from [`claude_policy`]
/// by the exact same subtract/formUnion/removeAll mutations.
pub(crate) fn claude_teams_policy() -> Policy {
    let mut policy = claude_policy();
    for key in ["--tmux", "--worktree", "-w"] {
        policy.value_options.remove(key);
    }
    for key in ["--prompt-suggestions", "--remote-control", "--worktree", "-w"] {
        policy.optional_value_options.insert(key);
    }
    policy
        .optional_value_choices
        .insert("--prompt-suggestions", hset(&["true", "false"]));
    for key in ["--remote-control", "--worktree", "-w"] {
        policy.greedy_optional_value_options.insert(key);
    }
    for key in ["--tmux", "--worktree", "-w"] {
        policy.dropped_options.remove(key);
    }
    policy
        .dropped_option_prefixes
        .retain(|prefix| *prefix != "--tmux=" && *prefix != "--worktree=");
    policy.prompt_boundary_options = hset(&["--tmux"]);
    policy
}
