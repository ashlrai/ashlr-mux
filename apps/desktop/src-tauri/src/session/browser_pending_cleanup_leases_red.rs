//! Collision-safe ownership for browser children whose cleanup must be retried.

use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug)]
struct FakeChild {
    id: u64,
    close_failures_remaining: usize,
}

impl FakeChild {
    fn close(&mut self) -> Result<(), String> {
        if self.close_failures_remaining > 0 {
            self.close_failures_remaining -= 1;
            return Err(format!("close {} failed", self.id));
        }
        Ok(())
    }
}

#[derive(Debug, Default)]
struct FakePanelEntry {
    main: Option<FakeChild>,
    pending: BTreeMap<u64, FakeChild>,
    network_records: Option<Vec<String>>,
    init_scripts: Option<Vec<String>>,
}

#[derive(Debug)]
struct FakePendingCleanupLease {
    panel_ids: BTreeSet<String>,
    entries: BTreeMap<String, FakePanelEntry>,
}

#[derive(Debug)]
struct FakeRollbackError {
    message: String,
    lease: FakePendingCleanupLease,
}

#[derive(Debug)]
struct FakeFinalizeError {
    failures: Vec<String>,
    retry: FakePendingCleanupLease,
}

#[derive(Debug)]
struct FakeMutationReservation {
    panel_id: String,
    pending_cleanup_id: Option<u64>,
}

impl FakeMutationReservation {
    fn new(panel_id: &str) -> Self {
        Self {
            panel_id: panel_id.to_string(),
            pending_cleanup_id: None,
        }
    }
}

#[derive(Default)]
struct FakeRegistry {
    reserved: BTreeSet<String>,
    main: BTreeMap<String, FakeChild>,
    pending: BTreeMap<String, BTreeMap<u64, FakeChild>>,
    network_records: BTreeMap<String, Vec<String>>,
    init_scripts: BTreeMap<String, Vec<String>>,
    lock_order: Vec<&'static str>,
}

impl FakeRegistry {
    fn retain_failed_build(
        &mut self,
        reservation: &mut FakeMutationReservation,
        mut child: FakeChild,
    ) -> String {
        let panel_id = reservation.panel_id.as_str();
        let error = child.close().unwrap_err();
        let pending_cleanup_id = child.id;
        self.pending
            .entry(panel_id.to_string())
            .or_default()
            .insert(pending_cleanup_id, child);
        reservation.pending_cleanup_id = Some(pending_cleanup_id);
        error
    }

    fn detach(
        &mut self,
        panel_ids: impl IntoIterator<Item = String>,
    ) -> Result<FakePendingCleanupLease, String> {
        let panel_ids = panel_ids.into_iter().collect::<BTreeSet<_>>();
        self.lock_order.extend([
            "reservations",
            "webviews",
            "pending_cleanup_children",
            "network_records",
            "init_scripts",
        ]);
        if let Some(panel_id) = panel_ids
            .iter()
            .find(|panel_id| self.reserved.contains(*panel_id))
        {
            return Err(format!("{panel_id} already reserved"));
        }
        self.reserved.extend(panel_ids.iter().cloned());
        let entries = panel_ids
            .iter()
            .map(|panel_id| {
                (
                    panel_id.clone(),
                    FakePanelEntry {
                        main: self.main.remove(panel_id),
                        pending: self.pending.remove(panel_id).unwrap_or_default(),
                        network_records: self.network_records.remove(panel_id),
                        init_scripts: self.init_scripts.remove(panel_id),
                    },
                )
            })
            .collect();
        Ok(FakePendingCleanupLease { panel_ids, entries })
    }

    fn rollback(&mut self, lease: FakePendingCleanupLease) -> Result<(), FakeRollbackError> {
        let lost = lease
            .panel_ids
            .iter()
            .find(|panel_id| !self.reserved.contains(*panel_id));
        let collision = lease.entries.iter().find_map(|(panel_id, entry)| {
            (entry.main.is_some() && self.main.contains_key(panel_id))
                .then_some(format!("main collision for {panel_id}"))
                .or_else(|| {
                    entry.pending.keys().find_map(|id| {
                        self.pending
                            .get(panel_id)
                            .is_some_and(|children| children.contains_key(id))
                            .then_some(format!("pending collision for {panel_id}/{id}"))
                    })
                })
                .or_else(|| {
                    (entry.network_records.is_some() && self.network_records.contains_key(panel_id))
                        .then_some(format!("network collision for {panel_id}"))
                })
                .or_else(|| {
                    (entry.init_scripts.is_some() && self.init_scripts.contains_key(panel_id))
                        .then_some(format!("script collision for {panel_id}"))
                })
        });
        if lost.is_some() || collision.is_some() {
            let message = lost
                .map(|panel_id| format!("ownership lost for {panel_id}"))
                .or(collision)
                .unwrap();
            return Err(FakeRollbackError { message, lease });
        }
        for (panel_id, entry) in lease.entries {
            if let Some(main) = entry.main {
                self.main.insert(panel_id.clone(), main);
            }
            if !entry.pending.is_empty() {
                self.pending
                    .entry(panel_id.clone())
                    .or_default()
                    .extend(entry.pending);
            }
            if let Some(records) = entry.network_records {
                self.network_records.insert(panel_id.clone(), records);
            }
            if let Some(scripts) = entry.init_scripts {
                self.init_scripts.insert(panel_id, scripts);
            }
        }
        for panel_id in lease.panel_ids {
            self.reserved.remove(&panel_id);
        }
        Ok(())
    }

    fn finalize(&mut self, lease: FakePendingCleanupLease) -> Result<(), FakeFinalizeError> {
        assert!(lease
            .panel_ids
            .iter()
            .all(|panel_id| self.reserved.contains(panel_id)));
        let mut failures = Vec::new();
        let mut retry_panel_ids = BTreeSet::new();
        let mut retry_entries = BTreeMap::new();
        for (panel_id, mut entry) in lease.entries {
            let mut retry = FakePanelEntry {
                network_records: entry.network_records.take(),
                init_scripts: entry.init_scripts.take(),
                ..FakePanelEntry::default()
            };
            if let Some(mut main) = entry.main.take() {
                if let Err(error) = main.close() {
                    failures.push(format!("{panel_id}/main: {error}"));
                    retry.main = Some(main);
                }
            }
            for (id, mut child) in entry.pending {
                if let Err(error) = child.close() {
                    failures.push(format!("{panel_id}/{id}: {error}"));
                    retry.pending.insert(id, child);
                }
            }
            if retry.main.is_none() && retry.pending.is_empty() {
                self.reserved.remove(&panel_id);
            } else {
                retry_panel_ids.insert(panel_id.clone());
                retry_entries.insert(panel_id, retry);
            }
        }
        if failures.is_empty() {
            Ok(())
        } else {
            Err(FakeFinalizeError {
                failures,
                retry: FakePendingCleanupLease {
                    panel_ids: retry_panel_ids,
                    entries: retry_entries,
                },
            })
        }
    }

    fn compensate_exact_pending(
        &mut self,
        reservation: &mut FakeMutationReservation,
    ) -> Result<(), String> {
        let panel_id = reservation.panel_id.as_str();
        let pending_id = reservation
            .pending_cleanup_id
            .take()
            .ok_or_else(|| "reservation has no pending child".to_string())?;
        let mut child = self
            .pending
            .get_mut(panel_id)
            .and_then(|children| children.remove(&pending_id))
            .ok_or_else(|| format!("missing pending child {pending_id}"))?;
        if let Err(error) = child.close() {
            self.pending
                .entry(panel_id.to_string())
                .or_default()
                .insert(pending_id, child);
            reservation.pending_cleanup_id = Some(pending_id);
            return Err(error);
        }
        if self.pending.get(panel_id).is_some_and(BTreeMap::is_empty) {
            self.pending.remove(panel_id);
        }
        Ok(())
    }
}

#[test]
fn failed_build_cleanup_never_overwrites_main_and_retains_every_extra_child() {
    let mut registry = FakeRegistry::default();
    let mut reservation = FakeMutationReservation::new("panel-a");
    registry.main.insert(
        "panel-a".into(),
        FakeChild {
            id: 10,
            close_failures_remaining: 0,
        },
    );
    for id in [20, 21] {
        let error = registry.retain_failed_build(
            &mut reservation,
            FakeChild {
                id,
                close_failures_remaining: 1,
            },
        );
        assert!(error.contains(&id.to_string()));
    }
    assert_eq!(registry.main["panel-a"].id, 10);
    assert_eq!(
        registry.pending["panel-a"]
            .keys()
            .copied()
            .collect::<Vec<_>>(),
        [20, 21]
    );
    assert_eq!(reservation.pending_cleanup_id, Some(21));
}

#[test]
fn failed_build_without_a_main_child_is_still_pending_not_authoritative() {
    let mut registry = FakeRegistry::default();
    let mut reservation = FakeMutationReservation::new("panel-empty");

    let error = registry.retain_failed_build(
        &mut reservation,
        FakeChild {
            id: 44,
            close_failures_remaining: 1,
        },
    );

    assert!(error.contains("44"));
    assert!(!registry.main.contains_key("panel-empty"));
    assert_eq!(registry.pending["panel-empty"][&44].id, 44);
    assert_eq!(reservation.pending_cleanup_id, Some(44));
}

#[test]
fn detach_moves_main_pending_and_metadata_atomically_in_fixed_order() {
    let mut registry = FakeRegistry::default();
    registry.main.insert(
        "panel-a".into(),
        FakeChild {
            id: 10,
            close_failures_remaining: 0,
        },
    );
    registry.pending.insert(
        "panel-a".into(),
        BTreeMap::from([
            (
                20,
                FakeChild {
                    id: 20,
                    close_failures_remaining: 0,
                },
            ),
            (
                21,
                FakeChild {
                    id: 21,
                    close_failures_remaining: 0,
                },
            ),
        ]),
    );
    registry
        .network_records
        .insert("panel-a".into(), vec!["request".into()]);
    registry
        .init_scripts
        .insert("panel-a".into(), vec!["script".into()]);

    let lease = registry.detach(["panel-a".into()]).unwrap();
    assert_eq!(lease.entries["panel-a"].main.as_ref().unwrap().id, 10);
    assert_eq!(
        lease.entries["panel-a"]
            .pending
            .keys()
            .copied()
            .collect::<Vec<_>>(),
        [20, 21]
    );
    assert_eq!(
        registry.lock_order,
        [
            "reservations",
            "webviews",
            "pending_cleanup_children",
            "network_records",
            "init_scripts",
        ]
    );
    assert!(registry.main.is_empty());
    assert!(registry.pending.is_empty());
    assert!(registry.network_records.is_empty());
    assert!(registry.init_scripts.is_empty());
}

#[test]
fn rollback_pending_collision_preserves_the_entire_lease() {
    let mut registry = FakeRegistry::default();
    registry.pending.insert(
        "panel-a".into(),
        BTreeMap::from([(
            20,
            FakeChild {
                id: 20,
                close_failures_remaining: 0,
            },
        )]),
    );
    registry
        .network_records
        .insert("panel-a".into(), vec!["request".into()]);
    let lease = registry.detach(["panel-a".into()]).unwrap();
    registry.pending.insert(
        "panel-a".into(),
        BTreeMap::from([(
            20,
            FakeChild {
                id: 200,
                close_failures_remaining: 0,
            },
        )]),
    );

    let error = registry.rollback(lease).unwrap_err();
    assert!(error.message.contains("pending collision"));
    assert_eq!(error.lease.entries["panel-a"].pending[&20].id, 20);
    assert_eq!(
        error.lease.entries["panel-a"].network_records.as_deref(),
        Some(["request".to_string()].as_slice())
    );
    assert_eq!(registry.pending["panel-a"][&20].id, 200);
    assert!(registry.reserved.contains("panel-a"));

    registry.pending.insert(
        "panel-a".into(),
        BTreeMap::from([(
            30,
            FakeChild {
                id: 30,
                close_failures_remaining: 0,
            },
        )]),
    );
    registry.rollback(error.lease).unwrap();
    assert_eq!(registry.pending["panel-a"][&20].id, 20);
    assert_eq!(registry.pending["panel-a"][&30].id, 30);
    assert!(!registry.reserved.contains("panel-a"));
}

#[test]
fn finalize_retries_every_failed_handle_without_reclosing_successes() {
    let mut registry = FakeRegistry::default();
    registry.main.insert(
        "panel-mixed".into(),
        FakeChild {
            id: 1,
            close_failures_remaining: 0,
        },
    );
    registry.pending.insert(
        "panel-mixed".into(),
        BTreeMap::from([
            (
                2,
                FakeChild {
                    id: 2,
                    close_failures_remaining: 1,
                },
            ),
            (
                3,
                FakeChild {
                    id: 3,
                    close_failures_remaining: 0,
                },
            ),
        ]),
    );
    registry.pending.insert(
        "panel-ok".into(),
        BTreeMap::from([(
            4,
            FakeChild {
                id: 4,
                close_failures_remaining: 0,
            },
        )]),
    );
    let lease = registry
        .detach(["panel-ok".into(), "panel-mixed".into()])
        .unwrap();

    let error = registry.finalize(lease).unwrap_err();
    assert_eq!(error.failures.len(), 1);
    assert!(!registry.reserved.contains("panel-ok"));
    assert!(registry.reserved.contains("panel-mixed"));
    assert!(error.retry.entries["panel-mixed"].main.is_none());
    assert_eq!(
        error.retry.entries["panel-mixed"]
            .pending
            .keys()
            .copied()
            .collect::<Vec<_>>(),
        [2]
    );
    registry.finalize(error.retry).unwrap();
    assert!(registry.reserved.is_empty());
}

#[test]
fn add_init_compensation_targets_only_its_pending_child() {
    let mut registry = FakeRegistry::default();
    let mut reservation = FakeMutationReservation {
        panel_id: "panel-a".into(),
        pending_cleanup_id: Some(200),
    };
    registry.main.insert(
        "panel-a".into(),
        FakeChild {
            id: 100,
            close_failures_remaining: 0,
        },
    );
    registry.pending.insert(
        "panel-a".into(),
        BTreeMap::from([
            (
                200,
                FakeChild {
                    id: 200,
                    close_failures_remaining: 0,
                },
            ),
            (
                201,
                FakeChild {
                    id: 201,
                    close_failures_remaining: 0,
                },
            ),
        ]),
    );

    registry.compensate_exact_pending(&mut reservation).unwrap();
    assert_eq!(registry.main["panel-a"].id, 100);
    assert_eq!(reservation.pending_cleanup_id, None);
    assert_eq!(
        registry.pending["panel-a"]
            .keys()
            .copied()
            .collect::<Vec<_>>(),
        [201]
    );
}
