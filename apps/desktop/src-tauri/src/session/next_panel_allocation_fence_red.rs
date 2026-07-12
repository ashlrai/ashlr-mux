//! Every panel-id reservation must share restore's snapshot/control gate.

use super::*;

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

fn compact(source: &str) -> String {
    source
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect()
}

fn reservation_is_fenced(body: &str) -> bool {
    let body = compact(body);
    let Some(reserve) = body.find("state.next_panel.fetch_add(1,Ordering::Relaxed)") else {
        return false;
    };
    let prefix = &body[..reserve];
    let outer_gate = prefix
        .rfind("state.lock_control_mutation()")
        .or_else(|| prefix.rfind("state.snapshot.lock_gate()"));
    if outer_gate.is_some() {
        return !body[reserve..].contains("drop(_control_guard)")
            && !body[reserve..].contains("drop(_snapshot_guard)")
            && !body[reserve..].contains("drop(_allocation_guard)");
    }

    // A snapshot guard is also sufficient when it is acquired before the
    // reservation and remains live while the reserved id mutates authority.
    prefix.contains("state.snapshot.lock()")
        && body[reserve..].contains("guard.clone()")
        && !body[reserve..].contains("drop(guard)")
}

#[test]
fn every_production_panel_reservation_is_fenced_before_fetch_add() {
    let session = include_str!("../session.rs");
    let routes = [
        "pub(crate) fn register_window_for_control(",
        "pub(crate) fn move_workspace_to_window_for_control(",
        "pub(crate) fn new_workspace_for_control(",
        "pub(crate) fn new_workspace_in_window_for_control(",
        "pub(crate) fn new_browser_workspace_for_control(",
        "pub(crate) fn reopen_closed_browser_tab_for_control(",
        "pub(crate) fn split_panel_for_control(",
        "pub(crate) fn new_terminal_tab_for_control(",
        "pub(crate) fn split_browser_for_control(",
        "fn open_ssh_url_request(",
        "pub fn session_split_browser(",
        "pub fn session_new_workspace(",
        "pub fn session_new_browser_workspace(",
        "pub fn session_reopen_closed_browser_tab(",
    ];
    assert_eq!(
        session.matches("state.next_panel.fetch_add").count(),
        routes.len(),
        "the production reservation inventory changed; classify every new owner"
    );
    let mut unfenced = Vec::new();
    for route in routes {
        let body = function_source(session, route);
        if !reservation_is_fenced(body) {
            unfenced.push(route.trim_end_matches('('));
        }
    }
    assert!(
        unfenced.is_empty(),
        "next_panel reservation occurs before the shared snapshot/control gate: {}",
        unfenced.join(", ")
    );
}

#[test]
fn canonical_layout_allocations_are_inside_the_callers_outer_gate() {
    let session = include_str!("../session.rs");
    let helper = function_source(session, "fn session_layout_from_cmux(");
    assert!(helper.contains("next_panel.fetch_add(1, Ordering::Relaxed)"));

    let caller = compact(function_source(
        session,
        "pub(crate) fn new_workspace_in_window_for_control(",
    ));
    let gate = caller
        .find("state.lock_control_mutation()")
        .or_else(|| caller.find("state.snapshot.lock_gate()"))
        .or_else(|| caller.find("state.snapshot.lock()"))
        .expect("layout caller must acquire the shared gate");
    let reserve = caller.find("state.next_panel.fetch_add").unwrap();
    let build = caller.find("session_layout_from_cmux(").unwrap();
    assert!(gate < reserve && reserve < build);
    assert!(!caller[gate..build].contains("drop("));
}

struct BlockingEmitPublication {
    emitted: std::sync::mpsc::Sender<()>,
    resume: std::sync::mpsc::Receiver<()>,
}

impl SnapshotPublicationOperations for BlockingEmitPublication {
    fn persist(&mut self, _candidate: &AppSessionSnapshot) -> Result<(), String> {
        Ok(())
    }

    fn update_event_baseline(&mut self, _candidate: &AppSessionSnapshot) {}

    fn emit(&mut self, _candidate: &AppSessionSnapshot) -> Result<(), String> {
        self.emitted.send(()).unwrap();
        self.resume.recv().unwrap();
        Ok(())
    }
}

#[test]
fn real_style_fenced_reservation_cannot_race_restore_reseed() {
    let current = initial_snapshot("surface-1");
    let restored = initial_snapshot("surface-2");
    let authority = GatedSnapshot::new(current);
    let next_panel = AtomicU64::new(2);
    let (emitted_tx, emitted_rx) = std::sync::mpsc::channel();
    let (resume_tx, resume_rx) = std::sync::mpsc::channel();
    let (reserved_tx, reserved_rx) = std::sync::mpsc::channel();

    let reserved_during_emit = std::thread::scope(|scope| {
        let restore = scope.spawn(|| {
            let mut publication = BlockingEmitPublication {
                emitted: emitted_tx,
                resume: resume_rx,
            };
            restore_previous_launch_transaction(&authority, &next_panel, &mut publication, || {
                Some(restored)
            })
            .unwrap();
        });
        emitted_rx.recv().unwrap();

        // Production allocators retain the shared gate from reservation through
        // the authority mutation that consumes the reserved identity.
        let allocator = scope.spawn(|| {
            let _allocation_guard = authority.lock_gate();
            let reserved = next_panel.fetch_add(1, Ordering::Relaxed);
            reserved_tx.send(reserved).unwrap();
            let mut snapshot = authority.lock().unwrap();
            apply_new_workspace(
                &mut snapshot,
                &format!("surface-{reserved}"),
                None,
                None,
                None,
                None,
            );
        });

        let observed = reserved_rx.recv_timeout(std::time::Duration::from_millis(50));
        resume_tx.send(()).unwrap();
        restore.join().unwrap();
        allocator.join().unwrap();
        observed
    });

    assert_eq!(
        reserved_during_emit,
        Err(std::sync::mpsc::RecvTimeoutError::Timeout),
        "a production-style allocator reserved an id before restore completed reseeding"
    );
}
