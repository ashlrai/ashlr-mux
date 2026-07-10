//! Canonical no-socket documentation index (`cmux docs`).

use crate::invocation::CliError;

pub const DOCS_USAGE: &str = "Usage: cmux docs [settings|shortcuts|api|browser|agents|dock]\n\nPrint the canonical docs URL, raw GitHub resources, and useful commands for a cmux topic.\nThis command does not require a running cmux app or socket.\n\nAgents:\n  Use `cmux docs settings` before editing ~/.config/cmux/cmux.json.\n  Use `cmux docs dock` before creating or editing .cmux/dock.json.\n  Back up any existing cmux.json file to a timestamped .bak copy before editing so the user can revert.\n  Fetch raw resources with the printed curl commands when you need the latest schema.";

struct Resource {
    label: &'static str,
    url: &'static str,
}
impl Resource {
    const fn new(label: &'static str, url: &'static str) -> Self {
        Self { label, url }
    }
}
struct Reference {
    topic: &'static str,
    aliases: &'static [&'static str],
    summary: &'static str,
    web_url: &'static str,
    resources: &'static [Resource],
    commands: &'static [&'static str],
}
impl Reference {
    const fn new(
        topic: &'static str,
        aliases: &'static [&'static str],
        summary: &'static str,
        web_url: &'static str,
        resources: &'static [Resource],
        commands: &'static [&'static str],
    ) -> Self {
        Self {
            topic,
            aliases,
            summary,
            web_url,
            resources,
            commands,
        }
    }
}

const SCHEMA: &str =
    "https://raw.githubusercontent.com/manaflow-ai/cmux/main/web/data/cmux.schema.json";
const REFS: &[Reference] = &[
    Reference::new(
        "settings",
        &["configuration", "config", "cmux-json", "settings-json", "settingsjson", "schema"],
        "cmux-owned settings, cmux.json locations, schema, and reload flow.",
        "https://cmux.com/docs/configuration#cmux-json",
        &[Resource::new("settings schema", SCHEMA), Resource::new("cmux skill", "https://raw.githubusercontent.com/manaflow-ai/cmux/main/skills/cmux/SKILL.md")],
        &["cmux settings path", "cmux settings cmux-json", "cmux config doctor", "cmux reload-config"],
    ),
    Reference::new(
        "shortcuts", &["keyboard", "keybindings", "keys"],
        "cmux-owned keyboard shortcuts and two-step chord syntax.",
        "https://cmux.com/docs/keyboard-shortcuts",
        &[Resource::new("shortcut data", "https://raw.githubusercontent.com/manaflow-ai/cmux/main/web/data/cmux-shortcuts.ts"), Resource::new("settings schema", SCHEMA)],
        &["cmux shortcuts", "cmux settings shortcuts", "cmux docs settings"],
    ),
    Reference::new(
        "api", &["cli", "socket", "automation", "handles"],
        "CLI/socket API, handle model, windows, workspaces, panes, and surfaces.",
        "https://cmux.com/docs/api",
        &[Resource::new("CLI contract", "https://raw.githubusercontent.com/manaflow-ai/cmux/main/docs/cli-contract.md"), Resource::new("cmux skill", "https://raw.githubusercontent.com/manaflow-ai/cmux/main/skills/cmux/SKILL.md")],
        &["cmux identify --json", "cmux tree --all"],
    ),
    Reference::new(
        "browser", &["browser-automation", "webview"],
        "Browser panel automation commands and snapshot-driven web interaction.",
        "https://cmux.com/docs/browser-automation",
        &[Resource::new("browser skill", "https://raw.githubusercontent.com/manaflow-ai/cmux/main/skills/cmux-browser/SKILL.md"), Resource::new("browser commands", "https://raw.githubusercontent.com/manaflow-ai/cmux/main/skills/cmux-browser/references/commands.md")],
        &["cmux browser --help", "cmux browser snapshot"],
    ),
    Reference::new(
        "agents", &["integrations", "agent-integrations"],
        "Agent hook integrations, Feed approvals, notifications, and session restore.",
        "https://cmux.com/docs/agent-integrations/oh-my-codex",
        &[Resource::new("agent hook docs", "https://raw.githubusercontent.com/manaflow-ai/cmux/main/docs/agent-hooks.md"), Resource::new("feed docs", "https://raw.githubusercontent.com/manaflow-ai/cmux/main/docs/feed.md"), Resource::new("notifications docs", "https://raw.githubusercontent.com/manaflow-ai/cmux/main/docs/notifications.md")],
        &["cmux hooks setup", "cmux hooks setup <agent>", "cmux hooks hermes-agent install", "cmux hooks hermes-agent uninstall", "cmux hooks <agent> uninstall"],
    ),
    Reference::new(
        "dock", &["doc", "controls", "right-sidebar", "dock-json"],
        "Custom right-sidebar terminal controls from .cmux/dock.json or ~/.config/cmux/dock.json.",
        "https://cmux.com/docs/dock",
        &[Resource::new("dock docs", "https://raw.githubusercontent.com/manaflow-ai/cmux/main/docs/dock.md"), Resource::new("dock web copy", "https://raw.githubusercontent.com/manaflow-ai/cmux/main/web/messages/en.json")],
        &["cmux docs dock", "cmux docs dock --json", "python3 -m json.tool .cmux/dock.json"],
    ),
    Reference::new(
        "sidebars", &["sidebar", "custom-sidebar", "custom-sidebars", "vibe-sidebar"],
        "Vibe-code a custom sidebar: a runtime-interpreted SwiftUI-style file in ~/.config/cmux/sidebars/ (beta).",
        "https://cmux.com/docs/custom-sidebars",
        &[Resource::new("custom sidebar authoring guide", "https://raw.githubusercontent.com/manaflow-ai/cmux/main/docs/custom-sidebars.md")],
        &["mkdir -p ~/.config/cmux/sidebars", "cat > ~/.config/cmux/sidebars/mine.swift   # write a SwiftUI-style view, then right-click the sidebar button to pick it", "cmux docs api   # discover cmux() action methods/params"],
    ),
];

pub fn run_docs_command(command_args: &[String], global_json: bool) -> Result<String, CliError> {
    let separator = command_args.iter().position(|arg| arg == "--");
    let head = separator.map_or(command_args, |index| &command_args[..index]);
    let tail = separator.map_or(&[][..], |index| &command_args[index + 1..]);
    let wants_json = global_json || head.iter().any(|arg| arg == "--json");
    let mut args: Vec<&str> = head
        .iter()
        .filter(|arg| arg.as_str() != "--json")
        .map(String::as_str)
        .collect();
    args.extend(tail.iter().map(String::as_str));
    if head
        .iter()
        .any(|arg| matches!(arg.as_str(), "--help" | "-h"))
        || args
            .first()
            .is_some_and(|arg| arg.eq_ignore_ascii_case("help"))
    {
        return Ok(DOCS_USAGE.to_string());
    }
    let Some(topic) = args.first().map(|topic| topic.to_lowercase()) else {
        return Ok(if wants_json {
            render_index_json()?
        } else {
            render_index()
        });
    };
    if args.len() != 1 {
        return Err(CliError::new(
            "Usage: cmux docs [settings|shortcuts|api|browser|agents|dock]",
        ));
    }
    if matches!(topic.as_str(), "list" | "all") {
        return Ok(if wants_json {
            render_index_json()?
        } else {
            render_index()
        });
    }
    let normalized = topic.replace('_', "-");
    let reference = REFS
        .iter()
        .find(|reference| {
            reference.topic == normalized || reference.aliases.contains(&normalized.as_str())
        })
        .ok_or_else(|| {
            CliError::new(format!(
                "Unknown docs topic '{topic}'. Run 'cmux docs' for topics."
            ))
        })?;
    if wants_json {
        serde_json::to_string_pretty(&payload(reference))
            .map_err(|error| CliError::new(format!("failed to encode docs JSON: {error}")))
    } else {
        Ok(render_reference(reference))
    }
}

fn payload(reference: &Reference) -> serde_json::Value {
    let mut value = serde_json::json!({
        "aliases": reference.aliases,
        "commands": reference.commands,
        "raw_resources": reference.resources.iter().map(|resource| serde_json::json!({"fetch":format!("curl -fsSL {}", resource.url),"label":resource.label,"url":resource.url})).collect::<Vec<_>>(),
        "summary": reference.summary,
        "topic": reference.topic,
        "web_url": reference.web_url,
    });
    if reference.topic == "settings" {
        let object = value.as_object_mut().expect("payload object");
        object.insert("backup".into(), serde_json::json!("Back up any existing cmux.json file to a timestamped .bak copy before editing so the user can revert."));
        object.insert("ghostty_config".into(), serde_json::json!({"note":"Not cmux-owned, but cmux reads it. Use for terminal transparency (background-opacity), blur, font, theme, etc.","path":"~/.config/ghostty/config"}));
        object.insert(
            "reload_command".into(),
            serde_json::json!("cmux reload-config"),
        );
        object.insert("reload_scope".into(), serde_json::json!("Reloads Ghostty config + cmux.json and refreshes terminals in place. No app restart needed."));
        object.insert("settings_files".into(), serde_json::json!({"fallback":"~/Library/Application Support/com.cmuxterm.app/settings.json","legacy":"~/.config/cmux/settings.json","primary":"~/.config/cmux/cmux.json"}));
    }
    value
}

fn render_index_json() -> Result<String, CliError> {
    serde_json::to_string_pretty(
        &serde_json::json!({"topics": REFS.iter().map(payload).collect::<Vec<_>>() }),
    )
    .map_err(|error| CliError::new(format!("failed to encode docs JSON: {error}")))
}

fn render_index() -> String {
    let mut output = "cmux docs\n\nTopics:\n".to_string();
    for reference in REFS {
        output.push_str(&format!(
            "  {:<10} {}\n",
            reference.topic, reference.summary
        ));
    }
    output.push_str("\nRun `cmux docs <topic>` for URLs, raw resources, and next commands.");
    output
}

fn render_reference(reference: &Reference) -> String {
    let mut output = format!(
        "{}: {}\n\nWeb:\n  {}",
        reference.topic, reference.summary, reference.web_url
    );
    if !reference.resources.is_empty() {
        append_section(
            &mut output,
            "Raw resources:",
            reference
                .resources
                .iter()
                .map(|resource| format!("{}: {}", resource.label, resource.url)),
        );
        append_section(
            &mut output,
            "Fetch:",
            reference
                .resources
                .iter()
                .map(|resource| format!("curl -fsSL {}", resource.url)),
        );
    }
    if !reference.commands.is_empty() {
        append_section(
            &mut output,
            "Useful commands:",
            reference.commands.iter().copied(),
        );
    }
    if reference.topic == "settings" {
        output.push_str("\n\nConfig files:\n  primary: ~/.config/cmux/cmux.json\n  legacy config: ~/.config/cmux/settings.json\n  legacy app support: ~/Library/Application Support/com.cmuxterm.app/settings.json\n\nRelated (not cmux-owned, but cmux reads it for terminal behavior):\n  ~/.config/ghostty/config\n  Use this for terminal transparency (background-opacity), blur, font, theme, etc.\n\nBefore editing cmux.json:\n  Back up any existing cmux.json file to a timestamped .bak copy so the user can revert.\n\nReload after editing cmux.json or Ghostty config:\n  cmux reload-config   (reloads BOTH and refreshes terminals; no app restart needed)");
    }
    output
}

fn append_section<I, S>(output: &mut String, heading: &str, lines: I)
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    output.push_str("\n\n");
    output.push_str(heading);
    for line in lines {
        output.push_str("\n  ");
        output.push_str(line.as_ref());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_canonical_index_alias_json_and_errors() {
        let index = run_docs_command(&[], false).unwrap();
        assert!(index.contains("settings   cmux-owned settings"));
        assert!(index.contains("sidebars   Vibe-code a custom sidebar"));

        let json = run_docs_command(&["configuration".into(), "--json".into()], false).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["topic"], "settings");
        assert_eq!(
            value["settings_files"]["primary"],
            "~/.config/cmux/cmux.json"
        );
        assert_eq!(
            value["raw_resources"][0]["fetch"],
            format!("curl -fsSL {SCHEMA}")
        );

        assert_eq!(
            run_docs_command(&["--help".into()], false).unwrap(),
            DOCS_USAGE
        );
        assert_eq!(
            run_docs_command(&["missing".into()], false)
                .unwrap_err()
                .message,
            "Unknown docs topic 'missing'. Run 'cmux docs' for topics."
        );
    }
}
