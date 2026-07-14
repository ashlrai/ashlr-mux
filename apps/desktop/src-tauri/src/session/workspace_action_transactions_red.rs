use super::*;
use crate::workspace_action::WorkspaceActionMutation;
use std::cell::Cell;
use std::rc::Rc;

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
    require_post_commit: Option<Rc<Cell<bool>>>,
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
        if let Some(required) = &self.require_post_commit {
            assert!(
                required.get(),
                "post-commit effects must precede publication"
            );
        }
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
        transact_workspace_action_mutation(&authority, &mut publication, &mutation, |_, _| {})
            .unwrap();
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
        transact_workspace_action_mutation(&authority, &mut publication, &mutation, |_, _| {},)
            .unwrap_err(),
        "snapshot write failed"
    );
    assert_eq!(publication.calls, ["persist"]);
    assert_eq!(*authority.lock().unwrap(), initial);
}

#[test]
fn close_transaction_returns_post_commit_history_and_teardown_effects() {
    let authority = GatedSnapshot::new(snapshot());
    let post_commit = Rc::new(Cell::new(false));
    let mut publication = PublicationOracle {
        require_post_commit: Some(post_commit.clone()),
        ..Default::default()
    };
    let mutation = WorkspaceActionMutation::Close {
        window_index: 0,
        workspace_indices: vec![0],
    };

    let (artifacts, committed) = transact_workspace_action_mutation(
        &authority,
        &mut publication,
        &mutation,
        |artifacts, committed| {
            assert_eq!(committed.windows[0].tab_manager.workspaces.len(), 1);
            assert_eq!(
                authority.lock().unwrap().windows[0]
                    .tab_manager
                    .workspaces
                    .len(),
                1
            );
            assert_eq!(artifacts.workspaces.len(), 1);
            post_commit.set(true);
        },
    )
    .unwrap();
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
        transact_workspace_action_mutation(&authority, &mut publication, &mutation, |_, _| {})
            .unwrap();
    assert!(publication.calls.is_empty());
    assert_eq!(committed, initial);
    assert_eq!(*authority.lock().unwrap(), initial);
}

#[derive(Default)]
struct FailingRemoteRenameController {
    calls: Mutex<usize>,
}

impl RemoteWorkspaceRenameController for FailingRemoteRenameController {
    fn rename(&self, _request: &RemoteWorkspaceRenameRequest) -> Result<(), String> {
        *self.calls.lock().unwrap() += 1;
        Err("remote rejected rename".into())
    }
}

#[test]
fn remote_rename_is_best_effort_after_local_commit() {
    let authority = GatedSnapshot::new(snapshot());
    let mut publication = PublicationOracle::default();
    let controller = FailingRemoteRenameController::default();
    let mutation = WorkspaceActionMutation::Rename {
        window_index: 0,
        workspace_index: 1,
        title: "Renamed".into(),
    };
    let request = RemoteWorkspaceRenameRequest {
        workspace_id: SECOND.into(),
        destination: "example.invalid".into(),
        port: None,
        identity_file: None,
        ssh_options: Vec::new(),
        session: Some("remote-session".into()),
        title: "Renamed".into(),
    };

    let (_, committed) = transact_workspace_action_and_propagate_remote(
        &authority,
        &mut publication,
        &mutation,
        &controller,
        Some(&request),
        |_, _| {},
    )
    .expect("remote failure must not reverse a committed local rename");
    assert_eq!(publication.calls, ["persist", "baseline", "emit"]);
    assert_eq!(*controller.calls.lock().unwrap(), 1);
    assert_eq!(
        committed.windows[0].tab_manager.workspaces[1]
            .custom_title
            .as_deref(),
        Some("Renamed")
    );
    assert_eq!(*authority.lock().unwrap(), committed);
}

#[test]
fn production_remote_rename_queue_does_not_dispatch_until_explicit_flush() {
    let controller = Arc::new(FailingRemoteRenameController::default());
    let mut state = SessionState::default();
    state.remote_workspace_rename_controller = controller.clone();
    let request = RemoteWorkspaceRenameRequest {
        workspace_id: SECOND.into(),
        destination: "example.invalid".into(),
        port: None,
        identity_file: None,
        ssh_options: Vec::new(),
        session: Some("remote-session".into()),
        title: "Renamed".into(),
    };

    state.defer_remote_workspace_rename(request);
    assert_eq!(*controller.calls.lock().unwrap(), 0);

    state.flush_deferred_remote_workspace_renames();
    assert_eq!(*controller.calls.lock().unwrap(), 1);
}
