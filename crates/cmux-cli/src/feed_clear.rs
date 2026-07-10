use std::fs;
use std::path::{Path, PathBuf};

use crate::invocation::CliError;

pub const FEED_USAGE: &str =
    "Usage: cmux feed tui [--opentui|--legacy]\n       cmux feed clear [--yes|-y]";

pub fn feed_history_path(home: &Path) -> PathBuf {
    home.join(".cmuxterm").join("workstream.jsonl")
}

pub fn run_feed_command<F>(args: &[String], home: &Path, confirm: F) -> Result<String, CliError>
where
    F: FnOnce(&str) -> Result<bool, CliError>,
{
    let subcommand = args.first().map(|arg| arg.to_ascii_lowercase());
    match subcommand.as_deref().unwrap_or("help") {
        "help" | "--help" | "-h" => Ok(FEED_USAGE.to_string()),
        "clear" => clear_feed_history(home, args, confirm),
        "tui" => Err(CliError::new(
            "feed tui is not yet ported; use the in-app Feed panel",
        )),
        unknown => Err(CliError::new(format!("Unknown feed subcommand: {unknown}"))),
    }
}

fn clear_feed_history<F>(home: &Path, args: &[String], confirm: F) -> Result<String, CliError>
where
    F: FnOnce(&str) -> Result<bool, CliError>,
{
    let path = feed_history_path(home);
    if !path.exists() {
        return Ok(format!(
            "No Feed history to clear ({} does not exist).",
            path.display()
        ));
    }
    let skip_confirmation = args.iter().any(|arg| arg == "--yes" || arg == "-y");
    if !skip_confirmation {
        let prompt = format!(
            "This will permanently delete {}. Proceed? [y/N] ",
            path.display()
        );
        if !confirm(&prompt)? {
            return Ok("Aborted.".to_string());
        }
    }
    fs::remove_file(&path)
        .map_err(|error| CliError::new(format!("failed to clear {}: {error}", path.display())))?;
    Ok(format!("Cleared {}", path.display()))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn clear_honors_confirmation_force_and_missing_history() {
        let home = std::env::temp_dir().join(format!("cmux-feed-clear-{}", uuid::Uuid::new_v4()));
        let path = feed_history_path(&home);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "history\n").unwrap();

        let aborted = run_feed_command(&["clear".into()], &home, |_| Ok(false)).unwrap();
        assert_eq!(aborted, "Aborted.");
        assert!(path.exists());

        let cleared = run_feed_command(&["clear".into(), "--yes".into()], &home, |_| {
            panic!("forced clear must not prompt")
        })
        .unwrap();
        assert_eq!(cleared, format!("Cleared {}", path.display()));
        assert!(!path.exists());

        let missing = run_feed_command(&["clear".into()], &home, |_| {
            panic!("missing history must not prompt")
        })
        .unwrap();
        assert_eq!(
            missing,
            format!(
                "No Feed history to clear ({} does not exist).",
                path.display()
            )
        );
        let _ = fs::remove_dir_all(home);
    }
}
