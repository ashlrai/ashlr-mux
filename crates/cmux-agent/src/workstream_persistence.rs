use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::ops::Range;
use std::path::Path;
use std::sync::OnceLock;

use regex::Regex;
use serde_json::Value;
use thiserror::Error;

use crate::{WorkstreamItem, WorkstreamPayload};

const READ_CHUNK_BYTES: usize = 64 * 1024;
const SENSITIVE_FRAGMENTS: &[&str] = &[
    "token",
    "secret",
    "password",
    "passwd",
    "api_key",
    "apikey",
    "access_key",
    "private_key",
    "authorization",
    "cookie",
    "credential",
    "env",
];

#[derive(Debug, Error)]
pub enum WorkstreamPersistenceError {
    #[error("workstream persistence IO failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("workstream persistence encoding failed: {0}")]
    Json(#[from] serde_json::Error),
}

pub fn append_workstream_item(
    path: &Path,
    item: &WorkstreamItem,
    home_path: Option<&str>,
) -> Result<(), WorkstreamPersistenceError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let persisted = redacted_for_persistence(item, home_path);
    let mut line = serde_json::to_vec(&persisted)?;
    line.push(b'\n');
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?
        .write_all(&line)?;
    Ok(())
}

pub fn load_recent_workstream_items(
    path: &Path,
    limit: usize,
) -> Result<Vec<WorkstreamItem>, WorkstreamPersistenceError> {
    if limit == 0 || !path.exists() {
        return Ok(Vec::new());
    }
    let mut file = File::open(path)?;
    let mut offset = file.seek(SeekFrom::End(0))?;
    if offset == 0 {
        return Ok(Vec::new());
    }

    let mut tail = Vec::new();
    while offset > 0 {
        let read_size = usize::try_from(offset.min(READ_CHUNK_BYTES as u64)).unwrap_or(0);
        offset -= read_size as u64;
        file.seek(SeekFrom::Start(offset))?;
        let mut chunk = vec![0; read_size];
        file.read_exact(&mut chunk)?;
        chunk.extend(tail);
        tail = chunk;
        if line_ranges(&tail).len() > limit {
            break;
        }
    }

    let ranges = line_ranges(&tail);
    let mut items = Vec::with_capacity(limit.min(ranges.len()));
    for range in ranges
        .into_iter()
        .rev()
        .take(limit)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
    {
        if let Ok(item) = serde_json::from_slice::<WorkstreamItem>(&tail[range]) {
            items.push(item);
        }
    }
    Ok(items)
}

fn line_ranges(bytes: &[u8]) -> Vec<Range<usize>> {
    let mut ranges = Vec::new();
    let mut start = 0;
    for (index, byte) in bytes.iter().enumerate() {
        if *byte != b'\n' {
            continue;
        }
        if start < index {
            ranges.push(start..index);
        }
        start = index + 1;
    }
    if start < bytes.len() {
        ranges.push(start..bytes.len());
    }
    ranges
}

fn redacted_for_persistence(item: &WorkstreamItem, home_path: Option<&str>) -> WorkstreamItem {
    let mut copy = item.clone();
    copy.payload = match &item.payload {
        WorkstreamPayload::PermissionRequest {
            request_id,
            tool_name,
            tool_input_json,
            pattern,
        } => WorkstreamPayload::PermissionRequest {
            request_id: request_id.clone(),
            tool_name: tool_name.clone(),
            tool_input_json: redact_tool_input(tool_input_json, home_path),
            pattern: pattern.clone(),
        },
        WorkstreamPayload::ToolUse {
            tool_name,
            tool_input_json,
        } => WorkstreamPayload::ToolUse {
            tool_name: tool_name.clone(),
            tool_input_json: redact_tool_input(tool_input_json, home_path),
        },
        WorkstreamPayload::ToolResult {
            tool_name,
            result_json,
            is_error,
        } => WorkstreamPayload::ToolResult {
            tool_name: tool_name.clone(),
            result_json: redact_tool_input(result_json, home_path),
            is_error: *is_error,
        },
        payload => payload.clone(),
    };
    copy
}

fn redact_tool_input(input: &str, home_path: Option<&str>) -> String {
    match serde_json::from_str::<Value>(input) {
        Ok(value) => serde_json::to_string(&redact_json(value, None, home_path))
            .unwrap_or_else(|_| redact_string(input, home_path)),
        Err(_) => redact_string(input, home_path),
    }
}

fn redact_json(value: Value, key: Option<&str>, home_path: Option<&str>) -> Value {
    if key.is_some_and(is_sensitive_key) {
        return Value::String("<redacted>".to_string());
    }
    match value {
        Value::Object(object) => Value::Object(
            object
                .into_iter()
                .map(|(key, value)| {
                    let value = redact_json(value, Some(&key), home_path);
                    (key, value)
                })
                .collect(),
        ),
        Value::Array(values) => Value::Array(
            values
                .into_iter()
                .map(|value| redact_json(value, None, home_path))
                .collect(),
        ),
        Value::String(value) => Value::String(redact_string(&value, home_path)),
        value => value,
    }
}

fn is_sensitive_key(key: &str) -> bool {
    let normalized = key.to_ascii_lowercase().replace('-', "_");
    SENSITIVE_FRAGMENTS
        .iter()
        .any(|fragment| normalized.contains(fragment))
}

fn redact_string(value: &str, home_path: Option<&str>) -> String {
    let home_redacted = home_path
        .filter(|home| !home.is_empty())
        .map_or_else(|| value.to_string(), |home| value.replace(home, "~"));
    environment_assignment_regex()
        .replace_all(&home_redacted, "$1=<redacted>")
        .into_owned()
}

fn environment_assignment_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(
            r#"(?i)\b((?:[A-Z_][A-Z0-9_]*?)?(?:TOKEN|SECRET|PASSWORD|PASSWD|API[_-]?KEY|ACCESS[_-]?KEY|PRIVATE[_-]?KEY|AUTHORIZATION|COOKIE|CREDENTIAL)[A-Z0-9_]*)=(?:"[^"]*"|'[^']*'|[^\s]+)"#,
        )
        .expect("static workstream redaction regex")
    })
}

#[cfg(test)]
mod tests {
    use std::fs;

    use crate::{make_item, HookEventName, WorkstreamEvent};

    fn permission_item(request_id: &str, tool_input: &str) -> crate::WorkstreamItem {
        let event = WorkstreamEvent::new(
            format!("session-{request_id}"),
            HookEventName::PermissionRequest,
            "claude",
        )
        .with_request_id(request_id)
        .with_tool_name("Write")
        .with_tool_input_json(tool_input);
        make_item(&event, None, &|_| None)
    }

    #[test]
    fn append_redacts_secrets_environment_assignments_and_home_paths() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("workstream.jsonl");
        let item = permission_item(
            "r1",
            r#"{"token":"abc","command":"API_KEY=secret C:/Users/Test/project"}"#,
        );

        super::append_workstream_item(&path, &item, Some("C:/Users/Test")).unwrap();
        let line = fs::read_to_string(&path).unwrap();
        assert!(!line.contains("abc"));
        assert!(!line.contains("API_KEY=secret"));
        assert!(!line.contains("C:/Users/Test"));
        assert!(line.contains("<redacted>"));
        assert!(line.contains("~/project"));
    }

    #[test]
    fn recent_load_is_oldest_first_bounded_and_skips_malformed_lines() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("workstream.jsonl");
        super::append_workstream_item(&path, &permission_item("r1", "{}"), None).unwrap();
        fs::write(
            &path,
            format!("{}not-json\n", fs::read_to_string(&path).unwrap()),
        )
        .unwrap();
        for id in ["r2", "r3"] {
            super::append_workstream_item(&path, &permission_item(id, "{}"), None).unwrap();
        }

        let items = super::load_recent_workstream_items(&path, 2).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].workstream_id, "session-r2");
        assert_eq!(items[1].workstream_id, "session-r3");
    }

    #[test]
    fn missing_history_loads_empty() {
        let dir = tempfile::tempdir().unwrap();
        assert!(
            super::load_recent_workstream_items(&dir.path().join("missing"), 200)
                .unwrap()
                .is_empty()
        );
    }
}
