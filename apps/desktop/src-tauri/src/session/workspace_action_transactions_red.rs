use super::*;
use crate::workspace_action::WorkspaceActionMutation;

const WINDOW: &str = "10000000-0000-4000-8000-000000000001";
const FIRST: &str = "20000000-0000-4000-8000-000000000001";
const SECOND: &str = "20000000-0000-4000-8000-000000000002";

fn snapshot() -> AppSessionSnapshot {
    let workspace = |id: &str| SessionWorkspaceSnapshot {
        workspace_id: Some(id.into()),
        process_title: id.into(),
        ..Default::default()
    };
    AppSessionSnapshot {
        windows: vec![SessionWindowSnapshot {
            window_id: Some(WINDOW.into()),
            selected_workspace_id: Some(SECOND.into()),
            tab_manager: cmux_core::session::SessionTabManagerSnapshot {
                selected_workspace_index: Some(1),
                workspaces: vec![workspace(FIRST), workspace(SECOND)],
                workspace_groups: None,
            },
            ..Default::default()
        }],
        ..Default::default()
    }
}

#[derive(Default)]
struct PublicationOracle {
    fail_persist: bool,
    calls: Vec<&'static str>,
}

impl SnapshotPublicationOperations for PublicationOracle {
    fn persist(&mut self, _candidate: &AppSessionSnapshot) -> Result<(), String> {
        self.calls.push("persist");
        if self.fail_persist {
            Err("snapshot write failed".into())
        } else {
            Ok(())
        }
    }

    fn update_event_baseline(&mut self, _candidate: &AppSessionSnapshot) {
        self.calls.push("baseline");
    }

    fn emit(&mut self, _candidate: &AppSessionSnapshot) -> Result<(), String> {
        self.calls.push("emit");
        Ok(())
    }
}

#[test]
fn metadata_action_persists_before_commit_and_publication() {
    let authority = GatedSnapshot::new(snapshot());
    let mut publication = PublicationOracle::default();
    let mutation = WorkspaceActionMutation::SetColor {
        window_index: 0,
        workspace_index: 1,
        color: Some("#ABCDEF".into()),
    };

    let (artifacts, committed) =
        transact_workspace_action_mutation(&authority, &mut publication, &mutation).unwrap();
    assert!(artifacts.browser_tabs.is_empty());
    assert!(artifacts.workspaces.is_empty());
    assert!(artifacts.teardowns.is_empty());
    assert_eq!(publication.calls, ["persist", "baseline", "emit"]);
    assert_eq!(
        committed.windows[0].tab_manager.workspaces[1]
            .custom_color
            .as_deref(),
        Some("#ABCDEF")
    );
    assert_eq!(*authority.lock().unwrap(), committed);
}

#[test]
fn persistence_failure_rolls_back_authority_and_exposes_no_close_effects() {
    let initial = snapshot();
    let authority = GatedSnapshot::new(initial.clone());
    let mut publication = PublicationOracle {
        fail_persist: true,
        ..Default::default()
    };
    let mutation = WorkspaceActionMutation::Close {
        window_index: 0,
        workspace_indices: vec![0],
    };

    assert_eq!(
        transact_workspace_action_mutation(&authority, &mut publication, &mutation).unwrap_err(),
        "snapshot write failed"
    );
    assert_eq!(publication.calls, ["persist"]);
    assert_eq!(*authority.lock().unwrap(), initial);
}

#[test]
fn close_transaction_returns_post_commit_history_and_teardown_effects() {
    let authority = GatedSnapshot::new(snapshot());
    let mut publication = PublicationOracle::default();
    let mutation = WorkspaceActionMutation::Close {
        window_index: 0,
        workspace_indices: vec![0],
    };

    let (artifacts, committed) =
        transact_workspace_action_mutation(&authority, &mut publication, &mutation).unwrap();
    assert_eq!(publication.calls, ["persist", "baseline", "emit"]);
    assert_eq!(committed.windows[0].tab_manager.workspaces.len(), 1);
    assert_eq!(
        committed.windows[0].tab_manager.workspaces[0]
            .workspace_id
            .as_deref(),
        Some(SECOND)
    );
    assert_eq!(
        committed.windows[0].selected_workspace_id.as_deref(),
        Some(SECOND)
    );
    assert_eq!(artifacts.workspaces.len(), 1);
    assert_eq!(artifacts.teardowns.len(), 1);
}

#[test]
fn no_op_action_skips_persistence_and_publication() {
    let initial = snapshot();
    let authority = GatedSnapshot::new(initial.clone());
    let mut publication = PublicationOracle::default();
    let mutation = WorkspaceActionMutation::SetColor {
        window_index: 0,
        workspace_index: 1,
        color: None,
    };

    let (_, committed) =
        transact_workspace_action_mutation(&authority, &mut publication, &mutation).unwrap();
    assert!(publication.calls.is_empty());
    assert_eq!(committed, initial);
    assert_eq!(*authority.lock().unwrap(), initial);
}
