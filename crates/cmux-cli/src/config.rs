//! Canonical read-only `cmux config` helpers that do not require a socket.

use crate::invocation::CliError;

pub const CONFIG_USAGE: &str = "Usage: cmux config <doctor|check|validate|path|paths|docs|documentation|reload|get|set|sidebar-font-size|surface-tab-bar-font-size>\n\nInspect cmux.json, print configuration references, update selected Ghostty config keys, or reload the running app.\n\nSubcommands:\n  doctor|check|validate [--path <path>]   Validate JSONC syntax for cmux config files.\n  path|paths                              Print cmux.json paths, docs URL, and schema URL.\n  docs|documentation                      Print the same output as `cmux docs settings`.\n  reload                                  Reload Ghostty config + cmux.json and refresh terminals (alias for `cmux reload-config`).\n  get <key>                               Print sidebar-font-size or surface-tab-bar-font-size.\n  set <key> <points>                      Set sidebar-font-size (10-20 pt) or surface-tab-bar-font-size (8-24 pt), then reload if cmux is running.\n  sidebar-font-size [points]              Get or set the left sidebar text size.\n  surface-tab-bar-font-size [points]      Get or set the workspace tab bar text size.\n\nConfig files:\n  ~/.config/cmux/cmux.json\n  legacy config: ~/.config/cmux/settings.json\n  legacy app support: ~/Library/Application Support/com.cmuxterm.app/settings.json\n\nRelated (not cmux-owned, but cmux reads it for terminal behavior):\n  ~/.config/ghostty/config\n\nExamples:\n  cmux config doctor\n  cmux config doctor --path .cmux/cmux.json\n  cmux config set sidebar-font-size 14\n  cmux config sidebar-font-size 12.5\n  cmux config set surface-tab-bar-font-size 13\n  cmux config surface-tab-bar-font-size 11\n  cmux config reload";

pub fn run_config_no_socket(
    command_args: &[String],
    global_json: bool,
) -> Result<String, CliError> {
    let parsed = crate::docs::parse_docs_settings_args(command_args, global_json);
    if parsed.help_requested() || parsed.arguments.is_empty() {
        return Ok(CONFIG_USAGE.to_string());
    }

    let args = &parsed.arguments;
    let subcommand = args[0].to_lowercase();
    match subcommand.as_str() {
        "path" | "paths" => {
            require_arity(args, 1, "Usage: cmux config path")?;
            crate::settings::run_settings_no_socket(&["path".to_string()], parsed.wants_json)
        }
        "docs" | "documentation" => {
            require_arity(args, 1, "Usage: cmux config docs")?;
            crate::docs::run_docs_command(&["settings".to_string()], parsed.wants_json)
        }
        "doctor"
        | "check"
        | "validate"
        | "get"
        | "sidebar-font-size"
        | "surface-tab-bar-font-size" => Err(CliError::new(format!(
            "'config {subcommand}' is not yet available in the Windows port"
        ))),
        _ => Err(CliError::new(format!(
            "Unknown config subcommand '{subcommand}'. Run 'cmux config --help'."
        ))),
    }
}

fn require_arity(args: &[&str], expected: usize, usage: &str) -> Result<(), CliError> {
    if args.len() == expected {
        Ok(())
    } else {
        Err(CliError::new(usage))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_help_and_reuses_settings_reference_outputs() {
        assert_eq!(run_config_no_socket(&[], false).unwrap(), CONFIG_USAGE);

        let config_paths =
            run_config_no_socket(&["--json".into(), "--".into(), "paths".into()], false).unwrap();
        let settings_paths =
            crate::settings::run_settings_no_socket(&["path".into()], true).unwrap();
        assert_eq!(config_paths, settings_paths);

        let docs = run_config_no_socket(&["documentation".into()], true).unwrap();
        let value: serde_json::Value = serde_json::from_str(&docs).unwrap();
        assert_eq!(value["topic"], "settings");
    }
}
