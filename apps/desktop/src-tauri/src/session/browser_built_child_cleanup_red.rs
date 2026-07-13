//! Ownership regression for failures after a browser child has been built.

use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug)]
struct FakeBuiltChild {
    generation: u64,
    close_failure: Option<String>,
}

impl FakeBuiltChild {
    fn close(&mut self) -> Result<(), String> {
        self.close_failure.take().map_or(Ok(()), Err)
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

    fn compensate_borrowed_build(&mut self, panel_id: &str) -> u64 {
        assert!(self.reserved_panel_ids.contains(panel_id));
        let generation = self
            .children
            .get(panel_id)
            .expect("failed-close child remains owned for compensation")
            .generation;
        self.ownership_events.push(format!("reuse:{panel_id}"));
        self.release(panel_id);
        generation
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
            close_failure: Some("close-41".into()),
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
fn borrowed_reservation_stays_owned_until_add_init_compensation_reuses_child() {
    let mut registry = FakeBrowserBuildRegistry::default();
    registry.reserve("panel-borrowed");

    let error = registry.settle_failed_build(
        "panel-borrowed",
        FakeBuiltChild {
            generation: 73,
            close_failure: Some("close-73".into()),
        },
        "zoom-73",
        FakeReservationOwnership::Borrowed,
    );

    assert!(error.contains("zoom-73"));
    assert!(error.contains("close-73"));
    assert!(registry.reserved_panel_ids.contains("panel-borrowed"));
    assert_eq!(registry.children["panel-borrowed"].generation, 73);
    assert_eq!(registry.compensate_borrowed_build("panel-borrowed"), 73);
    assert_eq!(
        registry.ownership_events,
        [
            "reserve:panel-borrowed",
            "close:panel-borrowed",
            "register:panel-borrowed",
            "reuse:panel-borrowed",
            "release:panel-borrowed",
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
            close_failure: None,
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

#[test]
fn production_routes_every_post_build_failure_through_owned_cleanup() {
    let browser = include_str!("../browser.rs");
    let upsert = source_item(browser, "fn upsert_browser_webview");
    let post_build = &upsert[upsert.find(".add_child(").expect("browser child build")..];

    assert!(
        !post_build.contains("let _ = webview.close()"),
        "post-build cleanup must never ignore a close error"
    );
    let cleanup_calls = post_build
        .matches("cleanup_built_browser_child_after_failure(")
        .count();
    assert!(
        cleanup_calls >= 4,
        "visibility, zoom, reservation-lock, and registry-lock failures must share owned cleanup"
    );

    let cleanup = source_item(browser, "fn cleanup_built_browser_child_after_failure(");
    for required in [
        "primary",
        "cleanup",
        ".close(",
        "webviews",
        ".insert(",
        "mutation_reservation",
    ] {
        assert!(
            cleanup.contains(required),
            "built-child cleanup is missing ownership element: {required}"
        );
    }
    assert!(
        cleanup.find(".close(").unwrap() < cleanup.find(".insert(").unwrap(),
        "only a child whose cleanup close failed is registered"
    );
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
