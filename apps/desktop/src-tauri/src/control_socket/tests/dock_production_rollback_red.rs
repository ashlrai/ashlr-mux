//! Behavioral fault injection for the production Dock commit boundary.

use super::pane_surface_lifecycle::{
    commit_lifecycle_transition, dispatch_lifecycle_request, LifecycleDispatchContext,
    LifecycleEffect, LifecycleEffectExecutor, LifecycleTransition,
};
use super::*;
use crate::dock::{DockCreateRequest, DockStore, DockSurfaceKind};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq)]
struct RuntimeRecord {
    generation: u64,
    kind: String,
}

#[derive(Default)]
struct ProductionDockFaultHarness {
    authoritative: AppSessionSnapshot,
    candidate: Option<AppSessionSnapshot>,
    effects: Vec<LifecycleEffect>,
    runtimes: BTreeMap<String, RuntimeRecord>,
    staged_creates: BTreeMap<String, RuntimeRecord>,
    fail_dock_publish: bool,
    fail_persist: bool,
    teardown_log: Vec<String>,
    recreation_log: Vec<String>,
}

impl ProductionDockFaultHarness {
    fn new(authoritative: AppSessionSnapshot) -> Self {
        Self {
            authoritative,
            ..Self::default()
        }
    }

    fn seed_runtime(&mut self, surface_id: &str, generation: u64, kind: &str) {
        self.runtimes.insert(
            surface_id.to_string(),
            RuntimeRecord {
                generation,
                kind: kind.to_string(),
            },
        );
    }

    fn persist_candidate(&mut self) -> Result<(), &'static str> {
        if self.fail_persist {
            return Err("injected Dock persistence failure");
        }
        self.authoritative = self.candidate.clone().expect("candidate prepared");
        Ok(())
    }

    fn publish_dock(&self) -> Result<(), &'static str> {
        if self.fail_dock_publish {
            Err("injected Dock publication failure")
        } else {
            Ok(())
        }
    }
}

impl LifecycleEffectExecutor for ProductionDockFaultHarness {
    type Error = &'static str;

    fn prepare_transition(&mut self, candidate: &AppSessionSnapshot) -> Result<(), Self::Error> {
        self.candidate = Some(candidate.clone());
        Ok(())
    }

    fn stage(&mut self, effect: &LifecycleEffect) -> Result<(), Self::Error> {
        match effect {
            LifecycleEffect::DockCreate {
                dock_surface_id,
                generation,
                kind,
                ..
            } => {
                self.staged_creates.insert(
                    dock_surface_id.clone(),
                    RuntimeRecord {
                        generation: *generation,
                        kind: kind.clone(),
                    },
                );
            }
            LifecycleEffect::RuntimeTeardown { surface_id, .. } => {
                if self.runtimes.remove(surface_id).is_some() {
                    self.teardown_log.push(surface_id.clone());
                }
            }
            _ => {}
        }
        self.effects.push(effect.clone());
        Ok(())
    }

    fn commit_staged(&mut self) -> Result<(), Self::Error> {
        // This deliberately follows the production executor's current order:
        // persist, publish staged runtime, then publish DockChanged. The RED
        // assertions below describe the transactional behavior required when
        // either of those post-stage boundaries fails.
        self.persist_candidate()?;
        for (surface_id, runtime) in std::mem::take(&mut self.staged_creates) {
            self.runtimes.insert(surface_id, runtime);
        }
        for effect in self.effects.clone() {
            if matches!(effect, LifecycleEffect::DockChanged { .. }) {
                let _ = self.publish_dock();
            }
        }
        Ok(())
    }

    fn rollback_staged(&mut self) {
        for (surface_id, _) in std::mem::take(&mut self.staged_creates) {
            self.runtimes.remove(&surface_id);
            self.teardown_log.push(surface_id);
        }
        self.effects.clear();
        self.candidate = None;
    }
}

fn context() -> LifecycleDispatchContext {
    LifecycleDispatchContext {
        viewport_size: Some((1_000.0, 800.0)),
        browser_enabled: true,
        dock_available: true,
        active_window_id: None,
    }
}

fn owner(snapshot: &AppSessionSnapshot) -> String {
    snapshot.windows[0]
        .window_id
        .clone()
        .unwrap_or_else(|| "main".into())
}

fn transition(snapshot: &AppSessionSnapshot, method: &str, params: Value) -> LifecycleTransition {
    dispatch_lifecycle_request(snapshot, method, params.as_object().unwrap(), &context())
}

#[test]
fn post_persist_dock_publish_failure_restores_authority_and_tears_down_staged_runtime() {
    let before = test_snapshot();
    let created = transition(
        &before,
        "surface.create",
        json!({
            "placement":"dock",
            "window_id":owner(&before),
            "type":"terminal",
            "focus":false,
        }),
    );
    let surface_id = created
        .effects
        .iter()
        .find_map(|effect| match effect {
            LifecycleEffect::DockCreate {
                dock_surface_id, ..
            } => Some(dock_surface_id.clone()),
            _ => None,
        })
        .expect("Dock create effect");
    let mut published = before.clone();
    let mut production = ProductionDockFaultHarness::new(before.clone());
    production.fail_dock_publish = true;

    let result = commit_lifecycle_transition(&mut published, created, &mut production);
    let mut violations = Vec::new();
    if result != Err("injected Dock publication failure") {
        violations.push(format!("publication failure was swallowed: {result:?}"));
    }
    if production.authoritative != before {
        violations.push("authoritative snapshot retained the failed Dock create".to_string());
    }
    if production.runtimes.contains_key(&surface_id) {
        violations.push("staged Dock runtime remained live after rollback".to_string());
    }
    if !production.teardown_log.contains(&surface_id) {
        violations.push("staged Dock runtime teardown was not observed".to_string());
    }
    if published != before {
        violations.push("caller-visible snapshot changed after failure".to_string());
    }
    assert!(violations.is_empty(), "{}", violations.join("; "));
}

#[test]
fn dock_close_persist_failure_recreates_original_runtime_and_keeps_snapshot_unchanged() {
    let mut before = test_snapshot();
    let owner_id = owner(&before);
    let created = DockStore
        .create(
            &mut before,
            &owner_id,
            DockCreateRequest {
                kind: DockSurfaceKind::Terminal,
                focus: true,
                ..DockCreateRequest::default()
            },
        )
        .unwrap();
    let surface_id = created.surface_id.to_string();
    let closed = transition(&before, "surface.close", json!({"surface_id":surface_id}));
    let mut published = before.clone();
    let mut production = ProductionDockFaultHarness::new(before.clone());
    production.seed_runtime(&surface_id, created.generation, "terminal");
    production.fail_persist = true;

    let result = commit_lifecycle_transition(&mut published, closed, &mut production);
    let mut violations = Vec::new();
    if result != Err("injected Dock persistence failure") {
        violations.push(format!(
            "persistence failure was not propagated: {result:?}"
        ));
    }
    if production.authoritative != before || published != before {
        violations.push("snapshot changed despite failed Dock close persistence".to_string());
    }
    if !production.runtimes.contains_key(&surface_id) {
        violations.push("original Dock runtime was not recreated after rollback".to_string());
    }
    if !production.recreation_log.contains(&surface_id) {
        violations.push("Dock runtime compensation was not observed".to_string());
    }
    assert!(violations.is_empty(), "{}", violations.join("; "));
}
