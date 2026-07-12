//! Deferred panel ids and durable activation-created workspace transactions.

use super::*;

#[derive(Debug)]
struct DeferredPanelIdsRed {
    base: u64,
    used: u64,
}

impl DeferredPanelIdsRed {
    fn new(counter: &AtomicU64) -> Self {
        Self {
            base: counter.load(Ordering::Relaxed),
            used: 0,
        }
    }

    fn next(&mut self) -> String {
        let id = format!("surface-{}", self.base + self.used);
        self.used += 1;
        id
    }

    fn layout(
        &mut self,
        layout: CmuxLayoutNode,
    ) -> Option<(
        SessionWorkspaceLayoutSnapshot,
        Option<String>,
        Vec<SessionPanelTerminalStartupSnapshot>,
    )> {
        let mut ids = DeferredPanelIds {
            base: self.base,
            used: self.used,
        };
        let built = session_layout_from_cmux(layout, &mut ids)?;
        self.used = ids.used;
        Some(built)
    }

    fn commit(self, counter: &AtomicU64) {
        counter.fetch_add(self.used, Ordering::Relaxed);
    }
}

#[derive(Debug, PartialEq, Eq)]
enum OpenSshUrlControlErrorRed {
    NotFound(String),
    Publication(String),
}

struct RecordingPublication<'a> {
    calls: Vec<&'static str>,
    persist_error: Option<String>,
    baseline: AppSessionSnapshot,
    events: Vec<AppSessionSnapshot>,
    counter: &'a AtomicU64,
    expected_counter: Option<u64>,
}

impl<'a> RecordingPublication<'a> {
    fn new(snapshot: &AppSessionSnapshot, counter: &'a AtomicU64) -> Self {
        Self {
            calls: Vec::new(),
            persist_error: None,
            baseline: snapshot.clone(),
            events: Vec::new(),
            counter,
            expected_counter: Some(counter.load(Ordering::Relaxed)),
        }
    }
}

impl SnapshotPublicationOperations for RecordingPublication<'_> {
    fn persist(&mut self, _candidate: &AppSessionSnapshot) -> Result<(), String> {
        self.calls.push("persist");
        if let Some(expected) = self.expected_counter {
            assert_eq!(self.counter.load(Ordering::Relaxed), expected);
        }
        self.persist_error.take().map_or(Ok(()), Err)
    }

    fn update_event_baseline(&mut self, candidate: &AppSessionSnapshot) {
        self.calls.push("baseline");
        if let Some(expected) = self.expected_counter {
            assert_eq!(self.counter.load(Ordering::Relaxed), expected);
        }
        self.baseline = candidate.clone();
    }

    fn emit(&mut self, candidate: &AppSessionSnapshot) -> Result<(), String> {
        self.calls.push("emit");
        if let Some(expected) = self.expected_counter {
            assert_eq!(self.counter.load(Ordering::Relaxed), expected);
        }
        self.events.push(candidate.clone());
        Ok(())
    }
}

fn current(authority: &GatedSnapshot) -> AppSessionSnapshot {
    authority.lock().unwrap().clone()
}

fn new_workspace_red(
    authority: &GatedSnapshot,
    counter: &AtomicU64,
    publication: &mut impl SnapshotPublicationOperations,
    current_directory: Option<&str>,
) -> Result<AppSessionSnapshot, String> {
    let _gate = authority.lock_gate();
    let before = current(authority);
    let mut ids = DeferredPanelIdsRed::new(counter);
    let panel_id = ids.next();
    let mut candidate = before.clone();
    apply_new_workspace(
        &mut candidate,
        &panel_id,
        current_directory,
        None,
        None,
        None,
    );
    let committed =
        publish_snapshot_transaction(authority, Some(&before), &candidate, publication)?;
    ids.commit(counter);
    Ok(committed)
}

#[allow(clippy::too_many_arguments)]
fn rich_workspace_red(
    authority: &GatedSnapshot,
    counter: &AtomicU64,
    publication: &mut impl SnapshotPublicationOperations,
    window_index: usize,
    title: &str,
    description: &str,
    workspace_environment: BTreeMap<String, String>,
    initial_environment: BTreeMap<String, String>,
    group_id: &str,
    layout: CmuxLayoutNode,
    focus: bool,
) -> Result<Option<(AppSessionSnapshot, usize)>, String> {
    let _gate = authority.lock_gate();
    let before = current(authority);
    if before.windows.get(window_index).is_none() {
        return Ok(None);
    }
    let mut ids = DeferredPanelIdsRed::new(counter);
    let panel_id = ids.next();
    let mut candidate = before.clone();
    let tabs = &mut candidate.windows[window_index].tab_manager;
    let previous = tabs.selected_workspace_index;
    session_ops::new_workspace(tabs, &panel_id);
    let created_index = usize::try_from(tabs.selected_workspace_index.unwrap()).unwrap();
    let workspace = &mut tabs.workspaces[created_index];
    workspace.custom_title = Some(title.into());
    workspace.custom_title_source = Some("user".into());
    workspace.custom_description = Some(description.into());
    workspace.group_id = Some(group_id.into());
    workspace.workspace_environment = Some(workspace_environment.clone());
    let (layout, focused_panel_id, mut startups) = ids.layout(layout).expect("layout");
    for startup in &mut startups {
        let mut environment = workspace_environment.clone();
        environment.extend(initial_environment.clone());
        environment.extend(
            startup
                .initial_terminal_environment
                .take()
                .unwrap_or_default(),
        );
        startup.initial_terminal_environment = Some(environment);
    }
    workspace.layout = Some(layout);
    workspace.focused_panel_id = focused_panel_id;
    workspace.panel_terminal_startups = Some(startups);
    workspace.initial_terminal_command = None;
    workspace.initial_terminal_environment = None;
    if !focus {
        tabs.selected_workspace_index = previous;
    }
    ensure_workspace_ids(&mut candidate);
    ensure_pane_ids(&mut candidate);
    let committed =
        publish_snapshot_transaction(authority, Some(&before), &candidate, publication)?;
    ids.commit(counter);
    Ok(Some((committed, created_index)))
}

fn open_ssh_red(
    authority: &GatedSnapshot,
    counter: &AtomicU64,
    publication: &mut impl SnapshotPublicationOperations,
    request: &cmux_ssh::CmuxSSHURLRequest,
) -> Result<(AppSessionSnapshot, String), OpenSshUrlControlErrorRed> {
    let _gate = authority.lock_gate();
    let before = current(authority);
    let anchor = active_panel_id(&before).ok_or_else(|| {
        OpenSshUrlControlErrorRed::NotFound(
            "no active terminal pane is available for SSH URL".into(),
        )
    })?;
    let mut ids = DeferredPanelIdsRed::new(counter);
    let panel_id = ids.next();
    let mut candidate = before.clone();
    if !apply_ssh_url_request(&mut candidate, &anchor, &panel_id, request) {
        return Err(OpenSshUrlControlErrorRed::NotFound(format!(
            "no pane holds panel id {anchor}"
        )));
    }
    let committed = publish_snapshot_transaction(authority, Some(&before), &candidate, publication)
        .map_err(OpenSshUrlControlErrorRed::Publication)?;
    ids.commit(counter);
    Ok((committed, panel_id))
}

fn reopen_workspace_red(
    authority: &GatedSnapshot,
    history: &mut Vec<ClosedWorkspaceSnapshot>,
    publication: &mut impl SnapshotPublicationOperations,
) -> Result<Option<AppSessionSnapshot>, String> {
    let _gate = authority.lock_gate();
    let before = current(authority);
    let Some(closed) = history.last().cloned() else {
        return Ok(None);
    };
    let mut candidate = before.clone();
    if !apply_reopen_closed_workspace(&mut candidate, closed) {
        return Ok(None);
    }
    let committed =
        publish_snapshot_transaction(authority, Some(&before), &candidate, publication)?;
    history.pop();
    Ok(Some(committed))
}

fn initial() -> AppSessionSnapshot {
    let mut snapshot = initial_snapshot("surface-1");
    snapshot.windows[0].window_id = Some("main".into());
    ensure_workspace_ids(&mut snapshot);
    ensure_pane_ids(&mut snapshot);
    snapshot
}

#[test]
fn deferred_ids_advance_only_after_emit_and_failures_or_invalid_domains_use_nothing() {
    let before = initial();
    let counter = AtomicU64::new(2);
    let authority = GatedSnapshot::new(before.clone());
    let mut publication = RecordingPublication::new(&before, &counter);
    publication.persist_error = Some("injected activation failure".into());
    assert!(new_workspace_red(&authority, &counter, &mut publication, None).is_err());
    assert_eq!(counter.load(Ordering::Relaxed), 2);
    assert_eq!(*authority.lock().unwrap(), before);
    assert_eq!(publication.calls, ["persist"]);

    let mut publication = RecordingPublication::new(&before, &counter);
    let layout: CmuxLayoutNode = serde_json::from_value(serde_json::json!({
        "pane": {"surfaces": [{"type": "terminal"}]}
    }))
    .unwrap();
    assert!(rich_workspace_red(
        &authority,
        &counter,
        &mut publication,
        99,
        "Title",
        "Description",
        BTreeMap::new(),
        BTreeMap::new(),
        "group",
        layout,
        true,
    )
    .unwrap()
    .is_none());
    assert!(publication.calls.is_empty());
    assert_eq!(counter.load(Ordering::Relaxed), 2);

    let mut no_layout = before.clone();
    no_layout.windows[0].tab_manager.workspaces[0].layout = None;
    let no_layout_authority = GatedSnapshot::new(no_layout.clone());
    let mut publication = RecordingPublication::new(&no_layout, &counter);
    let request = parse_ssh_uri("cmux://ssh?host=example.test").unwrap();
    assert!(matches!(
        open_ssh_red(&no_layout_authority, &counter, &mut publication, &request),
        Err(OpenSshUrlControlErrorRed::NotFound(_))
    ));
    assert!(publication.calls.is_empty());
    assert_eq!(counter.load(Ordering::Relaxed), 2);
}

#[test]
fn rich_layout_preserves_exact_ids_environment_group_and_focus() {
    let before = initial();
    let counter = AtomicU64::new(10);
    let authority = GatedSnapshot::new(before.clone());
    let mut publication = RecordingPublication::new(&before, &counter);
    let layout: CmuxLayoutNode = serde_json::from_value(serde_json::json!({
        "direction": "horizontal",
        "children": [
            {"pane": {"surfaces": [{"type": "terminal", "command": "cargo test", "env": {"LOCAL": "yes"}}]}},
            {"pane": {"surfaces": [{"type": "browser", "url": "https://example.test", "focus": true}]}}
        ]
    }))
    .unwrap();
    let (committed, index) = rich_workspace_red(
        &authority,
        &counter,
        &mut publication,
        0,
        "Review",
        "Rich workspace",
        BTreeMap::from([("WORKSPACE".into(), "yes".into())]),
        BTreeMap::from([("INITIAL".into(), "yes".into())]),
        "group-1",
        layout,
        false,
    )
    .unwrap()
    .unwrap();
    assert_eq!(publication.calls, ["persist", "baseline", "emit"]);
    assert_eq!(counter.load(Ordering::Relaxed), 13);
    assert_eq!(
        committed.windows[0].tab_manager.selected_workspace_index,
        Some(0)
    );
    let workspace = &committed.windows[0].tab_manager.workspaces[index];
    assert_eq!(workspace.custom_title.as_deref(), Some("Review"));
    assert_eq!(
        workspace.custom_description.as_deref(),
        Some("Rich workspace")
    );
    assert_eq!(workspace.group_id.as_deref(), Some("group-1"));
    assert_eq!(workspace.focused_panel_id.as_deref(), Some("surface-12"));
    let startups = workspace.panel_terminal_startups.as_ref().unwrap();
    let environment = startups[0].initial_terminal_environment.as_ref().unwrap();
    assert_eq!(
        environment.get("WORKSPACE").map(String::as_str),
        Some("yes")
    );
    assert_eq!(environment.get("INITIAL").map(String::as_str), Some("yes"));
    assert_eq!(environment.get("LOCAL").map(String::as_str), Some("yes"));
}

#[test]
fn ssh_publication_is_typed_and_reopen_peeks_until_success() {
    let before = initial();
    let counter = AtomicU64::new(2);
    let authority = GatedSnapshot::new(before.clone());
    let request = parse_ssh_uri("cmux://ssh?host=example.test").unwrap();
    let mut publication = RecordingPublication::new(&before, &counter);
    publication.persist_error = Some("injected ssh failure".into());
    assert_eq!(
        open_ssh_red(&authority, &counter, &mut publication, &request),
        Err(OpenSshUrlControlErrorRed::Publication(
            "injected ssh failure".into()
        ))
    );
    assert_eq!(counter.load(Ordering::Relaxed), 2);
    assert_eq!(*authority.lock().unwrap(), before);

    let mut source = before.clone();
    apply_new_workspace(&mut source, "surface-2", None, None, None, None);
    ensure_workspace_ids(&mut source);
    ensure_pane_ids(&mut source);
    let closed = closed_workspace_snapshot(&source, 0, 1).unwrap();
    source.windows[0].tab_manager.workspaces.remove(1);
    let authority = GatedSnapshot::new(source.clone());
    let mut history = vec![closed];
    let mut publication = RecordingPublication::new(&source, &counter);
    publication.persist_error = Some("injected reopen failure".into());
    assert!(reopen_workspace_red(&authority, &mut history, &mut publication).is_err());
    assert_eq!(history.len(), 1);
    let mut publication = RecordingPublication::new(&source, &counter);
    assert!(
        reopen_workspace_red(&authority, &mut history, &mut publication)
            .unwrap()
            .is_some()
    );
    assert!(history.is_empty());
}

#[test]
fn concurrent_activation_creates_serialize_unique_ids() {
    let before = initial();
    let authority = Arc::new(GatedSnapshot::new(before.clone()));
    let counter = Arc::new(AtomicU64::new(2));
    std::thread::scope(|scope| {
        for directory in ["C:/one", "C:/two"] {
            let authority = Arc::clone(&authority);
            let counter = Arc::clone(&counter);
            let baseline = before.clone();
            scope.spawn(move || {
                let mut publication = RecordingPublication::new(&baseline, &counter);
                publication.expected_counter = None;
                new_workspace_red(&authority, &counter, &mut publication, Some(directory)).unwrap();
            });
        }
    });
    assert_eq!(counter.load(Ordering::Relaxed), 4);
    let snapshot = authority.lock().unwrap();
    let panel_ids = snapshot.windows[0]
        .tab_manager
        .workspaces
        .iter()
        .skip(1)
        .filter_map(|workspace| workspace.layout.as_ref())
        .filter_map(|layout| match layout {
            SessionWorkspaceLayoutSnapshot::Pane(pane) => pane.panel_ids.first().cloned(),
            SessionWorkspaceLayoutSnapshot::Split(_) => None,
        })
        .collect::<HashSet<_>>();
    assert_eq!(
        panel_ids,
        HashSet::from(["surface-2".into(), "surface-3".into()])
    );
}

fn function_source<'a>(source: &'a str, signature: &str) -> &'a str {
    let start = source
        .find(signature)
        .unwrap_or_else(|| panic!("missing {signature}"));
    let tail = &source[start..];
    let body_start = tail.find('{').unwrap();
    let mut depth = 0;
    for (offset, character) in tail[body_start..].char_indices() {
        match character {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return &tail[..body_start + offset + 1];
                }
            }
            _ => {}
        }
    }
    panic!("unterminated {signature}")
}

fn assert_deferred_helper(body: &str) {
    assert!(body.contains("Result<"));
    assert!(body.contains("transact_"));
    assert!(!body.contains("snapshot.lock()"));
    assert!(!body.contains("notify_session_changed("));
    assert!(!body.contains("next_panel.fetch_add"));
}

#[test]
fn production_five_sites_use_deferred_transactions_and_success_ordered_callers() {
    let session = include_str!("../session.rs");
    assert!(session.contains("struct DeferredPanelIds"));
    let layout = function_source(session, "fn session_layout_from_cmux(");
    assert!(layout.contains("&mut DeferredPanelIds"));
    assert!(!layout.contains("AtomicU64"));

    let simple = function_source(session, "pub(crate) fn new_workspace_for_control(");
    assert!(simple.contains("Result<AppSessionSnapshot, String>"));
    assert_deferred_helper(simple);
    let public = function_source(session, "pub fn session_new_workspace(");
    assert!(public.contains("Result<AppSessionSnapshot, String>"));
    assert_eq!(public.matches("new_workspace_for_control(").count(), 1);
    assert!(!public.contains("snapshot.lock()") && !public.contains("notify_session_changed("));

    let rich = function_source(
        session,
        "pub(crate) fn new_workspace_in_window_for_control(",
    );
    assert!(rich.contains("Result<Option<(AppSessionSnapshot, usize)>, String>"));
    assert_deferred_helper(rich);
    let ssh = function_source(session, "fn open_ssh_url_request(");
    assert!(ssh.contains("OpenSshUrlControlError"));
    assert!(ssh.contains("NotFound"));
    assert!(ssh.contains("Publication"));
    assert_deferred_helper(ssh);
    assert!(ssh.find("active_panel_id(").unwrap() < ssh.find("DeferredPanelIds").unwrap());
    let reopen = function_source(
        session,
        "pub(crate) fn reopen_closed_workspace_for_control(",
    );
    assert!(reopen.contains("Result<Option<AppSessionSnapshot>, String>"));
    assert_deferred_helper(reopen);
    assert!(!reopen.contains(".pop()"));

    let socket = include_str!("../control_socket.rs");
    for route in ["fn workspace_create(", "fn workspace_reopen_closed("] {
        let body = function_source(socket, route);
        assert!(body.contains("Ok(Some("));
        assert!(body.contains("Ok(None)"));
        assert!(body.contains("Err(message)"));
        assert!(body.contains("\"internal\""));
        if let Some(focus) = body.find("set_focus(") {
            assert!(body.find("Ok(Some(").unwrap() < focus);
        }
    }

    let reopen_public = function_source(session, "pub fn session_reopen_closed_workspace(");
    assert!(reopen_public.contains("Result<AppSessionSnapshot, String>"));
    assert!(reopen_public.contains("reopen_closed_workspace_for_control("));
    assert!(reopen_public.contains('?'));
    assert!(reopen_public.contains("No recently closed workspace"));
    let ssh_public = function_source(session, "pub fn session_handle_ssh_uri(");
    assert!(ssh_public.contains("open_ssh_url_request("));
    assert!(ssh_public.contains("map_err("));
    assert!(!ssh_public.contains("set_focus("));

    let lib = include_str!("../lib.rs");
    let launch = function_source(lib, "fn route_launch_arguments(");
    assert!(launch.contains("new_workspace_for_control("));
    assert!(launch.contains("Ok("));
    assert!(launch.find("Ok(").unwrap() < launch.find("set_focus(").unwrap());
    let installer = function_source(lib, "fn open_claude_code_integration_installer(");
    assert!(installer.contains("session_new_workspace("));
    assert!(installer.contains("Err("));
}
