//! Durable publication for runtime, panel, agent, and git model facts.

use super::*;

const OBSERVED_AT: i64 = 71;

#[derive(Debug, Clone, Copy)]
enum WriterKind {
    ProcessTitle,
    PanelPortsControl,
    PanelTty,
    PanelShell,
    PanelPortsObserved,
    AgentPorts,
    SetAgentPid,
    ClearAgentPid,
    GitFacts,
    SetPullRequest,
    ClearPullRequest,
}

impl WriterKind {
    fn helper(self) -> &'static str {
        match self {
            Self::ProcessTitle => "set_process_title_for_panel",
            Self::PanelPortsControl => "set_panel_listening_ports_for_control",
            Self::PanelTty => "set_panel_tty_for_control",
            Self::PanelShell => "set_panel_shell_activity_for_control",
            Self::PanelPortsObserved => "set_panel_listening_ports_for_panel",
            Self::AgentPorts => "set_workspace_agent_listening_ports_for_control",
            Self::SetAgentPid => "set_workspace_agent_pid_for_control",
            Self::ClearAgentPid => "clear_workspace_agent_pid_for_control",
            Self::GitFacts => "set_workspace_git_facts_for_control",
            Self::SetPullRequest => "set_workspace_panel_pull_request_for_control",
            Self::ClearPullRequest => "clear_workspace_panel_pull_request_for_control",
        }
    }
}

const CHANGE_GATED: [WriterKind; 11] = [
    WriterKind::ProcessTitle,
    WriterKind::PanelPortsControl,
    WriterKind::PanelTty,
    WriterKind::PanelShell,
    WriterKind::PanelPortsObserved,
    WriterKind::AgentPorts,
    WriterKind::SetAgentPid,
    WriterKind::ClearAgentPid,
    WriterKind::GitFacts,
    WriterKind::SetPullRequest,
    WriterKind::ClearPullRequest,
];

struct RecordingPublication {
    calls: Vec<&'static str>,
    persist_error: Option<String>,
    baseline: AppSessionSnapshot,
    events: Vec<AppSessionSnapshot>,
}

impl RecordingPublication {
    fn new(snapshot: &AppSessionSnapshot) -> Self {
        Self {
            calls: Vec::new(),
            persist_error: None,
            baseline: snapshot.clone(),
            events: Vec::new(),
        }
    }
}

impl SnapshotPublicationOperations for RecordingPublication {
    fn persist(&mut self, _candidate: &AppSessionSnapshot) -> Result<(), String> {
        self.calls.push("persist");
        self.persist_error.take().map_or(Ok(()), Err)
    }

    fn update_event_baseline(&mut self, candidate: &AppSessionSnapshot) {
        self.calls.push("baseline");
        self.baseline = candidate.clone();
    }

    fn emit(&mut self, candidate: &AppSessionSnapshot) -> Result<(), String> {
        self.calls.push("emit");
        self.events.push(candidate.clone());
        Ok(())
    }
}

fn initial() -> AppSessionSnapshot {
    initial_snapshot("surface-1")
}

fn workspace(snapshot: &AppSessionSnapshot) -> &SessionWorkspaceSnapshot {
    &snapshot.windows[0].tab_manager.workspaces[0]
}

fn git_branch(panel_id: &str) -> SessionPanelGitBranchSnapshot {
    SessionPanelGitBranchSnapshot {
        panel_id: panel_id.into(),
        branch: "feature/facts".into(),
        is_dirty: true,
    }
}

fn pull_request(panel_id: &str, number: i64) -> SessionPanelPullRequestSnapshot {
    SessionPanelPullRequestSnapshot {
        panel_id: panel_id.into(),
        number,
        label: "Review".into(),
        url: format!("https://example.test/pull/{number}"),
        status: SessionPullRequestStatusSnapshot::Open,
        branch: Some("feature/facts".into()),
        is_stale: false,
    }
}

fn set_tty_at(snapshot: &mut AppSessionSnapshot, tty: &str) -> bool {
    set_workspace_panel_tty(
        &mut snapshot.windows[0].tab_manager.workspaces[0],
        "surface-1",
        tty,
        OBSERVED_AT,
    )
}

fn set_shell_at(snapshot: &mut AppSessionSnapshot) -> bool {
    set_workspace_panel_shell_activity(
        &mut snapshot.windows[0].tab_manager.workspaces[0],
        "surface-1",
        SessionPanelShellActivityStateSnapshot::CommandRunning,
        OBSERVED_AT,
    )
}

fn set_pid_at(snapshot: &mut AppSessionSnapshot) -> bool {
    set_workspace_agent_pid(
        &mut snapshot.windows[0].tab_manager.workspaces[0],
        " codex.session ",
        4242,
        OBSERVED_AT,
    )
}

fn apply_changed(kind: WriterKind, snapshot: &mut AppSessionSnapshot) -> bool {
    match kind {
        WriterKind::ProcessTitle => {
            apply_set_process_title(snapshot, "surface-1", "  cargo test  ")
        }
        WriterKind::PanelPortsControl | WriterKind::PanelPortsObserved => {
            apply_set_panel_listening_ports(snapshot, 0, " surface-1 ", &[7000, 3000, 7000])
        }
        WriterKind::PanelTty => set_tty_at(snapshot, "  pts/7  "),
        WriterKind::PanelShell => set_shell_at(snapshot),
        WriterKind::AgentPorts => {
            apply_set_workspace_agent_listening_ports(snapshot, 0, &[9000, 3000, 9000])
        }
        WriterKind::SetAgentPid => set_pid_at(snapshot),
        WriterKind::ClearAgentPid => apply_clear_workspace_agent_pid(snapshot, 0, "codex.session"),
        WriterKind::GitFacts => apply_set_workspace_git_facts(
            snapshot,
            0,
            Some(SessionGitBranchSnapshot {
                branch: "feature/facts".into(),
                is_dirty: true,
            }),
            vec![git_branch("surface-2"), git_branch("surface-1")],
            vec![pull_request("surface-2", 12), pull_request("surface-1", 7)],
        ),
        WriterKind::SetPullRequest => apply_set_workspace_panel_pull_request(
            snapshot,
            0,
            " surface-1 ",
            7,
            "Review",
            "https://example.test/pull/7",
            SessionPullRequestStatusSnapshot::Open,
            Some("feature/facts".into()),
            false,
        ),
        WriterKind::ClearPullRequest => {
            apply_clear_workspace_panel_pull_request(snapshot, 0, " surface-1 ")
        }
    }
}

fn seed_for_changed(kind: WriterKind) -> AppSessionSnapshot {
    let mut snapshot = initial();
    match kind {
        WriterKind::ProcessTitle => {
            apply_new_workspace(&mut snapshot, "surface-2", None, None, None, None);
        }
        WriterKind::PanelPortsControl | WriterKind::PanelPortsObserved => {
            assert!(apply_set_workspace_agent_listening_ports(
                &mut snapshot,
                0,
                &[5173]
            ));
        }
        WriterKind::AgentPorts => {
            assert!(apply_set_panel_listening_ports(
                &mut snapshot,
                0,
                "surface-1",
                &[5173]
            ));
        }
        WriterKind::ClearAgentPid => assert!(set_pid_at(&mut snapshot)),
        WriterKind::GitFacts => {
            assert!(apply_split(
                &mut snapshot,
                "surface-1",
                SessionSplitOrientation::Horizontal,
                "surface-2",
                false
            ));
        }
        WriterKind::ClearPullRequest => {
            assert!(apply_set_workspace_panel_pull_request(
                &mut snapshot,
                0,
                "surface-1",
                7,
                "Review",
                "https://example.test/pull/7",
                SessionPullRequestStatusSnapshot::Open,
                Some("feature/facts".into()),
                false,
            ));
        }
        _ => {}
    }
    snapshot
}

fn seed_for_noop(kind: WriterKind) -> AppSessionSnapshot {
    let mut snapshot = seed_for_changed(kind);
    assert!(apply_changed(kind, &mut snapshot));
    snapshot
}

fn apply_noop(kind: WriterKind, snapshot: &mut AppSessionSnapshot) -> bool {
    match kind {
        WriterKind::ProcessTitle => apply_set_process_title(snapshot, " surface-1 ", "cargo test"),
        WriterKind::PanelPortsControl | WriterKind::PanelPortsObserved => {
            apply_set_panel_listening_ports(snapshot, 0, "surface-1", &[3000, 7000])
        }
        WriterKind::PanelTty => set_tty_at(snapshot, "pts/7"),
        WriterKind::PanelShell => set_shell_at(snapshot),
        WriterKind::AgentPorts => {
            apply_set_workspace_agent_listening_ports(snapshot, 0, &[3000, 9000])
        }
        WriterKind::SetAgentPid => set_pid_at(snapshot),
        WriterKind::ClearAgentPid => apply_clear_workspace_agent_pid(snapshot, 0, "missing"),
        WriterKind::GitFacts => apply_set_workspace_git_facts(
            snapshot,
            0,
            Some(SessionGitBranchSnapshot {
                branch: "feature/facts".into(),
                is_dirty: true,
            }),
            vec![git_branch("surface-1"), git_branch("surface-2")],
            vec![pull_request("surface-1", 7), pull_request("surface-2", 12)],
        ),
        WriterKind::SetPullRequest => apply_set_workspace_panel_pull_request(
            snapshot,
            0,
            "surface-1",
            7,
            "Review",
            "https://example.test/pull/7",
            SessionPullRequestStatusSnapshot::Open,
            Some("feature/facts".into()),
            false,
        ),
        WriterKind::ClearPullRequest => {
            apply_clear_workspace_panel_pull_request(snapshot, 0, "missing")
        }
    }
}

#[test]
fn eleven_changed_fact_writers_publish_exact_normalized_candidates() {
    for kind in CHANGE_GATED {
        let before = seed_for_changed(kind);
        let mut expected = before.clone();
        assert!(apply_changed(kind, &mut expected), "{}", kind.helper());
        let authority = GatedSnapshot::new(before.clone());
        let mut publication = RecordingPublication::new(&before);
        let committed = transact_snapshot_if_changed(&authority, &mut publication, |candidate| {
            apply_changed(kind, candidate)
        })
        .unwrap();
        assert_eq!(committed, expected, "{}", kind.helper());
        assert_eq!(publication.calls, ["persist", "baseline", "emit"]);
        assert_eq!(publication.baseline, expected);
        assert_eq!(publication.events, [expected]);
    }

    let mut facts = seed_for_changed(WriterKind::PanelPortsControl);
    assert!(apply_changed(WriterKind::PanelPortsControl, &mut facts));
    assert_eq!(
        workspace(&facts).panel_listening_ports.as_ref().unwrap()[0].ports,
        [3000, 7000]
    );
    assert_eq!(
        workspace(&facts).listening_ports,
        Some(vec![3000, 5173, 7000])
    );
    assert_eq!(workspace(&facts).panel_ttys, None);

    let mut observed = seed_for_changed(WriterKind::PanelTty);
    assert!(apply_changed(WriterKind::PanelTty, &mut observed));
    assert_eq!(
        workspace(&observed).panel_ttys.as_ref().unwrap()[0].updated_at,
        OBSERVED_AT
    );
    assert!(apply_changed(WriterKind::PanelShell, &mut observed));
    assert_eq!(
        workspace(&observed).panel_shell_activity.as_ref().unwrap()[0].updated_at,
        OBSERVED_AT
    );
    assert!(apply_changed(WriterKind::SetAgentPid, &mut observed));
    assert_eq!(
        workspace(&observed).agent_pids.as_ref().unwrap()[0].updated_at,
        OBSERVED_AT
    );

    let mut git = seed_for_changed(WriterKind::GitFacts);
    assert!(apply_changed(WriterKind::GitFacts, &mut git));
    assert_eq!(
        workspace(&git)
            .panel_git_branches
            .as_ref()
            .unwrap()
            .iter()
            .map(|entry| entry.panel_id.as_str())
            .collect::<Vec<_>>(),
        ["surface-1", "surface-2"]
    );
    assert_eq!(
        workspace(&git)
            .panel_pull_requests
            .as_ref()
            .unwrap()
            .iter()
            .map(|entry| entry.panel_id.as_str())
            .collect::<Vec<_>>(),
        ["surface-1", "surface-2"]
    );

    let mut title = seed_for_changed(WriterKind::ProcessTitle);
    assert!(apply_changed(WriterKind::ProcessTitle, &mut title));
    assert_eq!(
        title.windows[0].tab_manager.workspaces[0].process_title,
        "cargo test"
    );
    assert_eq!(
        title.windows[0].tab_manager.workspaces[1].process_title,
        "Terminal"
    );
}

#[test]
fn eleven_same_or_missing_fact_writes_are_exact_zero_operation_noops() {
    for kind in CHANGE_GATED {
        let before = seed_for_noop(kind);
        let authority = GatedSnapshot::new(before.clone());
        let mut publication = RecordingPublication::new(&before);
        let returned = transact_snapshot_if_changed(&authority, &mut publication, |candidate| {
            apply_noop(kind, candidate)
        })
        .unwrap();
        assert_eq!(returned, before, "{}", kind.helper());
        assert_eq!(*authority.lock().unwrap(), before, "{}", kind.helper());
        assert!(publication.calls.is_empty(), "{}", kind.helper());
        assert!(publication.events.is_empty(), "{}", kind.helper());
    }
}

#[test]
fn eleven_fact_persist_failures_leak_no_candidate_or_publication() {
    for kind in CHANGE_GATED {
        let before = seed_for_changed(kind);
        let authority = GatedSnapshot::new(before.clone());
        let mut publication = RecordingPublication::new(&before);
        publication.persist_error = Some("injected fact persistence failure".into());
        assert_eq!(
            transact_snapshot_if_changed(&authority, &mut publication, |candidate| apply_changed(
                kind, candidate
            ))
            .unwrap_err(),
            "injected fact persistence failure"
        );
        assert_eq!(publication.calls, ["persist"], "{}", kind.helper());
        assert_eq!(*authority.lock().unwrap(), before, "{}", kind.helper());
        assert_eq!(publication.baseline, before, "{}", kind.helper());
        assert!(publication.events.is_empty(), "{}", kind.helper());
    }
}

#[test]
fn surface_kind_always_publishes_changed_same_and_missing_and_rolls_back_failure() {
    for (seed_kind, panel_id, kind) in [
        (None, "surface-1", Some("agent".to_string())),
        (Some("agent"), "surface-1", Some("agent".to_string())),
        (None, "missing", Some("agent".to_string())),
    ] {
        let mut before = initial();
        if let Some(seed_kind) = seed_kind {
            assert!(apply_set_surface_kind(
                &mut before,
                "surface-1",
                Some(seed_kind.into())
            ));
        }
        let mut expected = before.clone();
        let _ = apply_set_surface_kind(&mut expected, panel_id, kind.clone());
        let authority = GatedSnapshot::new(before.clone());
        let mut publication = RecordingPublication::new(&before);
        let committed = transact_snapshot_always(&authority, &mut publication, |candidate| {
            apply_set_surface_kind(candidate, panel_id, kind.clone())
        })
        .unwrap();
        assert_eq!(committed, expected);
        assert_eq!(publication.calls, ["persist", "baseline", "emit"]);
        assert_eq!(publication.baseline, expected);
        assert_eq!(publication.events, [committed]);
    }

    let before = initial();
    let authority = GatedSnapshot::new(before.clone());
    let mut publication = RecordingPublication::new(&before);
    publication.persist_error = Some("injected kind persistence failure".into());
    assert!(
        transact_snapshot_always(&authority, &mut publication, |candidate| {
            apply_set_surface_kind(candidate, "surface-1", Some("agent".into()))
        })
        .is_err()
    );
    assert_eq!(publication.calls, ["persist"]);
    assert_eq!(*authority.lock().unwrap(), before);
}
