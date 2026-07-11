//! Bare filesystem path opening through the packaged Windows desktop app.

use std::path::{Component, Path, PathBuf};

use crate::invocation::CliError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathOpenPlan {
    pub desktop_executable: PathBuf,
    pub directory: PathBuf,
}

pub fn prepare_path_open(
    raw_path: &str,
    cwd: &Path,
    current_executable: &Path,
    desktop_override: Option<&Path>,
) -> Result<PathOpenPlan, CliError> {
    let directory = directory_for_path_open(raw_path, cwd)?;
    let desktop_executable = desktop_executable(current_executable, desktop_override)
        .ok_or_else(|| CliError::new(format!("Failed to open {} in cmux", directory.display())))?;
    Ok(PathOpenPlan {
        desktop_executable,
        directory,
    })
}

pub fn run_open_path(raw_path: &str, cwd: &Path) -> Result<String, CliError> {
    let current_executable = std::env::current_exe().map_err(|error| {
        CliError::new(format!("Could not resolve cmux CLI executable: {error}"))
    })?;
    let override_path = std::env::var_os("CMUX_DESKTOP_EXE").map(PathBuf::from);
    let plan = prepare_path_open(raw_path, cwd, &current_executable, override_path.as_deref())?;
    std::process::Command::new(&plan.desktop_executable)
        .arg("--open-path")
        .arg(&plan.directory)
        .spawn()
        .map_err(|_| {
            CliError::new(format!(
                "Failed to open {} in cmux",
                plan.directory.display()
            ))
        })?;
    Ok("OK".into())
}

fn directory_for_path_open(raw_path: &str, cwd: &Path) -> Result<PathBuf, CliError> {
    let expanded = if raw_path == "~" {
        home_dir().ok_or_else(|| CliError::new("Could not resolve the user home directory"))?
    } else if let Some(relative) = raw_path
        .strip_prefix("~/")
        .or_else(|| raw_path.strip_prefix("~\\"))
    {
        home_dir()
            .ok_or_else(|| CliError::new("Could not resolve the user home directory"))?
            .join(relative)
    } else {
        PathBuf::from(raw_path)
    };
    let absolute = if expanded.is_absolute() {
        expanded
    } else {
        cwd.join(expanded)
    };
    let resolved = normalize_path(&absolute);
    if resolved.is_dir() {
        return Ok(resolved);
    }
    if resolved.is_file() {
        return resolved
            .parent()
            .map(Path::to_path_buf)
            .ok_or_else(|| CliError::new(format!("Path does not exist: {}", resolved.display())));
    }
    Err(CliError::new(format!(
        "Path does not exist: {}",
        resolved.display()
    )))
}

fn desktop_executable(
    current_executable: &Path,
    desktop_override: Option<&Path>,
) -> Option<PathBuf> {
    let mut candidates = desktop_override
        .into_iter()
        .map(Path::to_path_buf)
        .collect::<Vec<_>>();
    if let Some(directory) = current_executable.parent() {
        candidates.push(directory.join("cmux.exe"));
        if let Some(parent) = directory.parent() {
            candidates.push(parent.join("cmux.exe"));
        }
    }
    let current = normalize_compare_path(current_executable);
    candidates
        .into_iter()
        .find(|candidate| candidate.is_file() && normalize_compare_path(candidate) != current)
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            _ => normalized.push(component.as_os_str()),
        }
    }
    normalized
}

fn normalize_compare_path(path: &Path) -> String {
    normalize_path(path)
        .to_string_lossy()
        .replace('\\', "/")
        .to_lowercase()
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .or_else(|| std::env::var_os("USERPROFILE").filter(|value| !value.is_empty()))
        .map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plans_directory_and_file_parent_without_recursing_into_cli() {
        let root = std::env::temp_dir().join(format!("cmux-open-path-{}", uuid::Uuid::new_v4()));
        let project = root.join("project");
        let file = project.join("README.md");
        let cli = root.join("cmux-x86_64-pc-windows-msvc.exe");
        let desktop = root.join("cmux.exe");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(&file, b"readme").unwrap();
        std::fs::write(&cli, []).unwrap();
        std::fs::write(&desktop, []).unwrap();

        let directory_plan = prepare_path_open("project", &root, &cli, None).unwrap();
        assert_eq!(directory_plan.directory, project);
        assert_eq!(directory_plan.desktop_executable, desktop);
        let file_plan = prepare_path_open("project/README.md", &root, &cli, None).unwrap();
        assert_eq!(file_plan.directory, project);

        let missing = prepare_path_open("missing", &root, &cli, None).unwrap_err();
        assert!(missing.message.starts_with("Path does not exist:"));
        std::fs::remove_dir_all(root).unwrap();
    }
}
