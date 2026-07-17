//! Every panel-id reservation must share restore's snapshot/control gate.

use super::*;

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
