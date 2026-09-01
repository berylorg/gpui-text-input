use super::*;

#[gpui::test]
fn candidate_drop_and_repeated_sessions_leave_no_cross_session_state(cx: &mut TestAppContext) {
    let source = "repeat\nrepeat\nrepeat";
    let seed = seed(source, 2, 7);
    let window = cx.add_empty_window();
    window.update(|window, _| {
        let (environment, cleanup) =
            make_environment(16, config(source, 2, 32), window.text_system());
        let mut first = RangePrepublicationSession::new(seed, environment.clone()).unwrap();
        let first_generation = first.generation();
        let (candidate, _, _) = drive(&mut first, source, window.text_system(), &cleanup);
        assert!(candidate.retained_charge().bytes > 0);
        drop(candidate);
        let release = drain_cleanup(&cleanup);
        assert!(release.iter().any(|effect| matches!(
            effect,
            RangePrepublicationCleanupEffect::ReleaseCandidate { .. }
        )));
        assert!(
            release.iter().any(|effect| matches!(
                effect,
                RangePrepublicationCleanupEffect::ReleasePage { .. }
            ))
        );
        assert_eq!(first.ownership().candidate, false);

        let mut second = RangePrepublicationSession::new(seed, environment.clone()).unwrap();
        assert_ne!(second.generation(), first_generation);
        let (candidate, _, _) = drive(&mut second, source, window.text_system(), &cleanup);
        drop(candidate);
        let _ = drain_cleanup(&cleanup);
        assert_eq!(second.ownership().candidate, false);
    });
}

#[gpui::test]
fn multi_mib_source_keeps_detached_residency_bounded(cx: &mut TestAppContext) {
    let line = "abcdefghijklmno\n";
    let source = line.repeat((2 * 1024 * 1024) / line.len());
    let offset = (source.len() / 2) as u64;
    let seed = seed(&source, 9, offset);
    let window = cx.add_empty_window();
    let (environment, candidate, steps, max_bytes) = window.update(|window, _| {
        let (environment, cleanup) =
            make_environment(17, config(&source, 9, 4096), window.text_system());
        let mut session = RangePrepublicationSession::new(seed, environment.clone()).unwrap();
        let (candidate, steps, max_bytes) =
            drive(&mut session, &source, window.text_system(), &cleanup);
        (environment, candidate, steps, max_bytes)
    });
    assert!(steps > 100);
    assert!(max_bytes <= environment.config().limits.max_surface_bytes);
    assert!(candidate.retained_charge().bytes <= environment.config().limits.max_surface_bytes);
}

#[gpui::test]
fn invalid_seed_invariants_are_rejected_before_cleanup_or_work(cx: &mut TestAppContext) {
    let source = "seed";
    let window = cx.add_empty_window();
    window.update(|window, _| {
        let (environment, cleanup) =
            make_environment(21, config(source, 1, 32), window.text_system());
        let valid = seed(source, 1, 1);

        let mut wrong_history = valid;
        wrong_history.history.as_mut().unwrap().binding = binding(source, 2);
        assert_eq!(
            RangePrepublicationSession::new(wrong_history, environment.clone())
                .err()
                .unwrap(),
            RangePrepublicationFailure::SourceMismatch
        );

        let mut wrong_caret = valid;
        wrong_caret.caret = position(0);
        assert_eq!(
            RangePrepublicationSession::new(wrong_caret, environment.clone())
                .err()
                .unwrap(),
            RangePrepublicationFailure::SourceMismatch
        );

        let neighbor = InlineObjectNeighbor::new(InlineObjectId::new(1), InlineObjectOrder::new(1));
        let object_position =
            SourcePosition::new(ByteOffset::new(1), InlineObjectGap::before(neighbor));
        let mut incompatible_selection = valid;
        incompatible_selection.caret = object_position;
        incompatible_selection.selection = RangeSourceSelection {
            anchor: position(1),
            head: object_position,
        };
        assert_eq!(
            RangePrepublicationSession::new(incompatible_selection, environment.clone())
                .err()
                .unwrap(),
            RangePrepublicationFailure::SourceMismatch
        );

        let mut excessive_scroll = valid;
        excessive_scroll.scroll.intra_anchor = px(17.);
        assert_eq!(
            RangePrepublicationSession::new(excessive_scroll, environment)
                .err()
                .unwrap(),
            RangePrepublicationFailure::SourceMismatch
        );
        assert_eq!(cleanup.ownership().active, 0);
    });
}

#[gpui::test]
fn candidate_rejects_cross_window_adoption_and_releases_ledger_record(cx: &mut TestAppContext) {
    let source = "same window";
    let seed = seed(source, 2, 2);
    let first = cx.add_empty_window();
    let (environment, cleanup, candidate) = first.update(|window, _| {
        let (environment, cleanup) =
            make_environment(22, config(source, 2, 32), window.text_system());
        let mut session = RangePrepublicationSession::new(seed, environment.clone()).unwrap();
        let (candidate, _, _) = drive(&mut session, source, window.text_system(), &cleanup);
        (environment, cleanup, candidate)
    });
    let current = RangePrepublicationCurrent {
        binding: seed.binding,
        history: seed.history,
        available_capacity: candidate.adoption_peak(),
    };
    let observed = Rc::new(Cell::new(None));
    let observed_from_view = observed.clone();
    let fallback = environment.config().clone();
    let _ = cx.add_window_view(move |window, cx| {
        match RangeTextInput::new_with_prepublication(&environment, candidate, current, window, cx)
        {
            Ok(input) => input,
            Err(error) => {
                observed_from_view.set(Some(error));
                RangeTextInput::new(fallback, window, cx).unwrap()
            }
        }
    });
    assert_eq!(
        observed.get(),
        Some(RangePrepublicationAdoptionError::EnvironmentMismatch)
    );
    let release = drain_cleanup(&cleanup);
    assert!(release.iter().any(|effect| matches!(
        effect,
        RangePrepublicationCleanupEffect::ReleaseCandidate { .. }
    )));
}

#[gpui::test]
fn session_drop_is_cleanup_ready_and_slot_reuse_waits_for_ack(cx: &mut TestAppContext) {
    let source = "cleanup";
    let seed = seed(source, 3, 0);
    let window = cx.add_empty_window();
    window.update(|window, _| {
        let cleanup = RangePrepublicationCleanupLedger::new(window.text_system(), 1).unwrap();
        let environment = RangePrepublicationEnvironment::new(
            23,
            config(source, 3, 32),
            window.text_system(),
            cleanup.clone(),
        )
        .unwrap();
        let mut first = RangePrepublicationSession::new(seed, environment.clone()).unwrap();
        assert_eq!(first.service(window.text_system()).effects.len(), 1);
        drop(first);

        let mut second = RangePrepublicationSession::new(seed, environment).unwrap();
        let blocked = second.service(window.text_system());
        assert_eq!(blocked.status, RangePrepublicationStatus::CapacityBlocked);
        assert!(blocked.effects.is_empty());

        let cleanup_step = cleanup.service(1);
        assert!(matches!(
            cleanup_step.effects.as_slice(),
            [RangePrepublicationCleanupEffect::CancelValidation { .. }]
        ));
        assert_eq!(cleanup.ownership().awaiting_acknowledgement, 1);
        cleanup.acknowledge(cleanup_step.effects[0].token());
        assert_eq!(second.service(window.text_system()).effects.len(), 1);
    });
}

#[gpui::test]
fn malformed_exact_key_response_terminates_and_releases_custody(cx: &mut TestAppContext) {
    let source = "é";
    let seed = seed(source, 4, 1);
    let window = cx.add_empty_window();
    window.update(|window, _| {
        let (environment, cleanup) =
            make_environment(24, config(source, 4, 32), window.text_system());
        let mut session = RangePrepublicationSession::new(seed, environment).unwrap();
        let validation = session.service(window.text_system());
        let RangePrepublicationEffect::ValidateOwner(request) = validation.effects[0] else {
            panic!("validation expected")
        };
        session.deliver_validation(RangePrepublicationValidationResponse {
            key: request.key,
            binding: request.binding,
            history: request.history,
            current: true,
        });
        let page_step = session.service(window.text_system());
        let (generation, request) = page_step
            .effects
            .into_iter()
            .find_map(|effect| match effect {
                RangePrepublicationEffect::Page {
                    generation,
                    request,
                    ..
                } => Some((generation, request)),
                _ => None,
            })
            .unwrap();
        let malformed = page_for(source, 77, request);
        assert_eq!(
            session.deliver_page(generation, malformed),
            RangePrepublicationDelivery::Accepted
        );
        assert_eq!(
            session.service(window.text_system()).status,
            RangePrepublicationStatus::Failed(RangePrepublicationFailure::MalformedResponse)
        );
        let release = cleanup.service(1);
        assert!(matches!(
            release.effects.as_slice(),
            [RangePrepublicationCleanupEffect::ReleasePage { .. }]
        ));
        cleanup.acknowledge(release.effects[0].token());
    });
}

#[gpui::test]
fn retained_same_key_delivery_collides_and_releases_exactly_once(cx: &mut TestAppContext) {
    let source = "retained collision";
    let restoration = seed(source, 13, 3);
    let window = cx.add_empty_window();
    window.update(|window, _| {
        let (environment, cleanup) =
            make_environment(45, config(source, 13, 32), window.text_system());
        let mut session = RangePrepublicationSession::new(restoration, environment).unwrap();
        let validation = session.service(window.text_system());
        let RangePrepublicationEffect::ValidateOwner(request) = validation.effects[0] else {
            panic!("validation expected")
        };
        assert_eq!(
            session.deliver_validation(RangePrepublicationValidationResponse {
                key: request.key,
                binding: request.binding,
                history: request.history,
                current: true,
            }),
            RangePrepublicationDelivery::Accepted
        );
        let page_step = session.service(window.text_system());
        let (generation, request) = page_step
            .effects
            .into_iter()
            .find_map(|effect| match effect {
                RangePrepublicationEffect::Page {
                    generation,
                    request,
                    ..
                } => Some((generation, request)),
                _ => None,
            })
            .expect("page request expected");
        session.set_available_capacity(RangeSurfaceCharge { bytes: 1, items: 1 });
        assert_eq!(
            session.deliver_page(generation, page_for(source, 81_001, request)),
            RangePrepublicationDelivery::CapacityBlocked
        );
        assert_eq!(
            session.deliver_page(generation, page_for(source, 81_002, request)),
            RangePrepublicationDelivery::Terminal(RangePrepublicationFailure::ExactKeyCollision)
        );
        assert_eq!(
            session.status(),
            RangePrepublicationStatus::Failed(RangePrepublicationFailure::ExactKeyCollision)
        );
        let release = cleanup.service(16);
        assert!(matches!(
            release.effects.as_slice(),
            [RangePrepublicationCleanupEffect::ReleasePage { .. }]
        ));
        assert_eq!(
            cleanup.acknowledge(release.effects[0].token()),
            gpui_text_input::RangePrepublicationCleanupAcknowledgement::Accepted
        );
        assert!(cleanup.service(16).effects.is_empty());
        assert_eq!(cleanup.ownership().active, 0);
    });
}

#[gpui::test]
fn duplicate_delivery_during_release_draining_has_no_second_followup(cx: &mut TestAppContext) {
    let source = "draining duplicate";
    let restoration = seed(source, 14, 2);
    let window = cx.add_empty_window();
    window.update(|window, _| {
        let (environment, cleanup) =
            make_environment(46, config(source, 14, 32), window.text_system());
        let mut session = RangePrepublicationSession::new(restoration, environment).unwrap();
        let validation = session.service(window.text_system());
        let RangePrepublicationEffect::ValidateOwner(request) = validation.effects[0] else {
            panic!("validation expected")
        };
        session.deliver_validation(RangePrepublicationValidationResponse {
            key: request.key,
            binding: request.binding,
            history: request.history,
            current: true,
        });
        let page_step = session.service(window.text_system());
        let (generation, request) = page_step
            .effects
            .into_iter()
            .find_map(|effect| match effect {
                RangePrepublicationEffect::Page {
                    generation,
                    request,
                    ..
                } => Some((generation, request)),
                _ => None,
            })
            .expect("page request expected");
        session.cancel();
        let cancel = cleanup.service(1);
        assert!(matches!(
            cancel.effects.as_slice(),
            [RangePrepublicationCleanupEffect::CancelPage { .. }]
        ));
        assert_eq!(
            session.deliver_page(generation, page_for(source, 82_001, request)),
            RangePrepublicationDelivery::Obsolete
        );
        cleanup.acknowledge(cancel.effects[0].token());

        let release = cleanup.service(1);
        assert!(matches!(
            release.effects.as_slice(),
            [RangePrepublicationCleanupEffect::ReleasePage { .. }]
        ));
        assert_eq!(
            session.deliver_page(generation, page_for(source, 82_002, request)),
            RangePrepublicationDelivery::Obsolete
        );
        cleanup.acknowledge(release.effects[0].token());
        assert!(cleanup.service(16).effects.is_empty());
        assert_eq!(cleanup.ownership().active, 0);
        assert_eq!(session.status(), RangePrepublicationStatus::Cancelled);
    });
}

#[gpui::test]
fn object_gap_candidate_and_adoption_accept_exact_fit_and_reject_one_under(
    cx: &mut TestAppContext,
) {
    let source = "ab";
    let fact = object_fact(9, 1, 1);
    let neighbor = InlineObjectNeighbor::new(fact.id(), fact.order());
    let object_position =
        SourcePosition::new(ByteOffset::new(1), InlineObjectGap::before(neighbor));
    let mut restoration = seed(source, 5, 1);
    restoration.caret = object_position;
    restoration.selection = RangeSourceSelection::caret(object_position);
    restoration.scroll.position = object_position;
    let facts = vec![fact];
    let window = cx.add_empty_window();

    let (required, configured_required) = window.update(|window, cx| {
        let fresh = cx.new(|cx| RangeTextInput::new(config("", 5, 32), window, cx).unwrap());
        let diagnostics = fresh.read(cx).realization_diagnostics();
        let fresh = diagnostics.current;
        let support = RangeSurfaceCharge {
            bytes: fresh.owned_bytes - fresh.geometry_bytes - diagnostics.surface_charge.bytes,
            items: fresh.owned_items - fresh.geometry_items - diagnostics.surface_charge.items,
        };
        let (environment, cleanup) =
            make_environment(25, config(source, 5, 32), window.text_system());
        let mut session = RangePrepublicationSession::new(restoration, environment).unwrap();
        let (candidate, _, _) =
            drive_with_objects(&mut session, source, window.text_system(), &cleanup, &facts);
        let candidate_charge = candidate.retained_charge();
        let origin = session.ownership();
        let session_peak = session.high_water();
        drop(candidate);
        let _ = drain_cleanup(&cleanup);
        let required = RangeSurfaceCharge {
            bytes: origin.bytes + candidate_charge.bytes + support.bytes,
            items: origin.items + candidate_charge.items + support.items,
        };
        (
            required,
            RangeSurfaceCharge {
                bytes: required.bytes.max(session_peak.bytes),
                items: required.items.max(session_peak.items),
            },
        )
    });

    let (environment, cleanup, candidate) = window.update(|window, _| {
        let (environment, cleanup) =
            make_environment(26, config(source, 5, 32), window.text_system());
        let mut session =
            RangePrepublicationSession::new(restoration, environment.clone()).unwrap();
        let (candidate, _, _) =
            drive_with_objects(&mut session, source, window.text_system(), &cleanup, &facts);
        (environment, cleanup, candidate)
    });

    let current_one_under = RangePrepublicationCurrent {
        binding: restoration.binding,
        history: restoration.history,
        available_capacity: RangeSurfaceCharge {
            bytes: required.bytes - 1,
            items: required.items,
        },
    };
    let rejected = Rc::new(Cell::new(None));
    let rejected_from_view = rejected.clone();
    let fallback = environment.config().clone();
    let _ = window.update(|window, cx| {
        cx.new(|cx| {
            match RangeTextInput::new_with_prepublication(
                &environment,
                candidate,
                current_one_under,
                window,
                cx,
            ) {
                Ok(input) => input,
                Err(error) => {
                    rejected_from_view.set(Some(error));
                    RangeTextInput::new(fallback, window, cx).unwrap()
                }
            }
        })
    });
    assert_eq!(
        rejected.get(),
        Some(RangePrepublicationAdoptionError::CapacityMismatch)
    );
    let release = drain_cleanup(&cleanup);
    assert!(release.iter().any(|effect| matches!(
        effect,
        RangePrepublicationCleanupEffect::ReleaseCandidate { .. }
    )));

    let candidate = window.update(|window, _| {
        let mut session =
            RangePrepublicationSession::new(restoration, environment.clone()).unwrap();
        drive_with_objects(&mut session, source, window.text_system(), &cleanup, &facts).0
    });
    let current_item_one_under = RangePrepublicationCurrent {
        binding: restoration.binding,
        history: restoration.history,
        available_capacity: RangeSurfaceCharge {
            bytes: required.bytes,
            items: required.items - 1,
        },
    };
    let rejected = Rc::new(Cell::new(None));
    let rejected_from_view = rejected.clone();
    let fallback = environment.config().clone();
    let _ = window.update(|window, cx| {
        cx.new(|cx| {
            match RangeTextInput::new_with_prepublication(
                &environment,
                candidate,
                current_item_one_under,
                window,
                cx,
            ) {
                Ok(input) => input,
                Err(error) => {
                    rejected_from_view.set(Some(error));
                    RangeTextInput::new(fallback, window, cx).unwrap()
                }
            }
        })
    });
    assert_eq!(
        rejected.get(),
        Some(RangePrepublicationAdoptionError::CapacityMismatch)
    );
    let release = drain_cleanup(&cleanup);
    assert!(release.iter().any(|effect| matches!(
        effect,
        RangePrepublicationCleanupEffect::ReleaseCandidate { .. }
    )));

    let input = window.update(|window, cx| {
        let mut session =
            RangePrepublicationSession::new(restoration, environment.clone()).unwrap();
        let (candidate, _, _) =
            drive_with_objects(&mut session, source, window.text_system(), &cleanup, &facts);
        let current = RangePrepublicationCurrent {
            binding: restoration.binding,
            history: restoration.history,
            available_capacity: required,
        };
        cx.new(|cx| {
            RangeTextInput::new_with_prepublication(&environment, candidate, current, window, cx)
                .unwrap()
        })
    });
    window.update(|_, cx| {
        let input = input.read(cx);
        let surface = input.surface().unwrap();
        assert_eq!(surface.selection(), restoration.selection);
        assert_eq!(surface.scroll_position(), object_position);
        assert_eq!(surface.realized_objects().len(), 1);
    });

    window.update(|window, _| {
        let mut one_under = config(source, 5, 32);
        one_under.limits.max_surface_bytes = configured_required.bytes - 1;
        one_under.limits.max_surface_items = configured_required.items;
        let (environment, cleanup) = make_environment(27, one_under, window.text_system());
        let mut session = RangePrepublicationSession::new(restoration, environment).unwrap();
        let (failure, effects) = drive_failure_with_objects(
            &mut session,
            source,
            window.text_system(),
            &cleanup,
            &facts,
        );
        assert_eq!(failure, RangePrepublicationFailure::TerminalCapacity);
        assert!(effects.iter().any(|effect| matches!(
            effect,
            RangePrepublicationCleanupEffect::ReleaseObjectPage { .. }
        )));
    });

    window.update(|window, _| {
        let mut one_under = config(source, 5, 32);
        one_under.limits.max_surface_bytes = configured_required.bytes;
        one_under.limits.max_surface_items = configured_required.items - 1;
        let (environment, cleanup) = make_environment(28, one_under, window.text_system());
        let mut session = RangePrepublicationSession::new(restoration, environment).unwrap();
        let (failure, effects) = drive_failure_with_objects(
            &mut session,
            source,
            window.text_system(),
            &cleanup,
            &facts,
        );
        assert_eq!(failure, RangePrepublicationFailure::TerminalCapacity);
        assert!(effects.iter().any(|effect| matches!(
            effect,
            RangePrepublicationCleanupEffect::ReleaseObjectPage { .. }
        )));
    });
}
