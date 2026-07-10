use std::path::Path;
use std::process::Command;

use cmux_core::session::{
    AppSessionSnapshot, SessionGitBranchSnapshot, SessionPanelGitBranchSnapshot,
    SessionPanelPullRequestSnapshot, SessionPullRequestStatusSnapshot,
    SessionWorkspaceLayoutSnapshot,
};
use serde::Deserialize;
use tauri::{AppHandle, State};

pub fn pull_request_links_from_remote_output(remote_output: &str) -> Vec<String> {
    cmux_git::github_repository_slugs(remote_output)
        .into_iter()
        .map(|slug| format!("https://github.com/{slug}/pulls"))
        .collect()
}

pub fn workspace_pull_request_links_for_directory(directory: &str) -> Result<Vec<String>, String> {
    let trimmed = directory.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }
    if !Path::new(trimmed).is_dir() {
        return Err("Workspace directory does not exist.".to_string());
    }
    let output = Command::new("git")
        .arg("-C")
        .arg(trimmed)
        .arg("remote")
        .arg("-v")
        .output()
        .map_err(|error| format!("failed to run git remote -v: {error}"))?;
    if !output.status.success() {
        return Ok(Vec::new());
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(pull_request_links_from_remote_output(&stdout))
}

#[tauri::command]
pub fn workspace_pull_request_links(directory: String) -> Result<Vec<String>, String> {
    workspace_pull_request_links_for_directory(&directory)
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct WorkspaceGitRefreshResult {
    pub workspace_index: usize,
    pub workspace_id: Option<String>,
    pub directory: Option<String>,
    pub branch: Option<String>,
    pub is_dirty: bool,
    pub pull_request_url: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct GitBranchProbe {
    branch: String,
    is_dirty: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GhPullRequestView {
    number: i64,
    url: String,
    state: String,
    head_ref_name: Option<String>,
}

#[tauri::command]
pub fn workspace_git_refresh(
    app: AppHandle,
    session_state: State<'_, crate::session::SessionState>,
) -> Result<Vec<WorkspaceGitRefreshResult>, String> {
    let snapshot = crate::session::current_session_snapshot(&session_state);
    Ok(refresh_workspace_git_facts(&app, &session_state, &snapshot))
}

fn refresh_workspace_git_facts(
    app: &AppHandle,
    session_state: &crate::session::SessionState,
    snapshot: &AppSessionSnapshot,
) -> Vec<WorkspaceGitRefreshResult> {
    let Some(window) = snapshot.windows.first() else {
        return Vec::new();
    };

    let mut results = Vec::new();
    for (workspace_index, workspace) in window.tab_manager.workspaces.iter().enumerate() {
        let directory = workspace
            .current_directory
            .as_deref()
            .map(str::trim)
            .filter(|directory| !directory.is_empty())
            .map(str::to_string);
        let panel_ids = workspace
            .layout
            .as_ref()
            .map(panel_ids_from_layout)
            .unwrap_or_default();

        let mut error = None;
        let branch_probe = directory.as_deref().and_then(|directory| {
            match git_branch_probe_for_directory(directory) {
                Ok(branch_probe) => branch_probe,
                Err(message) => {
                    error = Some(message);
                    None
                }
            }
        });

        let mut panel_branches = Vec::new();
        let mut panel_pull_requests = Vec::new();
        let mut pull_request_url = None;
        let git_branch = branch_probe.as_ref().map(|probe| SessionGitBranchSnapshot {
            branch: probe.branch.clone(),
            is_dirty: probe.is_dirty,
        });

        if let Some(probe) = &branch_probe {
            panel_branches = panel_ids
                .iter()
                .map(|panel_id| SessionPanelGitBranchSnapshot {
                    panel_id: panel_id.clone(),
                    branch: probe.branch.clone(),
                    is_dirty: probe.is_dirty,
                })
                .collect();

            if let Some(directory) = directory.as_deref() {
                match current_branch_pull_request(directory, &probe.branch) {
                    Ok(Some(pull_request)) => {
                        pull_request_url = Some(pull_request.url.clone());
                        panel_pull_requests = panel_ids
                            .iter()
                            .map(|panel_id| SessionPanelPullRequestSnapshot {
                                panel_id: panel_id.clone(),
                                number: pull_request.number,
                                label: repo_label_from_github_url(&pull_request.url)
                                    .unwrap_or_else(|| "GitHub".to_string()),
                                url: pull_request.url.clone(),
                                status: pull_request_status_from_gh_state(&pull_request.state)
                                    .unwrap_or(SessionPullRequestStatusSnapshot::Open),
                                branch: pull_request
                                    .head_ref_name
                                    .clone()
                                    .or_else(|| Some(probe.branch.clone())),
                                is_stale: false,
                            })
                            .collect();
                    }
                    Ok(None) => {}
                    Err(message) => {
                        error = Some(message);
                    }
                }
            }
        }

        crate::session::set_workspace_git_facts_for_control(
            app,
            session_state,
            workspace_index,
            git_branch,
            panel_branches,
            panel_pull_requests,
        );

        let refreshed_branch = branch_probe.as_ref().map(|probe| probe.branch.clone());
        let refreshed_is_dirty = branch_probe
            .as_ref()
            .map(|probe| probe.is_dirty)
            .unwrap_or(false);

        results.push(WorkspaceGitRefreshResult {
            workspace_index,
            workspace_id: workspace.workspace_id.clone(),
            directory,
            branch: refreshed_branch,
            is_dirty: refreshed_is_dirty,
            pull_request_url,
            error,
        });
    }
    results
}

fn panel_ids_from_layout(layout: &SessionWorkspaceLayoutSnapshot) -> Vec<String> {
    match layout {
        SessionWorkspaceLayoutSnapshot::Pane(pane) => pane.panel_ids.clone(),
        SessionWorkspaceLayoutSnapshot::Split(split) => {
            let mut ids = panel_ids_from_layout(&split.first);
            ids.extend(panel_ids_from_layout(&split.second));
            ids
        }
    }
}

fn git_branch_probe_for_directory(directory: &str) -> Result<Option<GitBranchProbe>, String> {
    let trimmed = directory.trim();
    if trimmed.is_empty() || !Path::new(trimmed).is_dir() {
        return Ok(None);
    }
    if !git_success(trimmed, &["rev-parse", "--is-inside-work-tree"])? {
        return Ok(None);
    }

    let branch = git_stdout(trimmed, &["branch", "--show-current"])?
        .trim()
        .to_string();
    let branch = if branch.is_empty() {
        git_stdout(trimmed, &["rev-parse", "--short", "HEAD"])?
            .trim()
            .to_string()
    } else {
        branch
    };
    if branch.is_empty() {
        return Ok(None);
    }
    let is_dirty = !git_stdout(trimmed, &["status", "--porcelain"])?
        .trim()
        .is_empty();
    Ok(Some(GitBranchProbe { branch, is_dirty }))
}

fn current_branch_pull_request(
    directory: &str,
    branch: &str,
) -> Result<Option<GhPullRequestView>, String> {
    let output = Command::new("gh")
        .current_dir(directory)
        .env("GH_PROMPT_DISABLED", "1")
        .args([
            "pr",
            "view",
            branch,
            "--json",
            "number,url,state,headRefName",
        ])
        .output();
    let output = match output {
        Ok(output) => output,
        Err(_) => return Ok(None),
    };
    if !output.status.success() {
        return Ok(None);
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let pull_request = serde_json::from_str::<GhPullRequestView>(&stdout)
        .map_err(|error| format!("failed to parse gh pr view output: {error}"))?;
    Ok(Some(pull_request))
}

fn git_success(directory: &str, args: &[&str]) -> Result<bool, String> {
    Command::new("git")
        .arg("-C")
        .arg(directory)
        .args(args)
        .output()
        .map(|output| output.status.success())
        .map_err(|error| format!("failed to run git {}: {error}", args.join(" ")))
}

fn git_stdout(directory: &str, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(directory)
        .args(args)
        .output()
        .map_err(|error| format!("failed to run git {}: {error}", args.join(" ")))?;
    if !output.status.success() {
        return Ok(String::new());
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn pull_request_status_from_gh_state(state: &str) -> Option<SessionPullRequestStatusSnapshot> {
    match state.trim().to_ascii_uppercase().as_str() {
        "OPEN" => Some(SessionPullRequestStatusSnapshot::Open),
        "MERGED" => Some(SessionPullRequestStatusSnapshot::Merged),
        "CLOSED" => Some(SessionPullRequestStatusSnapshot::Closed),
        _ => None,
    }
}

fn repo_label_from_github_url(url: &str) -> Option<String> {
    let parsed = url::Url::parse(url).ok()?;
    if parsed.host_str()?.to_ascii_lowercase() != "github.com" {
        return None;
    }
    let mut parts = parsed.path_segments()?;
    let owner = parts.next()?.trim();
    let repo = parts.next()?.trim();
    (!owner.is_empty() && !repo.is_empty()).then(|| format!("{owner}/{repo}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pull_request_links_from_remote_output_uses_github_fetch_remotes() {
        let output = "\
origin\thttps://github.com/manaflow-ai/cmux.git (fetch)\n\
origin\thttps://github.com/manaflow-ai/cmux.git (push)\n\
upstream\tgit@github.com:openai/codex.git (fetch)\n";

        assert_eq!(
            pull_request_links_from_remote_output(output),
            vec![
                "https://github.com/openai/codex/pulls",
                "https://github.com/manaflow-ai/cmux/pulls"
            ]
        );
    }

    #[test]
    fn pull_request_links_from_remote_output_ignores_non_github_remotes() {
        let output = "origin\tgit@gitlab.com:owner/repo.git (fetch)\n";

        assert!(pull_request_links_from_remote_output(output).is_empty());
    }

    #[test]
    fn panel_ids_from_layout_walks_leaves_left_to_right() {
        let layout =
            SessionWorkspaceLayoutSnapshot::Split(cmux_core::session::SessionSplitLayoutSnapshot {
                orientation: cmux_core::session::SessionSplitOrientation::Horizontal,
                divider_position: 0.5,
                first: Box::new(SessionWorkspaceLayoutSnapshot::Pane(
                    cmux_core::session::SessionPaneLayoutSnapshot {
                        pane_id: None,
                        panel_ids: vec!["a".to_string(), "b".to_string()],
                        selected_panel_id: Some("a".to_string()),
                        surface_kind: None,
                        markdown_file_path: None,
                        file_path: None,
                        diff_viewer_token: None,
                        diff_viewer_request_path: None,
                        browser_url: None,
                        browser_proxy_url: None,
                        browser_back_history: None,
                        browser_forward_history: None,
                        browser_omnibar_visible: None,
                        browser_focus_mode_active: None,
                        browser_developer_tools_visible: None,
                        browser_developer_tools_panel: None,
                        browser_page_zoom: None,
                    },
                )),
                second: Box::new(SessionWorkspaceLayoutSnapshot::Pane(
                    cmux_core::session::SessionPaneLayoutSnapshot {
                        pane_id: None,
                        panel_ids: vec!["c".to_string()],
                        selected_panel_id: Some("c".to_string()),
                        surface_kind: None,
                        markdown_file_path: None,
                        file_path: None,
                        diff_viewer_token: None,
                        diff_viewer_request_path: None,
                        browser_url: None,
                        browser_proxy_url: None,
                        browser_back_history: None,
                        browser_forward_history: None,
                        browser_omnibar_visible: None,
                        browser_focus_mode_active: None,
                        browser_developer_tools_visible: None,
                        browser_developer_tools_panel: None,
                        browser_page_zoom: None,
                    },
                )),
            });

        assert_eq!(panel_ids_from_layout(&layout), vec!["a", "b", "c"]);
    }

    #[test]
    fn pull_request_status_from_gh_state_accepts_github_states() {
        assert_eq!(
            pull_request_status_from_gh_state("OPEN"),
            Some(SessionPullRequestStatusSnapshot::Open)
        );
        assert_eq!(
            pull_request_status_from_gh_state("merged"),
            Some(SessionPullRequestStatusSnapshot::Merged)
        );
        assert_eq!(
            pull_request_status_from_gh_state("CLOSED"),
            Some(SessionPullRequestStatusSnapshot::Closed)
        );
        assert_eq!(pull_request_status_from_gh_state("draft"), None);
    }

    #[test]
    fn repo_label_from_github_url_extracts_owner_and_repo() {
        assert_eq!(
            repo_label_from_github_url("https://github.com/manaflow-ai/cmux/pull/42").as_deref(),
            Some("manaflow-ai/cmux")
        );
        assert_eq!(
            repo_label_from_github_url("https://example.com/o/r/pull/1"),
            None
        );
    }
}
