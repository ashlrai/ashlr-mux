//! Platform-neutral extraction of desktop launch path arguments.

use std::path::{Path, PathBuf};

pub fn launch_open_directories(args: &[String], cwd: &Path) -> Vec<PathBuf> {
    let mut directories = Vec::new();
    let mut index = 0;
    while index < args.len() {
        if args[index] == "--open-path" {
            if let Some(raw) = args.get(index + 1) {
                let path = PathBuf::from(raw);
                let path = if path.is_absolute() {
                    path
                } else {
                    cwd.join(path)
                };
                if path.is_dir() {
                    directories.push(path);
                }
            }
            index += 2;
        } else {
            index += 1;
        }
    }
    directories
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_only_explicit_existing_directories() {
        let root = std::env::temp_dir().join(format!(
            "cmux-launch-args-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let project = root.join("project");
        std::fs::create_dir_all(&project).unwrap();
        let args = vec![
            "cmux.exe".to_string(),
            "--open-path".to_string(),
            "project".to_string(),
            "--open-path".to_string(),
            "missing".to_string(),
        ];
        assert_eq!(launch_open_directories(&args, &root), vec![project]);
        std::fs::remove_dir_all(root).unwrap();
    }
}
