//! Ownership regression for failures after a browser child has been built.

use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug)]
struct FakeBuiltChild {
    generation: u64,
    close_failures: Vec<String>,
}

impl FakeBuiltChild {
    fn close(&mut self) -> Result<(), String> {
        if self.close_failures.is_empty() {
            Ok(())
        } else {
            Err(self.close_failures.remove(0))
        }
    }
}

#[derive(Clone, Copy)]
enum FakeReservationOwnership {
    Owned,
    Borrowed,
}

#[derive(Default)]
struct FakeBrowserBuildRegistry {
    reserved_panel_ids: BTreeSet<String>,
    children: BTreeMap<String, FakeBuiltChild>,
    ownership_events: Vec<String>,
}

impl FakeBrowserBuildRegistry {
    fn reserve(&mut self, panel_id: &str) {
        assert!(self.reserved_panel_ids.insert(panel_id.to_string()));
        self.ownership_events.push(format!("reserve:{panel_id}"));
    }

    fn settle_failed_build(
        &mut self,
        panel_id: &str,
        mut child: FakeBuiltChild,
        primary: &str,
        ownership: FakeReservationOwnership,
    ) -> String {
        assert!(self.reserved_panel_ids.contains(panel_id));
        self.ownership_events.push(format!("close:{panel_id}"));
        let cleanup = child.close();
        if let Err(cleanup) = cleanup {
            self.children.insert(panel_id.to_string(), child);
            self.ownership_events.push(format!("register:{panel_id}"));
            if matches!(ownership, FakeReservationOwnership::Owned) {
                self.release(panel_id);
            }
            return format!("{primary}; failed to close built browser child: {cleanup}");
        }
        if matches!(ownership, FakeReservationOwnership::Owned) {
            self.release(panel_id);
        }
        primary.to_string()
    }

    fn compensate_borrowed_build(
        &mut self,
        panel_id: &str,
        prior_generation: u64,
    ) -> Result<u64, String> {
        assert!(self.reserved_panel_ids.contains(panel_id));
        let mut child = self
            .children
            .remove(panel_id)
            .expect("failed-close child remains owned for compensation");
        self.ownership_events
            .push(format!("retry-close:{panel_id}"));
        if let Err(error) = child.close() {
            self.children.insert(panel_id.to_string(), child);
            self.ownership_events.push(format!("retain:{panel_id}"));
            return Err(error);
        }
        self.ownership_events
            .push(format!("rebuild-prior:{panel_id}:{prior_generation}"));
        self.children.insert(
            panel_id.to_string(),
            FakeBuiltChild {
                generation: prior_generation,
                close_failures: Vec::new(),
            },
        );
        self.release(panel_id);
        Ok(prior_generation)
    }

    fn release(&mut self, panel_id: &str) {
        assert!(
            !self.ownership_events.is_empty(),
            "ownership must be settled before release"
        );
        assert!(self.reserved_panel_ids.remove(panel_id));
        self.ownership_events.push(format!("release:{panel_id}"));
    }
}

#[test]
fn failed_cleanup_registers_the_built_child_before_owned_reservation_release() {
    let mut registry = FakeBrowserBuildRegistry::default();
    registry.reserve("panel-owned");

    let error = registry.settle_failed_build(
        "panel-owned",
        FakeBuiltChild {
            generation: 41,
            close_failures: vec!["close-41".into()],
        },
        "visibility-41",
        FakeReservationOwnership::Owned,
    );

    assert!(error.contains("visibility-41"));
    assert!(error.contains("close-41"));
    assert_eq!(registry.children["panel-owned"].generation, 41);
    assert!(!registry.reserved_panel_ids.contains("panel-owned"));
    assert_eq!(
        registry.ownership_events,
        [
            "reserve:panel-owned",
            "close:panel-owned",
            "register:panel-owned",
            "release:panel-owned",
        ]
    );
}

#[test]
fn add_init_compensation_closes_new_script_child_before_rebuilding_prior_scripts() {
    let mut registry = FakeBrowserBuildRegistry::default();
    registry.reserve("panel-borrowed");

    let error = registry.settle_failed_build(
        "panel-borrowed",
        FakeBuiltChild {
            generation: 73,
            close_failures: vec!["close-73".into()],
        },
        "zoom-73",
        FakeReservationOwnership::Borrowed,
    );

    assert!(error.contains("zoom-73"));
    assert!(error.contains("close-73"));
    assert!(registry.reserved_panel_ids.contains("panel-borrowed"));
    assert_eq!(registry.children["panel-borrowed"].generation, 73);
    assert_eq!(
        registry.compensate_borrowed_build("panel-borrowed", 72),
        Ok(72)
    );
    assert_eq!(registry.children["panel-borrowed"].generation, 72);
    assert_eq!(
        registry.ownership_events,
        [
            "reserve:panel-borrowed",
            "close:panel-borrowed",
            "register:panel-borrowed",
            "retry-close:panel-borrowed",
            "rebuild-prior:panel-borrowed:72",
            "release:panel-borrowed",
        ]
    );
}

#[test]
fn add_init_compensation_retains_new_script_child_when_retry_close_fails() {
    let mut registry = FakeBrowserBuildRegistry::default();
    registry.reserve("panel-borrowed");

    let primary = registry.settle_failed_build(
        "panel-borrowed",
        FakeBuiltChild {
            generation: 73,
            close_failures: vec!["first-close-73".into(), "retry-close-73".into()],
        },
        "zoom-73",
        FakeReservationOwnership::Borrowed,
    );
    let restore = registry
        .compensate_borrowed_build("panel-borrowed", 72)
        .unwrap_err();

    assert!(primary.contains("first-close-73"));
    assert_eq!(restore, "retry-close-73");
    assert_eq!(registry.children["panel-borrowed"].generation, 73);
    assert!(registry.reserved_panel_ids.contains("panel-borrowed"));
    assert!(!registry
        .ownership_events
        .iter()
        .any(|event| event.starts_with("rebuild-prior:")));
    assert_eq!(
        registry.ownership_events,
        [
            "reserve:panel-borrowed",
            "close:panel-borrowed",
            "register:panel-borrowed",
            "retry-close:panel-borrowed",
            "retain:panel-borrowed",
        ]
    );
}

#[test]
fn successful_cleanup_releases_owned_reservation_without_registering_a_child() {
    let mut registry = FakeBrowserBuildRegistry::default();
    registry.reserve("panel-clean");

    let error = registry.settle_failed_build(
        "panel-clean",
        FakeBuiltChild {
            generation: 9,
            close_failures: Vec::new(),
        },
        "visibility-9",
        FakeReservationOwnership::Owned,
    );

    assert_eq!(error, "visibility-9");
    assert!(!registry.children.contains_key("panel-clean"));
    assert!(!registry.reserved_panel_ids.contains("panel-clean"));
    assert_eq!(
        registry.ownership_events,
        [
            "reserve:panel-clean",
            "close:panel-clean",
            "release:panel-clean",
        ]
    );
}
