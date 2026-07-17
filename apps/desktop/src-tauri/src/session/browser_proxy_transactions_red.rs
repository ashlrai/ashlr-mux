//! Durable browser creation/open transactions with compensating proxy leases.

use super::*;

#[derive(Debug, PartialEq, Eq)]
enum BrowserPanelCreateErrorRed {
    NotFound(String),
    Publication(String),
}

#[derive(Clone)]
struct ProxyLeaseRed {
    prior: Option<String>,
    prepared: String,
}

trait BrowserProxyEffectsRed {
    fn prepare(&mut self, panel_id: &str) -> Option<ProxyLeaseRed>;
    fn rollback(&mut self, lease: ProxyLeaseRed);
    fn commit(&mut self, lease: ProxyLeaseRed);
}

struct RecordingProxyEffects {
    calls: Arc<Mutex<Vec<&'static str>>>,
    active: Option<String>,
    unavailable: bool,
}

impl BrowserProxyEffectsRed for RecordingProxyEffects {
    fn prepare(&mut self, panel_id: &str) -> Option<ProxyLeaseRed> {
        self.calls.lock().unwrap().push("prepare");
        if self.unavailable {
            return None;
        }
        let prepared = format!("socks5://prepared/{panel_id}");
        let lease = ProxyLeaseRed {
            prior: self.active.clone(),
            prepared: prepared.clone(),
        };
        self.active = Some(prepared);
        Some(lease)
    }

    fn rollback(&mut self, lease: ProxyLeaseRed) {
        self.calls.lock().unwrap().push("rollback");
        self.active = lease.prior;
    }

    fn commit(&mut self, lease: ProxyLeaseRed) {
        self.calls.lock().unwrap().push("commit");
        self.active = Some(lease.prepared);
    }
}

struct RecordingPublication<'a> {
    calls: Arc<Mutex<Vec<&'static str>>>,
    persist_error: Option<String>,
    baseline: AppSessionSnapshot,
    events: Vec<AppSessionSnapshot>,
    next_panel: &'a AtomicU64,
    expected_counter: u64,
}

impl SnapshotPublicationOperations for RecordingPublication<'_> {
    fn persist(&mut self, _candidate: &AppSessionSnapshot) -> Result<(), String> {
        self.calls.lock().unwrap().push("persist");
        assert_eq!(
            self.next_panel.load(Ordering::Relaxed),
            self.expected_counter
        );
        self.persist_error.take().map_or(Ok(()), Err)
    }

    fn update_event_baseline(&mut self, candidate: &AppSessionSnapshot) {
        self.calls.lock().unwrap().push("baseline");
        assert_eq!(
            self.next_panel.load(Ordering::Relaxed),
            self.expected_counter
        );
        self.baseline = candidate.clone();
    }

    fn emit(&mut self, candidate: &AppSessionSnapshot) -> Result<(), String> {
        self.calls.lock().unwrap().push("emit");
        assert_eq!(
            self.next_panel.load(Ordering::Relaxed),
            self.expected_counter
        );
        self.events.push(candidate.clone());
        Ok(())
    }
}

fn bind_prepared_proxy(
    snapshot: &mut AppSessionSnapshot,
    panel_id: &str,
    lease: Option<&ProxyLeaseRed>,
) {
    let Some(lease) = lease else {
        return;
    };
    let Some(slot) = active_layout_slot(snapshot) else {
        return;
    };
    let Some(layout) = slot.as_mut() else {
        return;
    };
    assert!(set_layout_browser_proxy_url_for_panel(
        layout,
        panel_id,
        Some(&lease.prepared)
    ));
}

fn publish_browser_candidate_red(
    authority: &GatedSnapshot,
    publication: &mut impl SnapshotPublicationOperations,
    proxy: &mut impl BrowserProxyEffectsRed,
    current: &AppSessionSnapshot,
    mut candidate: AppSessionSnapshot,
    panel_id: &str,
) -> Result<AppSessionSnapshot, String> {
    let lease = proxy.prepare(panel_id);
    bind_prepared_proxy(&mut candidate, panel_id, lease.as_ref());
    match publish_snapshot_transaction(authority, Some(current), &candidate, publication) {
        Ok(committed) => {
            if let Some(lease) = lease {
                proxy.commit(lease);
            }
            Ok(committed)
        }
        Err(message) => {
            if let Some(lease) = lease {
                proxy.rollback(lease);
            }
            Err(message)
        }
    }
}

fn current(authority: &GatedSnapshot) -> AppSessionSnapshot {
    authority.lock().unwrap().clone()
}

fn open_browser_red(
    authority: &GatedSnapshot,
    publication: &mut impl SnapshotPublicationOperations,
    proxy: &mut impl BrowserProxyEffectsRed,
    panel_id: &str,
    url: Option<&str>,
) -> Result<Option<AppSessionSnapshot>, String> {
    let before = current(authority);
    let mut candidate = before.clone();
    if !apply_open_browser_url(&mut candidate, panel_id, url) {
        return Ok(None);
    }
    publish_browser_candidate_red(authority, publication, proxy, &before, candidate, panel_id)
        .map(Some)
}

fn split_browser_red(
    authority: &GatedSnapshot,
    next_panel: &AtomicU64,
    publication: &mut impl SnapshotPublicationOperations,
    proxy: &mut impl BrowserProxyEffectsRed,
    panel_id: &str,
) -> Result<AppSessionSnapshot, BrowserPanelCreateErrorRed> {
    let before = current(authority);
    let new_panel_id = format!("surface-{}", next_panel.load(Ordering::Relaxed));
    let mut candidate = before.clone();
    if !apply_split(
        &mut candidate,
        panel_id,
        SessionSplitOrientation::Horizontal,
        &new_panel_id,
        false,
    ) {
        return Err(BrowserPanelCreateErrorRed::NotFound(format!(
            "no pane holds panel id {panel_id}"
        )));
    }
    apply_open_browser_url(&mut candidate, &new_panel_id, Some("https://example.test"));
    let committed = publish_browser_candidate_red(
        authority,
        publication,
        proxy,
        &before,
        candidate,
        &new_panel_id,
    )
    .map_err(BrowserPanelCreateErrorRed::Publication)?;
    next_panel.fetch_add(1, Ordering::Relaxed);
    Ok(committed)
}

fn new_browser_workspace_red(
    authority: &GatedSnapshot,
    next_panel: &AtomicU64,
    publication: &mut impl SnapshotPublicationOperations,
    proxy: &mut impl BrowserProxyEffectsRed,
) -> Result<AppSessionSnapshot, String> {
    let before = current(authority);
    let new_panel_id = format!("surface-{}", next_panel.load(Ordering::Relaxed));
    let mut candidate = before.clone();
    apply_new_workspace(&mut candidate, &new_panel_id, None, None, None, None);
    apply_open_browser_url(&mut candidate, &new_panel_id, Some("https://example.test"));
    let committed = publish_browser_candidate_red(
        authority,
        publication,
        proxy,
        &before,
        candidate,
        &new_panel_id,
    )?;
    next_panel.fetch_add(1, Ordering::Relaxed);
    Ok(committed)
}

fn reopen_browser_red(
    authority: &GatedSnapshot,
    next_panel: &AtomicU64,
    history: &mut Vec<ClosedBrowserTabSnapshot>,
    publication: &mut impl SnapshotPublicationOperations,
    proxy: &mut impl BrowserProxyEffectsRed,
) -> Result<AppSessionSnapshot, String> {
    let before = current(authority);
    let Some(tab) = history.last().cloned() else {
        return Ok(before);
    };
    let new_panel_id = format!("surface-{}", next_panel.load(Ordering::Relaxed));
    let mut candidate = before.clone();
    assert!(apply_reopen_closed_browser_tab(
        &mut candidate,
        &tab,
        &new_panel_id
    ));
    let committed = publish_browser_candidate_red(
        authority,
        publication,
        proxy,
        &before,
        candidate,
        &new_panel_id,
    )?;
    history.pop();
    next_panel.fetch_add(1, Ordering::Relaxed);
    Ok(committed)
}

fn harness<'a>(
    initial: &AppSessionSnapshot,
    next_panel: &'a AtomicU64,
) -> (
    Arc<Mutex<Vec<&'static str>>>,
    GatedSnapshot,
    RecordingPublication<'a>,
    RecordingProxyEffects,
) {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let authority = GatedSnapshot::new(initial.clone());
    let publication = RecordingPublication {
        calls: Arc::clone(&calls),
        persist_error: None,
        baseline: initial.clone(),
        events: Vec::new(),
        next_panel,
        expected_counter: next_panel.load(Ordering::Relaxed),
    };
    let proxy = RecordingProxyEffects {
        calls: Arc::clone(&calls),
        active: Some("socks5://prior".into()),
        unavailable: false,
    };
    (calls, authority, publication, proxy)
}

#[test]
fn proxy_prepare_precedes_persist_and_commit_follows_emit_before_counter_advance() {
    let initial = initial_snapshot("surface-1");
    let next_panel = AtomicU64::new(2);
    let (calls, authority, mut publication, mut proxy) = harness(&initial, &next_panel);
    let committed =
        new_browser_workspace_red(&authority, &next_panel, &mut publication, &mut proxy).unwrap();
    assert_eq!(
        *calls.lock().unwrap(),
        ["prepare", "persist", "baseline", "emit", "commit"]
    );
    assert_eq!(next_panel.load(Ordering::Relaxed), 3);
    assert_eq!(*authority.lock().unwrap(), committed);
    assert_eq!(proxy.active.as_deref(), Some("socks5://prepared/surface-2"));
}

#[test]
fn persist_failure_rolls_back_prior_broker_counter_authority_and_reopen_history() {
    let initial = initial_snapshot("surface-1");
    let next_panel = AtomicU64::new(2);
    let (calls, authority, mut publication, mut proxy) = harness(&initial, &next_panel);
    publication.persist_error = Some("injected browser persistence failure".into());
    let mut history = vec![ClosedBrowserTabSnapshot {
        url: "https://closed.test".into(),
    }];
    assert_eq!(
        reopen_browser_red(
            &authority,
            &next_panel,
            &mut history,
            &mut publication,
            &mut proxy,
        ),
        Err("injected browser persistence failure".into())
    );
    assert_eq!(*calls.lock().unwrap(), ["prepare", "persist", "rollback"]);
    assert_eq!(proxy.active.as_deref(), Some("socks5://prior"));
    assert_eq!(next_panel.load(Ordering::Relaxed), 2);
    assert_eq!(*authority.lock().unwrap(), initial);
    assert_eq!(publication.baseline, initial);
    assert!(publication.events.is_empty());
    assert_eq!(history.len(), 1);
}

#[test]
fn unavailable_proxy_is_best_effort_and_domain_noops_have_no_effects() {
    let initial = initial_snapshot("surface-1");
    let next_panel = AtomicU64::new(2);
    let (calls, authority, mut publication, mut proxy) = harness(&initial, &next_panel);
    proxy.unavailable = true;
    let opened = open_browser_red(
        &authority,
        &mut publication,
        &mut proxy,
        "surface-1",
        Some("https://example.test"),
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        *calls.lock().unwrap(),
        ["prepare", "persist", "baseline", "emit"]
    );
    assert_eq!(*authority.lock().unwrap(), opened);

    calls.lock().unwrap().clear();
    assert_eq!(
        open_browser_red(&authority, &mut publication, &mut proxy, "missing", None,).unwrap(),
        None
    );
    assert_eq!(
        split_browser_red(
            &authority,
            &next_panel,
            &mut publication,
            &mut proxy,
            "missing",
        ),
        Err(BrowserPanelCreateErrorRed::NotFound(
            "no pane holds panel id missing".into()
        ))
    );
    assert!(calls.lock().unwrap().is_empty());
    assert_eq!(next_panel.load(Ordering::Relaxed), 2);
}

#[test]
fn empty_reopen_is_exact_success_noop_without_history_counter_proxy_or_publication() {
    let initial = initial_snapshot("surface-1");
    let next_panel = AtomicU64::new(9);
    let (calls, authority, mut publication, mut proxy) = harness(&initial, &next_panel);
    let mut history = Vec::new();
    assert_eq!(
        reopen_browser_red(
            &authority,
            &next_panel,
            &mut history,
            &mut publication,
            &mut proxy,
        )
        .unwrap(),
        initial
    );
    assert!(calls.lock().unwrap().is_empty());
    assert_eq!(next_panel.load(Ordering::Relaxed), 9);
    assert!(history.is_empty());
}

#[test]
fn split_publication_failure_is_typed_and_successful_reopen_consumes_history_after_emit() {
    let initial = initial_snapshot("surface-1");
    let next_panel = AtomicU64::new(2);
    let (_calls, authority, mut publication, mut proxy) = harness(&initial, &next_panel);
    publication.persist_error = Some("injected split persistence failure".into());
    assert_eq!(
        split_browser_red(
            &authority,
            &next_panel,
            &mut publication,
            &mut proxy,
            "surface-1",
        ),
        Err(BrowserPanelCreateErrorRed::Publication(
            "injected split persistence failure".into()
        ))
    );
    assert_eq!(next_panel.load(Ordering::Relaxed), 2);
    assert_eq!(*authority.lock().unwrap(), initial);
    assert_eq!(proxy.active.as_deref(), Some("socks5://prior"));

    let (calls, authority, mut publication, mut proxy) = harness(&initial, &next_panel);
    let mut history = vec![ClosedBrowserTabSnapshot {
        url: "https://closed.test".into(),
    }];
    reopen_browser_red(
        &authority,
        &next_panel,
        &mut history,
        &mut publication,
        &mut proxy,
    )
    .unwrap();
    assert_eq!(
        *calls.lock().unwrap(),
        ["prepare", "persist", "baseline", "emit", "commit"]
    );
    assert!(history.is_empty());
    assert_eq!(next_panel.load(Ordering::Relaxed), 3);
}
