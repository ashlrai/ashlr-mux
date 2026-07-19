
    #[test]
    fn ui_attach_reuses_staged_panel_but_control_replace_reserves_a_new_session() {
        let staged = [(41_u32, Some("dock-surface")), (42, Some("other"))];
        assert_eq!(
            reusable_panel_session_id(staged.into_iter(), Some("dock-surface"), true),
            Some(41),
            "the UI open attaches to the one staged live terminal"
        );
        assert_eq!(
            reusable_panel_session_id(staged.into_iter(), Some("dock-surface"), false),
            None,
            "control staging, including TerminalReplace, must create a distinct session"
        );
    }

    #[test]
    fn open_identity_reservation_fences_reuse_and_rolls_back_exactly() {
        let state = TerminalState::default();
        let reservation =
            match super::reserve_terminal_open_for_control(&state, Some("  panel-a  "), false)
                .expect("reserve a new terminal identity")
            {
                super::TerminalOpenReservation::Reserved(reservation) => reservation,
                super::TerminalOpenReservation::Existing(_) => {
                    panic!("unexpected existing runtime")
                }
            };

        {
            let registry = state.registry.lock().unwrap();
            assert!(registry.reserved_session_ids.contains(&reservation.id));
            assert!(registry.reserved_panel_ids.contains("panel-a"));
            assert!(registry.sessions.is_empty());
        }
        assert!(super::reserve_terminal_open_for_control(&state, Some("panel-a"), true).is_err());

        super::rollback_terminal_open_reservation_for_control(&state, reservation)
            .expect("roll back exact reservation");
        let registry = state.registry.lock().unwrap();
        assert!(registry.reserved_session_ids.is_empty());
        assert!(registry.reserved_panel_ids.is_empty());
        assert!(registry.sessions.is_empty());
    }

    #[test]
    fn ui_open_reuses_published_runtime_while_control_open_reserves_a_distinct_identity() {
        let state = TerminalState::default();
        state
            .next_id
            .store(42, std::sync::atomic::Ordering::Relaxed);
        state.registry.lock().unwrap().sessions.insert(
            41,
            test_session(test_process(false), test_transport(io::sink()), "panel-a"),
        );

        assert!(matches!(
            super::reserve_terminal_open_for_control(&state, Some("panel-a"), true).unwrap(),
            super::TerminalOpenReservation::Existing(41)
        ));
        let replacement =
            match super::reserve_terminal_open_for_control(&state, Some("panel-a"), false).unwrap()
            {
                super::TerminalOpenReservation::Reserved(reservation) => reservation,
                super::TerminalOpenReservation::Existing(_) => {
                    panic!("control open reused runtime")
                }
            };
        assert_eq!(replacement.id, 42);
        assert_eq!(replacement.panel_id.as_deref(), Some("panel-a"));
        assert!(state.registry.lock().unwrap().sessions.contains_key(&41));
        super::rollback_terminal_open_reservation_for_control(&state, replacement).unwrap();
    }

    #[test]
    fn open_publication_is_atomic_with_releasing_the_exact_reservation() {
        let state = TerminalState::default();
        let reservation =
            match super::reserve_terminal_open_for_control(&state, Some("panel-a"), false).unwrap()
            {
                super::TerminalOpenReservation::Reserved(reservation) => reservation,
                super::TerminalOpenReservation::Existing(_) => {
                    panic!("unexpected existing runtime")
                }
            };
        let id = reservation.id;
        let session = test_session(test_process(false), test_transport(io::sink()), "panel-a");

        let published = super::publish_terminal_open_reservation(&state, &reservation, session)
            .map_err(|(error, _session)| error)
            .expect("publish reserved terminal");
        assert_eq!(published, id);
        let registry = state.registry.lock().unwrap();
        assert!(registry.sessions.contains_key(&id));
        assert!(!registry.reserved_session_ids.contains(&id));
        assert!(!registry.reserved_panel_ids.contains("panel-a"));
    }

    #[test]
    fn abandoned_open_guard_releases_its_unpublished_identity() {
        let state = TerminalState::default();
        let reservation =
            match super::reserve_terminal_open_for_control(&state, Some("abandoned-panel"), false)
                .unwrap()
            {
                super::TerminalOpenReservation::Reserved(reservation) => reservation,
                super::TerminalOpenReservation::Existing(_) => {
                    panic!("unexpected existing runtime")
                }
            };
        {
            let _guard = super::TerminalOpenReservationGuard::new(&state, reservation);
            assert!(state
                .registry
                .lock()
                .unwrap()
                .reserved_panel_ids
                .contains("abandoned-panel"));
        }
        let registry = state.registry.lock().unwrap();
        assert!(registry.reserved_session_ids.is_empty());
        assert!(!registry.reserved_panel_ids.contains("abandoned-panel"));
    }

    #[test]
    fn startup_input_runs_while_the_reserved_registry_remains_available() {
        let state = Arc::new(TerminalState::default());
        let reservation =
            match super::reserve_terminal_open_for_control(&state, Some("startup-panel"), false)
                .unwrap()
            {
                super::TerminalOpenReservation::Reserved(reservation) => reservation,
                super::TerminalOpenReservation::Existing(_) => {
                    panic!("unexpected existing runtime")
                }
            };
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let startup = std::thread::spawn(move || {
            let mut writer = BlockingWriter {
                entered: entered_tx,
                release: release_rx,
                captured: Arc::new(Mutex::new(Vec::new())),
                block_once: true,
            };
            super::write_terminal_initial_input(&mut writer, Some("startup"))
        });
        entered_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("startup writer reached blocking I/O");
        let registry_available = state.registry.try_lock().is_ok();
        release_tx.send(()).unwrap();
        startup.join().unwrap().unwrap();
        assert!(registry_available, "startup I/O held the global registry");
        super::rollback_terminal_open_reservation_for_control(&state, reservation).unwrap();
    }

    #[test]
    fn resize_and_shutdown_process_io_do_not_hold_the_global_registry() {
        let resize_state = Arc::new(TerminalState::default());
        let (resize_entered_tx, resize_entered_rx) = mpsc::channel();
        let (resize_release_tx, resize_release_rx) = mpsc::channel();
        resize_state.registry.lock().unwrap().sessions.insert(
            1,
            test_session(
                Arc::new(Mutex::new(Box::new(BlockingResizeProcess {
                    entered: resize_entered_tx,
                    release: resize_release_rx,
                }))),
                test_transport(io::sink()),
                "resize-panel",
            ),
        );
        let resize_worker_state = resize_state.clone();
        let resize_worker = std::thread::spawn(move || {
            super::terminal_resize_id_for_control(&resize_worker_state, 1, 100, 30)
        });
        resize_entered_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("resize reached process I/O");
        let resize_registry_available = resize_state.registry.try_lock().is_ok();
        resize_release_tx.send(()).unwrap();
        resize_worker.join().unwrap().unwrap();
        assert!(resize_registry_available, "resize held the global registry");

        let shutdown_state = Arc::new(TerminalState::default());
        let (kill_entered_tx, kill_entered_rx) = mpsc::channel();
        let (kill_release_tx, kill_release_rx) = mpsc::channel();
        shutdown_state.registry.lock().unwrap().sessions.insert(
            2,
            test_session(
                Arc::new(Mutex::new(Box::new(BlockingKillProcess {
                    entered: kill_entered_tx,
                    release: kill_release_rx,
                }))),
                test_transport(io::sink()),
                "kill-panel",
            ),
        );
        let kill_worker_state = shutdown_state.clone();
        let kill_worker = std::thread::spawn(move || {
            super::terminal_shutdown_id_preserving_authority_for_control(&kill_worker_state, 2)
        });
        kill_entered_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("kill reached process I/O");
        let kill_registry_available = shutdown_state.registry.try_lock().is_ok();
        kill_release_tx.send(()).unwrap();
        kill_worker.join().unwrap().unwrap();
        assert!(kill_registry_available, "shutdown held the global registry");
    }

    #[test]
    fn finalization_kills_without_registry_lock_and_retains_failed_authority() {
        let state = Arc::new(TerminalState::default());
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        {
            let mut registry = state.registry.lock().unwrap();
            registry.sessions.insert(
                7,
                test_session(
                    Arc::new(Mutex::new(Box::new(BlockingKillProcess {
                        entered: entered_tx,
                        release: release_rx,
                    }))),
                    test_transport(io::sink()),
                    "ok-panel",
                ),
            );
            registry.sessions.insert(
                8,
                test_session(
                    Arc::new(Mutex::new(Box::new(FailingKillProcess))),
                    test_transport(io::sink()),
                    "retry-panel",
                ),
            );
        }
        let panels = ["ok-panel".to_string(), "retry-panel".to_string()]
            .into_iter()
            .collect();
        let lease = super::detach_terminal_panels_for_control(&state, &panels).unwrap();
        let worker_state = state.clone();
        let worker = std::thread::spawn(move || {
            super::finalize_terminal_panels_for_control(&worker_state, lease)
        });
        entered_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("finalize reached process kill");
        let registry_available = state.registry.try_lock().is_ok();
        release_tx.send(()).unwrap();
        let failure = worker.join().unwrap().expect_err("one kill must fail");
        assert!(registry_available, "finalize held the global registry");

        {
            let registry = state.registry.lock().unwrap();
            assert!(!registry.reserved_session_ids.contains(&7));
            assert!(!registry.reserved_panel_ids.contains("ok-panel"));
            assert!(registry.reserved_session_ids.contains(&8));
            assert!(registry.reserved_panel_ids.contains("retry-panel"));
            assert!(registry.sessions.is_empty());
        }
        super::rollback_terminal_panels_for_control(&state, failure.retry)
            .unwrap_or_else(|error| panic!("retry rollback failed: {}", error.message));
        assert_eq!(
            super::terminal_ids_for_panel_for_control(&state, "retry-panel"),
            vec![8]
        );
        assert!(super::terminal_ids_for_panel_for_control(&state, "ok-panel").is_empty());
    }

    #[test]
    fn rollback_collision_is_all_or_nothing_and_preserves_the_lease() {
        let state = TerminalState::default();
        {
            let mut registry = state.registry.lock().unwrap();
            registry.sessions.insert(
                10,
                test_session(test_process(false), test_transport(io::sink()), "panel-a"),
            );
            registry.sessions.insert(
                11,
                test_session(test_process(false), test_transport(io::sink()), "panel-b"),
            );
        }
        let panels = ["panel-a".to_string(), "panel-b".to_string()]
            .into_iter()
            .collect();
        let lease = super::detach_terminal_panels_for_control(&state, &panels).unwrap();
        state.registry.lock().unwrap().sessions.insert(
            10,
            test_session(test_process(false), test_transport(io::sink()), "intruder"),
        );

        let collision = super::rollback_terminal_panels_for_control(&state, lease)
            .expect_err("collision must preserve the entire lease");
        {
            let registry = state.registry.lock().unwrap();
            assert_eq!(registry.sessions.len(), 1);
            assert!(registry.sessions.contains_key(&10));
            assert!(!registry.sessions.contains_key(&11));
            assert!(registry.reserved_session_ids.contains(&10));
            assert!(registry.reserved_session_ids.contains(&11));
        }
        state.registry.lock().unwrap().sessions.remove(&10);
        super::rollback_terminal_panels_for_control(&state, collision.lease)
            .unwrap_or_else(|error| panic!("rollback after collision failed: {}", error.message));
        assert_eq!(
            super::terminal_ids_for_panel_for_control(&state, "panel-a"),
            vec![10]
        );
        assert_eq!(
            super::terminal_ids_for_panel_for_control(&state, "panel-b"),
            vec![11]
        );
    }

    #[test]
    fn poisoned_registry_returns_lifecycle_errors_without_panicking() {
        let state = Arc::new(TerminalState::default());
        let poison_state = state.clone();
        assert!(std::thread::spawn(move || {
            let _guard = poison_state.registry.lock().unwrap();
            panic!("poison terminal registry for lifecycle contract");
        })
        .join()
        .is_err());

        assert!(super::reserve_terminal_open_for_control(&state, Some("panel"), false).is_err());
        assert!(super::detach_terminal_panels_for_control(
            &state,
            &["panel".to_string()].into_iter().collect(),
        )
        .is_err());
        assert!(super::terminal_resize_id_for_control(&state, 1, 80, 24).is_err());
        assert!(super::terminal_shutdown_id_preserving_authority_for_control(&state, 1).is_err());
    }

    #[test]
    fn control_panel_reservation_fences_every_id_based_mutation_of_the_old_runtime() {
        let state = TerminalState::default();
        let captured = Arc::new(Mutex::new(Vec::new()));
        state
            .next_id
            .store(42, std::sync::atomic::Ordering::Relaxed);
        state.registry.lock().unwrap().sessions.insert(
            41,
            test_session(
                test_process(false),
                test_transport(CapturingWriter(captured.clone())),
                "panel-a",
            ),
        );
        let replacement =
            match super::reserve_terminal_open_for_control(&state, Some("panel-a"), false).unwrap()
            {
                super::TerminalOpenReservation::Reserved(reservation) => reservation,
                super::TerminalOpenReservation::Existing(_) => {
                    panic!("control open reused runtime")
                }
            };

        assert!(super::terminal_write_id_for_control(&state, 41, b"blocked").is_err());
        assert!(super::terminal_resize_id_for_control(&state, 41, 100, 30).is_err());
        assert!(super::terminal_shutdown_id_preserving_authority_for_control(&state, 41).is_err());
        assert!(super::terminal_remove_id_for_control(&state, 41).is_err());
        assert!(super::terminal_close_id_for_control(&state, 41).is_err());
        assert!(state.registry.lock().unwrap().sessions.contains_key(&41));

        super::rollback_terminal_open_reservation_for_control(&state, replacement).unwrap();
        super::terminal_write_id_for_control(&state, 41, b"reopened").unwrap();
        assert_eq!(&*captured.lock().unwrap(), b"reopened");
        let registry = state.registry.lock().unwrap();
        assert!(registry.reserved_session_ids.is_empty());
        assert!(registry.reserved_panel_ids.is_empty());
    }

    #[test]
    fn terminal_pump_stays_dormant_until_the_consumer_is_ready() {
        let (activation, ready) = super::TerminalPumpActivation::pending();
        let (emitted_tx, emitted_rx) = mpsc::channel();
        let pump = std::thread::spawn(move || {
            if ready.recv().is_ok() {
                emitted_tx.send(()).unwrap();
            }
        });

        activation.advance(super::TerminalPumpReadiness::Published);
        assert!(emitted_rx.recv_timeout(Duration::from_millis(50)).is_err());
        activation.advance(super::TerminalPumpReadiness::ConsumerReady);
        emitted_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("activation released dormant pump");
        pump.join().unwrap();
    }

    #[test]
    fn open_rollback_recovers_exact_authority_after_mid_transaction_registry_poison() {
        let state = Arc::new(TerminalState::default());
        let captured = Arc::new(Mutex::new(Vec::new()));
        state.registry.lock().unwrap().sessions.insert(
            41,
            test_session(
                test_process(false),
                test_transport(CapturingWriter(captured.clone())),
                "panel-a",
            ),
        );
        let reservation =
            match super::reserve_terminal_open_for_control(&state, Some("panel-a"), false).unwrap()
            {
                super::TerminalOpenReservation::Reserved(reservation) => reservation,
                super::TerminalOpenReservation::Existing(_) => {
                    panic!("control open reused runtime")
                }
            };
        let poison_state = state.clone();
        assert!(std::thread::spawn(move || {
            let _guard = poison_state.registry.lock().unwrap();
            panic!("poison registry during open transaction");
        })
        .join()
        .is_err());

        assert!(
            super::rollback_terminal_open_reservation_for_control(&state, reservation).is_err()
        );
        state.registry.clear_poison();
        {
            let registry = state.registry.lock().unwrap();
            assert!(registry.sessions.contains_key(&41));
            assert!(registry.reserved_session_ids.is_empty());
            assert!(registry.reserved_panel_ids.is_empty());
        }
        super::terminal_write_id_for_control(&state, 41, b"reopened").unwrap();
        assert_eq!(&*captured.lock().unwrap(), b"reopened");
    }

    #[test]
    fn detach_waits_for_inflight_input_resize_and_shutdown_before_returning_lease() {
        let input_state = Arc::new(TerminalState::default());
        let (input_entered_tx, input_entered_rx) = mpsc::channel();
        let (input_release_tx, input_release_rx) = mpsc::channel();
        input_state.registry.lock().unwrap().sessions.insert(
            1,
            test_session(
                test_process(false),
                test_transport(BlockingWriter {
                    entered: input_entered_tx,
                    release: input_release_rx,
                    captured: Arc::new(Mutex::new(Vec::new())),
                    block_once: true,
                }),
                "input-panel",
            ),
        );
        let input_worker_state = input_state.clone();
        let input_worker = std::thread::spawn(move || {
            super::terminal_write_id_for_control(&input_worker_state, 1, b"input")
        });
        input_entered_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("input reached writer");
        let input_detach_state = input_state.clone();
        let (input_lease_tx, input_lease_rx) = mpsc::channel();
        let input_detach = std::thread::spawn(move || {
            let lease = super::detach_terminal_panels_for_control(
                &input_detach_state,
                &["input-panel".to_string()].into_iter().collect(),
            )
            .unwrap();
            input_lease_tx.send(lease).unwrap();
        });
        assert!(input_lease_rx
            .recv_timeout(Duration::from_millis(50))
            .is_err());
        input_release_tx.send(()).unwrap();
        input_worker.join().unwrap().unwrap();
        let input_lease = input_lease_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        input_detach.join().unwrap();
        super::rollback_terminal_panels_for_control(&input_state, input_lease)
            .unwrap_or_else(|error| panic!("input rollback failed: {}", error.message));

        let resize_state = Arc::new(TerminalState::default());
        let (resize_entered_tx, resize_entered_rx) = mpsc::channel();
        let (resize_release_tx, resize_release_rx) = mpsc::channel();
        resize_state.registry.lock().unwrap().sessions.insert(
            2,
            test_session(
                Arc::new(Mutex::new(Box::new(BlockingResizeProcess {
                    entered: resize_entered_tx,
                    release: resize_release_rx,
                }))),
                test_transport(io::sink()),
                "resize-panel",
            ),
        );
        let resize_worker_state = resize_state.clone();
        let resize_worker = std::thread::spawn(move || {
            super::terminal_resize_id_for_control(&resize_worker_state, 2, 100, 30)
        });
        resize_entered_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("resize reached process");
        let resize_detach_state = resize_state.clone();
        let (resize_lease_tx, resize_lease_rx) = mpsc::channel();
        let resize_detach = std::thread::spawn(move || {
            let lease = super::detach_terminal_panels_for_control(
                &resize_detach_state,
                &["resize-panel".to_string()].into_iter().collect(),
            )
            .unwrap();
            resize_lease_tx.send(lease).unwrap();
        });
        assert!(resize_lease_rx
            .recv_timeout(Duration::from_millis(50))
            .is_err());
        resize_release_tx.send(()).unwrap();
        resize_worker.join().unwrap().unwrap();
        let resize_lease = resize_lease_rx
            .recv_timeout(Duration::from_secs(2))
            .unwrap();
        resize_detach.join().unwrap();
        super::rollback_terminal_panels_for_control(&resize_state, resize_lease)
            .unwrap_or_else(|error| panic!("resize rollback failed: {}", error.message));

        let shutdown_state = Arc::new(TerminalState::default());
        let (kill_entered_tx, kill_entered_rx) = mpsc::channel();
        let (kill_release_tx, kill_release_rx) = mpsc::channel();
        shutdown_state.registry.lock().unwrap().sessions.insert(
            3,
            test_session(
                Arc::new(Mutex::new(Box::new(BlockingKillProcess {
                    entered: kill_entered_tx,
                    release: kill_release_rx,
                }))),
                test_transport(io::sink()),
                "shutdown-panel",
            ),
        );
        let shutdown_worker_state = shutdown_state.clone();
        let shutdown_worker = std::thread::spawn(move || {
            super::terminal_shutdown_id_preserving_authority_for_control(&shutdown_worker_state, 3)
        });
        kill_entered_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("shutdown reached process");
        let shutdown_detach_state = shutdown_state.clone();
        let (shutdown_lease_tx, shutdown_lease_rx) = mpsc::channel();
        let shutdown_detach = std::thread::spawn(move || {
            let lease = super::detach_terminal_panels_for_control(
                &shutdown_detach_state,
                &["shutdown-panel".to_string()].into_iter().collect(),
            )
            .unwrap();
            shutdown_lease_tx.send(lease).unwrap();
        });
        assert!(shutdown_lease_rx
            .recv_timeout(Duration::from_millis(50))
            .is_err());
        kill_release_tx.send(()).unwrap();
        shutdown_worker.join().unwrap().unwrap();
        let shutdown_lease = shutdown_lease_rx
            .recv_timeout(Duration::from_secs(2))
            .unwrap();
        shutdown_detach.join().unwrap();
        super::rollback_terminal_panels_for_control(&shutdown_state, shutdown_lease)
            .unwrap_or_else(|error| panic!("shutdown rollback failed: {}", error.message));
    }

    #[test]
    fn finalize_retry_releases_completed_ids_after_registry_recovery() {
        let state = Arc::new(TerminalState::default());
        state.registry.lock().unwrap().sessions.insert(
            9,
            test_session(
                Arc::new(Mutex::new(Box::new(PoisonRegistryDuringKillProcess {
                    state: Arc::downgrade(&state),
                }))),
                test_transport(io::sink()),
                "panel",
            ),
        );
        let lease = super::detach_terminal_panels_for_control(
            &state,
            &["panel".to_string()].into_iter().collect(),
        )
        .unwrap();
        let failed = super::finalize_terminal_panels_for_control(&state, lease)
            .expect_err("post-kill registry poison must preserve retry authority");
        state.registry.clear_poison();
        super::finalize_terminal_panels_for_control(&state, failed.retry)
            .unwrap_or_else(|error| panic!("retry finalize failed: {:?}", error.failures));

        let registry = state.registry.lock().unwrap();
        assert!(registry.reserved_session_ids.is_empty());
        assert!(registry.reserved_panel_ids.is_empty());
        assert!(registry.sessions.is_empty());
    }

    #[test]
    fn close_failure_is_classified_and_retains_exact_runtime_authority() {
        let failing = TerminalState::default();
        failing.registry.lock().unwrap().sessions.insert(
            12,
            test_session(
                Arc::new(Mutex::new(Box::new(FailingKillProcess))),
                test_transport(io::sink()),
                "failing-panel",
            ),
        );
        assert!(super::terminal_close_id_for_control(&failing, 12).is_err());
        super::terminal_write_id_for_control(&failing, 12, b"still-owned").unwrap();
        {
            let registry = failing.registry.lock().unwrap();
            assert!(registry.sessions.contains_key(&12));
            assert!(registry.reserved_session_ids.is_empty());
            assert!(registry.reserved_panel_ids.is_empty());
        }

        let poisoned = TerminalState::default();
        let process = test_process(false);
        poisoned.registry.lock().unwrap().sessions.insert(
            13,
            test_session(
                process.clone(),
                test_transport(io::sink()),
                "poisoned-panel",
            ),
        );
        assert!(std::thread::spawn(move || {
            let _guard = process.lock().unwrap();
            panic!("poison process before close");
        })
        .join()
        .is_err());
        assert!(super::terminal_close_id_for_control(&poisoned, 13).is_err());
        let registry = poisoned.registry.lock().unwrap();
        assert!(registry.sessions.contains_key(&13));
        assert!(registry.reserved_session_ids.is_empty());
        assert!(registry.reserved_panel_ids.is_empty());
    }

    #[test]
    fn close_completes_exact_cleanup_after_post_kill_registry_poison() {
        let state = Arc::new(TerminalState::default());
        state.registry.lock().unwrap().sessions.insert(
            14,
            test_session(
                Arc::new(Mutex::new(Box::new(PoisonRegistryDuringKillProcess {
                    state: Arc::downgrade(&state),
                }))),
                test_transport(io::sink()),
                "poison-during-close",
            ),
        );

        assert!(super::terminal_close_id_for_control(&state, 14).is_err());
        state.registry.clear_poison();
        let registry = state.registry.lock().unwrap();
        assert!(!registry.sessions.contains_key(&14));
        assert!(registry.reserved_session_ids.is_empty());
        assert!(registry.reserved_panel_ids.is_empty());
    }

    #[test]
    fn empty_and_large_idle_live_input_bypass_the_pending_budget() {
        let captured = Arc::new(Mutex::new(Vec::new()));
        let input = test_transport(CapturingWriter(captured.clone()));
        let exited = test_process(true);

        assert_eq!(
            send_terminal_input(exited, input.clone(), b""),
            TerminalInputOutcome::Sent
        );
        assert_eq!(input.pending.lock().unwrap().bytes, 0);
        assert!(input.pending.lock().unwrap().entries.is_empty());

        let payload = vec![b'x'; TERMINAL_PENDING_INPUT_LIMIT + 1];
        assert_eq!(
            send_terminal_input(test_process(false), input, &payload),
            TerminalInputOutcome::Sent
        );
        assert_eq!(*captured.lock().unwrap(), payload);
    }

    #[test]
    fn contended_input_is_fifo_bounded_and_does_not_hold_the_registry() {
        let state = Arc::new(TerminalState::default());
        let captured = Arc::new(Mutex::new(Vec::new()));
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        state.runtime_registry().sessions.insert(
            1,
            test_session(
                test_process(false),
                test_transport(BlockingWriter {
                    entered: entered_tx,
                    release: release_rx,
                    captured: captured.clone(),
                    block_once: true,
                }),
                "panel",
            ),
        );

        let owner_state = state.clone();
        let owner = std::thread::spawn(move || {
            super::terminal_send_panel_bytes_for_control(&owner_state, "panel", b"first")
        });
        entered_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("owner reached writer I/O");
        assert!(state.registry.try_lock().is_ok());

        let queued = vec![b'q'; TERMINAL_PENDING_INPUT_LIMIT];
        assert_eq!(
            super::terminal_send_panel_bytes_for_control(&state, "panel", &queued),
            TerminalInputOutcome::Queued
        );
        assert_eq!(
            super::terminal_send_panel_bytes_for_control(&state, "panel", b"overflow"),
            TerminalInputOutcome::InputQueueFull
        );
        release_tx.send(()).unwrap();
        assert_eq!(owner.join().unwrap(), TerminalInputOutcome::Sent);

        let captured = captured.lock().unwrap();
        assert_eq!(&captured[..5], b"first");
        assert_eq!(&captured[5..], queued);
    }

    #[test]
    fn queued_write_failure_retains_fifo_and_preserves_caller_relative_outcomes() {
        let captured = Arc::new(Mutex::new(Vec::new()));
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let input = test_transport(FailSecondWriteOnce {
            entered: entered_tx,
            release: release_rx,
            captured: captured.clone(),
            writes: 0,
        });
        let process = test_process(false);

        let owner_input = input.clone();
        let owner_process = process.clone();
        let owner =
            std::thread::spawn(move || send_terminal_input(owner_process, owner_input, b"first"));
        entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(
            send_terminal_input(process.clone(), input.clone(), b"second"),
            TerminalInputOutcome::Queued
        );
        release_tx.send(()).unwrap();
        assert_eq!(owner.join().unwrap(), TerminalInputOutcome::Sent);
        {
            let pending = input.pending.lock().unwrap();
            assert_eq!(pending.bytes, b"second".len());
            assert_eq!(pending.entries.len(), 1);
        }

        assert_eq!(
            send_terminal_input(process, input.clone(), b"third"),
            TerminalInputOutcome::Queued
        );
        assert_eq!(&*captured.lock().unwrap(), b"firstsecondthird");
        let pending = input.pending.lock().unwrap();
        assert_eq!(pending.bytes, 0);
        assert!(pending.entries.is_empty());
    }

    #[test]
    fn partial_queued_write_recovers_from_the_exact_unwritten_offset() {
        let captured = Arc::new(Mutex::new(Vec::new()));
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let input = test_transport(PartialQueuedFailureOnce {
            entered: entered_tx,
            release: release_rx,
            captured: captured.clone(),
            writes: 0,
        });
        let process = test_process(false);

        let owner_input = input.clone();
        let owner_process = process.clone();
        let owner =
            std::thread::spawn(move || send_terminal_input(owner_process, owner_input, b"first"));
        entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(
            send_terminal_input(process.clone(), input.clone(), b"second"),
            TerminalInputOutcome::Queued
        );
        release_tx.send(()).unwrap();
        assert_eq!(owner.join().unwrap(), TerminalInputOutcome::Sent);

        assert_eq!(
            send_terminal_input(process, input.clone(), b"third"),
            TerminalInputOutcome::Queued
        );
        assert_eq!(&*captured.lock().unwrap(), b"firstsecondthird");
        let pending = input.pending.lock().unwrap();
        assert_eq!(pending.bytes, 0);
        assert!(pending.entries.is_empty());
    }

    #[test]
    fn writer_panic_releases_owner_and_rejects_new_input_without_queueing() {
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let input = test_transport(PanicWriter {
            entered: entered_tx,
            release: release_rx,
        });
        let process = test_process(false);

        let owner_input = input.clone();
        let owner_process = process.clone();
        let owner =
            std::thread::spawn(move || send_terminal_input(owner_process, owner_input, b"first"));
        entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(
            send_terminal_input(process.clone(), input.clone(), b"second"),
            TerminalInputOutcome::Queued
        );
        release_tx.send(()).unwrap();
        assert!(owner.join().is_err());
        let bytes_before = input.pending.lock().unwrap().bytes;

        assert_eq!(
            send_terminal_input(process, input.clone(), b"third"),
            TerminalInputOutcome::SurfaceUnavailable
        );
        assert_eq!(input.pending.lock().unwrap().bytes, bytes_before);
    }

    #[test]
    fn process_exit_precedes_writer_poison_classification() {
        let input = test_transport(CapturingWriter(Arc::new(Mutex::new(Vec::new()))));
        let poison_input = input.clone();
        assert!(std::thread::spawn(move || {
            let _guard = poison_input.writer.lock().unwrap();
            panic!("poison writer for precedence contract");
        })
        .join()
        .is_err());

        assert_eq!(
            send_terminal_input(test_process(true), input, b"input"),
            TerminalInputOutcome::ProcessExited
        );
    }

    #[test]
    fn process_query_race_cannot_enqueue_after_writer_poison() {
        let (writer_entered_tx, writer_entered_rx) = mpsc::channel();
        let (writer_release_tx, writer_release_rx) = mpsc::channel();
        let input = test_transport(PanicWriter {
            entered: writer_entered_tx,
            release: writer_release_rx,
        });
        let (query_entered_tx, query_entered_rx) = mpsc::channel();
        let (query_release_tx, query_release_rx) = mpsc::channel();
        let process: Arc<Mutex<Box<dyn TerminalProcess>>> =
            Arc::new(Mutex::new(Box::new(BlockingNthWaitProcess {
                calls: 0,
                block_on: 3,
                entered: query_entered_tx,
                release: query_release_rx,
            })));

        let owner_input = input.clone();
        let owner_process = process.clone();
        let owner =
            std::thread::spawn(move || send_terminal_input(owner_process, owner_input, b"first"));
        writer_entered_rx
            .recv_timeout(Duration::from_secs(2))
            .unwrap();
        assert_eq!(
            send_terminal_input(process.clone(), input.clone(), b"retained"),
            TerminalInputOutcome::Queued
        );

        let racer_input = input.clone();
        let racer_process = process.clone();
        let racer =
            std::thread::spawn(move || send_terminal_input(racer_process, racer_input, b"racer"));
        query_entered_rx
            .recv_timeout(Duration::from_secs(2))
            .unwrap();
        writer_release_tx.send(()).unwrap();
        assert!(owner.join().is_err());
        query_release_tx.send(()).unwrap();

        assert_eq!(
            racer.join().unwrap(),
            TerminalInputOutcome::SurfaceUnavailable
        );
        let pending = input.pending.lock().unwrap();
        assert_eq!(pending.bytes, b"retained".len());
        assert_eq!(pending.entries.len(), 1);
    }

    #[test]
    fn empty_compatibility_writes_succeed_before_target_resolution() {
        let state = TerminalState::default();

        assert_eq!(
            super::terminal_write_id_for_control(&state, 404, b""),
            Ok(())
        );
        assert_eq!(super::terminal_write_panel(&state, "missing", ""), Ok(()));
    }

    #[test]
    fn live_input_classifies_exit_and_writer_failures_exactly() {
        assert_eq!(
            send_terminal_input(
                test_process(true),
                test_transport(CapturingWriter(Arc::new(Mutex::new(Vec::new())))),
                b"input",
            ),
            TerminalInputOutcome::ProcessExited
        );
        assert_eq!(
            send_terminal_input(
                test_process(false),
                test_transport(ErrorWriter(io::ErrorKind::BrokenPipe)),
                b"input",
            ),
            TerminalInputOutcome::ProcessExited
        );
        assert_eq!(
            send_terminal_input(
                Arc::new(Mutex::new(Box::new(TestProcess {
                    wait: Err("injected process query failure".to_string()),
                }))),
                test_transport(CapturingWriter(Arc::new(Mutex::new(Vec::new())))),
                b"input",
            ),
            TerminalInputOutcome::SurfaceUnavailable
        );
        assert_eq!(
            send_terminal_input(
                test_process(false),
                test_transport(ErrorWriter(io::ErrorKind::Other)),
                b"input",
            ),
            TerminalInputOutcome::SurfaceUnavailable
        );
    }

    #[test]
    fn queued_compatibility_adapters_accept_without_retry() {
        let state = Arc::new(TerminalState::default());
        let captured = Arc::new(Mutex::new(Vec::new()));
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        state.runtime_registry().sessions.insert(
            7,
            test_session(
                test_process(false),
                test_transport(BlockingWriter {
                    entered: entered_tx,
                    release: release_rx,
                    captured: captured.clone(),
                    block_once: true,
                }),
                "panel",
            ),
        );

        let owner_state = state.clone();
        let owner = std::thread::spawn(move || {
            super::terminal_send_panel_bytes_for_control(&owner_state, "panel", b"first")
        });
        entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(
            super::terminal_write_panel(&state, "panel", "panel"),
            Ok(())
        );
        assert_eq!(
            super::terminal_write_id_for_control(&state, 7, b"id"),
            Ok(())
        );
        release_tx.send(()).unwrap();
        assert_eq!(owner.join().unwrap(), TerminalInputOutcome::Sent);
        assert_eq!(&*captured.lock().unwrap(), b"firstpanelid");
    }

    #[test]
    fn live_materialization_events_remain_ordered_after_input_queues() {
        let state = Arc::new(TerminalState::default());
        let captured = Arc::new(Mutex::new(Vec::new()));
        let emitted = Arc::new(Mutex::new(Vec::new()));
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        state.runtime_registry().sessions.insert(
            7,
            test_session(
                test_process(false),
                test_transport(BlockingWriter {
                    entered: entered_tx,
                    release: release_rx,
                    captured: captured.clone(),
                    block_once: true,
                }),
                "panel",
            ),
        );

        let owner_state = state.clone();
        let owner = std::thread::spawn(move || {
            super::terminal_send_panel_bytes_for_control(&owner_state, "panel", b"owner")
        });
        entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();

        let emitted_for_call = emitted.clone();
        assert_eq!(
            super::terminal_apply_materialization_events_with(
                &state,
                "panel",
                vec![
                    super::TerminalMaterializationEvent::Input(b"first".to_vec()),
                    super::TerminalMaterializationEvent::ProcessOutput(b"output".to_vec()),
                    super::TerminalMaterializationEvent::Input(b"second".to_vec()),
                ],
                move |_id, bytes, _titles| {
                    emitted_for_call.lock().unwrap().extend_from_slice(bytes);
                    Ok(())
                },
            ),
            TerminalInputOutcome::Queued
        );
        assert_eq!(&*emitted.lock().unwrap(), b"output");

        release_tx.send(()).unwrap();
        assert_eq!(owner.join().unwrap(), TerminalInputOutcome::Sent);
        assert_eq!(&*captured.lock().unwrap(), b"ownerfirstsecond");
    }

    #[test]
    fn control_input_navigation_tracks_application_cursor_mode() {
        let mut grid = TerminalGrid::new(GridSize::new(80, 24));
        assert_eq!(
            super::terminal_input_bytes_for_grid(&grid, b"\x1b[A\x1bOA\x1b[5~"),
            b"\x1b[A\x1bOA\x1b[5~"
        );

        grid.advance(b"\x1b[?1h");
        assert_eq!(
            super::terminal_input_bytes_for_grid(&grid, b"\x1b[A\x1bOB\x1b[5~\xc3\xa9"),
            b"\x1bOA\x1bOB\x1b[5~\xc3\xa9"
        );
    }

    #[test]
    fn live_only_terminal_input_never_starts_a_cold_runtime() {
        let state = TerminalState::default();
        let events = vec![super::TerminalMaterializationEvent::Input(
            b"remote".to_vec(),
        )];
        let oversized = vec![super::TerminalMaterializationEvent::Input(vec![
            b'x';
            TERMINAL_PENDING_INPUT_LIMIT
                + 1
        ])];
        assert_eq!(
            super::request_live_terminal_input(&state, "remote", events.clone()),
            super::TerminalMaterializationDemand::SurfaceUnavailable
        );
        assert_eq!(
            super::request_live_terminal_input(&state, "remote", oversized.clone()),
            super::TerminalMaterializationDemand::SurfaceUnavailable
        );

        state.runtime_registry().sessions.insert(
            7,
            test_session(
                test_process(false),
                test_transport(CapturingWriter(Arc::new(Mutex::new(Vec::new())))),
                "remote",
            ),
        );
        assert_eq!(
            super::request_live_terminal_input(&state, "remote", events.clone()),
            super::TerminalMaterializationDemand::Live(events)
        );
        assert_eq!(
            super::request_live_terminal_input(&state, "remote", oversized.clone()),
            super::TerminalMaterializationDemand::Live(oversized)
        );
        assert!(state.runtime_registry().materializations.is_empty());
    }

    #[test]
    fn oversized_live_only_input_preserves_process_exit_classification() {
        let state = TerminalState::default();
        state.runtime_registry().sessions.insert(
            7,
            test_session(
                test_process(true),
                test_transport(CapturingWriter(Arc::new(Mutex::new(Vec::new())))),
                "remote",
            ),
        );
        let events = vec![super::TerminalMaterializationEvent::Input(vec![
            b'x';
            TERMINAL_PENDING_INPUT_LIMIT
                + 1
        ])];
        let demand = super::request_live_terminal_input(&state, "remote", events);
        let super::TerminalMaterializationDemand::Live(events) = demand else {
            panic!("exited live runtime must classify through its process state");
        };
        assert_eq!(
            super::terminal_apply_materialization_events_with(
                &state,
                "remote",
                events,
                |_id, _bytes, _titles| Ok(()),
            ),
            TerminalInputOutcome::ProcessExited
        );
    }

    #[test]
    fn poisoned_registry_returns_classified_input_failure() {
        let state = Arc::new(TerminalState::default());
        let poison_state = state.clone();
        assert!(std::thread::spawn(move || {
            let _guard = poison_state.registry.lock().unwrap();
            panic!("poison terminal registry for input contract");
        })
        .join()
        .is_err());

        assert_eq!(
            super::terminal_send_panel_bytes_for_control(&state, "panel", b"input"),
            TerminalInputOutcome::SurfaceUnavailable
        );
        assert!(super::terminal_write_id_for_control(&state, 1, b"input").is_err());
    }

    #[test]
    fn base64_matches_rfc_test_vectors() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foob"), "Zm9vYg==");
        assert_eq!(base64_encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn base64_round_trips_all_byte_values() {
        // Encode every byte value and confirm padding/length invariants hold for
        // a chunk that is not a multiple of three.
        let bytes: Vec<u8> = (0u8..=255).collect();
        let encoded = base64_encode(&bytes);
        assert_eq!(encoded.len(), bytes.len().div_ceil(3) * 4);
        assert!(encoded.is_ascii());
        // 256 bytes -> 256 % 3 == 1 -> exactly two '=' pad chars at the end.
        assert!(encoded.ends_with("=="));
        assert_eq!(encoded.matches('=').count(), 2);
    }

    #[test]
    fn base64_encodes_high_bytes_without_panicking() {
        // Non-UTF-8 bytes must encode fine — this is exactly why the output
        // bridge is base64 rather than a UTF-8 string.
        assert_eq!(base64_encode(&[0xff, 0xfe, 0xfd]), "//79");
        assert_eq!(base64_encode(&[0x00]), "AA==");
    }

    #[test]
    fn terminal_text_reads_viewport_scrollback_and_line_tail() {
        let mut grid = TerminalGrid::new(GridSize::new(20, 2));
        grid.advance(b"one\r\ntwo\r\nthree");

        assert_eq!(terminal_text(&grid, false, None), "two\nthree");
        assert_eq!(terminal_text(&grid, true, None), "one\ntwo\nthree");
        assert_eq!(terminal_text(&grid, true, Some(2)), "two\nthree");
    }

    #[test]
    fn default_shell_command_carries_a_working_directory_when_provided() {
        let command = default_shell_command(Some("C:/repo"), None, None);
        assert_eq!(
            command.cwd.as_deref(),
            Some(std::path::Path::new("C:/repo"))
        );
    }

    #[test]
    fn default_shell_command_ignores_an_empty_working_directory() {
        let command = default_shell_command(Some(""), None, None);
        assert_eq!(command.cwd, None);
    }

    #[test]
    fn default_shell_command_applies_startup_command_and_environment() {
        let command = default_shell_command(
            Some("C:/repo"),
            Some("echo ready"),
            Some(BTreeMap::from([("CMUX_FORK".to_string(), "1".to_string())])),
        );
        assert_eq!(
            command.cwd.as_deref(),
            Some(std::path::Path::new("C:/repo"))
        );
        assert_eq!(command.env.get("CMUX_FORK").map(String::as_str), Some("1"));
        if cfg!(windows) {
            assert_eq!(command.program, "powershell.exe");
            assert!(command.args.iter().any(|arg| arg == "-NoExit"));
            assert_eq!(command.args.last().map(String::as_str), Some("echo ready"));
        } else {
            assert_eq!(command.program, "/bin/bash");
            assert_eq!(command.args, vec!["-l", "-c", "echo ready"]);
        }
    }

    #[test]
    fn terminal_title_parser_extracts_bel_terminated_window_titles() {
        let mut parser = TerminalTitleParser::default();

        assert_eq!(
            parser.consume(b"\x1b]0;Claude Code loading\x07"),
            vec!["Claude Code loading"]
        );
        assert_eq!(
            parser.consume(b"\x1b]1;icon only\x07"),
            Vec::<String>::new()
        );
    }

    #[test]
    fn terminal_title_parser_extracts_st_terminated_titles_across_chunks() {
        let mut parser = TerminalTitleParser::default();

        assert!(parser.consume(b"prefix\x1b]2;cargo ").is_empty());
        assert_eq!(parser.consume(b"test\x1b\\suffix"), vec!["cargo test"]);
    }

    #[test]
    fn terminal_title_parser_ignores_shell_integration_osc_sequences() {
        let mut parser = TerminalTitleParser::default();

        assert_eq!(
            parser.consume(b"\x1b]133;A\x07\x1b]2;pwsh\x07"),
            vec!["pwsh"]
        );
    }

    #[test]
    fn descendant_pid_set_includes_root_and_nested_children() {
        let pids = descendant_pid_set(
            10,
            &[(10, 1), (11, 10), (12, 11), (20, 1), (21, 20), (13, 10)],
        );
        assert_eq!(pids, [10, 11, 12, 13].into_iter().collect());
    }

    #[test]
    fn terminal_runtime_snapshot_reports_process_tree_and_foreground_leaf() {
        let snapshot = terminal_runtime_snapshot_from_processes(
            7,
            Some("surface-1".to_string()),
            Some(10),
            &Ok(vec![
                ProcessSnapshotEntry {
                    pid: 10,
                    parent_pid: 1,
                    name: Some("powershell.exe".to_string()),
                },
                ProcessSnapshotEntry {
                    pid: 11,
                    parent_pid: 10,
                    name: Some("node.exe".to_string()),
                },
                ProcessSnapshotEntry {
                    pid: 12,
                    parent_pid: 11,
                    name: Some("vite.exe".to_string()),
                },
                ProcessSnapshotEntry {
                    pid: 20,
                    parent_pid: 1,
                    name: Some("other.exe".to_string()),
                },
            ]),
        );

        assert_eq!(snapshot.id, 7);
        assert_eq!(snapshot.panel_id.as_deref(), Some("surface-1"));
        assert_eq!(snapshot.root_pid, Some(10));
        assert_eq!(snapshot.descendant_pids, vec![10, 11, 12]);
        assert_eq!(snapshot.child_pids, vec![11]);
        assert_eq!(snapshot.process_count, 3);
        assert_eq!(snapshot.foreground_pid, Some(12));
        assert_eq!(
            snapshot.foreground_process_name.as_deref(),
            Some("vite.exe")
        );
        assert_eq!(
            snapshot.foreground_process_source,
            "pid_tree_leaf_approximation"
        );
        assert_eq!(snapshot.process_error, None);
    }

    #[test]
    fn terminal_runtime_snapshot_falls_back_to_root_on_process_scan_error() {
        let snapshot = terminal_runtime_snapshot_from_processes(
            3,
            Some("surface-2".to_string()),
            Some(99),
            &Err("snapshot failed".to_string()),
        );

        assert_eq!(snapshot.root_pid, Some(99));
        assert_eq!(snapshot.descendant_pids, vec![99]);
        assert_eq!(snapshot.foreground_pid, Some(99));
        assert_eq!(snapshot.foreground_process_source, "root_process");
        assert_eq!(snapshot.process_error.as_deref(), Some("snapshot failed"));
    }

    #[test]
    fn ports_for_pid_set_sorts_and_deduplicates_matching_listener_ports() {
        let pids = [10, 11].into_iter().collect();
        assert_eq!(
            ports_for_pid_set(&pids, &[(10, 5173), (20, 9000), (11, 3000), (10, 3000)]),
            vec![3000, 5173]
        );
    }

    #[test]
    fn tcp_port_from_owner_pid_row_decodes_network_byte_order() {
        assert_eq!(
            tcp_port_from_owner_pid_row(u16::to_be(5173) as u32),
            Some(5173)
        );
        assert_eq!(tcp_port_from_owner_pid_row(0), None);
    }
