//! Reversible browser-runtime detachment foundation for durable close.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

#[derive(Debug)]
struct FakeBrowserChild {
    generation: u64,
    close_failures_remaining: usize,
    closes: Arc<AtomicUsize>,
    any_registry_lock_held: Arc<AtomicBool>,
}

impl FakeBrowserChild {
    fn close(&mut self) -> Result<(), String> {
        assert!(
            !self.any_registry_lock_held.load(Ordering::SeqCst),
            "browser child close must happen outside every registry lock"
        );
        if self.close_failures_remaining > 0 {
            self.close_failures_remaining -= 1;
            return Err(format!("close generation {} failed", self.generation));
        }
        self.closes.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

#[derive(Debug)]
struct FakeBrowserPanelEntry {
    child: Option<FakeBrowserChild>,
    network_records: Option<Vec<String>>,
    init_scripts: Option<Vec<String>>,
}

#[derive(Debug)]
struct FakeBrowserPanelRuntimeLease {
    panel_ids: BTreeSet<String>,
    entries: BTreeMap<String, FakeBrowserPanelEntry>,
}

#[derive(Debug)]
struct FakeBrowserRollbackError {
    message: String,
    lease: FakeBrowserPanelRuntimeLease,
}

#[derive(Debug)]
struct FakeBrowserFinalizeError {
    failures: Vec<String>,
    retry: FakeBrowserPanelRuntimeLease,
}

#[derive(Default)]
struct FakeBrowserRegistry {
    reserved_panel_ids: BTreeSet<String>,
    webviews: BTreeMap<String, FakeBrowserChild>,
    network_records: BTreeMap<String, Vec<String>>,
    init_scripts: BTreeMap<String, Vec<String>>,
    any_registry_lock_held: Arc<AtomicBool>,
    lock_order: Vec<&'static str>,
}

impl FakeBrowserRegistry {
    fn child(
        &self,
        generation: u64,
        close_failures_remaining: usize,
        closes: &Arc<AtomicUsize>,
    ) -> FakeBrowserChild {
        FakeBrowserChild {
            generation,
            close_failures_remaining,
            closes: Arc::clone(closes),
            any_registry_lock_held: Arc::clone(&self.any_registry_lock_held),
        }
    }

    fn lock_all(&mut self) {
        self.any_registry_lock_held.store(true, Ordering::SeqCst);
        self.lock_order.extend([
            "reservations",
            "webviews",
            "network_records",
            "init_scripts",
        ]);
    }

    fn unlock_all(&self) {
        self.any_registry_lock_held.store(false, Ordering::SeqCst);
    }

    fn detach_panels(
        &mut self,
        panel_ids: impl IntoIterator<Item = String>,
    ) -> Result<FakeBrowserPanelRuntimeLease, String> {
        let panel_ids = panel_ids.into_iter().collect::<BTreeSet<_>>();
        self.lock_all();
        if let Some(panel_id) = panel_ids
            .iter()
            .find(|panel_id| self.reserved_panel_ids.contains(*panel_id))
        {
            self.unlock_all();
            return Err(format!("browser panel {panel_id} is already reserved"));
        }

        self.reserved_panel_ids.extend(panel_ids.iter().cloned());
        let entries = panel_ids
            .iter()
            .map(|panel_id| {
                (
                    panel_id.clone(),
                    FakeBrowserPanelEntry {
                        child: self.webviews.remove(panel_id),
                        network_records: self.network_records.remove(panel_id),
                        init_scripts: self.init_scripts.remove(panel_id),
                    },
                )
            })
            .collect();
        self.unlock_all();
        Ok(FakeBrowserPanelRuntimeLease { panel_ids, entries })
    }

    fn attach(&mut self, panel_id: &str, child: FakeBrowserChild) -> Result<(), String> {
        if self.reserved_panel_ids.contains(panel_id) {
            return Err(format!("browser panel {panel_id} is reserved"));
        }
        self.webviews.insert(panel_id.into(), child);
        Ok(())
    }

    fn finish_built_child(
        &mut self,
        panel_id: &str,
        mut child: FakeBrowserChild,
    ) -> Result<(), String> {
        self.any_registry_lock_held.store(true, Ordering::SeqCst);
        let reserved = self.reserved_panel_ids.contains(panel_id);
        self.any_registry_lock_held.store(false, Ordering::SeqCst);
        if reserved {
            child.close()?;
            return Err(format!("browser panel {panel_id} became reserved"));
        }
        self.webviews.insert(panel_id.into(), child);
        Ok(())
    }

    fn add_init_script(&mut self, panel_id: &str, script: &str) -> Result<(), String> {
        if self.reserved_panel_ids.contains(panel_id) {
            return Err(format!("browser panel {panel_id} is reserved"));
        }
        self.init_scripts
            .entry(panel_id.into())
            .or_default()
            .push(script.into());
        Ok(())
    }

    fn record_network_callback(&mut self, panel_id: &str, record: &str) -> Result<(), String> {
        if self.reserved_panel_ids.contains(panel_id) {
            return Err(format!("browser panel {panel_id} is reserved"));
        }
        self.network_records
            .entry(panel_id.into())
            .or_default()
            .push(record.into());
        Ok(())
    }

    fn clear_network_records(&mut self, panel_id: &str) -> Result<(), String> {
        if self.reserved_panel_ids.contains(panel_id) {
            return Err(format!("browser panel {panel_id} is reserved"));
        }
        self.network_records.remove(panel_id);
        Ok(())
    }

    fn rollback(
        &mut self,
        lease: FakeBrowserPanelRuntimeLease,
    ) -> Result<(), FakeBrowserRollbackError> {
        self.lock_all();
        let lost_ownership = lease
            .panel_ids
            .iter()
            .find(|panel_id| !self.reserved_panel_ids.contains(*panel_id));
        let collision = lease.entries.iter().find_map(|(panel_id, entry)| {
            (entry.child.is_some() && self.webviews.contains_key(panel_id))
                .then_some(format!("webview collision for {panel_id}"))
                .or_else(|| {
                    (entry.network_records.is_some() && self.network_records.contains_key(panel_id))
                        .then_some(format!("network record collision for {panel_id}"))
                })
                .or_else(|| {
                    (entry.init_scripts.is_some() && self.init_scripts.contains_key(panel_id))
                        .then_some(format!("init script collision for {panel_id}"))
                })
        });
        if lost_ownership.is_some() || collision.is_some() {
            let message = lost_ownership
                .map(|panel_id| format!("reservation ownership lost for {panel_id}"))
                .or(collision)
                .expect("rollback precheck failed");
            self.unlock_all();
            return Err(FakeBrowserRollbackError { message, lease });
        }

        for (panel_id, entry) in lease.entries {
            if let Some(child) = entry.child {
                self.webviews.insert(panel_id.clone(), child);
            }
            if let Some(records) = entry.network_records {
                self.network_records.insert(panel_id.clone(), records);
            }
            if let Some(scripts) = entry.init_scripts {
                self.init_scripts.insert(panel_id, scripts);
            }
        }
        for panel_id in lease.panel_ids {
            self.reserved_panel_ids.remove(&panel_id);
        }
        self.unlock_all();
        Ok(())
    }

    fn finalize(
        &mut self,
        lease: FakeBrowserPanelRuntimeLease,
    ) -> Result<(), FakeBrowserFinalizeError> {
        self.any_registry_lock_held.store(true, Ordering::SeqCst);
        let owned = lease
            .panel_ids
            .iter()
            .all(|panel_id| self.reserved_panel_ids.contains(panel_id));
        self.any_registry_lock_held.store(false, Ordering::SeqCst);
        assert!(owned, "finalize requires every reservation");

        let mut failures = Vec::new();
        let mut retry_panel_ids = BTreeSet::new();
        let mut retry_entries = BTreeMap::new();
        for (panel_id, mut entry) in lease.entries {
            let result = entry.child.as_mut().map_or(Ok(()), FakeBrowserChild::close);
            if let Err(error) = result {
                failures.push(format!("{panel_id}: {error}"));
                retry_panel_ids.insert(panel_id.clone());
                retry_entries.insert(panel_id, entry);
            } else {
                self.reserved_panel_ids.remove(&panel_id);
            }
        }
        if failures.is_empty() {
            Ok(())
        } else {
            Err(FakeBrowserFinalizeError {
                failures,
                retry: FakeBrowserPanelRuntimeLease {
                    panel_ids: retry_panel_ids,
                    entries: retry_entries,
                },
            })
        }
    }
}

#[test]
fn browser_detach_is_atomic_deterministic_and_does_not_close() {
    let closes = Arc::new(AtomicUsize::new(0));
    let mut registry = FakeBrowserRegistry::default();
    registry
        .webviews
        .insert("panel-b".into(), registry.child(2, 0, &closes));
    registry
        .network_records
        .insert("panel-a".into(), vec!["request-a".into()]);
    registry
        .init_scripts
        .insert("panel-b".into(), vec!["script-b".into()]);

    let lease = registry
        .detach_panels(["panel-b".into(), "panel-a".into(), "panel-b".into()])
        .unwrap();

    assert_eq!(
        lease
            .panel_ids
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["panel-a", "panel-b"]
    );
    assert_eq!(
        registry.lock_order,
        [
            "reservations",
            "webviews",
            "network_records",
            "init_scripts"
        ]
    );
    assert!(lease.entries["panel-a"].child.is_none());
    assert_eq!(
        lease.entries["panel-a"].network_records.as_deref(),
        Some(["request-a".to_string()].as_slice())
    );
    assert_eq!(closes.load(Ordering::SeqCst), 0);
    assert!(registry.webviews.is_empty());
    assert!(registry.network_records.is_empty());
    assert!(registry.init_scripts.is_empty());

    let before = registry.reserved_panel_ids.clone();
    assert!(registry.detach_panels(["panel-a".into()]).is_err());
    assert_eq!(registry.reserved_panel_ids, before);
}

#[test]
fn reservations_fence_mutations_and_late_upsert_cleans_new_child() {
    let closes = Arc::new(AtomicUsize::new(0));
    let mut registry = FakeBrowserRegistry::default();
    let lease = registry.detach_panels(["panel-a".into()]).unwrap();

    assert!(registry
        .attach("panel-a", registry.child(1, 0, &closes))
        .is_err());
    assert!(registry.add_init_script("panel-a", "script").is_err());
    assert!(registry
        .record_network_callback("panel-a", "request")
        .is_err());
    assert!(registry.clear_network_records("panel-a").is_err());

    registry.reserved_panel_ids.insert("panel-race".into());
    assert!(registry
        .finish_built_child("panel-race", registry.child(2, 0, &closes))
        .is_err());
    assert_eq!(closes.load(Ordering::SeqCst), 1);
    assert!(!registry.webviews.contains_key("panel-race"));
    registry.rollback(lease).unwrap();
}

#[test]
fn rollback_prechecks_every_collision_and_preserves_the_complete_lease() {
    let closes = Arc::new(AtomicUsize::new(0));
    let mut registry = FakeBrowserRegistry::default();
    registry
        .webviews
        .insert("panel-a".into(), registry.child(7, 0, &closes));
    registry
        .network_records
        .insert("panel-a".into(), vec!["request-a".into()]);
    registry
        .init_scripts
        .insert("panel-a".into(), vec!["script-a".into()]);
    let lease = registry.detach_panels(["panel-a".into()]).unwrap();

    registry.reserved_panel_ids.remove("panel-a");
    let ownership_error = registry.rollback(lease).unwrap_err();
    assert!(ownership_error
        .message
        .contains("reservation ownership lost"));
    assert!(registry.webviews.is_empty());
    assert!(registry.network_records.is_empty());
    assert!(registry.init_scripts.is_empty());
    assert_eq!(
        ownership_error.lease.entries["panel-a"]
            .child
            .as_ref()
            .unwrap()
            .generation,
        7
    );

    registry.reserved_panel_ids.insert("panel-a".into());
    registry
        .network_records
        .insert("panel-a".into(), vec!["intruder".into()]);

    let error = registry.rollback(ownership_error.lease).unwrap_err();
    assert!(error.message.contains("network record collision"));
    assert!(!registry.webviews.contains_key("panel-a"));
    assert!(!registry.init_scripts.contains_key("panel-a"));
    assert_eq!(registry.network_records["panel-a"], ["intruder"]);
    assert!(registry.reserved_panel_ids.contains("panel-a"));
    assert_eq!(
        error.lease.entries["panel-a"]
            .child
            .as_ref()
            .unwrap()
            .generation,
        7
    );
    assert_eq!(
        error.lease.entries["panel-a"].network_records.as_deref(),
        Some(["request-a".to_string()].as_slice())
    );
    assert_eq!(
        error.lease.entries["panel-a"].init_scripts.as_deref(),
        Some(["script-a".to_string()].as_slice())
    );

    registry.network_records.remove("panel-a");
    registry.rollback(error.lease).unwrap();
    assert_eq!(registry.webviews["panel-a"].generation, 7);
    assert_eq!(registry.network_records["panel-a"], ["request-a"]);
    assert_eq!(registry.init_scripts["panel-a"], ["script-a"]);
    assert!(!registry.reserved_panel_ids.contains("panel-a"));
    assert_eq!(closes.load(Ordering::SeqCst), 0);
}

#[test]
fn finalize_closes_outside_locks_and_returns_complete_retry_entries() {
    let closes = Arc::new(AtomicUsize::new(0));
    let mut registry = FakeBrowserRegistry::default();
    registry
        .webviews
        .insert("panel-ok".into(), registry.child(1, 0, &closes));
    registry
        .webviews
        .insert("panel-retry".into(), registry.child(2, 1, &closes));
    registry
        .network_records
        .insert("panel-retry".into(), vec!["request".into()]);
    registry
        .init_scripts
        .insert("panel-retry".into(), vec!["script".into()]);
    let lease = registry
        .detach_panels([
            "panel-empty".into(),
            "panel-retry".into(),
            "panel-ok".into(),
        ])
        .unwrap();

    let error = registry.finalize(lease).unwrap_err();
    assert_eq!(error.failures.len(), 1);
    assert_eq!(closes.load(Ordering::SeqCst), 1);
    assert!(!registry.reserved_panel_ids.contains("panel-ok"));
    assert!(!registry.reserved_panel_ids.contains("panel-empty"));
    assert!(registry.reserved_panel_ids.contains("panel-retry"));
    assert_eq!(
        error.retry.panel_ids,
        BTreeSet::from(["panel-retry".into()])
    );
    assert_eq!(
        error.retry.entries["panel-retry"]
            .network_records
            .as_deref(),
        Some(["request".to_string()].as_slice())
    );
    assert_eq!(
        error.retry.entries["panel-retry"].init_scripts.as_deref(),
        Some(["script".to_string()].as_slice())
    );

    registry.finalize(error.retry).unwrap();
    assert_eq!(closes.load(Ordering::SeqCst), 2);
    assert!(registry.reserved_panel_ids.is_empty());
}

#[test]
fn browser_runtime_lease_contract_is_present_in_production() {
    let browser = include_str!("../browser.rs");
    assert!(
        browser.contains("reserved_panel_ids: Mutex<BTreeSet<String>>"),
        "BrowserWebviewState needs a dedicated reservation mutex"
    );
    for required in [
        "BrowserPanelRuntimeLease",
        "BrowserPanelRollbackError",
        "BrowserPanelFinalizeError",
        "detach_browser_panels_for_control",
        "rollback_browser_panels_for_control",
        "finalize_browser_panels_for_control",
    ] {
        assert!(
            browser.contains(required),
            "missing browser lease API: {required}"
        );
    }

    let detach = source_item(browser, "detach_browser_panels_for_control");
    let reservations = detach.find("reserved_panel_ids").unwrap();
    let webviews = detach[reservations..].find("webviews").unwrap() + reservations;
    let records = detach[webviews..].find("network_records").unwrap() + webviews;
    let scripts = detach[records..].find("init_scripts").unwrap() + records;
    assert!(reservations < webviews && webviews < records && records < scripts);
    assert!(
        !detach.contains(".close("),
        "detach transfers ownership without closing"
    );

    let upsert = source_item(browser, "fn upsert_browser_webview(");
    let build = upsert.find(".add_child(").expect("upsert builds a child");
    let reservation_checks = upsert
        .match_indices("reserved_panel_ids")
        .collect::<Vec<_>>();
    assert!(
        reservation_checks.len() >= 2,
        "upsert must check reservations twice"
    );
    assert!(reservation_checks[0].0 < build);
    assert!(reservation_checks.last().unwrap().0 > build);
    assert!(
        upsert[build..].contains(".close("),
        "late rejection must close the built child"
    );

    for mutation in [
        "pub(crate) fn browser_add_init_script_for_control(",
        "pub(crate) fn browser_clear_network_requests_for_control(",
        "fn push_network_record(",
    ] {
        assert!(
            source_item(browser, mutation).contains("reserved_panel_ids"),
            "{mutation} must reject reserved panels"
        );
    }

    let rollback = source_item(browser, "rollback_browser_panels_for_control");
    assert!(
        rollback.contains("contains_key"),
        "rollback must precheck key collisions"
    );
    let finalize = source_item(browser, "finalize_browser_panels_for_control");
    assert!(finalize.contains(".close("));
    assert!(finalize.contains("retry"));
    assert!(finalize.contains("drop("));
    assert!(finalize.find("drop(").unwrap() < finalize.find(".close(").unwrap());

    let session = include_str!("../session.rs");
    for close_path in [
        "fn close_panel_for_control",
        "fn close_workspace_in_window_for_control",
    ] {
        let item = source_item(session, close_path);
        assert!(!item.contains("detach_browser_panels_for_control"));
        assert!(!item.contains("BrowserPanelRuntimeLease"));
    }
}

fn source_item<'a>(source: &'a str, marker: &str) -> &'a str {
    let start = source
        .find(marker)
        .unwrap_or_else(|| panic!("missing source marker: {marker}"));
    let brace = source[start..]
        .find('{')
        .map(|offset| start + offset)
        .unwrap_or_else(|| panic!("missing opening brace after: {marker}"));
    let mut depth = 0usize;
    for (offset, byte) in source[brace..].bytes().enumerate() {
        match byte {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return &source[start..=brace + offset];
                }
            }
            _ => {}
        }
    }
    panic!("unclosed source item: {marker}")
}
