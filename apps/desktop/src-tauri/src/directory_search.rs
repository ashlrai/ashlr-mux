use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const DEFAULT_RESULT_LIMIT: usize = 100;
const MAX_RESULT_LIMIT: usize = 500;
const MAX_FILE_BYTES: u64 = 1_000_000;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectorySearchRequest {
    pub directory: String,
    pub query: String,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectorySearchResult {
    pub path: String,
    pub line_number: usize,
    pub line_text: String,
}

fn should_skip_dir(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    matches!(
        name,
        ".git" | ".hg" | ".svn" | "node_modules" | "target" | "dist" | "build"
    )
}

fn looks_binary(bytes: &[u8]) -> bool {
    bytes.iter().take(1024).any(|byte| *byte == 0)
}

fn search_file(
    path: &Path,
    query_lower: &str,
    results: &mut Vec<DirectorySearchResult>,
    limit: usize,
) {
    let Ok(metadata) = fs::metadata(path) else {
        return;
    };
    if metadata.len() > MAX_FILE_BYTES {
        return;
    }
    let Ok(bytes) = fs::read(path) else {
        return;
    };
    if looks_binary(&bytes) {
        return;
    }
    let Ok(text) = String::from_utf8(bytes) else {
        return;
    };
    for (line_index, line) in text.lines().enumerate() {
        if !line.to_lowercase().contains(query_lower) {
            continue;
        }
        results.push(DirectorySearchResult {
            path: path.to_string_lossy().to_string(),
            line_number: line_index + 1,
            line_text: line.trim().to_string(),
        });
        if results.len() >= limit {
            return;
        }
    }
}

pub fn search_directory(
    request: DirectorySearchRequest,
) -> Result<Vec<DirectorySearchResult>, String> {
    let directory = PathBuf::from(request.directory.trim());
    if !directory.is_dir() {
        return Err("Directory does not exist.".to_string());
    }
    let query = request.query.trim().to_lowercase();
    if query.is_empty() {
        return Ok(Vec::new());
    }
    let limit = request
        .limit
        .unwrap_or(DEFAULT_RESULT_LIMIT)
        .clamp(1, MAX_RESULT_LIMIT);
    let mut queue = VecDeque::from([directory]);
    let mut results = Vec::new();

    while let Some(dir) = queue.pop_front() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if !should_skip_dir(&path) {
                    queue.push_back(path);
                }
                continue;
            }
            if path.is_file() {
                search_file(&path, &query, &mut results, limit);
                if results.len() >= limit {
                    return Ok(results);
                }
            }
        }
    }

    Ok(results)
}

#[tauri::command]
pub fn find_in_directory(
    request: DirectorySearchRequest,
) -> Result<Vec<DirectorySearchResult>, String> {
    search_directory(request)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn search_directory_returns_matching_lines() {
        let tmp = TempDir::new().expect("temp dir");
        let file = tmp.path().join("src").join("main.rs");
        fs::create_dir_all(file.parent().unwrap()).expect("mkdir");
        fs::write(&file, "fn main() {}\nlet needle = true;\n").expect("write");

        let results = search_directory(DirectorySearchRequest {
            directory: tmp.path().to_string_lossy().to_string(),
            query: "NEEDLE".to_string(),
            limit: Some(10),
        })
        .expect("search");

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].line_number, 2);
        assert!(results[0].path.ends_with("main.rs"));
        assert_eq!(results[0].line_text, "let needle = true;");
    }

    #[test]
    fn search_directory_skips_heavy_generated_dirs() {
        let tmp = TempDir::new().expect("temp dir");
        let skipped = tmp.path().join("node_modules").join("pkg.js");
        fs::create_dir_all(skipped.parent().unwrap()).expect("mkdir");
        fs::write(skipped, "needle").expect("write");

        let results = search_directory(DirectorySearchRequest {
            directory: tmp.path().to_string_lossy().to_string(),
            query: "needle".to_string(),
            limit: Some(10),
        })
        .expect("search");

        assert!(results.is_empty());
    }
}
