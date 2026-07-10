//! Canonical no-socket `cmux settings` helpers.

use crate::invocation::CliError;

pub const SETTINGS_USAGE: &str = "Usage: cmux settings [open [target]|path|docs|<target>]\n\nOpen cmux Settings, print cmux.json paths, or show settings documentation.\n\nSubcommands:\n  open [target]       Open Settings, optionally to a target section.\n  path                Print cmux.json paths, docs URL, and schema URL.\n  docs                Print the same output as `cmux docs settings`.\n\nTargets:\n  account, app, terminal, sidebar-appearance, custom-sidebars,\n  automation, browser, browser-import, global-hotkey,\n  keyboard-shortcuts, shortcuts, workspace-colors, cmux-json,\n  json, reset\n\nConfig file:\n  ~/.config/cmux/cmux.json\n  legacy config: ~/.config/cmux/settings.json\n  legacy app support: ~/Library/Application Support/com.cmuxterm.app/settings.json\n\nRelated (not cmux-owned, but cmux reads it for terminal behavior):\n  ~/.config/ghostty/config\n\nBefore editing cmux.json:\n  Back up any existing cmux.json file to a timestamped .bak copy so the user can revert.\n\nReload after editing cmux.json or Ghostty config:\n  cmux reload-config   (reloads BOTH and refreshes terminals; no app restart needed)";

pub(crate) const DOCS_URL: &str = "https://cmux.com/docs/configuration#cmux-json";
pub(crate) const SCHEMA_URL: &str =
    "https://raw.githubusercontent.com/manaflow-ai/cmux/main/web/data/cmux.schema.json";

pub fn run_settings_no_socket(
    command_args: &[String],
    global_json: bool,
) -> Result<String, CliError> {
    let parsed = crate::docs::parse_docs_settings_args(command_args, global_json);
    if parsed.help_requested() {
        return Ok(SETTINGS_USAGE.to_string());
    }
    let args = &parsed.arguments;
    let wants_json = parsed.wants_json;
    match args.first().map(|arg| arg.to_lowercase()).as_deref() {
        Some("path" | "paths") => {
            if args.len() != 1 {
                return Err(CliError::new("Usage: cmux settings path"));
            }
            if wants_json {
                render_paths_json()
            } else {
                Ok(render_paths())
            }
        }
        Some("docs" | "documentation") => {
            if args.len() != 1 {
                return Err(CliError::new("Usage: cmux settings docs"));
            }
            crate::docs::run_docs_command(&["settings".to_string()], wants_json)
        }
        Some(other) => Err(CliError::new(format!(
            "Unknown settings subcommand '{other}'. Run 'cmux settings --help'."
        ))),
        None => Err(CliError::new(
            "Usage: cmux settings [open [target]|path|docs|<target>]",
        )),
    }
}

fn render_paths_json() -> Result<String, CliError> {
    serde_json::to_string_pretty(&serde_json::json!({
        "backup": "Back up any existing cmux.json file to a timestamped .bak copy before editing so the user can revert.",
        "docs_url": DOCS_URL,
        "fallback": "~/Library/Application Support/com.cmuxterm.app/settings.json",
        "ghostty_config": {
            "note": "Not cmux-owned, but cmux reads it. Use for terminal transparency (background-opacity), blur, font, theme, etc.",
            "path": "~/.config/ghostty/config"
        },
        "legacy": "~/.config/cmux/settings.json",
        "primary": "~/.config/cmux/cmux.json",
        "reload_command": "cmux reload-config",
        "reload_scope": "Reloads Ghostty config + cmux.json and refreshes terminals in place. No app restart needed.",
        "schema_url": SCHEMA_URL
    }))
    .map_err(|error| CliError::new(format!("failed to encode settings JSON: {error}")))
}

fn render_paths() -> String {
    format!(
        "Config files:\n  primary:  ~/.config/cmux/cmux.json\n  legacy config: ~/.config/cmux/settings.json\n  legacy app support: ~/Library/Application Support/com.cmuxterm.app/settings.json\n\nRelated (not cmux-owned, but cmux reads it for terminal behavior):\n  ~/.config/ghostty/config\n\nDocs:\n  {DOCS_URL}\n\nSchema:\n  {SCHEMA_URL}\n\nBefore editing cmux.json:\n  Back up any existing cmux.json file to a timestamped .bak copy so the user can revert.\n\nReload after editing (covers BOTH cmux.json and Ghostty config; no app restart needed):\n  cmux reload-config"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_paths_json_and_reuses_settings_docs() {
        let text = run_settings_no_socket(&["paths".into()], false).unwrap();
        assert!(text.contains("primary:  ~/.config/cmux/cmux.json"));
        assert!(text.contains(SCHEMA_URL));

        let json = run_settings_no_socket(&["path".into(), "--json".into()], false).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["primary"], "~/.config/cmux/cmux.json");
        assert_eq!(value["ghostty_config"]["path"], "~/.config/ghostty/config");

        let docs = run_settings_no_socket(&["documentation".into()], true).unwrap();
        let docs_value: serde_json::Value = serde_json::from_str(&docs).unwrap();
        assert_eq!(docs_value["topic"], "settings");
    }
}
