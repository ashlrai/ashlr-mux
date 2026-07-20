    use std::collections::BTreeMap;
    use std::io::{self, Read, Write};
    use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};
    use std::sync::{mpsc, Arc, Barrier, Mutex, Weak};
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    use super::{
        base64_encode, default_shell_command, descendant_pid_set, ports_for_pid_set,
        reusable_panel_session_id, send_terminal_input, tcp_port_from_owner_pid_row,
        terminal_runtime_snapshot_from_processes, terminal_text, ProcessSnapshotEntry,
        TerminalInputOutcome, TerminalInputTransport, TerminalProcess, TerminalSession,
        TerminalState, TerminalTitleParser, TERMINAL_PENDING_INPUT_LIMIT,
    };
    use cmux_terminal::conpty::ConPtySize;
    use cmux_terminal::engine::{GridSize, TerminalGrid};

    struct TestProcess {
        wait: Result<Option<u32>, String>,
    }

    struct BlockingResizeProcess {
        entered: mpsc::Sender<()>,
        release: mpsc::Receiver<()>,
    }

    impl TerminalProcess for BlockingResizeProcess {
        fn kill(&mut self) -> Result<(), String> {
            Ok(())
        }

        fn resize(&mut self, _size: ConPtySize) -> Result<(), String> {
            self.entered.send(()).unwrap();
            self.release.recv().unwrap();
            Ok(())
        }

        fn try_wait(&mut self) -> Result<Option<u32>, String> {
            Ok(None)
        }
    }

    struct BlockingKillProcess {
        entered: mpsc::Sender<()>,
        release: mpsc::Receiver<()>,
    }

    struct LockCheckingProcess {
        state: Weak<TerminalState>,
        kills: Arc<AtomicUsize>,
    }

    impl TerminalProcess for LockCheckingProcess {
        fn kill(&mut self) -> Result<(), String> {
            let state = self.state.upgrade().expect("test terminal state");
            assert!(
                state.registry.try_lock().is_ok(),
                "kill ran under registry lock"
            );
            self.kills.fetch_add(1, AtomicOrdering::SeqCst);
            Ok(())
        }

        fn resize(&mut self, _size: ConPtySize) -> Result<(), String> {
            Ok(())
        }

        fn try_wait(&mut self) -> Result<Option<u32>, String> {
            let state = self.state.upgrade().expect("test terminal state");
            assert!(
                state.registry.try_lock().is_ok(),
                "process check ran under registry lock"
            );
            Ok(None)
        }
    }

    struct LockCheckingWriter {
        state: Weak<TerminalState>,
        captured: Arc<Mutex<Vec<u8>>>,
    }

    impl Write for LockCheckingWriter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            let state = self.state.upgrade().expect("test terminal state");
            assert!(
                state.registry.try_lock().is_ok(),
                "PTY write ran under registry lock"
            );
            self.captured.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl TerminalProcess for BlockingKillProcess {
        fn kill(&mut self) -> Result<(), String> {
            self.entered.send(()).unwrap();
            self.release.recv().unwrap();
            Ok(())
        }

        fn resize(&mut self, _size: ConPtySize) -> Result<(), String> {
            Ok(())
        }

        fn try_wait(&mut self) -> Result<Option<u32>, String> {
            Ok(None)
        }
    }

    struct FailingKillProcess;

    impl TerminalProcess for FailingKillProcess {
        fn kill(&mut self) -> Result<(), String> {
            Err("injected kill failure".to_string())
        }

        fn resize(&mut self, _size: ConPtySize) -> Result<(), String> {
            Ok(())
        }

        fn try_wait(&mut self) -> Result<Option<u32>, String> {
            Ok(None)
        }
    }

    struct PoisonRegistryDuringKillProcess {
        state: Weak<TerminalState>,
    }

    impl TerminalProcess for PoisonRegistryDuringKillProcess {
        fn kill(&mut self) -> Result<(), String> {
            let state = self.state.upgrade().expect("terminal state remains live");
            let poisoned = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let _guard = state.registry.lock().unwrap();
                panic!("poison registry after successful kill");
            }));
            assert!(poisoned.is_err());
            Ok(())
        }

        fn resize(&mut self, _size: ConPtySize) -> Result<(), String> {
            Ok(())
        }

        fn try_wait(&mut self) -> Result<Option<u32>, String> {
            Ok(None)
        }
    }

    impl TerminalProcess for TestProcess {
        fn kill(&mut self) -> Result<(), String> {
            Ok(())
        }

        fn resize(&mut self, _size: ConPtySize) -> Result<(), String> {
            Ok(())
        }

        fn try_wait(&mut self) -> Result<Option<u32>, String> {
            self.wait.clone()
        }
    }

    struct CapturingWriter(Arc<Mutex<Vec<u8>>>);

    impl Write for CapturingWriter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    struct ErrorWriter(io::ErrorKind);

    impl Write for ErrorWriter {
        fn write(&mut self, _bytes: &[u8]) -> io::Result<usize> {
            Err(io::Error::from(self.0))
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    struct BlockingWriter {
        entered: mpsc::Sender<()>,
        release: mpsc::Receiver<()>,
        captured: Arc<Mutex<Vec<u8>>>,
        block_once: bool,
    }

    impl Write for BlockingWriter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if self.block_once {
                self.block_once = false;
                self.entered.send(()).unwrap();
                self.release.recv().unwrap();
            }
            self.captured.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    struct FailSecondWriteOnce {
        entered: mpsc::Sender<()>,
        release: mpsc::Receiver<()>,
        captured: Arc<Mutex<Vec<u8>>>,
        writes: usize,
    }

    struct PartialQueuedFailureOnce {
        entered: mpsc::Sender<()>,
        release: mpsc::Receiver<()>,
        captured: Arc<Mutex<Vec<u8>>>,
        writes: usize,
    }

    struct PanicWriter {
        entered: mpsc::Sender<()>,
        release: mpsc::Receiver<()>,
    }

    struct BlockingNthWaitProcess {
        calls: usize,
        block_on: usize,
        entered: mpsc::Sender<()>,
        release: mpsc::Receiver<()>,
    }

    impl TerminalProcess for BlockingNthWaitProcess {
        fn kill(&mut self) -> Result<(), String> {
            Ok(())
        }

        fn resize(&mut self, _size: ConPtySize) -> Result<(), String> {
            Ok(())
        }

        fn try_wait(&mut self) -> Result<Option<u32>, String> {
            self.calls += 1;
            if self.calls == self.block_on {
                self.entered.send(()).unwrap();
                self.release.recv().unwrap();
            }
            Ok(None)
        }
    }

    impl Write for PanicWriter {
        fn write(&mut self, _bytes: &[u8]) -> io::Result<usize> {
            self.entered.send(()).unwrap();
            self.release.recv().unwrap();
            panic!("injected active-writer panic");
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl Write for PartialQueuedFailureOnce {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.writes += 1;
            if self.writes == 1 {
                self.entered.send(()).unwrap();
                self.release.recv().unwrap();
            } else if self.writes == 2 {
                let written = bytes.len().min(3);
                self.captured
                    .lock()
                    .unwrap()
                    .extend_from_slice(&bytes[..written]);
                return Ok(written);
            } else if self.writes == 3 {
                return Err(io::Error::other("injected failure after partial write"));
            }
            self.captured.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl Write for FailSecondWriteOnce {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.writes += 1;
            if self.writes == 1 {
                self.entered.send(()).unwrap();
                self.release.recv().unwrap();
            } else if self.writes == 2 {
                return Err(io::Error::other("injected queued-write failure"));
            }
            self.captured.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    fn test_process(exited: bool) -> Arc<Mutex<Box<dyn TerminalProcess>>> {
        Arc::new(Mutex::new(Box::new(TestProcess {
            wait: Ok(exited.then_some(0)),
        })))
    }

    fn test_transport(writer: impl Write + Send + 'static) -> Arc<TerminalInputTransport> {
        Arc::new(TerminalInputTransport::new(Box::new(writer)))
    }

    fn test_session(
        process: Arc<Mutex<Box<dyn TerminalProcess>>>,
        input: Arc<TerminalInputTransport>,
        panel_id: &str,
    ) -> TerminalSession {
        TerminalSession {
            pty: process,
            input,
            grid: Arc::new(Mutex::new(TerminalGrid::new(GridSize::new(80, 24)))),
            cell_dimensions: Arc::new(Mutex::new(None)),
            pane_grid_fields: Arc::new(Mutex::new(None)),
            title_parser: Arc::new(Mutex::new(TerminalTitleParser::default())),
            operations: Arc::new(super::TerminalOperationGate::default()),
            pump_activation: super::TerminalPumpActivation::active(),
            panel_id: Some(panel_id.to_string()),
            root_pid: None,
        }
    }

    fn pending_test_session(
        process: Arc<Mutex<Box<dyn TerminalProcess>>>,
        input: Arc<TerminalInputTransport>,
        panel_id: &str,
    ) -> (TerminalSession, Arc<super::TerminalPumpActivation>) {
        let mut session = test_session(process, input, panel_id);
        let (activation, _ready) = super::TerminalPumpActivation::pending();
        session.pump_activation = activation.clone();
        (session, activation)
    }

    fn pump_is_pending(activation: &super::TerminalPumpActivation) -> bool {
        activation.sender.lock().unwrap().is_some()
    }

    fn redesign_spec(cwd: &str) -> super::TerminalMaterializationSpec {
        super::TerminalMaterializationSpec::new(
            Some(cwd.to_string()),
            Some("echo ready".to_string()),
            b"initial".to_vec(),
            BTreeMap::from([("CMUX_TEST".to_string(), "1".to_string())]),
        )
    }

    fn redesign_start(
        state: &TerminalState,
        panel_id: &str,
        spec: &super::TerminalMaterializationSpec,
        event: super::TerminalMaterializationEvent,
    ) -> super::TerminalMaterializationLease {
        match super::request_terminal_materialization(state, panel_id, spec, vec![event]) {
            super::TerminalMaterializationDemand::Start(lease) => lease,
            _ => panic!("first cold demand must own the sole start lease"),
        }
    }

    #[test]
    fn cold_materialization_redesign_empty_and_budget_are_atomic() {
        let state = TerminalState::default();
        let spec = redesign_spec("C:/repo");
        assert_eq!(
            super::request_terminal_materialization(&state, "   ", &spec, Vec::new()),
            super::TerminalMaterializationDemand::Noop
        );
        assert_eq!(
            super::request_terminal_materialization(
                &state,
                "panel-empty",
                &spec,
                vec![
                    super::TerminalMaterializationEvent::Input(Vec::new()),
                    super::TerminalMaterializationEvent::ProcessOutput(Vec::new()),
                ],
            ),
            super::TerminalMaterializationDemand::Noop
        );
        assert_eq!(
            state
                .registry
                .lock()
                .unwrap()
                .next_materialization_generation,
            0
        );

        let exact = vec![
            super::TerminalMaterializationEvent::Input(vec![
                b'i';
                TERMINAL_PENDING_INPUT_LIMIT - 4
            ]),
            super::TerminalMaterializationEvent::ProcessOutput(b"out!".to_vec()),
        ];
        let lease = match super::request_terminal_materialization(
            &state,
            "panel-budget",
            &spec,
            exact.clone(),
        ) {
            super::TerminalMaterializationDemand::Start(lease) => lease,
            _ => panic!("an exact-budget batch must start"),
        };
        assert_eq!(
            super::request_terminal_materialization(
                &state,
                "panel-budget",
                &spec,
                vec![super::TerminalMaterializationEvent::Input(vec![b'x'])],
            ),
            super::TerminalMaterializationDemand::InputQueueFull
        );
        assert_eq!(
            super::cancel_terminal_materialization(&state, &lease).unwrap(),
            Some(exact)
        );
        assert_eq!(
            super::request_terminal_materialization(
                &state,
                "panel-oversized",
                &spec,
                vec![super::TerminalMaterializationEvent::ProcessOutput(vec![
                    b'x';
                    TERMINAL_PENDING_INPUT_LIMIT + 1
                ])],
            ),
            super::TerminalMaterializationDemand::InputQueueFull
        );
    }

    #[test]
    fn cold_materialization_redesign_fifo_publication_and_live_handoff() {
        let state = TerminalState::default();
        let spec = redesign_spec("C:/repo");
        let first = super::TerminalMaterializationEvent::Input(b"first".to_vec());
        let second = super::TerminalMaterializationEvent::ProcessOutput(b"second".to_vec());
        let lease = redesign_start(&state, " panel-a ", &spec, first.clone());
        assert_eq!(
            super::request_terminal_materialization(&state, "panel-a", &spec, vec![second.clone()],),
            super::TerminalMaterializationDemand::Queued
        );
        assert_eq!(
            super::request_terminal_materialization(
                &state,
                "panel-a",
                &redesign_spec("C:/other"),
                vec![super::TerminalMaterializationEvent::Input(
                    b"wrong".to_vec()
                )],
            ),
            super::TerminalMaterializationDemand::SurfaceUnavailable
        );
        super::publish_terminal_materialization_runtime(
            &state,
            &lease,
            7,
            test_session(
                test_process(false),
                test_transport(CapturingWriter(Arc::new(Mutex::new(Vec::new())))),
                "panel-a",
            ),
        )
        .unwrap();
        for expected in [first, second] {
            assert_eq!(
                super::drain_terminal_materialization_event(&state, &lease).unwrap(),
                Some(expected)
            );
        }
        assert_eq!(
            super::drain_terminal_materialization_event(&state, &lease).unwrap(),
            None
        );
        let live = super::TerminalMaterializationEvent::Input(b"live".to_vec());
        assert_eq!(
            super::request_terminal_materialization(
                &state,
                "panel-a",
                &redesign_spec("C:/live"),
                vec![live.clone()],
            ),
            super::TerminalMaterializationDemand::Live(vec![live])
        );
    }

    #[test]
    fn cold_materialization_redesign_concurrency_and_poison_are_classified() {
        let state = Arc::new(TerminalState::default());
        let barrier = Arc::new(Barrier::new(3));
        let mut workers = Vec::new();
        for byte in *b"ab" {
            let state = state.clone();
            let barrier = barrier.clone();
            workers.push(std::thread::spawn(move || {
                barrier.wait();
                super::request_terminal_materialization(
                    &state,
                    "panel-race",
                    &redesign_spec("C:/repo"),
                    vec![super::TerminalMaterializationEvent::Input(vec![byte])],
                )
            }));
        }
        barrier.wait();
        let outcomes = workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            outcomes
                .iter()
                .filter(|outcome| matches!(outcome, super::TerminalMaterializationDemand::Start(_)))
                .count(),
            1
        );
        assert_eq!(
            outcomes
                .iter()
                .filter(|outcome| matches!(outcome, super::TerminalMaterializationDemand::Queued))
                .count(),
            1
        );

        let poisoned = Arc::new(TerminalState::default());
        let target = poisoned.clone();
        assert!(std::thread::spawn(move || {
            let _guard = target.registry.lock().unwrap();
            panic!("poison redesign registry");
        })
        .join()
        .is_err());
        assert_eq!(
            super::request_terminal_materialization(
                &poisoned,
                "panel-poisoned",
                &redesign_spec("C:/repo"),
                vec![super::TerminalMaterializationEvent::Input(
                    b"input".to_vec()
                )],
            ),
            super::TerminalMaterializationDemand::SurfaceUnavailable
        );
    }

    #[test]
    fn cold_materialization_redesign_full_failure_retries_without_append() {
        let state = TerminalState::default();
        let spec = redesign_spec("C:/repo");
        let retained =
            super::TerminalMaterializationEvent::Input(vec![b'x'; TERMINAL_PENDING_INPUT_LIMIT]);
        let first = redesign_start(&state, "panel-full", &spec, retained.clone());
        assert!(super::fail_terminal_materialization_start(&state, &first).unwrap());
        assert!(!super::owns_terminal_materialization_start(&state, &first).unwrap());
        let retry = super::retry_terminal_materialization_start(&state, "panel-full", &spec)
            .unwrap()
            .expect("full retained FIFO must retry without another byte");
        assert_ne!(first.generation(), retry.generation());
        assert_eq!(
            super::cancel_terminal_materialization(&state, &retry).unwrap(),
            Some(vec![retained])
        );
    }

    #[test]
    fn cold_materialization_redesign_starting_fence_invalidates_spawn() {
        let state = TerminalState::default();
        let spec = redesign_spec("C:/repo");
        let first = super::TerminalMaterializationEvent::Input(b"first".to_vec());
        let second = super::TerminalMaterializationEvent::ProcessOutput(b"second".to_vec());
        let stale = redesign_start(&state, "panel-starting-fence", &spec, first.clone());
        let panels = ["panel-starting-fence".to_string()].into_iter().collect();
        let lifecycle = super::detach_terminal_panels_for_control(&state, &panels).unwrap();
        assert!(!super::owns_terminal_materialization_start(&state, &stale).unwrap());
        assert_eq!(
            super::request_terminal_materialization(
                &state,
                "panel-starting-fence",
                &spec,
                vec![second.clone()],
            ),
            super::TerminalMaterializationDemand::Queued
        );
        assert!(
            super::retry_terminal_materialization_start(&state, "panel-starting-fence", &spec,)
                .unwrap()
                .is_none()
        );
        super::rollback_terminal_panels_for_control(&state, lifecycle)
            .map_err(|error| error.message)
            .unwrap();
        assert!(!super::owns_terminal_materialization_start(&state, &stale).unwrap());
        let retry =
            super::retry_terminal_materialization_start(&state, "panel-starting-fence", &spec)
                .unwrap()
                .expect("rollback restores dormant retry authority");
        assert_ne!(stale.generation(), retry.generation());
        assert_eq!(
            super::cancel_terminal_materialization(&state, &retry).unwrap(),
            Some(vec![first, second])
        );

        let final_state = TerminalState::default();
        let final_lease = redesign_start(
            &final_state,
            "panel-starting-finalize",
            &spec,
            super::TerminalMaterializationEvent::Input(b"discard".to_vec()),
        );
        let panels = ["panel-starting-finalize".to_string()]
            .into_iter()
            .collect();
        let lifecycle = super::detach_terminal_panels_for_control(&final_state, &panels).unwrap();
        assert!(super::finalize_terminal_panels_for_control(&final_state, lifecycle).is_ok());
        assert!(
            super::cancel_terminal_materialization(&final_state, &final_lease)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn cold_materialization_redesign_cancel_rejects_fenced_starting() {
        let state = TerminalState::default();
        let spec = redesign_spec("C:/repo");
        let first = super::TerminalMaterializationEvent::Input(b"first".to_vec());
        let stale = redesign_start(&state, "panel-cancel-fenced-start", &spec, first.clone());
        let panels = ["panel-cancel-fenced-start".to_string()]
            .into_iter()
            .collect();
        let lifecycle = super::detach_terminal_panels_for_control(&state, &panels).unwrap();

        assert!(super::cancel_terminal_materialization(&state, &stale)
            .unwrap()
            .is_none());
        super::rollback_terminal_panels_for_control(&state, lifecycle)
            .map_err(|error| error.message)
            .unwrap();
        let retry =
            super::retry_terminal_materialization_start(&state, "panel-cancel-fenced-start", &spec)
                .unwrap()
                .expect("rollback must retain the FIFO under fresh start authority");
        assert_eq!(
            super::cancel_terminal_materialization(&state, &retry).unwrap(),
            Some(vec![first])
        );
    }

    #[test]
    fn cold_materialization_redesign_cancel_rejects_published() {
        let state = TerminalState::default();
        let spec = redesign_spec("C:/repo");
        let first = super::TerminalMaterializationEvent::Input(b"first".to_vec());
        let lease = redesign_start(&state, "panel-cancel-published", &spec, first.clone());
        super::publish_terminal_materialization_runtime(
            &state,
            &lease,
            81,
            test_session(
                test_process(false),
                test_transport(CapturingWriter(Arc::new(Mutex::new(Vec::new())))),
                "panel-cancel-published",
            ),
        )
        .unwrap();

        assert!(super::cancel_terminal_materialization(&state, &lease)
            .unwrap()
            .is_none());
        assert_eq!(
            super::drain_terminal_materialization_event(&state, &lease).unwrap(),
            Some(first)
        );
    }

    #[test]
    fn cold_materialization_redesign_cancel_rejects_fenced_published() {
        let state = TerminalState::default();
        let spec = redesign_spec("C:/repo");
        let first = super::TerminalMaterializationEvent::Input(b"first".to_vec());
        let lease = redesign_start(
            &state,
            "panel-cancel-fenced-published",
            &spec,
            first.clone(),
        );
        super::publish_terminal_materialization_runtime(
            &state,
            &lease,
            82,
            test_session(
                test_process(false),
                test_transport(CapturingWriter(Arc::new(Mutex::new(Vec::new())))),
                "panel-cancel-fenced-published",
            ),
        )
        .unwrap();
        let transfer = super::begin_terminal_session_transfer(&state, 82)
            .unwrap()
            .unwrap();

        assert!(super::cancel_terminal_materialization(&state, &lease)
            .unwrap()
            .is_none());
        super::release_terminal_session_transfer(&state, transfer, false).unwrap();
        assert_eq!(
            super::drain_terminal_materialization_event(&state, &lease).unwrap(),
            Some(first)
        );
    }

    #[test]
    fn cold_materialization_redesign_detached_published_fence_is_append_only() {
        let state = TerminalState::default();
        let spec = redesign_spec("C:/repo");
        let first = super::TerminalMaterializationEvent::Input(b"first".to_vec());
        let second = super::TerminalMaterializationEvent::ProcessOutput(b"second".to_vec());
        let lease = redesign_start(&state, "panel-detached", &spec, first.clone());
        super::publish_terminal_materialization_runtime(
            &state,
            &lease,
            8,
            test_session(
                test_process(false),
                test_transport(CapturingWriter(Arc::new(Mutex::new(Vec::new())))),
                "panel-detached",
            ),
        )
        .unwrap();
        let panels = ["panel-detached".to_string()].into_iter().collect();
        let lifecycle = super::detach_terminal_panels_for_control(&state, &panels).unwrap();
        assert_eq!(
            super::request_terminal_materialization(
                &state,
                "panel-detached",
                &spec,
                vec![second.clone()],
            ),
            super::TerminalMaterializationDemand::Queued
        );
        assert!(super::drain_terminal_materialization_event(&state, &lease).is_err());
        super::rollback_terminal_panels_for_control(&state, lifecycle)
            .map_err(|error| error.message)
            .unwrap();
        for expected in [first, second] {
            assert_eq!(
                super::drain_terminal_materialization_event(&state, &lease).unwrap(),
                Some(expected)
            );
        }
    }

    #[test]
    fn cold_materialization_redesign_direct_transfer_fences_drain() {
        let state = TerminalState::default();
        let spec = redesign_spec("C:/repo");
        let first = super::TerminalMaterializationEvent::Input(b"first".to_vec());
        let second = super::TerminalMaterializationEvent::ProcessOutput(b"second".to_vec());
        let lease = redesign_start(&state, "panel-direct", &spec, first.clone());
        super::publish_terminal_materialization_runtime(
            &state,
            &lease,
            9,
            test_session(
                test_process(false),
                test_transport(CapturingWriter(Arc::new(Mutex::new(Vec::new())))),
                "panel-direct",
            ),
        )
        .unwrap();
        let transfer = super::begin_terminal_session_transfer(&state, 9)
            .unwrap()
            .unwrap();
        assert_eq!(
            super::request_terminal_materialization(
                &state,
                "panel-direct",
                &spec,
                vec![second.clone()],
            ),
            super::TerminalMaterializationDemand::Queued
        );
        assert!(super::drain_terminal_materialization_event(&state, &lease).is_err());
        super::release_terminal_session_transfer(&state, transfer, false).unwrap();
        for expected in [first, second] {
            assert_eq!(
                super::drain_terminal_materialization_event(&state, &lease).unwrap(),
                Some(expected)
            );
        }

        let remove_state = TerminalState::default();
        let remove_lease = redesign_start(
            &remove_state,
            "panel-direct-remove",
            &spec,
            super::TerminalMaterializationEvent::Input(b"discard".to_vec()),
        );
        super::publish_terminal_materialization_runtime(
            &remove_state,
            &remove_lease,
            10,
            test_session(
                test_process(false),
                test_transport(CapturingWriter(Arc::new(Mutex::new(Vec::new())))),
                "panel-direct-remove",
            ),
        )
        .unwrap();
        let transfer = super::begin_terminal_session_transfer(&remove_state, 10)
            .unwrap()
            .unwrap();
        super::release_terminal_session_transfer(&remove_state, transfer, true).unwrap();
        assert!(super::drain_terminal_materialization_event(&remove_state, &remove_lease).is_err());
    }

    #[test]
    fn cold_materialization_redesign_open_and_materialization_are_exclusive() {
        let state = TerminalState::default();
        let spec = redesign_spec("C:/repo");
        let stale = redesign_start(
            &state,
            "panel-exclusive",
            &spec,
            super::TerminalMaterializationEvent::Input(b"retained".to_vec()),
        );
        assert!(
            super::reserve_terminal_open_for_control(&state, Some("panel-exclusive"), false,)
                .is_err()
        );
        let panels = ["panel-exclusive".to_string()].into_iter().collect();
        let lifecycle = super::detach_terminal_panels_for_control(&state, &panels).unwrap();
        let publication = super::publish_terminal_materialization_runtime(
            &state,
            &stale,
            11,
            test_session(
                test_process(false),
                test_transport(CapturingWriter(Arc::new(Mutex::new(Vec::new())))),
                "panel-exclusive",
            ),
        )
        .unwrap_err();
        super::rollback_terminal_panels_for_control(&state, lifecycle)
            .map_err(|error| error.message)
            .unwrap();
        let retry = super::retry_terminal_materialization_start(&state, "panel-exclusive", &spec)
            .unwrap()
            .unwrap();
        super::publish_terminal_materialization_runtime(&state, &retry, 11, publication.session)
            .unwrap();

        let reserved = TerminalState::default();
        let reservation = match super::reserve_terminal_open_for_control(
            &reserved,
            Some("panel-reserved-first"),
            false,
        )
        .unwrap()
        {
            super::TerminalOpenReservation::Reserved(reservation) => reservation,
            super::TerminalOpenReservation::Existing(_) => panic!("unexpected existing runtime"),
        };
        assert_eq!(
            super::request_terminal_materialization(
                &reserved,
                "panel-reserved-first",
                &spec,
                vec![super::TerminalMaterializationEvent::Input(
                    b"blocked".to_vec()
                )],
            ),
            super::TerminalMaterializationDemand::SurfaceUnavailable
        );
        super::rollback_terminal_open_reservation_for_control(&reserved, reservation).unwrap();
    }

    #[test]
    fn conpty_materialization_rechecks_lifecycle_and_retries_spawn_failure() {
        let state = Arc::new(TerminalState::default());
        let spec = redesign_spec("C:/repo");
        let first = redesign_start(
            &state,
            "panel-recheck",
            &spec,
            super::TerminalMaterializationEvent::Input(b"retained".to_vec()),
        );
        let panels = ["panel-recheck".to_string()].into_iter().collect();
        let lifecycle = super::detach_terminal_panels_for_control(&state, &panels).unwrap();
        let spawned = Arc::new(AtomicUsize::new(0));
        let spawn_counter = spawned.clone();
        let error = super::fulfill_terminal_materialization_with(
            &state,
            first,
            ConPtySize::new(80, 24),
            move |_, _, _, _| {
                spawn_counter.fetch_add(1, AtomicOrdering::SeqCst);
                panic!("a fenced start must not invoke the external factory")
            },
            |_, _, _| Ok(()),
        )
        .unwrap_err();
        assert!(error.contains("ownership"));
        assert_eq!(spawned.load(AtomicOrdering::SeqCst), 0);

        super::rollback_terminal_panels_for_control(&state, lifecycle)
            .map_err(|error| error.message)
            .unwrap();
        let retry = super::retry_terminal_materialization_start(&state, "panel-recheck", &spec)
            .unwrap()
            .expect("rollback exposes a fresh start owner");
        let failed_generation = retry.generation();
        let error = super::fulfill_terminal_materialization_with(
            &state,
            retry,
            ConPtySize::new(80, 24),
            |_, _, _, _| Err("injected ConPTY spawn failure".to_string()),
            |_, _, _| Ok(()),
        )
        .unwrap_err();
        assert!(error.contains("injected ConPTY spawn failure"));
        let retry = super::retry_terminal_materialization_start(&state, "panel-recheck", &spec)
            .unwrap()
            .expect("spawn failure retains the FIFO for retry");
        assert_ne!(retry.generation(), failed_generation);
        assert_eq!(
            super::cancel_terminal_materialization(&state, &retry).unwrap(),
            Some(vec![super::TerminalMaterializationEvent::Input(
                b"retained".to_vec()
            )])
        );
    }

    #[test]
    fn conpty_materialization_publication_loss_kills_exact_runtime_and_retains_fifo() {
        let state = Arc::new(TerminalState::default());
        let spec = redesign_spec("C:/repo");
        let first = super::TerminalMaterializationEvent::Input(b"first".to_vec());
        let second = super::TerminalMaterializationEvent::ProcessOutput(b"second".to_vec());
        let lease = redesign_start(&state, "panel-stale-publish", &spec, first.clone());
        let lifecycle = Arc::new(Mutex::new(None));
        let saved_lifecycle = lifecycle.clone();
        let spawn_state = state.clone();
        let kills = Arc::new(AtomicUsize::new(0));
        let process_kills = kills.clone();
        let error = super::fulfill_terminal_materialization_with(
            &state,
            lease,
            ConPtySize::new(80, 24),
            move |_, panel_id, _, _| {
                assert!(
                    spawn_state.registry.try_lock().is_ok(),
                    "factory ran under registry lock"
                );
                let panels = [panel_id.to_string()].into_iter().collect();
                *saved_lifecycle.lock().unwrap() =
                    Some(super::detach_terminal_panels_for_control(&spawn_state, &panels).unwrap());
                Ok(test_session(
                    Arc::new(Mutex::new(Box::new(LockCheckingProcess {
                        state: Arc::downgrade(&spawn_state),
                        kills: process_kills,
                    }))),
                    test_transport(CapturingWriter(Arc::new(Mutex::new(Vec::new())))),
                    panel_id,
                ))
            },
            |_, _, _| panic!("stale publication must not drain its FIFO"),
        )
        .unwrap_err();
        assert!(error.contains("publication"));
        assert_eq!(kills.load(AtomicOrdering::SeqCst), 1);
        assert_eq!(
            super::request_terminal_materialization(
                &state,
                "panel-stale-publish",
                &spec,
                vec![second.clone()],
            ),
            super::TerminalMaterializationDemand::Queued
        );
        super::rollback_terminal_panels_for_control(
            &state,
            lifecycle.lock().unwrap().take().unwrap(),
        )
        .map_err(|error| error.message)
        .unwrap();
        let retry =
            super::retry_terminal_materialization_start(&state, "panel-stale-publish", &spec)
                .unwrap()
                .expect("publication loss retains a retryable FIFO");
        assert_eq!(
            super::cancel_terminal_materialization(&state, &retry).unwrap(),
            Some(vec![first, second])
        );
    }

    #[test]
    fn conpty_materialization_flushes_fifo_parser_and_concurrent_append_before_activation() {
        let state = Arc::new(TerminalState::default());
        let spec = redesign_spec("C:/repo");
        let lease = redesign_start(
            &state,
            "panel-fifo",
            &spec,
            super::TerminalMaterializationEvent::Input(b"one".to_vec()),
        );
        assert_eq!(
            super::request_terminal_materialization(
                &state,
                "panel-fifo",
                &spec,
                vec![super::TerminalMaterializationEvent::ProcessOutput(
                    b"\x1b]2;cold-title\x07visible".to_vec(),
                )],
            ),
            super::TerminalMaterializationDemand::Queued
        );
        let captured = Arc::new(Mutex::new(Vec::new()));
        let process_kills = Arc::new(AtomicUsize::new(0));
        let activation = Arc::new(Mutex::new(None));
        let spawn_activation = activation.clone();
        let spawn_state = state.clone();
        let writer_bytes = captured.clone();
        let output_state = state.clone();
        let output_spec = spec.clone();
        let id = super::fulfill_terminal_materialization_with(
            &state,
            lease,
            ConPtySize::new(80, 24),
            move |_, panel_id, _, _| {
                assert!(spawn_state.registry.try_lock().is_ok());
                let (session, pending) = pending_test_session(
                    Arc::new(Mutex::new(Box::new(LockCheckingProcess {
                        state: Arc::downgrade(&spawn_state),
                        kills: process_kills,
                    }))),
                    test_transport(LockCheckingWriter {
                        state: Arc::downgrade(&spawn_state),
                        captured: writer_bytes,
                    }),
                    panel_id,
                );
                *spawn_activation.lock().unwrap() = Some(pending);
                Ok(session)
            },
            move |id, bytes, titles| {
                assert!(output_state.registry.try_lock().is_ok());
                let (grid, pending) = {
                    let registry = output_state.runtime_registry();
                    let session = registry.sessions.get(&id).unwrap();
                    (session.grid.clone(), session.pump_activation.clone())
                };
                assert!(pump_is_pending(&pending));
                assert!(terminal_text(&grid.lock().unwrap(), false, None).contains("visible"));
                assert_eq!(bytes, b"\x1b]2;cold-title\x07visible");
                assert_eq!(titles, ["cold-title".to_string()]);
                assert_eq!(
                    super::request_terminal_materialization(
                        &output_state,
                        "panel-fifo",
                        &output_spec,
                        vec![super::TerminalMaterializationEvent::Input(
                            b"three".to_vec()
                        )],
                    ),
                    super::TerminalMaterializationDemand::Queued
                );
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(&*captured.lock().unwrap(), b"onethree");
        assert!(!pump_is_pending(
            activation.lock().unwrap().as_ref().unwrap()
        ));
        assert_eq!(
            super::request_terminal_materialization(
                &state,
                "panel-fifo",
                &spec,
                vec![super::TerminalMaterializationEvent::Input(b"live".to_vec())],
            ),
            super::TerminalMaterializationDemand::Live(vec![
                super::TerminalMaterializationEvent::Input(b"live".to_vec())
            ])
        );
        assert!(state.runtime_registry().sessions.contains_key(&id));
    }

    #[test]
    fn conpty_materialization_lifecycle_waits_and_rollback_resumes_without_spawn() {
        let state = Arc::new(TerminalState::default());
        let spec = redesign_spec("C:/repo");
        let lease = redesign_start(
            &state,
            "panel-fenced-flush",
            &spec,
            super::TerminalMaterializationEvent::ProcessOutput(b"claimed".to_vec()),
        );
        assert_eq!(
            super::request_terminal_materialization(
                &state,
                "panel-fenced-flush",
                &spec,
                vec![super::TerminalMaterializationEvent::Input(
                    b"retained".to_vec()
                )],
            ),
            super::TerminalMaterializationDemand::Queued
        );
        let captured = Arc::new(Mutex::new(Vec::new()));
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let fulfill_state = state.clone();
        let fulfill_captured = captured.clone();
        let fulfill = std::thread::spawn(move || {
            super::fulfill_terminal_materialization_with(
                &fulfill_state,
                lease,
                ConPtySize::new(80, 24),
                move |_, panel_id, _, _| {
                    Ok(test_session(
                        test_process(false),
                        test_transport(CapturingWriter(fulfill_captured)),
                        panel_id,
                    ))
                },
                move |_, _, _| {
                    entered_tx.send(()).unwrap();
                    release_rx.recv().unwrap();
                    Ok(())
                },
            )
        });
        entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();

        let detach_state = state.clone();
        let (detached_tx, detached_rx) = mpsc::channel();
        let detach = std::thread::spawn(move || {
            let panels = ["panel-fenced-flush".to_string()].into_iter().collect();
            let result = super::detach_terminal_panels_for_control(&detach_state, &panels);
            detached_tx.send(()).unwrap();
            result
        });
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if state
                .runtime_registry()
                .reserved_panel_ids
                .contains("panel-fenced-flush")
            {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "lifecycle fence was not established"
            );
            std::thread::yield_now();
        }
        assert!(
            detached_rx.try_recv().is_err(),
            "lifecycle must wait for claimed apply"
        );
        release_tx.send(()).unwrap();
        assert!(fulfill.join().unwrap().is_err());
        detached_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        let lifecycle = detach.join().unwrap().unwrap();
        assert!(
            captured.lock().unwrap().is_empty(),
            "fence prevented the next FIFO pop"
        );
        super::rollback_terminal_panels_for_control(&state, lifecycle)
            .map_err(|error| error.message)
            .unwrap();
        let resume =
            super::retry_terminal_materialization_flush(&state, "panel-fenced-flush", &spec)
                .unwrap()
                .expect("rollback exposes the published FIFO for resumption");
        super::fulfill_terminal_materialization_with(
            &state,
            resume,
            ConPtySize::new(80, 24),
            |_, _, _, _| panic!("published resume must not spawn a second ConPTY"),
            |_, _, _| Ok(()),
        )
        .unwrap();
        assert_eq!(&*captured.lock().unwrap(), b"retained");
    }

    #[test]
    fn conpty_materialization_apply_failure_discards_batch_without_stranding_runtime() {
        let state = Arc::new(TerminalState::default());
        let spec = redesign_spec("C:/repo");
        let lease = redesign_start(
            &state,
            "panel-apply-failure",
            &spec,
            super::TerminalMaterializationEvent::ProcessOutput(b"fails".to_vec()),
        );
        assert_eq!(
            super::request_terminal_materialization(
                &state,
                "panel-apply-failure",
                &spec,
                vec![super::TerminalMaterializationEvent::Input(
                    b"discarded".to_vec()
                )],
            ),
            super::TerminalMaterializationDemand::Queued
        );
        let activation = Arc::new(Mutex::new(None));
        let saved_activation = activation.clone();
        let error = super::fulfill_terminal_materialization_with(
            &state,
            lease,
            ConPtySize::new(80, 24),
            move |_, panel_id, _, _| {
                let (session, pending) = pending_test_session(
                    test_process(false),
                    test_transport(CapturingWriter(Arc::new(Mutex::new(Vec::new())))),
                    panel_id,
                );
                *saved_activation.lock().unwrap() = Some(pending);
                Ok(session)
            },
            |_, _, _| Err("injected output apply failure".to_string()),
        )
        .unwrap_err();
        assert!(error.contains("injected output apply failure"));
        assert!(!pump_is_pending(
            activation.lock().unwrap().as_ref().unwrap()
        ));
        assert_eq!(
            super::request_terminal_materialization(
                &state,
                "panel-apply-failure",
                &spec,
                vec![super::TerminalMaterializationEvent::Input(
                    b"later".to_vec()
                )],
            ),
            super::TerminalMaterializationDemand::Live(vec![
                super::TerminalMaterializationEvent::Input(b"later".to_vec())
            ])
        );
    }

    #[test]
    fn conpty_materialization_apply_failure_recovers_poisoned_registry() {
        let state = Arc::new(TerminalState::default());
        let spec = redesign_spec("C:/repo");
        let lease = redesign_start(
            &state,
            "panel-poisoned-abort",
            &spec,
            super::TerminalMaterializationEvent::ProcessOutput(b"fails".to_vec()),
        );
        assert_eq!(
            super::request_terminal_materialization(
                &state,
                "panel-poisoned-abort",
                &spec,
                vec![super::TerminalMaterializationEvent::Input(
                    b"discarded".to_vec()
                )],
            ),
            super::TerminalMaterializationDemand::Queued
        );
        let activation = Arc::new(Mutex::new(None));
        let saved_activation = activation.clone();
        let poison_state = state.clone();
        let error = super::fulfill_terminal_materialization_with(
            &state,
            lease,
            ConPtySize::new(80, 24),
            move |_, panel_id, _, _| {
                let (session, pending) = pending_test_session(
                    test_process(false),
                    test_transport(CapturingWriter(Arc::new(Mutex::new(Vec::new())))),
                    panel_id,
                );
                *saved_activation.lock().unwrap() = Some(pending);
                Ok(session)
            },
            move |_, _, _| {
                let state = poison_state.clone();
                assert!(std::thread::spawn(move || {
                    let _registry = state.registry.lock().unwrap();
                    panic!("poison registry during materialization apply failure");
                })
                .join()
                .is_err());
                Err("injected poisoned output apply failure".to_string())
            },
        )
        .unwrap_err();
        assert!(error.contains("injected poisoned output apply failure"));
        assert!(error.contains("terminal runtime registry mutex poisoned"));
        let registry = match state.registry.lock() {
            Ok(_) => panic!("registry must remain classified as poisoned"),
            Err(error) => error.into_inner(),
        };
        assert!(registry
            .sessions
            .values()
            .any(|session| { session.panel_id.as_deref() == Some("panel-poisoned-abort") }));
        assert!(!registry
            .materializations
            .contains_key("panel-poisoned-abort"));
        drop(registry);
        assert!(!pump_is_pending(
            activation.lock().unwrap().as_ref().unwrap()
        ));
    }

    #[cfg(windows)]
    #[test]
    fn conpty_materialization_real_round_trip_owns_process_and_spec() {
        let directory = std::env::temp_dir().join(format!(
            "cmux-conpty-materialization-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let expected_cwd = directory.to_string_lossy().to_string();
        let command = "$line=[Console]::In.ReadLine(); Write-Output ('CMUX-OWNED|' + $env:CMUX_OWNED_TEST + '|' + (Get-Location).Path + '|' + $line); Start-Sleep -Seconds 30";
        let spec = super::TerminalMaterializationSpec::new(
            Some(expected_cwd.clone()),
            Some(command.to_string()),
            b"initial-payload\r\n".to_vec(),
            BTreeMap::from([("CMUX_OWNED_TEST".to_string(), "environment-ok".to_string())]),
        );
        let components =
            super::spawn_terminal_process_components(&spec, ConPtySize::new(100, 30)).unwrap();
        assert!(components.root_pid.is_some_and(|pid| pid > 0));
        let mut process = components.process;
        let mut reader = components.reader;
        let mut writer = components.writer;
        let (output_tx, output_rx) = mpsc::channel();
        let reader_thread = std::thread::spawn(move || {
            let mut chunk = [0_u8; 4096];
            loop {
                match reader.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(read) => output_tx.send(chunk[..read].to_vec()).unwrap(),
                }
            }
        });
        let marker = format!("CMUX-OWNED|environment-ok|{}|initial-payload", expected_cwd);
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut output = Vec::new();
        let mut answered_cursor_request = false;
        while !String::from_utf8_lossy(&output).contains(&marker) {
            if !answered_cursor_request && output.windows(4).any(|bytes| bytes == b"\x1b[6n") {
                writer.write_all(b"\x1b[1;1R").unwrap();
                writer.flush().unwrap();
                answered_cursor_request = true;
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                let _ = process.kill();
                drop(writer);
                drop(process);
                reader_thread.join().unwrap();
                let _ = std::fs::remove_dir(&directory);
                panic!(
                    "ConPTY marker missing: {}",
                    String::from_utf8_lossy(&output)
                );
            }
            match output_rx.recv_timeout(remaining.min(Duration::from_millis(250))) {
                Ok(chunk) => output.extend(chunk),
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    let _ = process.kill();
                    drop(writer);
                    drop(process);
                    reader_thread.join().unwrap();
                    let _ = std::fs::remove_dir(&directory);
                    panic!(
                        "ConPTY reader closed before marker: {}",
                        String::from_utf8_lossy(&output)
                    );
                }
            }
        }
        process.kill().unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if process.try_wait().unwrap().is_some() {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "owned ConPTY did not exit after kill"
            );
            std::thread::yield_now();
        }
        drop(writer);
        drop(process);
        reader_thread.join().unwrap();
        std::fs::remove_dir(&directory).unwrap();
    }
