//! No-socket `cmux window default-display` persistence.

use std::path::Path;

use crate::invocation::CliError;

pub fn run_window_default_display(
    command_args: &[String],
    json_output: bool,
) -> Result<String, CliError> {
    let path = cmux_config::config_path()
        .ok_or_else(|| CliError::new("Could not resolve the user configuration directory"))?;
    run_window_default_display_at(command_args, json_output, &path)
}

fn run_window_default_display_at(
    command_args: &[String],
    json_output: bool,
    path: &Path,
) -> Result<String, CliError> {
    let args = if command_args
        .first()
        .is_some_and(|argument| argument.eq_ignore_ascii_case("default-display"))
    {
        &command_args[1..]
    } else {
        command_args
    };

    if args.iter().any(|argument| argument == "--clear") {
        cmux_config::set_dev_window_display_at(path, None).map_err(CliError::new)?;
        return if json_output {
            render_json(None)
        } else {
            Ok("Cleared dev window display default.".into())
        };
    }

    if let Some(raw) = args.iter().find(|argument| !argument.starts_with('-')) {
        let name = raw.trim();
        if name.is_empty() {
            return Err(CliError::new(
                "window default-display requires a display name, or --clear",
            ));
        }
        cmux_config::set_dev_window_display_at(path, Some(name)).map_err(CliError::new)?;
        let stored = name.to_string();
        return if json_output {
            render_json(Some(&stored))
        } else {
            Ok(format!(
                "Dev builds will open on \"{stored}\" (DEBUG builds, applied at window creation)."
            ))
        };
    }

    let current = cmux_config::dev_window_display_at(path).map_err(CliError::new)?;
    if json_output {
        render_json(current.as_deref())
    } else {
        Ok(current.unwrap_or_else(|| "(unset)".into()))
    }
}

fn render_json(value: Option<&str>) -> Result<String, CliError> {
    serde_json::to_string_pretty(&serde_json::json!({"default_display": value}))
        .map_err(|error| CliError::new(format!("failed to encode default display JSON: {error}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_query_and_clear_match_the_canonical_contract() {
        let root = std::env::temp_dir().join(format!(
            "cmux-window-default-display-{}",
            uuid::Uuid::new_v4()
        ));
        let path = root.join("cmux.json");

        assert_eq!(
            run_window_default_display_at(&["default-display".into()], false, &path).unwrap(),
            "(unset)"
        );
        let set = run_window_default_display_at(
            &["default-display".into(), "  Display 2  ".into()],
            false,
            &path,
        )
        .unwrap();
        assert_eq!(
            set,
            "Dev builds will open on \"Display 2\" (DEBUG builds, applied at window creation)."
        );
        let queried = run_window_default_display_at(
            &["default-display".into(), "--json".into()],
            true,
            &path,
        )
        .unwrap();
        let queried: serde_json::Value = serde_json::from_str(&queried).unwrap();
        assert_eq!(queried["default_display"], "Display 2");

        let cleared = run_window_default_display_at(
            &["default-display".into(), "ignored".into(), "--clear".into()],
            true,
            &path,
        )
        .unwrap();
        let cleared: serde_json::Value = serde_json::from_str(&cleared).unwrap();
        assert!(cleared["default_display"].is_null());

        std::fs::remove_dir_all(root).unwrap();
    }
}
