//! Hidden stdio/broken-pipe regression probes.
//!
//! Windows has no `SIGPIPE`, `F_GETNOSIGPIPE`, or `execv`. The canonical
//! commands are still useful as an observable process/pipe contract, so the
//! Windows port reports the equivalent safe disposition (`default`, flags 0),
//! uses a child spawn for the exec-mode observation, and verifies that a child
//! closing stdin turns into an ignored `BrokenPipe` write error.

use std::fs;
use std::io::{ErrorKind, Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use serde::Serialize;
use uuid::Uuid;

use crate::invocation::CliError;

#[derive(Debug, Serialize)]
struct InspectionPayload {
    signal: &'static str,
    stdout_nosigpipe: i32,
    stderr_nosigpipe: i32,
}

struct TemporaryInspection(PathBuf);

impl TemporaryInspection {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!("cmux-sigpipe-{}.json", Uuid::new_v4())))
    }

    fn path(&self) -> &PathBuf {
        &self.0
    }
}

impl Drop for TemporaryInspection {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

/// Render or write the canonical inspection payload. `None` means `--out`
/// consumed the output; otherwise the caller should print the returned JSON.
pub fn run_sigpipe_inspect(args: &[String]) -> Result<Option<String>, CliError> {
    let output_path = match args {
        [] => None,
        [flag, path] if flag == "--out" => Some(PathBuf::from(path)),
        _ => {
            return Err(CliError::new(
                "Unknown SIGPIPE inspect arguments. Expected no args or --out <path>.",
            ));
        }
    };
    let output = inspection_json()?;
    if let Some(path) = output_path {
        fs::write(&path, &output).map_err(|error| {
            CliError::new(format!("failed to write {}: {error}", path.display()))
        })?;
        Ok(None)
    } else {
        Ok(Some(output))
    }
}

/// Verify that writing a large stdin payload to a child which exits without
/// reading it completes normally rather than terminating the CLI.
pub fn run_sigpipe_stdin_pipe_probe() -> Result<&'static str, CliError> {
    let executable = probe_executable_path()?;
    let inspection = TemporaryInspection::new();
    let mut command = inspection_command(&executable, inspection.path());
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| CliError::new(format!("SIGPIPE stdin-pipe probe failed (-1): {error}")))?;
    let mut stdin = child.stdin.take().ok_or_else(|| {
        CliError::new("SIGPIPE stdin-pipe probe failed (-1): missing child stdin")
    })?;
    let writer = thread::spawn(move || stdin.write_all(&vec![b'x'; 1_048_576]));
    let deadline = Instant::now() + Duration::from_secs(5);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                let stderr = read_child_stderr(&mut child);
                let _ = writer.join();
                return Err(CliError::new(format!(
                    "SIGPIPE stdin-pipe probe timed out: {stderr}"
                )));
            }
            Err(error) => {
                let _ = writer.join();
                return Err(CliError::new(format!(
                    "SIGPIPE stdin-pipe probe failed (-1): {error}"
                )));
            }
        }
    };
    let write_result = writer
        .join()
        .map_err(|_| CliError::new("SIGPIPE stdin-pipe probe failed (-1): writer panicked"))?;
    let stderr = read_child_stderr(&mut child);
    if let Err(error) = write_result {
        if error.kind() != ErrorKind::BrokenPipe {
            return Err(CliError::new(format!(
                "SIGPIPE stdin-pipe probe failed ({}): {error}",
                status_code(&status)
            )));
        }
    }
    if !status.success() {
        return Err(CliError::new(format!(
            "SIGPIPE stdin-pipe probe failed ({}): {stderr}",
            status_code(&status)
        )));
    }
    Ok("ok")
}

/// Run one child-disposition probe. Spawn modes return the child inspection for
/// the parent to print; exec mode lets the child write directly and returns
/// `None`, matching the observable canonical contract.
pub fn run_sigpipe_probe(args: &[String]) -> Result<Option<String>, CliError> {
    let mode = args
        .first()
        .map(|value| value.trim().to_lowercase())
        .unwrap_or_else(|| "spawn".to_string());
    let executable = probe_executable_path()?;
    if mode == "exec" {
        let status = Command::new(&executable)
            .arg("__sigpipe-inspect")
            .stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()
            .map_err(|error| CliError::new(format!("SIGPIPE exec probe failed: {error}")))?;
        if !status.success() {
            return Err(CliError::new(format!(
                "SIGPIPE exec probe failed ({})",
                status_code(&status)
            )));
        }
        return Ok(None);
    }
    if mode != "spawn" && mode != "spawn-stderr" {
        return Err(CliError::new(format!(
            "Unknown SIGPIPE probe mode '{mode}'. Expected spawn, spawn-stderr, or exec."
        )));
    }

    let inspection = TemporaryInspection::new();
    let mut command = inspection_command(&executable, inspection.path());
    command.stdin(Stdio::null()).stderr(Stdio::inherit());
    if mode == "spawn-stderr" {
        command.stdout(Stdio::inherit());
    } else {
        command.stdout(Stdio::null());
    }
    let status = command
        .status()
        .map_err(|error| CliError::new(format!("SIGPIPE {mode} probe failed (-1): {error}")))?;
    if !status.success() {
        let label = if mode == "spawn-stderr" {
            "stderr-spawn"
        } else {
            "spawn"
        };
        return Err(CliError::new(format!(
            "SIGPIPE {label} probe failed ({})",
            status_code(&status)
        )));
    }
    fs::read_to_string(inspection.path())
        .map(Some)
        .map_err(|error| {
            CliError::new(format!(
                "failed to read SIGPIPE inspection {}: {error}",
                inspection.path().display()
            ))
        })
}

fn inspection_json() -> Result<String, CliError> {
    serde_json::to_string_pretty(&InspectionPayload {
        signal: "default",
        stdout_nosigpipe: 0,
        stderr_nosigpipe: 0,
    })
    .map_err(|error| CliError::new(format!("failed to encode SIGPIPE inspection: {error}")))
}

fn probe_executable_path() -> Result<PathBuf, CliError> {
    let candidate = std::env::var_os("CMUX_CLI_PATH")
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::current_exe().ok())
        .filter(|path| path.is_file());
    candidate.ok_or_else(|| CliError::new("SIGPIPE probe could not resolve cmux executable path"))
}

fn inspection_command(executable: &PathBuf, output_path: &PathBuf) -> Command {
    let mut command = Command::new(executable);
    command
        .arg("__sigpipe-inspect")
        .arg("--out")
        .arg(output_path);
    command
}

fn read_child_stderr(child: &mut std::process::Child) -> String {
    let mut output = String::new();
    if let Some(mut stderr) = child.stderr.take() {
        let _ = stderr.read_to_string(&mut output);
    }
    output
}

fn status_code(status: &std::process::ExitStatus) -> i32 {
    status.code().unwrap_or(-1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_string()).collect()
    }

    #[test]
    fn inspect_renders_writes_and_rejects_arguments() {
        let output = run_sigpipe_inspect(&[]).unwrap().unwrap();
        let value: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(value["signal"], "default");
        assert_eq!(value["stdout_nosigpipe"], 0);
        assert_eq!(value["stderr_nosigpipe"], 0);

        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("inspection.json");
        assert_eq!(
            run_sigpipe_inspect(&strings(&["--out", path.to_str().unwrap()])).unwrap(),
            None
        );
        assert_eq!(fs::read_to_string(path).unwrap(), output);
        assert_eq!(
            run_sigpipe_inspect(&strings(&["--out"]))
                .unwrap_err()
                .message,
            "Unknown SIGPIPE inspect arguments. Expected no args or --out <path>."
        );
    }

    #[test]
    fn unknown_probe_mode_matches_canonical_error() {
        let error = run_sigpipe_probe(&strings(&["wat"])).unwrap_err();
        assert_eq!(
            error.message,
            "Unknown SIGPIPE probe mode 'wat'. Expected spawn, spawn-stderr, or exec."
        );
    }
}
