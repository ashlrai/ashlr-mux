//! Hidden local helpers used by the diff-viewer branch-base picker.
//!
//! These are intentionally no-socket commands: the diff-viewer HTTP/custom
//! scheme layer can invoke them while serving a local page. The security
//! boundary for regenerated pages is the same `cmux-diff` token + trusted-root
//! manifest jail used by the desktop scheme handler.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::invocation::CliError;
use cmux_diff::manifest::manifest_file_name;
use cmux_diff::{load_manifest_files, DiffSessionRegistry, RegisteredFile};
use serde_json::{json, Value};

const DEFAULT_GROUP: &str = "branch";

#[derive(Debug, Clone, Default)]
struct QueryArgs {
    repo: Option<String>,
    token: Option<String>,
    base: Option<String>,
    group: Option<String>,
}

#[derive(Debug, Clone)]
struct GitRef {
    name: String,
    secondary: Option<String>,
}

/// `cmux __diff-viewer-refs --repo <path> [--token <token>] [--base <ref>]`
/// prints the branch-picker frozen contract: `{ "groups": [...] }`.
pub fn run_diff_viewer_refs_command(args: &[String], cwd: &Path) -> Result<String, CliError> {
    let parsed = parse_query_args(args)?;
    if let Some(token) = parsed.token.as_deref() {
        validate_token(token)?;
    }
    let repo = resolve_repo(parsed.repo.as_deref(), cwd)?;
    let refs = git_refs(&repo)?;
    let head = git_output(&repo, &["symbolic-ref", "--quiet", "--short", "HEAD"])
        .ok()
        .and_then(first_nonempty_line)
        .unwrap_or_else(|| "HEAD".to_string());
    Ok(refs_response_json(&refs, &head, parsed.base.as_deref()).to_string())
}

/// `cmux __diff-viewer-branch --repo <path> --token <token> --base <ref>`
/// writes a patch + HTML page under the trusted diff-viewer root, updates the
/// token manifest, and prints `{ "url": "/diff-<group>-branch.html", ... }`.
pub fn run_diff_viewer_branch_command(args: &[String], cwd: &Path) -> Result<String, CliError> {
    run_diff_viewer_branch_command_with_root(args, cwd, &default_trusted_root()?)
}

/// Root-injected branch regeneration used by the loopback HTTP server. Keeping
/// generation here ensures the hidden CLI helper and HTTP route cannot drift.
pub fn run_diff_viewer_branch_command_with_root(
    args: &[String],
    cwd: &Path,
    trusted_root: &Path,
) -> Result<String, CliError> {
    let parsed = parse_query_args(args)?;
    let token = parsed
        .token
        .as_deref()
        .ok_or_else(|| CliError::new("__diff-viewer-branch requires --token <token>"))?;
    validate_token(token)?;
    let base = parsed
        .base
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| CliError::new("__diff-viewer-branch requires --base <ref>"))?;
    let repo = resolve_repo(parsed.repo.as_deref(), cwd)?;
    git_output(
        &repo,
        &["rev-parse", "--verify", &format!("{base}^{{commit}}")],
    )?;
    let patch = git_output(
        &repo,
        &[
            "diff",
            "--binary",
            "--find-renames",
            "--no-ext-diff",
            &format!("{base}...HEAD"),
        ],
    )?;

    let token_dir = trusted_root.join(token);
    std::fs::create_dir_all(&token_dir).map_err(|error| {
        CliError::new(format!(
            "failed to create diff-viewer token directory: {error}"
        ))
    })?;

    let group = sanitize_group(parsed.group.as_deref().unwrap_or(DEFAULT_GROUP));
    let html_request_path = format!("/diff-{group}-branch.html");
    let patch_request_path = format!("/diff-{group}-branch.patch");
    let html_path = token_dir.join(format!("diff-{group}-branch.html"));
    let patch_path = token_dir.join(format!("diff-{group}-branch.patch"));
    std::fs::write(&patch_path, patch).map_err(|error| {
        CliError::new(format!("failed to write diff-viewer branch patch: {error}"))
    })?;
    std::fs::write(
        &html_path,
        branch_html(&repo, token, base, &html_request_path, &patch_request_path),
    )
    .map_err(|error| CliError::new(format!("failed to write diff-viewer branch page: {error}")))?;

    upsert_manifest(
        trusted_root,
        token,
        &[
            RegisteredFile {
                request_path: html_request_path.clone(),
                file_path: html_path,
                mime_type: "text/html".to_string(),
            },
            RegisteredFile {
                request_path: patch_request_path.clone(),
                file_path: patch_path,
                mime_type: "text/x-diff".to_string(),
            },
        ],
    )?;

    Ok(json!({
        "url": html_request_path,
        "request_path": html_request_path,
        "patch_request_path": patch_request_path,
        "token": token,
    })
    .to_string())
}

fn validate_token(token: &str) -> Result<(), CliError> {
    if DiffSessionRegistry::is_valid_token(token) {
        Ok(())
    } else {
        Err(CliError::new("invalid diff viewer token"))
    }
}

fn resolve_repo(raw: Option<&str>, cwd: &Path) -> Result<PathBuf, CliError> {
    let candidate = raw
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| cwd.to_path_buf());
    let repo = if candidate.is_absolute() {
        candidate
    } else {
        cwd.join(candidate)
    };
    let root = git_output(&repo, &["rev-parse", "--show-toplevel"])?;
    let root = first_nonempty_line(root).ok_or_else(|| CliError::new("git repo root is empty"))?;
    Ok(PathBuf::from(root))
}

fn git_refs(repo: &Path) -> Result<Vec<GitRef>, CliError> {
    let output = git_output(
        repo,
        &[
            "for-each-ref",
            "--format=%(refname:short)%00%(committerdate:relative)",
            "refs/heads",
            "refs/remotes",
        ],
    )?;
    let mut refs = Vec::new();
    let mut seen = BTreeSet::new();
    for line in output.lines() {
        let mut parts = line.splitn(2, '\0');
        let name = parts.next().unwrap_or("").trim();
        if name.is_empty() || name.ends_with("/HEAD") || !seen.insert(name.to_string()) {
            continue;
        }
        refs.push(GitRef {
            name: name.to_string(),
            secondary: parts
                .next()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned),
        });
    }
    Ok(refs)
}

fn refs_response_json(refs: &[GitRef], head: &str, current: Option<&str>) -> Value {
    let current = current.map(str::trim).filter(|value| !value.is_empty());
    let mut by_name: BTreeMap<&str, &GitRef> = BTreeMap::new();
    for r in refs {
        by_name.insert(&r.name, r);
    }

    let mut groups = Vec::new();
    let suggested_names = ["origin/main", "origin/master", "main", "master"];
    let suggested = suggested_names
        .iter()
        .filter_map(|name| by_name.get(name).copied())
        .map(|r| ref_row(r, current, Some("default branch")))
        .collect::<Vec<_>>();
    if !suggested.is_empty() {
        groups.push(json!({ "id": "suggested", "label": "Suggested", "rows": suggested }));
    }

    let branches = refs
        .iter()
        .filter(|r| !r.name.contains('/'))
        .map(|r| {
            ref_row(
                r,
                current,
                if r.name == head {
                    Some("current branch")
                } else {
                    None
                },
            )
        })
        .collect::<Vec<_>>();
    if !branches.is_empty() {
        groups.push(json!({ "id": "branches", "label": "Branches", "rows": branches }));
    }

    let remotes = refs
        .iter()
        .filter(|r| r.name.contains('/'))
        .map(|r| ref_row(r, current, None))
        .collect::<Vec<_>>();
    if !remotes.is_empty() {
        groups.push(json!({ "id": "remotes", "label": "Remotes", "rows": remotes }));
    }

    json!({ "groups": groups })
}

fn ref_row(r: &GitRef, current: Option<&str>, reason: Option<&str>) -> Value {
    let mut row = json!({
        "ref": r.name,
        "label": r.name,
        "current": current == Some(r.name.as_str()),
    });
    if let Some(secondary) = &r.secondary {
        row["secondary"] = json!(secondary);
    }
    if let Some(reason) = reason {
        row["reason"] = json!(reason);
    }
    row
}

fn branch_html(
    repo: &Path,
    token: &str,
    base: &str,
    html_request_path: &str,
    patch_request_path: &str,
) -> String {
    let repo_root = repo.to_string_lossy();
    let config = json!({
        "payload": {
            "title": format!("Diff against {base}"),
            "sourceLabel": "branch",
            "repoRoot": repo_root,
            "branchBaseRef": base,
            "patchURL": patch_request_path,
            "branchPicker": {
                "repoRoot": repo_root,
                "headRef": "HEAD",
                "currentRef": base,
                "currentReason": "selected base",
                "confidence": "high",
                "aheadBehind": null,
                "refsURL": format!("/__cmux_diff_viewer_refs?repo={}&token={}&base={}", percent_encode(&repo_root), percent_encode(token), percent_encode(base)),
                "regenerateURLTemplate": format!("/__cmux_diff_viewer_branch?repo={}&token={}&group={}&base={{ref}}", percent_encode(&repo_root), percent_encode(token), percent_encode(DEFAULT_GROUP)),
            }
        }
    });
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>Diff against {}</title></head>\
         <body data-cmux-webview-kind=\"diff\"><div id=\"root\"></div>\
         <script id=\"cmux-diff-viewer-config\" type=\"application/json\">{}</script>\
         <script type=\"module\" src=\"/main.mjs\"></script>\
         <p data-cmux-diff-request-path=\"{}\" hidden></p></body></html>",
        escape_html(base),
        config,
        escape_html(html_request_path)
    )
}

fn upsert_manifest(root: &Path, token: &str, entries: &[RegisteredFile]) -> Result<(), CliError> {
    let mut by_path: BTreeMap<String, RegisteredFile> = BTreeMap::new();
    if let Some(existing) = load_manifest_files(root, token) {
        for file in existing {
            by_path.insert(file.request_path.clone(), file);
        }
    }
    for file in entries {
        by_path.insert(file.request_path.clone(), file.clone());
    }
    let files: Vec<Value> = by_path
        .values()
        .map(|file| {
            json!({
                "request_path": file.request_path,
                "file_path": file.file_path.to_string_lossy(),
                "mime_type": file.mime_type,
            })
        })
        .collect();
    let manifest = json!({ "token": token, "files": files }).to_string();
    std::fs::write(root.join(manifest_file_name(token)), manifest)
        .map_err(|error| CliError::new(format!("failed to write diff-viewer manifest: {error}")))
}

fn default_trusted_root() -> Result<PathBuf, CliError> {
    let root = std::env::temp_dir().join("cmux-diff-viewer");
    std::fs::create_dir_all(&root)
        .map_err(|error| CliError::new(format!("failed to create diff-viewer root: {error}")))?;
    std::fs::canonicalize(&root)
        .map_err(|error| CliError::new(format!("failed to canonicalize diff-viewer root: {error}")))
}

fn git_output(repo: &Path, args: &[&str]) -> Result<String, CliError> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .map_err(|error| CliError::new(format!("failed to run git: {error}")))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(CliError::new(format!(
            "git command failed: {}",
            stderr.trim()
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn first_nonempty_line(output: String) -> Option<String> {
    output
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(ToOwned::to_owned)
}

fn parse_query_args(args: &[String]) -> Result<QueryArgs, CliError> {
    let mut parsed = QueryArgs::default();
    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        if let Some(query) = arg
            .strip_prefix('?')
            .or_else(|| arg.split_once('?').map(|(_, q)| q))
        {
            apply_query_pairs(&mut parsed, query)?;
            index += 1;
            continue;
        }
        if let Some((key, value)) = arg.split_once('=') {
            apply_pair(&mut parsed, key.trim_start_matches('-'), value)?;
            index += 1;
            continue;
        }
        if arg.starts_with("--") {
            let key = arg.trim_start_matches('-');
            let value = args
                .get(index + 1)
                .ok_or_else(|| CliError::new(format!("{arg} requires a value")))?;
            apply_pair(&mut parsed, key, value)?;
            index += 2;
            continue;
        }
        return Err(CliError::new(format!(
            "unexpected diff-viewer argument: {arg}"
        )));
    }
    Ok(parsed)
}

fn apply_query_pairs(parsed: &mut QueryArgs, query: &str) -> Result<(), CliError> {
    for pair in query.split('&').filter(|pair| !pair.is_empty()) {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        apply_pair(parsed, &percent_decode(key)?, &percent_decode(value)?)?;
    }
    Ok(())
}

fn apply_pair(parsed: &mut QueryArgs, key: &str, value: &str) -> Result<(), CliError> {
    match key {
        "repo" | "repoRoot" | "cwd" => parsed.repo = Some(value.to_string()),
        "token" => parsed.token = Some(value.to_string()),
        "base" | "ref" => parsed.base = Some(value.to_string()),
        "group" => parsed.group = Some(value.to_string()),
        "" => {}
        other => {
            return Err(CliError::new(format!(
                "unknown diff-viewer argument: {other}"
            )))
        }
    }
    Ok(())
}

fn sanitize_group(group: &str) -> String {
    let sanitized: String = group
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else {
                '_'
            }
        })
        .collect();
    let trimmed = sanitized.trim_matches(['.', '_', '-']).to_string();
    if trimmed.is_empty() {
        DEFAULT_GROUP.to_string()
    } else {
        trimmed
    }
}

fn percent_decode(value: &str) -> Result<String, CliError> {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                let hi = hex(bytes[i + 1])?;
                let lo = hex(bytes[i + 2])?;
                out.push((hi << 4) | lo);
                i += 3;
            }
            b'%' => return Err(CliError::new("invalid percent escape")),
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8(out).map_err(|_| CliError::new("percent-decoded value is not utf-8"))
}

fn percent_encode(value: &str) -> String {
    value
        .bytes()
        .flat_map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                vec![b as char]
            }
            _ => format!("%{b:02X}").chars().collect(),
        })
        .collect()
}

fn hex(b: u8) -> Result<u8, CliError> {
    match b {
        b'0'..=b'9' => Ok(b - b'0'),
        b'a'..=b'f' => Ok(b - b'a' + 10),
        b'A'..=b'F' => Ok(b - b'A' + 10),
        _ => Err(CliError::new("invalid percent escape")),
    }
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    struct TempRoot {
        path: PathBuf,
    }

    impl TempRoot {
        fn new() -> Self {
            let n = COUNTER.fetch_add(1, Ordering::Relaxed);
            let pid = std::process::id();
            let path = std::env::temp_dir().join(format!("cmux-cli-diff-viewer-{pid}-{n}"));
            std::fs::create_dir_all(&path).unwrap();
            Self {
                path: std::fs::canonicalize(path).unwrap(),
            }
        }

        fn file(&self, name: &str, contents: &str) -> PathBuf {
            let path = self.path.join(name);
            std::fs::write(&path, contents).unwrap();
            path
        }
    }

    impl Drop for TempRoot {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn parses_flags_query_and_percent_encoding() {
        let args = vec![
            "/__cmux_diff_viewer_branch?repo=C%3A%5Crepo&token=tok-abcdef0123456789".to_string(),
            "--base".to_string(),
            "origin/main".to_string(),
            "group=feature/x".to_string(),
        ];
        let parsed = parse_query_args(&args).unwrap();
        assert_eq!(parsed.repo.as_deref(), Some("C:\\repo"));
        assert_eq!(parsed.token.as_deref(), Some("tok-abcdef0123456789"));
        assert_eq!(parsed.base.as_deref(), Some("origin/main"));
        assert_eq!(parsed.group.as_deref(), Some("feature/x"));
    }

    #[test]
    fn refs_response_groups_local_and_remote_refs() {
        let refs = vec![
            GitRef {
                name: "main".to_string(),
                secondary: Some("2 days ago".to_string()),
            },
            GitRef {
                name: "feature/x".to_string(),
                secondary: None,
            },
            GitRef {
                name: "origin/main".to_string(),
                secondary: None,
            },
        ];
        let value = refs_response_json(&refs, "feature/x", Some("origin/main"));
        assert_eq!(value["groups"][0]["id"], "suggested");
        assert_eq!(value["groups"][0]["rows"][0]["current"], true);
        assert_eq!(value["groups"][1]["id"], "branches");
        assert_eq!(value["groups"][2]["id"], "remotes");
    }

    #[test]
    fn branch_html_contains_config_and_escaped_request_path() {
        let html = branch_html(
            Path::new("C:/repo"),
            "tok-abcdef0123456789",
            "origin/main",
            "/diff-branch.html",
            "/diff-branch.patch",
        );
        assert!(html.contains("cmux-diff-viewer-config"));
        assert!(html.contains("\"patchURL\":\"/diff-branch.patch\""));
        assert!(html.contains("/__cmux_diff_viewer_refs?repo=C%3A%2Frepo"));
    }

    #[test]
    fn upsert_manifest_writes_registry_restorable_entries() {
        let root = TempRoot::new();
        let token = "tok-abcdef0123456789";
        let html = root.file("index.html", "<!doctype html>");
        let patch = root.file("index.patch", "diff --git a/a b/a\n");

        upsert_manifest(
            &root.path,
            token,
            &[
                RegisteredFile {
                    request_path: "/index.html".to_string(),
                    file_path: html,
                    mime_type: "text/html".to_string(),
                },
                RegisteredFile {
                    request_path: "/index.patch".to_string(),
                    file_path: patch,
                    mime_type: "text/x-diff".to_string(),
                },
            ],
        )
        .unwrap();

        let registry = DiffSessionRegistry::new(&root.path);
        assert!(registry.has_active_session(token, std::time::SystemTime::now()));
        assert!(registry
            .registered_file(token, "/index.html", std::time::SystemTime::now())
            .is_some());
        assert!(registry
            .registered_file(token, "/index.patch", std::time::SystemTime::now())
            .is_some());
    }
}
