//! Deterministic RED contract for short-lived remote runtime lifecycle leases.
//!
//! `remote_proxy` transport is intentionally out of scope. These tests cover
//! only the command/callback ownership boundary between a lifecycle request,
//! authoritative tmux observation, and local topology publication.

use super::pane_surface_lifecycle::{
    dispatch_lifecycle_request, reconcile_runtime_arrival, LifecycleDispatchContext,
    LifecycleEffect, LifecycleTransition, RuntimeArrival, RuntimeDeparture,
};
use super::*;
use crate::dock::{DockCreateRequest, DockStore, DockSurfaceKind};
use cmux_core::session::{
    SessionSurfaceKindSnapshot, SessionSurfaceMetadataSnapshot, SessionSurfaceSnapshot,
    SessionWorkspaceLayoutSnapshot,
};

const WINDOW: &str = "10000000-0000-0000-0000-000000000001";
const WORKSPACE: &str = "20000000-0000-0000-0000-000000000001";
const PANE: &str = "30000000-0000-0000-0000-000000000001";
const SOURCE: &str = "40000000-0000-0000-0000-000000000001";
const SIBLING: &str = "40000000-0000-0000-0000-000000000002";
const RESERVED: &str = "40000000-0000-0000-0000-000000000003";
const RESERVED_PANE: &str = "30000000-0000-0000-0000-000000000003";
const SECOND_WORKSPACE: &str = "20000000-0000-0000-0000-000000000004";
const SECOND_PANE: &str = "30000000-0000-0000-0000-000000000004";
const SECOND_SOURCE: &str = "40000000-0000-0000-0000-000000000007";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LeaseSurfaceKind {
    RemoteTerminal,
    LocalTerminal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SourceWitness {
    surface_id: String,
    generation: u64,
    kind: LeaseSurfaceKind,
    remote_token: String,
    move_generation: u64,
    restore_epoch: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LeaseScope {
    endpoint: String,
    session: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ArrivalCallback {
    lease_id: u64,
    scope: LeaseScope,
    source: SourceWitness,
    result_surface_id: String,
    result_pane_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CommandOutcome {
    Succeeded,
    Failed,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LeaseDisposition {
    AwaitCallback,
    ReconcileObservation,
    Compensate,
    Committed,
    Cancelled,
    CleanupRetry,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CallbackDecision {
    Commit,
    Duplicate,
    Stale,
}

#[derive(Debug, Clone)]
struct RuntimeLease {
    id: u64,
    scope: LeaseScope,
    source: SourceWitness,
    reserved_surface_id: String,
    reserved_pane_id: String,
    expires_at: u64,
    disposition: LeaseDisposition,
    cleanup_attempts: usize,
}

#[derive(Default)]
struct LeaseCoordinator {
    now: u64,
    restore_epoch: u64,
    next_id: u64,
    leases: BTreeMap<u64, RuntimeLease>,
}

impl LeaseCoordinator {
    fn reserve(
        &mut self,
        scope: LeaseScope,
        mut source: SourceWitness,
        surface_id: &str,
        pane_id: &str,
        ttl: u64,
    ) -> u64 {
        self.next_id += 1;
        source.restore_epoch = self.restore_epoch;
        let id = self.next_id;
        self.leases.insert(
            id,
            RuntimeLease {
                id,
                scope,
                source,
                reserved_surface_id: surface_id.into(),
                reserved_pane_id: pane_id.into(),
                expires_at: self.now + ttl,
                disposition: LeaseDisposition::AwaitCallback,
                cleanup_attempts: 0,
            },
        );
        id
    }

    fn command_outcome(&mut self, id: u64, outcome: CommandOutcome) -> LeaseDisposition {
        let lease = self.leases.get_mut(&id).unwrap();
        lease.disposition = match outcome {
            CommandOutcome::Succeeded => LeaseDisposition::AwaitCallback,
            CommandOutcome::Failed => LeaseDisposition::Compensate,
            CommandOutcome::Unknown => LeaseDisposition::ReconcileObservation,
        };
        lease.disposition
    }

    fn callback(&mut self, callback: &ArrivalCallback) -> CallbackDecision {
        let Some(lease) = self.leases.get_mut(&callback.lease_id) else {
            return CallbackDecision::Stale;
        };
        if lease.disposition == LeaseDisposition::Committed {
            return CallbackDecision::Duplicate;
        }
        let exact = lease.id == callback.lease_id
            && lease.scope == callback.scope
            && lease.source == callback.source
            && lease.source.restore_epoch == self.restore_epoch
            && lease.reserved_surface_id == callback.result_surface_id
            && lease.reserved_pane_id == callback.result_pane_id
            && self.now < lease.expires_at
            && matches!(
                lease.disposition,
                LeaseDisposition::AwaitCallback | LeaseDisposition::ReconcileObservation
            );
        if !exact {
            return CallbackDecision::Stale;
        }
        lease.disposition = LeaseDisposition::Committed;
        CallbackDecision::Commit
    }

    fn expire(&mut self, id: u64) -> bool {
        let lease = self.leases.get_mut(&id).unwrap();
        if self.now < lease.expires_at
            || matches!(
                lease.disposition,
                LeaseDisposition::Committed | LeaseDisposition::Cancelled
            )
        {
            return false;
        }
        lease.disposition = LeaseDisposition::Compensate;
        true
    }

    fn cleanup_result(&mut self, id: u64, succeeded: bool) {
        let lease = self.leases.get_mut(&id).unwrap();
        if succeeded {
            lease.disposition = LeaseDisposition::Cancelled;
        } else {
            lease.cleanup_attempts += 1;
            lease.disposition = LeaseDisposition::CleanupRetry;
        }
    }

    fn source_disappeared(&mut self, surface_id: &str) {
        for lease in self.leases.values_mut() {
            if lease.source.surface_id == surface_id
                && lease.disposition != LeaseDisposition::Committed
            {
                lease.disposition = LeaseDisposition::Compensate;
            }
        }
    }

    fn restore(&mut self) {
        self.restore_epoch += 1;
        for lease in self.leases.values_mut() {
            if lease.disposition != LeaseDisposition::Committed {
                lease.disposition = LeaseDisposition::Cancelled;
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct DepartureKey {
    endpoint: String,
    session: String,
    surface_id: String,
    generation: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DepartureObservation {
    Present,
    TransientDisconnect,
    Absent,
}

#[derive(Default)]
struct DepartureCoordinator {
    pending: BTreeMap<DepartureKey, usize>,
}

impl DepartureCoordinator {
    fn register(&mut self, key: DepartureKey) {
        self.pending.entry(key).or_default();
    }

    fn observe(&mut self, key: &DepartureKey, observation: DepartureObservation) -> bool {
        self.pending.contains_key(key) && observation == DepartureObservation::Absent
    }

    fn commit_result(&mut self, key: &DepartureKey, succeeded: bool) {
        if succeeded {
            self.pending.remove(key);
        } else if let Some(attempts) = self.pending.get_mut(key) {
            *attempts += 1;
        }
    }
}

fn scope(endpoint: &str, session: &str) -> LeaseScope {
    LeaseScope {
        endpoint: endpoint.into(),
        session: session.into(),
    }
}

fn witness() -> SourceWitness {
    SourceWitness {
        surface_id: SOURCE.into(),
        generation: 1,
        kind: LeaseSurfaceKind::RemoteTerminal,
        remote_token: "%source".into(),
        move_generation: 7,
        restore_epoch: 0,
    }
}

fn callback(id: u64) -> ArrivalCallback {
    ArrivalCallback {
        lease_id: id,
        scope: scope("ssh://host-a", "tmux-a"),
        source: witness(),
        result_surface_id: RESERVED.into(),
        result_pane_id: RESERVED_PANE.into(),
    }
}

fn remote_surface(id: &str) -> SessionSurfaceSnapshot {
    SessionSurfaceSnapshot {
        surface_id: id.into(),
        pane_id: PANE.into(),
        generation: 1,
        kind: SessionSurfaceKindSnapshot::RemoteTerminal {
            remote_session_id: Some("%source".into()),
            remote_context: None,
            arrival_generation: Some(1),
        },
        metadata: SessionSurfaceMetadataSnapshot::default(),
        terminal_startup: None,
        scrollback: None,
    }
}

fn local_surface(id: &str) -> SessionSurfaceSnapshot {
    SessionSurfaceSnapshot {
        surface_id: id.into(),
        pane_id: PANE.into(),
        generation: 1,
        kind: SessionSurfaceKindSnapshot::Terminal,
        metadata: SessionSurfaceMetadataSnapshot::default(),
        terminal_startup: None,
        scrollback: None,
    }
}

fn remote_snapshot() -> AppSessionSnapshot {
    let mut snapshot = test_snapshot();
    let window = &mut snapshot.windows[0];
    window.window_id = Some(WINDOW.into());
    window.selected_workspace_id = Some(WORKSPACE.into());
    let workspace = &mut window.tab_manager.workspaces[0];
    workspace.workspace_id = Some(WORKSPACE.into());
    workspace.focused_panel_id = Some(SOURCE.into());
    let SessionWorkspaceLayoutSnapshot::Pane(pane) = workspace.layout.as_mut().unwrap() else {
        panic!("single-pane fixture")
    };
    pane.pane_id = Some(PANE.into());
    pane.panel_ids = vec![SOURCE.into(), SIBLING.into()];
    pane.selected_panel_id = Some(SOURCE.into());
    workspace.surfaces = Some(vec![remote_surface(SOURCE), local_surface(SIBLING)]);
    let mut encoded = serde_json::to_value(snapshot).unwrap();
    encoded["windows"][0]["tab_manager"]["workspaces"][0]["remote"] = json!({
        "enabled": true,
        "connected": true,
        "state": "connected",
        "transport": "tmux",
        "destination": "host-a",
        "persistent_daemon_slot": "tmux-a"
    });
    serde_json::from_value(encoded).unwrap()
}

fn remote_snapshot_with_second_slot() -> AppSessionSnapshot {
    let mut snapshot = remote_snapshot();
    snapshot.windows[0].tab_manager.workspaces[0]
        .surfaces
        .as_mut()
        .unwrap()[0]
        .kind = SessionSurfaceKindSnapshot::RemoteTerminal {
        remote_session_id: Some("%34".into()),
        remote_context: None,
        arrival_generation: Some(1),
    };
    let mut workspace = snapshot.windows[0].tab_manager.workspaces[0].clone();
    workspace.workspace_id = Some(SECOND_WORKSPACE.into());
    workspace.focused_panel_id = Some(SECOND_SOURCE.into());
    let SessionWorkspaceLayoutSnapshot::Pane(pane) = workspace.layout.as_mut().unwrap() else {
        panic!("single-pane fixture")
    };
    pane.pane_id = Some(SECOND_PANE.into());
    pane.panel_ids = vec![SECOND_SOURCE.into()];
    pane.selected_panel_id = Some(SECOND_SOURCE.into());
    workspace.surfaces = Some(vec![SessionSurfaceSnapshot {
        surface_id: SECOND_SOURCE.into(),
        pane_id: SECOND_PANE.into(),
        generation: 1,
        kind: SessionSurfaceKindSnapshot::RemoteTerminal {
            remote_session_id: Some("%35".into()),
            remote_context: None,
            arrival_generation: Some(1),
        },
        metadata: SessionSurfaceMetadataSnapshot::default(),
        terminal_startup: None,
        scrollback: None,
    }]);
    let remote = workspace.remote.as_mut().unwrap();
    remote.destination = Some("host-a".into());
    remote.persistent_daemon_slot = Some("tmux-b".into());
    snapshot.windows[0].tab_manager.workspaces.push(workspace);
    snapshot
}

fn context() -> LifecycleDispatchContext {
    LifecycleDispatchContext {
        viewport_size: Some((1200.0, 800.0)),
        browser_enabled: true,
        dock_available: true,
        active_window_id: Some(WINDOW.into()),
    }
}

fn remote_snapshot_with_dock() -> (AppSessionSnapshot, String) {
    let mut snapshot = remote_snapshot();
    let created = DockStore
        .create(
            &mut snapshot,
            WINDOW,
            DockCreateRequest {
                kind: DockSurfaceKind::Terminal,
                focus: false,
                ..DockCreateRequest::default()
            },
        )
        .expect("seed Dock destination");
    (snapshot, created.pane_id.to_string())
}

fn dispatch(snapshot: &AppSessionSnapshot, method: &str, params: Value) -> LifecycleTransition {
    dispatch_lifecycle_request(snapshot, method, params.as_object().unwrap(), &context())
}

fn ok_value(transition: &LifecycleTransition) -> Value {
    let ControlCallResult::Ok(value) = &transition.result else {
        panic!("expected success, got {:?}", transition.result)
    };
    value.clone().into()
}

#[test]
fn pre_command_exact_lease_reserves_private_result_identities_before_remote_mutation() {
    let mut coordinator = LeaseCoordinator::default();
    let id = coordinator.reserve(
        scope("ssh://host-a", "tmux-a"),
        witness(),
        RESERVED,
        PANE,
        30,
    );
    let lease = &coordinator.leases[&id];
    assert_eq!(
        (&lease.reserved_surface_id, &lease.reserved_pane_id),
        (&RESERVED.to_string(), &PANE.to_string())
    );

    let transition = dispatch(
        &remote_snapshot(),
        "surface.action",
        json!({"surface_id":SOURCE,"action":"new-terminal-right"}),
    );
    let result = ok_value(&transition);
    assert_eq!(result["accepted"], true);
    assert_eq!(result["routed"], "remote-tmux");
    assert!(result["created_surface_id"].is_null());
    assert!(result["created_tab_id"].is_null());
    assert_eq!(transition.snapshot, remote_snapshot());
    assert!(transition
        .events
        .iter()
        .all(|event| event.name != "surface.created"));
}

#[test]
fn every_remote_create_entrypoint_uses_the_persistent_tmux_session_scope() {
    let snapshot = remote_snapshot_with_second_slot();
    let pane_a = dispatch(
        &snapshot,
        "pane.create",
        json!({"workspace_id":WORKSPACE,"surface_id":SOURCE,"type":"terminal","direction":"right"}),
    );
    let action_a = dispatch(
        &snapshot,
        "surface.action",
        json!({"workspace_id":WORKSPACE,"surface_id":SOURCE,"action":"new-terminal-right"}),
    );
    let pane_b = dispatch(
        &snapshot,
        "pane.create",
        json!({"workspace_id":SECOND_WORKSPACE,"surface_id":SECOND_SOURCE,"type":"terminal","direction":"right"}),
    );
    let mut fallback = remote_snapshot();
    fallback.windows[0].tab_manager.workspaces[0]
        .surfaces
        .as_mut()
        .unwrap()[0]
        .kind = SessionSurfaceKindSnapshot::RemoteTerminal {
        remote_session_id: Some("%34".into()),
        remote_context: None,
        arrival_generation: Some(1),
    };
    fallback.windows[0].tab_manager.workspaces[0]
        .remote
        .as_mut()
        .unwrap()
        .persistent_daemon_slot = None;
    let pane_fallback = dispatch(
        &fallback,
        "pane.create",
        json!({"workspace_id":WORKSPACE,"surface_id":SOURCE,"type":"terminal","direction":"right"}),
    );
    let action_fallback = dispatch(
        &fallback,
        "surface.action",
        json!({"workspace_id":WORKSPACE,"surface_id":SOURCE,"action":"new-terminal-right"}),
    );

    let scope = |transition: &LifecycleTransition| {
        transition.effects.iter().find_map(|effect| match effect {
            LifecycleEffect::RemoteCreate {
                destination,
                remote_session_id,
                ..
            } => Some((destination.clone(), remote_session_id.clone())),
            _ => None,
        })
    };
    assert_eq!(
        scope(&pane_a),
        Some(("host-a".into(), "tmux-a".into())),
        "{:?}",
        pane_a.result
    );
    assert_eq!(scope(&action_a), Some(("host-a".into(), "tmux-a".into())));
    assert_eq!(scope(&pane_b), Some(("host-a".into(), "tmux-b".into())));
    assert_eq!(
        scope(&pane_fallback),
        Some(("host-a".into(), "host-a".into()))
    );
    assert_eq!(
        scope(&action_fallback),
        Some(("host-a".into(), "host-a".into()))
    );
}

#[test]
fn unknown_command_outcome_keeps_one_owner_for_reconcile_then_compensation() {
    let mut coordinator = LeaseCoordinator::default();
    let id = coordinator.reserve(
        scope("ssh://host-a", "tmux-a"),
        witness(),
        RESERVED,
        RESERVED_PANE,
        30,
    );
    assert_eq!(
        coordinator.command_outcome(id, CommandOutcome::Unknown),
        LeaseDisposition::ReconcileObservation
    );
    coordinator.command_outcome(id, CommandOutcome::Failed);
    assert_eq!(
        coordinator.leases[&id].disposition,
        LeaseDisposition::Compensate
    );
    assert_eq!(
        remote_observation_action(true, Some("remote command outcome unknown"), 0),
        RemoteObservationAction::Reconcile,
        "an unknown command outcome must query authoritative state, never retry the mutation"
    );
}

#[test]
fn publishing_claim_renews_the_lease_before_snapshot_commit() {
    let mut registry = RemoteRuntimeLeaseRegistry::default();
    let id = registry.reserve(
        RemoteRuntimeLeaseScope {
            endpoint: "ssh://host-a".into(),
            session: "tmux-a".into(),
        },
        RemoteRuntimeSourceWitness {
            window_id: WINDOW.into(),
            workspace_id: WORKSPACE.into(),
            pane_id: PANE.into(),
            surface_id: SOURCE.into(),
            surface_generation: 1,
            remote_token: "%source".into(),
            move_generation: 1,
            restore_epoch: 0,
        },
        RESERVED.into(),
        RESERVED_PANE.into(),
        RemoteTmuxTarget::Window,
    );
    registry.record_command_outcome(
        id,
        RemoteRuntimeCommandOutcome::Succeeded,
        Some("@42".into()),
    );
    registry.leases.get_mut(&id).unwrap().expires_at = Instant::now() + Duration::from_secs(1);
    let original_deadline = registry.leases[&id].expires_at;
    let lease = registry.leases[&id].clone();

    assert_eq!(
        registry.claim_callback(
            id,
            &lease.scope,
            &lease.source,
            &lease.reserved_surface_id,
            &lease.reserved_pane_id,
        ),
        RemoteRuntimeCallbackClaim::Publish
    );
    assert!(
        registry.leases[&id].expires_at >= original_deadline + Duration::from_secs(20),
        "the publishing claim needs a fresh deadline before the original watchdog can expire it"
    );
}

#[test]
fn successful_command_with_unparseable_output_reconciles_authoritative_topology() {
    let before = vec![RemoteTmuxTopologyEntry {
        window_id: "@1".into(),
        pane_id: "%1".into(),
    }];
    let after = vec![
        before[0].clone(),
        RemoteTmuxTopologyEntry {
            window_id: "@42".into(),
            pane_id: "%34".into(),
        },
    ];
    let (window, pane) = resolve_remote_tmux_create_observation(
        RemoteTmuxTarget::Window,
        "successful-but-unparseable-output",
        &before,
        || Ok(after),
    )
    .expect("authoritative topology should recover the created runtime identity");
    assert_eq!(
        window,
        Some(RemoteTmuxObservation {
            window_token: "@42".into(),
            pane_token: "%34".into(),
        })
    );
    assert_eq!(pane, None);
}

#[test]
fn definitive_remote_command_failure_retires_without_topology_compensation() {
    let mut registry = RemoteRuntimeLeaseRegistry::default();
    let id = registry.reserve(
        RemoteRuntimeLeaseScope {
            endpoint: "ssh://host-a".into(),
            session: "tmux-a".into(),
        },
        RemoteRuntimeSourceWitness {
            window_id: WINDOW.into(),
            workspace_id: WORKSPACE.into(),
            pane_id: PANE.into(),
            surface_id: SOURCE.into(),
            surface_generation: 1,
            remote_token: "%source".into(),
            move_generation: 1,
            restore_epoch: 0,
        },
        RESERVED.into(),
        RESERVED_PANE.into(),
        RemoteTmuxTarget::Window,
    );
    registry.record_command_outcome(id, RemoteRuntimeCommandOutcome::Failed, None);
    assert!(
        !registry.leases.contains_key(&id),
        "a definitive nonzero command exit must not retain a baseline that could kill an unrelated create"
    );
}

#[test]
fn unresolved_same_scope_create_blocks_a_second_ambiguous_baseline_owner() {
    let state = RemoteRuntimeLeaseRegistryState::default();
    let scope = RemoteRuntimeLeaseScope {
        endpoint: "ssh://host-a".into(),
        session: "tmux-a".into(),
    };
    let source = RemoteRuntimeSourceWitness {
        window_id: WINDOW.into(),
        workspace_id: WORKSPACE.into(),
        pane_id: PANE.into(),
        surface_id: SOURCE.into(),
        surface_generation: 1,
        remote_token: "%source".into(),
        move_generation: 1,
        restore_epoch: 0,
    };
    let first = reserve_remote_runtime_lease(
        &state,
        scope.clone(),
        source.clone(),
        RESERVED.into(),
        RESERVED_PANE.into(),
        RemoteTmuxTarget::Window,
        Vec::new(),
    )
    .unwrap();
    state.registry.lock().unwrap().record_command_outcome(
        first,
        RemoteRuntimeCommandOutcome::Unknown,
        None,
    );
    assert!(
        reserve_remote_runtime_lease(
            &state,
            scope,
            source,
            "40000000-0000-0000-0000-000000000006".into(),
            "30000000-0000-0000-0000-000000000006".into(),
            RemoteTmuxTarget::Window,
            Vec::new(),
        )
        .is_err(),
        "an unresolved create must remain the only topology-diff owner for its endpoint/session"
    );
}

#[test]
fn uninitialized_topology_baseline_can_never_select_a_compensation_target() {
    let mut registry = RemoteRuntimeLeaseRegistry::default();
    let id = registry.reserve(
        RemoteRuntimeLeaseScope {
            endpoint: "ssh://host-a".into(),
            session: "tmux-a".into(),
        },
        RemoteRuntimeSourceWitness {
            window_id: WINDOW.into(),
            workspace_id: WORKSPACE.into(),
            pane_id: PANE.into(),
            surface_id: SOURCE.into(),
            surface_generation: 1,
            remote_token: "%source".into(),
            move_generation: 1,
            restore_epoch: 0,
        },
        RESERVED.into(),
        RESERVED_PANE.into(),
        RemoteTmuxTarget::Window,
    );
    let lease = registry.leases[&id].clone();
    let unrelated_existing_runtime = vec![RemoteTmuxTopologyEntry {
        window_id: "@1".into(),
        pane_id: "%1".into(),
    }];
    assert_eq!(
        remote_runtime_compensation_token(&lease, &unrelated_existing_runtime),
        None,
        "absence of a baseline is not evidence that every observed runtime was newly created"
    );
}

#[test]
fn baseline_completion_revalidates_source_witness_before_remote_mutation() {
    let mut registry = RemoteRuntimeLeaseRegistry::default();
    let id = registry.reserve(
        RemoteRuntimeLeaseScope {
            endpoint: "ssh://host-a".into(),
            session: "tmux-a".into(),
        },
        RemoteRuntimeSourceWitness {
            window_id: WINDOW.into(),
            workspace_id: WORKSPACE.into(),
            pane_id: PANE.into(),
            surface_id: SOURCE.into(),
            surface_generation: 1,
            remote_token: "%source".into(),
            move_generation: 1,
            restore_epoch: 0,
        },
        RESERVED.into(),
        RESERVED_PANE.into(),
        RemoteTmuxTarget::Window,
    );
    let mut moved = remote_snapshot();
    moved.windows[0].tab_manager.workspaces[0]
        .surfaces
        .as_mut()
        .unwrap()[0]
        .generation = 2;
    assert!(validate_remote_runtime_mutation_fence(&mut registry, id, &moved).is_err());
    assert!(
        !registry.leases.contains_key(&id),
        "a stale source must retire ownership before the mutating SSH command is issued"
    );
}

#[test]
fn expired_await_command_lease_rejects_late_success_without_resurrection() {
    let mut registry = RemoteRuntimeLeaseRegistry::default();
    let id = registry.reserve(
        RemoteRuntimeLeaseScope {
            endpoint: "ssh://host-a".into(),
            session: "tmux-a".into(),
        },
        RemoteRuntimeSourceWitness {
            window_id: WINDOW.into(),
            workspace_id: WORKSPACE.into(),
            pane_id: PANE.into(),
            surface_id: SOURCE.into(),
            surface_generation: 1,
            remote_token: "%source".into(),
            move_generation: 1,
            restore_epoch: 0,
        },
        RESERVED.into(),
        RESERVED_PANE.into(),
        RemoteTmuxTarget::Window,
    );
    assert!(registry
        .expire(id, Instant::now() + REMOTE_RUNTIME_LEASE_TTL)
        .is_some());
    assert!(!registry.record_command_outcome(
        id,
        RemoteRuntimeCommandOutcome::Succeeded,
        Some("@42".into()),
    ));
    assert_eq!(
        registry.leases[&id].remote_target_token.as_deref(),
        Some("@42"),
        "late success cannot publish, but its exact token must replace ambiguous cleanup inference"
    );
    assert_eq!(
        registry.leases[&id].disposition,
        RemoteRuntimeLeaseDisposition::Compensate,
        "a late command result cannot retake ownership from watchdog compensation"
    );
}

#[test]
fn spawn_failure_is_definitive_but_wait_failure_retains_unknown_ownership() {
    assert_eq!(
        remote_runtime_process_error_outcome(false),
        RemoteRuntimeCommandOutcome::Failed
    );
    assert_eq!(
        remote_runtime_process_error_outcome(true),
        RemoteRuntimeCommandOutcome::Unknown
    );
    let production = include_str!("../../control_socket.rs");
    assert!(production.contains(".spawn()"));
    assert!(production.contains(".wait_with_output()"));
}

#[test]
fn wait_failure_never_kills_an_unproven_topology_diff() {
    let mut registry = RemoteRuntimeLeaseRegistry::default();
    let id = registry.reserve(
        RemoteRuntimeLeaseScope {
            endpoint: "ssh://host-a".into(),
            session: "tmux-a".into(),
        },
        RemoteRuntimeSourceWitness {
            window_id: WINDOW.into(),
            workspace_id: WORKSPACE.into(),
            pane_id: PANE.into(),
            surface_id: SOURCE.into(),
            surface_generation: 1,
            remote_token: "%source".into(),
            move_generation: 1,
            restore_epoch: 0,
        },
        RESERVED.into(),
        RESERVED_PANE.into(),
        RemoteTmuxTarget::Window,
    );
    registry.leases.get_mut(&id).unwrap().topology_before = Some(vec![RemoteTmuxTopologyEntry {
        window_id: "@1".into(),
        pane_id: "%1".into(),
    }]);
    registry.record_command_outcome(id, RemoteRuntimeCommandOutcome::Unknown, None);
    let after = vec![
        RemoteTmuxTopologyEntry {
            window_id: "@1".into(),
            pane_id: "%1".into(),
        },
        RemoteTmuxTopologyEntry {
            window_id: "@42".into(),
            pane_id: "%34".into(),
        },
    ];
    assert_eq!(
        remote_runtime_compensation_token(&registry.leases[&id], &after),
        None,
        "a wait failure cannot prove whether a concurrent external runtime owns the topology diff"
    );
}

#[test]
fn publishing_claim_is_not_interruptible_by_watchdog_compensation() {
    let mut registry = RemoteRuntimeLeaseRegistry::default();
    let id = registry.reserve(
        RemoteRuntimeLeaseScope {
            endpoint: "ssh://host-a".into(),
            session: "tmux-a".into(),
        },
        RemoteRuntimeSourceWitness {
            window_id: WINDOW.into(),
            workspace_id: WORKSPACE.into(),
            pane_id: PANE.into(),
            surface_id: SOURCE.into(),
            surface_generation: 1,
            remote_token: "%source".into(),
            move_generation: 1,
            restore_epoch: 0,
        },
        RESERVED.into(),
        RESERVED_PANE.into(),
        RemoteTmuxTarget::Window,
    );
    registry.record_command_outcome(
        id,
        RemoteRuntimeCommandOutcome::Succeeded,
        Some("@42".into()),
    );
    let lease = registry.leases[&id].clone();
    assert_eq!(
        registry.claim_callback(
            id,
            &lease.scope,
            &lease.source,
            &lease.reserved_surface_id,
            &lease.reserved_pane_id,
        ),
        RemoteRuntimeCallbackClaim::Publish
    );
    assert!(registry
        .expire(id, Instant::now() + REMOTE_RUNTIME_LEASE_TTL)
        .is_none());
    assert_eq!(
        registry.leases[&id].disposition,
        RemoteRuntimeLeaseDisposition::Publishing,
        "watchdog compensation cannot race an in-flight local snapshot commit"
    );
}

#[test]
fn stale_callbacks_are_fenced_by_source_kind_token_move_and_restore_epoch() {
    let mut coordinator = LeaseCoordinator::default();
    let id = coordinator.reserve(
        scope("ssh://host-a", "tmux-a"),
        witness(),
        RESERVED,
        RESERVED_PANE,
        30,
    );
    coordinator.command_outcome(id, CommandOutcome::Succeeded);
    for mutate in [
        |source: &mut SourceWitness| source.generation += 1,
        |source: &mut SourceWitness| source.kind = LeaseSurfaceKind::LocalTerminal,
        |source: &mut SourceWitness| source.remote_token = "%replacement".into(),
        |source: &mut SourceWitness| source.move_generation += 1,
        |source: &mut SourceWitness| source.restore_epoch += 1,
    ] {
        let mut stale = callback(id);
        mutate(&mut stale.source);
        assert_eq!(coordinator.callback(&stale), CallbackDecision::Stale);
    }

    let mut current = remote_snapshot();
    current.windows[0].tab_manager.workspaces[0]
        .surfaces
        .as_mut()
        .unwrap()[0]
        .generation = 2;
    let arrival = RuntimeArrival::remote_tab(
        WINDOW,
        WORKSPACE,
        PANE,
        RESERVED,
        "%replacement",
        1,
        SOURCE,
        false,
    );
    assert_eq!(
        reconcile_runtime_arrival(&current, arrival).snapshot,
        current,
        "production callback path must carry and validate the complete source witness"
    );
}

#[test]
fn no_callback_expiry_is_bounded_and_failed_cleanup_remains_retryable() {
    let mut coordinator = LeaseCoordinator::default();
    let id = coordinator.reserve(
        scope("ssh://host-a", "tmux-a"),
        witness(),
        RESERVED,
        RESERVED_PANE,
        10,
    );
    coordinator.now = 10;
    assert!(coordinator.expire(id));
    coordinator.cleanup_result(id, false);
    assert_eq!(
        coordinator.leases[&id].disposition,
        LeaseDisposition::CleanupRetry
    );
    assert_eq!(coordinator.leases[&id].cleanup_attempts, 1);

    assert_eq!(
        remote_observation_action(true, None, REMOTE_OBSERVATION_MAX_RETRIES + 1),
        RemoteObservationAction::CompensateKillWindow,
        "a command that never produces a callback must expire instead of reconciling forever"
    );
}

#[test]
fn source_close_wins_the_close_arrival_race_and_late_arrival_cannot_reanchor() {
    let mut coordinator = LeaseCoordinator::default();
    let id = coordinator.reserve(
        scope("ssh://host-a", "tmux-a"),
        witness(),
        RESERVED,
        RESERVED_PANE,
        30,
    );
    coordinator.source_disappeared(SOURCE);
    assert_eq!(coordinator.callback(&callback(id)), CallbackDecision::Stale);

    let mut closed = remote_snapshot();
    let workspace = &mut closed.windows[0].tab_manager.workspaces[0];
    workspace.surfaces.as_mut().unwrap().remove(0);
    let SessionWorkspaceLayoutSnapshot::Pane(pane) = workspace.layout.as_mut().unwrap() else {
        panic!("single-pane fixture")
    };
    pane.panel_ids.remove(0);
    pane.selected_panel_id = Some(SIBLING.into());
    let arrival =
        RuntimeArrival::remote_tab(WINDOW, WORKSPACE, PANE, RESERVED, "%new", 1, SOURCE, false);
    assert_eq!(
        reconcile_runtime_arrival(&closed, arrival).snapshot,
        closed,
        "late arrival must not fall back from its vanished source to a sibling tab"
    );
}

#[test]
fn duplicate_arrival_consumes_only_the_exact_lease_once() {
    let mut coordinator = LeaseCoordinator::default();
    let id = coordinator.reserve(
        scope("ssh://host-a", "tmux-a"),
        witness(),
        RESERVED,
        RESERVED_PANE,
        30,
    );
    assert_eq!(
        coordinator.callback(&callback(id)),
        CallbackDecision::Commit
    );
    assert_eq!(
        coordinator.callback(&callback(id)),
        CallbackDecision::Duplicate
    );

    let snapshot = remote_snapshot();
    let first = reconcile_runtime_arrival(
        &snapshot,
        RuntimeArrival::remote_tab(
            WINDOW,
            WORKSPACE,
            PANE,
            RESERVED,
            "%same-runtime",
            1,
            SOURCE,
            false,
        ),
    )
    .snapshot;
    let duplicate = reconcile_runtime_arrival(
        &first,
        RuntimeArrival::remote_tab(
            WINDOW,
            WORKSPACE,
            PANE,
            "40000000-0000-0000-0000-000000000004",
            "%same-runtime",
            1,
            SOURCE,
            false,
        ),
    )
    .snapshot;
    assert_eq!(
        duplicate, first,
        "a repeated runtime identity cannot consume a second fabricated result identity"
    );
}

#[test]
fn runtime_identity_collision_on_another_remote_owner_does_not_consume_this_arrival() {
    const FOREIGN_WINDOW: &str = "10000000-0000-0000-0000-000000000002";
    const FOREIGN_WORKSPACE: &str = "20000000-0000-0000-0000-000000000002";
    const FOREIGN_PANE: &str = "30000000-0000-0000-0000-000000000002";
    const FOREIGN_SURFACE: &str = "40000000-0000-0000-0000-000000000005";

    let mut snapshot = remote_snapshot();
    let mut foreign = snapshot.windows[0].clone();
    foreign.window_id = Some(FOREIGN_WINDOW.into());
    foreign.selected_workspace_id = Some(FOREIGN_WORKSPACE.into());
    let workspace = &mut foreign.tab_manager.workspaces[0];
    workspace.workspace_id = Some(FOREIGN_WORKSPACE.into());
    workspace.focused_panel_id = Some(FOREIGN_SURFACE.into());
    let SessionWorkspaceLayoutSnapshot::Pane(pane) = workspace.layout.as_mut().unwrap() else {
        panic!("single-pane fixture")
    };
    pane.pane_id = Some(FOREIGN_PANE.into());
    pane.panel_ids = vec![FOREIGN_SURFACE.into()];
    pane.selected_panel_id = Some(FOREIGN_SURFACE.into());
    workspace.surfaces = Some(vec![SessionSurfaceSnapshot {
        surface_id: FOREIGN_SURFACE.into(),
        pane_id: FOREIGN_PANE.into(),
        generation: 1,
        kind: SessionSurfaceKindSnapshot::RemoteTerminal {
            remote_session_id: Some("%same-runtime".into()),
            remote_context: None,
            arrival_generation: Some(1),
        },
        metadata: SessionSurfaceMetadataSnapshot::default(),
        terminal_startup: None,
        scrollback: None,
    }]);
    snapshot.windows.push(foreign);
    let mut encoded = serde_json::to_value(snapshot).unwrap();
    encoded["windows"][1]["tab_manager"]["workspaces"][0]["remote"]["destination"] =
        json!("host-b");
    let snapshot: AppSessionSnapshot = serde_json::from_value(encoded).unwrap();

    let reconciled = reconcile_runtime_arrival(
        &snapshot,
        RuntimeArrival::remote_tab(
            WINDOW,
            WORKSPACE,
            PANE,
            RESERVED,
            "%same-runtime",
            1,
            SOURCE,
            false,
        ),
    )
    .snapshot;
    assert!(
        reconciled.windows[0].tab_manager.workspaces[0]
            .surfaces
            .as_deref()
            .unwrap()
            .iter()
            .any(|surface| surface.surface_id == RESERVED),
        "an identical tmux token/generation owned by another remote host cannot consume this lease"
    );
}

#[test]
fn departure_registration_deduplicates_the_exact_generation() {
    let key = DepartureKey {
        endpoint: "ssh://host-a".into(),
        session: "tmux-a".into(),
        surface_id: RESERVED.into(),
        generation: 3,
    };
    let mut coordinator = DepartureCoordinator::default();
    coordinator.register(key.clone());
    coordinator.register(key);
    assert_eq!(coordinator.pending.len(), 1);

    let departure = RuntimeDeparture {
        window_id: WINDOW.into(),
        workspace_id: WORKSPACE.into(),
        pane_id: PANE.into(),
        surface_id: SOURCE.into(),
        generation: 1,
    };
    let mut registry = RemoteWindowDepartureRegistry::default();
    registry.register(PendingRemoteWindowDeparture {
        destination: "host-a".into(),
        remote_window_id: "@42".into(),
        departure: departure.clone(),
    });
    registry.register(PendingRemoteWindowDeparture {
        destination: "host-a".into(),
        remote_window_id: "@42".into(),
        departure,
    });
    assert_eq!(
        registry.len(),
        1,
        "production departure ownership must be keyed by endpoint/session/surface/generation"
    );
}

#[test]
fn departure_commit_failure_rolls_back_to_retryable_exact_ownership() {
    let key = DepartureKey {
        endpoint: "ssh://host-a".into(),
        session: "tmux-a".into(),
        surface_id: RESERVED.into(),
        generation: 3,
    };
    let mut coordinator = DepartureCoordinator::default();
    coordinator.register(key.clone());
    assert!(coordinator.observe(&key, DepartureObservation::Absent));
    coordinator.commit_result(&key, false);
    assert_eq!(coordinator.pending[&key], 1);

    let mut registry = RemoteWindowDepartureRegistry::default();
    let production_key = registry.register(PendingRemoteWindowDeparture {
        destination: "host-a".into(),
        remote_window_id: "@42".into(),
        departure: RuntimeDeparture {
            window_id: WINDOW.into(),
            workspace_id: WORKSPACE.into(),
            pane_id: PANE.into(),
            surface_id: SOURCE.into(),
            generation: 1,
        },
    });
    assert!(
        !registry.record_commit_result(&production_key, Err("injected publication failure".into()))
    );
    assert!(registry.contains(&production_key));
    assert_eq!(registry.retry_attempts(&production_key), Some(1));
}

#[test]
fn external_disappearance_is_distinct_from_reconnect_and_genuine_end() {
    let key = DepartureKey {
        endpoint: "ssh://host-a".into(),
        session: "tmux-a".into(),
        surface_id: RESERVED.into(),
        generation: 3,
    };
    let mut coordinator = DepartureCoordinator::default();
    coordinator.register(key.clone());
    assert!(!coordinator.observe(&key, DepartureObservation::Present));
    assert!(!coordinator.observe(&key, DepartureObservation::TransientDisconnect));
    assert!(coordinator.observe(&key, DepartureObservation::Absent));

    assert_eq!(
        classify_remote_tmux_window_presence(
            "@42",
            Ok(RemoteTmuxCommandOutput {
                exit_code: 0,
                stdout: "@42\n".into(),
                stderr: String::new(),
            })
        ),
        RemoteWindowPresenceObservation::Present
    );
    assert_eq!(
        classify_remote_tmux_window_presence("@42", Err("transient ssh loss".into())),
        RemoteWindowPresenceObservation::QueryFailed
    );
    assert_eq!(
        classify_remote_tmux_window_presence(
            "@42",
            Ok(RemoteTmuxCommandOutput {
                exit_code: 0,
                stdout: "@7\n".into(),
                stderr: String::new(),
            })
        ),
        RemoteWindowPresenceObservation::Absent
    );
}

#[test]
fn endpoint_session_scope_restore_and_unsupported_actions_are_closed_boundaries() {
    let mut coordinator = LeaseCoordinator::default();
    let id = coordinator.reserve(
        scope("ssh://host-a", "tmux-a"),
        witness(),
        RESERVED,
        RESERVED_PANE,
        30,
    );
    let mut wrong_endpoint = callback(id);
    wrong_endpoint.scope = scope("ssh://host-b", "tmux-a");
    assert_eq!(
        coordinator.callback(&wrong_endpoint),
        CallbackDecision::Stale
    );
    coordinator.restore();
    assert_eq!(
        coordinator.leases[&id].disposition,
        LeaseDisposition::Cancelled
    );

    let snapshot = remote_snapshot();
    for transition in [
        dispatch(
            &snapshot,
            "surface.action",
            json!({"surface_id":SOURCE,"action":"move_to_new_workspace"}),
        ),
        dispatch(
            &snapshot,
            "surface.respawn",
            json!({"surface_id":SOURCE,"command":"cmd.exe"}),
        ),
    ] {
        assert!(
            matches!(transition.result, ControlCallResult::Err { .. }),
            "remote mirrors cannot be moved or respawned as locally owned runtimes"
        );
        assert_eq!(transition.snapshot, snapshot);
    }

    let (dock_snapshot, dock_pane_id) = remote_snapshot_with_dock();
    let move_into_dock = dispatch(
        &dock_snapshot,
        "surface.move",
        json!({"surface_id":SOURCE,"pane_id":dock_pane_id,"focus":false}),
    );
    assert!(
        matches!(move_into_dock.result, ControlCallResult::Err { .. }),
        "remote mirrors cannot move into locally owned Dock topology"
    );
    assert_eq!(move_into_dock.snapshot, dock_snapshot);
}

#[test]
fn production_lease_seams_are_stateful_and_separate_from_remote_proxy_transport() {
    let source = include_str!("../../control_socket.rs");
    for required in [
        "RemoteRuntimeLeaseRegistryState",
        "reserve_remote_runtime_lease",
        "expire_remote_runtime_lease",
    ] {
        assert!(
            source.contains(required),
            "missing production lease seam: {required}"
        );
    }
    assert!(
        !source.contains("remote_proxy::"),
        "remote runtime lease ownership must remain separate from remote_proxy transport"
    );
}
