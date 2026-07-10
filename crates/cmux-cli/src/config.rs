//! Canonical read-only `cmux config` helpers that do not require a socket.

use std::path::{Path, PathBuf};

use crate::invocation::CliError;

pub const CONFIG_USAGE: &str = "Usage: cmux config <doctor|check|validate|path|paths|docs|documentation|reload|get|set|sidebar-font-size|surface-tab-bar-font-size>\n\nInspect cmux.json, print configuration references, update selected Ghostty config keys, or reload the running app.\n\nSubcommands:\n  doctor|check|validate [--path <path>]   Validate JSONC syntax for cmux config files.\n  path|paths                              Print cmux.json paths, docs URL, and schema URL.\n  docs|documentation                      Print the same output as `cmux docs settings`.\n  reload                                  Reload Ghostty config + cmux.json and refresh terminals (alias for `cmux reload-config`).\n  get <key>                               Print sidebar-font-size or surface-tab-bar-font-size.\n  set <key> <points>                      Set sidebar-font-size (10-20 pt) or surface-tab-bar-font-size (8-24 pt), then reload if cmux is running.\n  sidebar-font-size [points]              Get or set the left sidebar text size.\n  surface-tab-bar-font-size [points]      Get or set the workspace tab bar text size.\n\nConfig files:\n  ~/.config/cmux/cmux.json\n  legacy config: ~/.config/cmux/settings.json\n  legacy app support: ~/Library/Application Support/com.cmuxterm.app/settings.json\n\nRelated (not cmux-owned, but cmux reads it for terminal behavior):\n  ~/.config/ghostty/config\n\nExamples:\n  cmux config doctor\n  cmux config doctor --path .cmux/cmux.json\n  cmux config set sidebar-font-size 14\n  cmux config sidebar-font-size 12.5\n  cmux config set surface-tab-bar-font-size 13\n  cmux config surface-tab-bar-font-size 11\n  cmux config reload";

#[derive(Debug)]
pub struct ConfigCommandOutput {
    pub output: String,
    pub failure: Option<CliError>,
}

impl ConfigCommandOutput {
    fn success(output: String) -> Self {
        Self {
            output,
            failure: None,
        }
    }
}

pub fn run_config_no_socket(
    command_args: &[String],
    global_json: bool,
) -> Result<ConfigCommandOutput, CliError> {
    let parsed = crate::docs::parse_docs_settings_args(command_args, global_json);
    if parsed.help_requested() || parsed.arguments.is_empty() {
        return Ok(ConfigCommandOutput::success(CONFIG_USAGE.to_string()));
    }

    let args = &parsed.arguments;
    let subcommand = args[0].to_lowercase();
    match subcommand.as_str() {
        "path" | "paths" => {
            require_arity(args, 1, "Usage: cmux config path")?;
            crate::settings::run_settings_no_socket(&["path".to_string()], parsed.wants_json)
                .map(ConfigCommandOutput::success)
        }
        "docs" | "documentation" => {
            require_arity(args, 1, "Usage: cmux config docs")?;
            crate::docs::run_docs_command(&["settings".to_string()], parsed.wants_json)
                .map(ConfigCommandOutput::success)
        }
        "doctor" | "check" | "validate" => run_config_doctor(&args[1..], parsed.wants_json),
        "get" | "sidebar-font-size" | "surface-tab-bar-font-size" => Err(CliError::new(format!(
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

#[derive(Debug)]
struct DoctorTarget {
    label: String,
    display_path: String,
    path: PathBuf,
    missing_is_error: bool,
}

#[derive(Debug)]
struct DoctorFinding {
    label: String,
    display_path: String,
    path: String,
    status: &'static str,
    message: Option<String>,
    keys: Vec<String>,
    byte_count: Option<usize>,
}

impl DoctorFinding {
    fn is_error(&self) -> bool {
        self.status == "error"
    }

    fn payload(&self) -> serde_json::Value {
        let mut value = serde_json::json!({
            "display_path": self.display_path,
            "keys": self.keys,
            "label": self.label,
            "ok": !self.is_error(),
            "path": self.path,
            "status": self.status,
        });
        let object = value.as_object_mut().expect("doctor finding is an object");
        if let Some(message) = &self.message {
            object.insert("message".into(), serde_json::json!(message));
        }
        if let Some(byte_count) = self.byte_count {
            object.insert("bytes".into(), serde_json::json!(byte_count));
        }
        value
    }
}

fn run_config_doctor(
    arguments: &[&str],
    wants_json: bool,
) -> Result<ConfigCommandOutput, CliError> {
    let paths = parse_doctor_paths(arguments)?;
    let targets = if paths.is_empty() {
        default_doctor_targets()?
    } else {
        paths
            .into_iter()
            .enumerate()
            .map(|(index, raw_path)| {
                let path = absolute_path(&raw_path)?;
                Ok(DoctorTarget {
                    label: format!("custom {}", index + 1),
                    display_path: tilde_path(&path),
                    path,
                    missing_is_error: true,
                })
            })
            .collect::<Result<Vec<_>, CliError>>()?
    };
    let findings: Vec<_> = targets.iter().map(doctor_finding).collect();
    let error_count = findings.iter().filter(|finding| finding.is_error()).count();
    let output = if wants_json {
        render_doctor_json(&findings, error_count)?
    } else {
        render_doctor_text(&findings)
    };
    Ok(ConfigCommandOutput {
        output,
        failure: (error_count > 0)
            .then(|| CliError::new(format!("cmux config doctor found {error_count} error(s)"))),
    })
}

fn parse_doctor_paths(arguments: &[&str]) -> Result<Vec<String>, CliError> {
    let mut paths = Vec::new();
    let mut index = 0;
    while index < arguments.len() {
        let argument = arguments[index];
        if argument == "--path" {
            let Some(path) = arguments.get(index + 1) else {
                return Err(CliError::new("cmux config doctor --path requires a path"));
            };
            paths.push((*path).to_string());
            index += 2;
        } else if let Some(path) = argument.strip_prefix("--path=") {
            if path.is_empty() {
                return Err(CliError::new("cmux config doctor --path requires a path"));
            }
            paths.push(path.to_string());
            index += 1;
        } else if argument.starts_with('-') {
            return Err(CliError::new(format!(
                "Unknown config doctor option '{argument}'"
            )));
        } else {
            return Err(CliError::new(format!(
                "Unknown config doctor argument '{argument}'. Use --path <path>."
            )));
        }
    }
    Ok(paths)
}

fn default_doctor_targets() -> Result<Vec<DoctorTarget>, CliError> {
    let primary = cmux_config::config_path()
        .ok_or_else(|| CliError::new("Could not resolve the user configuration directory"))?;
    let mut targets = vec![DoctorTarget {
        label: "primary".into(),
        display_path: tilde_path(&primary),
        path: primary.clone(),
        missing_is_error: false,
    }];

    if let Some(project) = find_project_config_path()? {
        if project != primary {
            targets.push(DoctorTarget {
                label: "project".into(),
                display_path: tilde_path(&project),
                path: project,
                missing_is_error: false,
            });
        }
    }

    let mut optional = Vec::new();
    if let Some(parent) = primary.parent() {
        optional.push(("legacy config", parent.join("settings.json")));
    }
    if let Some(home) = home_dir() {
        optional.push((
            "legacy app support",
            home.join("Library")
                .join("Application Support")
                .join("com.cmuxterm.app")
                .join("settings.json"),
        ));
    }
    for (label, path) in optional {
        if path != primary && path.exists() && !targets.iter().any(|target| target.path == path) {
            targets.push(DoctorTarget {
                label: label.into(),
                display_path: tilde_path(&path),
                path,
                missing_is_error: false,
            });
        }
    }
    Ok(targets)
}

fn find_project_config_path() -> Result<Option<PathBuf>, CliError> {
    let current = std::env::current_dir()
        .map_err(|error| CliError::new(format!("Could not resolve current directory: {error}")))?;
    Ok(find_project_config_path_from(
        current,
        home_dir().as_deref(),
    ))
}

fn find_project_config_path_from(mut current: PathBuf, home: Option<&Path>) -> Option<PathBuf> {
    loop {
        if home.is_some_and(|home| current == home) {
            return None;
        }
        for candidate in [
            current.join(".cmux").join("cmux.json"),
            current.join("cmux.json"),
        ] {
            if candidate.is_file() {
                return Some(candidate);
            }
        }
        if !current.pop() {
            return None;
        }
    }
}

fn doctor_finding(target: &DoctorTarget) -> DoctorFinding {
    let path = target.path.to_string_lossy().into_owned();
    let metadata = match std::fs::metadata(&target.path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return DoctorFinding {
                label: target.label.clone(),
                display_path: target.display_path.clone(),
                path,
                status: if target.missing_is_error {
                    "error"
                } else {
                    "missing"
                },
                message: Some(if target.missing_is_error {
                    "file not found".into()
                } else {
                    "not found; cmux will use defaults until this file exists".into()
                }),
                keys: Vec::new(),
                byte_count: None,
            };
        }
        Err(error) => return error_finding(target, path, error.to_string(), None),
    };
    if metadata.is_dir() {
        return error_finding(
            target,
            path,
            "path is a directory, expected a file".into(),
            None,
        );
    }

    let bytes = match std::fs::read(&target.path) {
        Ok(bytes) => bytes,
        Err(error) => return error_finding(target, path, error.to_string(), None),
    };
    if bytes.is_empty() {
        return error_finding(target, path, "file is empty".into(), Some(0));
    }
    let sanitized = match cmux_jsonc::preprocess(&bytes) {
        Ok(sanitized) => sanitized,
        Err(error) => return error_finding(target, path, error.to_string(), None),
    };
    let value: serde_json::Value = match serde_json::from_slice(&sanitized) {
        Ok(value) => value,
        Err(error) => return error_finding(target, path, error.to_string(), None),
    };
    let Some(object) = value.as_object() else {
        return error_finding(
            target,
            path,
            "top-level value must be a JSON object".into(),
            Some(bytes.len()),
        );
    };
    let mut keys: Vec<_> = object.keys().cloned().collect();
    keys.sort();
    DoctorFinding {
        label: target.label.clone(),
        display_path: target.display_path.clone(),
        path,
        status: "ok",
        message: Some("JSONC syntax is valid".into()),
        keys,
        byte_count: Some(bytes.len()),
    }
}

fn error_finding(
    target: &DoctorTarget,
    path: String,
    message: String,
    byte_count: Option<usize>,
) -> DoctorFinding {
    DoctorFinding {
        label: target.label.clone(),
        display_path: target.display_path.clone(),
        path,
        status: "error",
        message: Some(message),
        keys: Vec::new(),
        byte_count,
    }
}

fn render_doctor_json(findings: &[DoctorFinding], error_count: usize) -> Result<String, CliError> {
    serde_json::to_string_pretty(&serde_json::json!({
        "docs_url": crate::settings::DOCS_URL,
        "error_count": error_count,
        "findings": findings.iter().map(DoctorFinding::payload).collect::<Vec<_>>(),
        "ok": error_count == 0,
        "reload_command": "cmux reload-config",
        "schema_url": crate::settings::SCHEMA_URL,
    }))
    .map_err(|error| CliError::new(format!("failed to encode config doctor JSON: {error}")))
}

fn render_doctor_text(findings: &[DoctorFinding]) -> String {
    let mut lines = vec!["cmux config doctor".to_string()];
    for finding in findings {
        lines.push(format!(
            "{} {}: {}",
            finding.status.to_uppercase(),
            finding.label,
            finding.display_path
        ));
        lines.push(format!("  path: {}", finding.path));
        if let Some(byte_count) = finding.byte_count {
            lines.push(format!("  bytes: {byte_count}"));
        }
        if !finding.keys.is_empty() {
            lines.push(format!("  keys: {}", finding.keys.join(", ")));
        }
        if let Some(message) = &finding.message {
            lines.push(format!("  {message}"));
        }
    }
    lines.extend([
        String::new(),
        format!("Docs: {}", crate::settings::DOCS_URL),
        format!("Schema: {}", crate::settings::SCHEMA_URL),
        "Reload: cmux reload-config".into(),
    ]);
    lines.join("\n")
}

fn absolute_path(raw_path: &str) -> Result<PathBuf, CliError> {
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
        std::env::current_dir()
            .map_err(|error| {
                CliError::new(format!("Could not resolve current directory: {error}"))
            })?
            .join(expanded)
    };
    Ok(normalize_path(&absolute))
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            _ => normalized.push(component.as_os_str()),
        }
    }
    normalized
}

fn tilde_path(path: &Path) -> String {
    let Some(home) = home_dir() else {
        return path.to_string_lossy().into_owned();
    };
    if path == home {
        return "~".into();
    }
    path.strip_prefix(&home).map_or_else(
        |_| path.to_string_lossy().into_owned(),
        |relative| format!("~/{}", relative.to_string_lossy().replace('\\', "/")),
    )
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

    fn string_args(args: &[&str]) -> Vec<String> {
        args.iter().map(|arg| (*arg).to_string()).collect()
    }

    #[test]
    fn renders_help_and_reuses_settings_reference_outputs() {
        assert_eq!(
            run_config_no_socket(&[], false).unwrap().output,
            CONFIG_USAGE
        );

        let config_paths =
            run_config_no_socket(&["--json".into(), "--".into(), "paths".into()], false)
                .unwrap()
                .output;
        let settings_paths =
            crate::settings::run_settings_no_socket(&["path".into()], true).unwrap();
        assert_eq!(config_paths, settings_paths);

        let docs = run_config_no_socket(&["documentation".into()], true)
            .unwrap()
            .output;
        let value: serde_json::Value = serde_json::from_str(&docs).unwrap();
        assert_eq!(value["topic"], "settings");
    }

    #[test]
    fn doctor_validates_jsonc_and_defers_failure_until_after_the_report() {
        let root =
            std::env::temp_dir().join(format!("cmux-config-doctor-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let valid_path = root.join("valid.json");
        let invalid_path = root.join("invalid.json");
        std::fs::write(&valid_path, b"{ // comment\n \"browser\": {},\n}").unwrap();
        std::fs::write(&invalid_path, b"{\"browser\": }").unwrap();

        let valid = run_config_no_socket(
            &string_args(&["doctor", "--path", valid_path.to_str().unwrap()]),
            true,
        )
        .unwrap();
        let valid_json: serde_json::Value = serde_json::from_str(&valid.output).unwrap();
        assert_eq!(valid_json["ok"], true);
        assert_eq!(
            valid_json["findings"][0]["keys"],
            serde_json::json!(["browser"])
        );
        assert!(valid.failure.is_none());

        let invalid = run_config_no_socket(
            &string_args(&["validate", "--path", invalid_path.to_str().unwrap()]),
            true,
        )
        .unwrap();
        let invalid_json: serde_json::Value = serde_json::from_str(&invalid.output).unwrap();
        assert_eq!(invalid_json["ok"], false);
        assert_eq!(invalid_json["error_count"], 1);
        assert_eq!(
            invalid.failure.unwrap().message,
            "cmux config doctor found 1 error(s)"
        );

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn doctor_reports_edge_findings_and_rejects_unknown_options() {
        let root =
            std::env::temp_dir().join(format!("cmux-config-doctor-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let empty_path = root.join("empty.json");
        let array_path = root.join("array.json");
        let missing_path = root.join("missing.json");
        std::fs::write(&empty_path, []).unwrap();
        std::fs::write(&array_path, b"[]").unwrap();

        let result = run_config_no_socket(
            &string_args(&[
                "check",
                &format!("--path={}", empty_path.display()),
                "--path",
                array_path.to_str().unwrap(),
                "--path",
                missing_path.to_str().unwrap(),
                "--path",
                root.to_str().unwrap(),
            ]),
            true,
        )
        .unwrap();
        let value: serde_json::Value = serde_json::from_str(&result.output).unwrap();
        assert_eq!(value["error_count"], 4);
        assert_eq!(value["findings"][0]["bytes"], 0);
        assert_eq!(
            value["findings"][1]["message"],
            "top-level value must be a JSON object"
        );
        assert_eq!(value["findings"][2]["message"], "file not found");
        assert_eq!(
            value["findings"][3]["message"],
            "path is a directory, expected a file"
        );

        let error = run_config_no_socket(&string_args(&["doctor", "--wat"]), false).unwrap_err();
        assert_eq!(error.message, "Unknown config doctor option '--wat'");

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn project_discovery_prefers_dot_cmux_and_stops_at_home() {
        let root =
            std::env::temp_dir().join(format!("cmux-config-doctor-{}", uuid::Uuid::new_v4()));
        let child = root.join("project").join("child");
        let dot_cmux = root.join("project").join(".cmux").join("cmux.json");
        let direct = root.join("project").join("cmux.json");
        std::fs::create_dir_all(dot_cmux.parent().unwrap()).unwrap();
        std::fs::create_dir_all(&child).unwrap();
        std::fs::write(&dot_cmux, b"{}").unwrap();
        std::fs::write(&direct, b"{}").unwrap();

        assert_eq!(
            find_project_config_path_from(child.clone(), Some(&root)),
            Some(dot_cmux)
        );
        assert_eq!(
            find_project_config_path_from(root.clone(), Some(&root)),
            None
        );

        std::fs::remove_dir_all(root).unwrap();
    }
}
