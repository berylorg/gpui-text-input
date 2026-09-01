use super::*;

#[gpui::test]
fn bounded_multi_step_candidate_adopts_into_fresh_widget(cx: &mut TestAppContext) {
    let source = (0..256)
        .map(|index| format!("line {index}\n"))
        .collect::<String>();
    let seed = seed(&source, 5, 384);
    let observed_steps = Rc::new(Cell::new(0));
    let steps_from_view = observed_steps.clone();
    let (input, cx) = cx.add_window_view(move |window, cx| {
        let (environment, cleanup) =
            make_environment(11, config(&source, 5, 64), window.text_system());
        let mut session = RangePrepublicationSession::new(seed, environment.clone()).unwrap();
        let (candidate, steps, _) = drive(&mut session, &source, window.text_system(), &cleanup);
        steps_from_view.set(steps);
        let current = RangePrepublicationCurrent {
            binding: seed.binding,
            history: seed.history,
            available_capacity: candidate.adoption_peak(),
        };
        RangeTextInput::new_with_prepublication(&environment, candidate, current, window, cx)
            .unwrap()
    });
    assert!(observed_steps.get() > 8);
    cx.update(|window, app| window.draw(app).clear());
    cx.run_until_parked();
    input.read_with(cx, |input, _| {
        let surface = input.surface().unwrap();
        assert_eq!(surface.binding(), seed.binding);
        assert_eq!(surface.selection(), seed.selection);
        assert!(!surface.fragments().is_empty());
        assert_eq!(input.history_frontier(), seed.history.unwrap());
    });
}

#[gpui::test]
fn exact_validation_and_adoption_mismatch_are_terminal(cx: &mut TestAppContext) {
    let source = "alpha\nbeta\ngamma";
    let seed = seed(source, 8, 6);
    let window = cx.add_empty_window();
    let (environment, cleanup) = window
        .update(|window, _| make_environment(12, config(source, 8, 32), window.text_system()));
    let mut stale = RangePrepublicationSession::new(seed, environment.clone()).unwrap();
    window.update(|window, _| {
        let step = stale.service(window.text_system());
        let RangePrepublicationEffect::ValidateOwner(request) = &step.effects[0] else {
            panic!("validation expected")
        };
        assert_eq!(
            stale.deliver_validation(RangePrepublicationValidationResponse {
                key: request.key,
                binding: request.binding,
                history: request.history,
                current: false,
            }),
            RangePrepublicationDelivery::Accepted
        );
        assert_eq!(
            stale.service(window.text_system()).status,
            RangePrepublicationStatus::Failed(RangePrepublicationFailure::Stale)
        );
    });

    let mut session = RangePrepublicationSession::new(seed, environment.clone()).unwrap();
    let (candidate, _, _) =
        window.update(|window, _| drive(&mut session, source, window.text_system(), &cleanup));
    let mismatch = RangePrepublicationCurrent {
        binding: seed.binding,
        history: None,
        available_capacity: candidate.adoption_peak(),
    };
    let observed = Rc::new(Cell::new(None));
    let observed_from_view = observed.clone();
    let fallback = environment.config().clone();
    let _ = window.update(move |window, cx| {
        cx.new(|cx| {
            match RangeTextInput::new_with_prepublication(
                &environment,
                candidate,
                mismatch,
                window,
                cx,
            ) {
                Ok(input) => input,
                Err(error) => {
                    observed_from_view.set(Some(error));
                    RangeTextInput::new(fallback, window, cx).unwrap()
                }
            }
        })
    });
    assert_eq!(
        observed.get(),
        Some(RangePrepublicationAdoptionError::HistoryMismatch)
    );
}

#[gpui::test]
fn cancellation_releases_pending_custody_and_late_response_is_obsolete(cx: &mut TestAppContext) {
    let source = "one\ntwo\nthree";
    let seed = seed(source, 3, 0);
    let window = cx.add_empty_window();
    let (environment, cleanup) = window
        .update(|window, _| make_environment(13, config(source, 3, 32), window.text_system()));
    let mut session = RangePrepublicationSession::new(seed, environment).unwrap();
    window.update(|window, _| {
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
        let (_, request) = page_step
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
        let generation = session.generation();
        let page = page_for(source, 99, request);
        session.cancel();
        let cancellation = cleanup.service(1);
        assert!(matches!(
            cancellation.effects.as_slice(),
            [RangePrepublicationCleanupEffect::CancelPage { .. }]
        ));
        assert_eq!(
            session.deliver_page(generation, page),
            RangePrepublicationDelivery::Obsolete
        );
        cleanup.acknowledge(cancellation.effects[0].token());
        let release = cleanup.service(1);
        assert!(matches!(
            release.effects.as_slice(),
            [RangePrepublicationCleanupEffect::ReleasePage { .. }]
        ));
        cleanup.acknowledge(release.effects[0].token());
        let ownership = session.ownership();
        assert_eq!(ownership.pending_pages, 0);
        assert_eq!(ownership.pending_object_pages, 0);
        assert_eq!(ownership.resident_pages, 0);
        assert!(!ownership.candidate);
    });
}

#[gpui::test]
fn capacity_blocking_is_retryable_and_initial_denial_is_typed(cx: &mut TestAppContext) {
    let source = "capacity\ncheck";
    let seed = seed(source, 4, 0);
    let window = cx.add_empty_window();
    let (environment, _) = window
        .update(|window, _| make_environment(14, config(source, 4, 32), window.text_system()));
    let mut session = RangePrepublicationSession::new(seed, environment.clone()).unwrap();
    window.update(|window, _| {
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
        session.set_available_capacity(RangeSurfaceCharge { bytes: 1, items: 1 });
        assert_eq!(
            session.deliver_page(generation, page_for(source, 101, request)),
            RangePrepublicationDelivery::CapacityBlocked
        );
        assert_eq!(
            session.service(window.text_system()).status,
            RangePrepublicationStatus::CapacityBlocked
        );
        session.set_available_capacity(RangeSurfaceCharge {
            bytes: usize::MAX,
            items: usize::MAX,
        });
        assert_ne!(
            session.service(window.text_system()).status,
            RangePrepublicationStatus::CapacityBlocked
        );
    });

    let mut denied = config(source, 4, 32);
    denied.limits.max_surface_bytes = 1;
    window.update(|window, _| {
        let cleanup = RangePrepublicationCleanupLedger::new(window.text_system(), 1).unwrap();
        assert_eq!(
            RangePrepublicationEnvironment::new(15, denied, window.text_system(), cleanup,)
                .unwrap_err(),
            RangePrepublicationFailure::InitialCapacityDenied
        );
    });
}

#[gpui::test]
fn valid_resident_response_is_released_on_cancel_failure_and_drop(cx: &mut TestAppContext) {
    let source = "resident response custody";
    let restoration = seed(source, 6, 3);
    let wrong_text_system = {
        let window = cx.add_empty_window();
        window.update(|window, _| window.text_system().clone())
    };
    let first = cx.add_empty_window();

    let make_resident = |session: &mut RangePrepublicationSession,
                         text_system: &Arc<WindowTextSystem>,
                         page_id: u64| {
        let validation = session.service(text_system);
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
        let page_step = session.service(text_system);
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
        assert_eq!(
            session.deliver_page(generation, page_for(source, page_id, request)),
            RangePrepublicationDelivery::Accepted
        );
        let _ = session.service(text_system);
        assert!(session.ownership().resident_pages > 0);
    };

    let (environment, cleanup) =
        first.update(|window, _| make_environment(31, config(source, 6, 32), window.text_system()));
    first.update(|window, _| {
        let mut session =
            RangePrepublicationSession::new(restoration, environment.clone()).unwrap();
        make_resident(&mut session, window.text_system(), 60_001);
        session.cancel();
        let effects = drain_cleanup(&cleanup);
        assert!(
            effects.iter().any(|effect| matches!(
                effect,
                RangePrepublicationCleanupEffect::ReleasePage { .. }
            ))
        );
    });

    let mut failed = RangePrepublicationSession::new(restoration, environment.clone()).unwrap();
    first.update(|window, _| make_resident(&mut failed, window.text_system(), 60_002));
    let step = failed.service(&wrong_text_system);
    assert_eq!(
        step.status,
        RangePrepublicationStatus::Failed(RangePrepublicationFailure::Stale)
    );
    assert!(step.effects.is_empty());
    let effects = drain_cleanup(&cleanup);
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, RangePrepublicationCleanupEffect::ReleasePage { .. }))
    );

    first.update(|window, _| {
        let mut dropped = RangePrepublicationSession::new(restoration, environment).unwrap();
        make_resident(&mut dropped, window.text_system(), 60_003);
        drop(dropped);
        let effects = drain_cleanup(&cleanup);
        assert!(
            effects.iter().any(|effect| matches!(
                effect,
                RangePrepublicationCleanupEffect::ReleasePage { .. }
            ))
        );
    });
}

#[gpui::test]
fn wrong_window_text_system_cancels_outstanding_request(cx: &mut TestAppContext) {
    let source = "wrong window";
    let restoration = seed(source, 7, 0);
    let wrong_text_system = {
        let window = cx.add_empty_window();
        window.update(|window, _| window.text_system().clone())
    };
    let first = cx.add_empty_window();
    let (environment, cleanup) =
        first.update(|window, _| make_environment(32, config(source, 7, 32), window.text_system()));
    let mut session = RangePrepublicationSession::new(restoration, environment).unwrap();
    first.update(|window, _| {
        assert!(matches!(
            session.service(window.text_system()).effects.as_slice(),
            [RangePrepublicationEffect::ValidateOwner(_)]
        ));
    });
    let step = session.service(&wrong_text_system);
    assert_eq!(
        step.status,
        RangePrepublicationStatus::Failed(RangePrepublicationFailure::Stale)
    );
    assert!(step.effects.is_empty());
    let effects = drain_cleanup(&cleanup);
    assert!(matches!(
        effects.as_slice(),
        [RangePrepublicationCleanupEffect::CancelValidation { .. }]
    ));
}

#[gpui::test]
fn adopted_surface_matches_ordinary_restoration_surface_and_both_render(cx: &mut TestAppContext) {
    let source = String::new();
    let restoration = seed(&source, 11, 0);
    let ordinary_source = source.clone();
    let ordinary_snapshot = {
        let (ordinary, visual) = cx.add_window_view(move |window, cx| {
            RangeTextInput::new(config(&ordinary_source, 11, 64), window, cx).unwrap()
        });
        visual.update(|window, app| window.draw(app).clear());
        visual.run_until_parked();
        ordinary.read_with(visual, |input, _| {
            let diagnostics = input.realization_diagnostics();
            assert_eq!(diagnostics.adopted_custody_bytes, 0);
            assert_eq!(diagnostics.adopted_custody_items, 0);
            let surface = input.surface().unwrap();
            (
                (
                    input.is_surface_current_and_interactive(),
                    surface.binding(),
                    surface.selection(),
                    surface.caret(),
                    surface.scroll_position(),
                    surface.scroll_block(),
                    surface.scroll_intra_anchor(),
                    surface.viewport(),
                ),
                (
                    surface.overscan(),
                    surface.visual_lines(),
                    surface.content_height(),
                    surface.caret_bounds(px(16.)),
                    format!("{:?}", surface.fragments()),
                    surface.realized_objects().to_vec(),
                    surface.fillers().collect::<Vec<_>>(),
                ),
            )
        })
    };

    let adopted_source = source.clone();
    let (adopted, visual) = cx.add_window_view(move |window, cx| {
        let (environment, cleanup) =
            make_environment(56, config(&adopted_source, 11, 64), window.text_system());
        let mut session =
            RangePrepublicationSession::new(restoration, environment.clone()).unwrap();
        let candidate = drive(
            &mut session,
            &adopted_source,
            window.text_system(),
            &cleanup,
        )
        .0;
        let current = RangePrepublicationCurrent {
            binding: restoration.binding,
            history: restoration.history,
            available_capacity: candidate.adoption_peak(),
        };
        let input =
            RangeTextInput::new_with_prepublication(&environment, candidate, current, window, cx)
                .unwrap();
        assert_eq!(cleanup.ownership().active, 0);
        input
    });
    visual.update(|window, app| window.draw(app).clear());
    visual.run_until_parked();
    let adopted_snapshot = adopted.read_with(visual, |input, _| {
        let surface = input.surface().unwrap();
        (
            (
                input.is_surface_current_and_interactive(),
                surface.binding(),
                surface.selection(),
                surface.caret(),
                surface.scroll_position(),
                surface.scroll_block(),
                surface.scroll_intra_anchor(),
                surface.viewport(),
            ),
            (
                surface.overscan(),
                surface.visual_lines(),
                surface.content_height(),
                surface.caret_bounds(px(16.)),
                format!("{:?}", surface.fragments()),
                surface.realized_objects().to_vec(),
                surface.fillers().collect::<Vec<_>>(),
            ),
        )
    });
    assert_eq!(ordinary_snapshot, adopted_snapshot);
}

#[gpui::test]
fn adopted_text_and_object_custody_releases_once_on_replacement_disposal_and_drop(
    cx: &mut TestAppContext,
) {
    let source = "ab";
    let fact = object_fact(91, 1, 1);
    let neighbor = InlineObjectNeighbor::new(fact.id(), fact.order());
    let object_position =
        SourcePosition::new(ByteOffset::new(1), InlineObjectGap::before(neighbor));
    let mut restoration = seed(source, 12, 1);
    restoration.caret = object_position;
    restoration.selection = RangeSourceSelection::caret(object_position);
    restoration.scroll.position = object_position;
    let facts = vec![fact];

    let window = cx.add_empty_window();
    let (environment, cleanup) = window
        .update(|window, _| make_environment(40, config(source, 12, 32), window.text_system()));
    let mut session = RangePrepublicationSession::new(restoration, environment.clone()).unwrap();
    let candidate = window.update(|window, _| {
        drive_with_objects(&mut session, source, window.text_system(), &cleanup, &facts).0
    });
    let current = RangePrepublicationCurrent {
        binding: restoration.binding,
        history: restoration.history,
        available_capacity: candidate.adoption_peak(),
    };
    let input = window.update(|window, cx| {
        cx.new(|cx| {
            RangeTextInput::new_with_prepublication(&environment, candidate, current, window, cx)
                .unwrap()
        })
    });
    let (before_import, import_custody_charge, import_records) =
        asserted_adopted_custody(&input, window, &cleanup, environment.config());

    window.update(|_, cx| {
        input
            .update(cx, |input, cx| input.import_restoration(restoration, cx))
            .unwrap()
    });
    let import_effects = assert_and_ack_exact_resident_releases(&cleanup, import_records);
    let after_import = input.read_with(window, |input, _| input.realization_diagnostics());
    assert_eq!(after_import.adopted_custody_bytes, 0);
    assert_eq!(after_import.adopted_custody_items, 0);
    assert_custody_charge_left(before_import, after_import.current, import_custody_charge);

    window.update(|window, cx| {
        let _ = input.update(cx, |input, cx| input.dispose(window, cx));
    });
    window.update(|_, _| {});
    assert!(
        cleanup
            .service(cleanup.ownership().slots)
            .effects
            .is_empty()
    );
    assert_cleanup_empty_and_reusable(&cleanup, &environment, restoration, window);
    assert_eq!(
        import_effects
            .iter()
            .map(|effect| effect.token().id())
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        import_effects.len()
    );

    let replacement_window = cx.add_empty_window();
    let (replacement_environment, replacement_cleanup) = replacement_window
        .update(|window, _| make_environment(41, config(source, 12, 32), window.text_system()));
    let mut replacement_session =
        RangePrepublicationSession::new(restoration, replacement_environment.clone()).unwrap();
    let replacement_candidate = replacement_window.update(|window, _| {
        drive_with_objects(
            &mut replacement_session,
            source,
            window.text_system(),
            &replacement_cleanup,
            &facts,
        )
        .0
    });
    let replacement_current = RangePrepublicationCurrent {
        binding: restoration.binding,
        history: restoration.history,
        available_capacity: replacement_candidate.adoption_peak(),
    };
    let replacement_input = replacement_window.update(|window, cx| {
        cx.new(|cx| {
            RangeTextInput::new_with_prepublication(
                &replacement_environment,
                replacement_candidate,
                replacement_current,
                window,
                cx,
            )
            .unwrap()
        })
    });
    let (before_replacement, replacement_custody_charge, replacement_records) =
        asserted_adopted_custody(
            &replacement_input,
            replacement_window,
            &replacement_cleanup,
            replacement_environment.config(),
        );
    let old_geometry = replacement_input.read_with(replacement_window, |input, _| {
        input.surface().unwrap().geometry_key()
    });
    let mut replacement_layout = replacement_environment.config().layout.clone();
    replacement_layout.wrap_width = px(96.);
    replacement_window.update(|_, cx| {
        replacement_input
            .update(cx, |input, cx| {
                input.set_layout(
                    replacement_layout,
                    replacement_environment.config().style.clone(),
                    cx,
                )
            })
            .unwrap()
    });
    drive_ordinary_publication_replacement(
        &replacement_input,
        replacement_window,
        source,
        &facts,
        &replacement_cleanup,
        old_geometry,
    );
    let replacement_effects =
        assert_and_ack_exact_resident_releases(&replacement_cleanup, replacement_records);
    let after_replacement = replacement_input.read_with(replacement_window, |input, _| {
        let surface = input.surface().unwrap();
        assert_ne!(surface.geometry_key(), old_geometry);
        assert_eq!(surface.realized_objects().len(), 1);
        input.realization_diagnostics()
    });
    assert_eq!(after_replacement.adopted_custody_bytes, 0);
    assert_eq!(after_replacement.adopted_custody_items, 0);
    assert_custody_charge_left(
        before_replacement,
        after_replacement.current,
        replacement_custody_charge,
    );

    drop(replacement_input);
    replacement_window.update(|_, _| {});
    assert!(
        replacement_cleanup
            .service(replacement_cleanup.ownership().slots)
            .effects
            .is_empty()
    );
    assert_cleanup_empty_and_reusable(
        &replacement_cleanup,
        &replacement_environment,
        restoration,
        replacement_window,
    );
    assert_eq!(
        replacement_effects
            .iter()
            .map(|effect| effect.token().id())
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        replacement_effects.len()
    );
}

fn asserted_adopted_custody(
    input: &gpui::Entity<RangeTextInput>,
    window: &mut gpui::VisualTestContext,
    cleanup: &RangePrepublicationCleanupLedger,
    config: &RangeTextInputConfig,
) -> (
    gpui_text_input::RangeRealizationOwnership,
    RangeSurfaceCharge,
    usize,
) {
    let records = cleanup.ownership().active;
    assert!(records >= 2);
    let record_charge = window.update(|window, _| {
        let one = RangePrepublicationCleanupLedger::new(window.text_system(), 1)
            .unwrap()
            .ownership()
            .retained_charge;
        let two = RangePrepublicationCleanupLedger::new(window.text_system(), 2)
            .unwrap()
            .ownership()
            .retained_charge;
        RangeSurfaceCharge {
            bytes: two.bytes - one.bytes,
            items: two.items - one.items,
        }
    });
    let text_custody_layout = std::alloc::Layout::new::<PageId>()
        .extend(std::alloc::Layout::new::<
            gpui_text_input::RangePrepublicationCleanupToken,
        >())
        .unwrap()
        .0
        .pad_to_align();
    let object_custody_layout = std::alloc::Layout::new::<ObjectPageId>()
        .extend(std::alloc::Layout::new::<
            gpui_text_input::RangePrepublicationCleanupToken,
        >())
        .unwrap()
        .0
        .pad_to_align();
    let custody_charge = RangeSurfaceCharge {
        bytes: record_charge
            .bytes
            .checked_mul(records)
            .and_then(|bytes| {
                bytes.checked_add(
                    text_custody_layout
                        .size()
                        .checked_mul(config.residency_limits.max_resident_pages())?,
                )
            })
            .and_then(|bytes| {
                bytes.checked_add(
                    object_custody_layout
                        .size()
                        .checked_mul(config.object_residency_limits.max_resident_pages())?,
                )
            })
            .unwrap(),
        items: record_charge
            .items
            .checked_mul(records)
            .and_then(|items| items.checked_add(config.residency_limits.max_resident_pages()))
            .and_then(|items| {
                items.checked_add(config.object_residency_limits.max_resident_pages())
            })
            .unwrap(),
    };
    let (diagnostics, surface_charge) = input.read_with(window, |input, _| {
        (
            input.realization_diagnostics(),
            input.surface().unwrap().charge(),
        )
    });
    assert_eq!(diagnostics.adopted_custody_bytes, custody_charge.bytes);
    assert_eq!(diagnostics.adopted_custody_items, custody_charge.items);
    assert!(
        diagnostics.current.owned_bytes
            >= surface_charge
                .bytes
                .checked_add(custody_charge.bytes)
                .unwrap()
    );
    assert!(
        diagnostics.current.owned_items
            >= surface_charge
                .items
                .checked_add(custody_charge.items)
                .unwrap()
    );
    (diagnostics.current, custody_charge, records)
}

fn assert_custody_charge_left(
    before: gpui_text_input::RangeRealizationOwnership,
    after: gpui_text_input::RangeRealizationOwnership,
    custody_floor: RangeSurfaceCharge,
) {
    assert!(
        before.owned_bytes.saturating_sub(after.owned_bytes) >= custody_floor.bytes,
        "before={before:?} after={after:?} custody_floor={custody_floor:?}"
    );
    assert!(
        after.owned_items <= before.owned_items,
        "released custody must not increase retained items: before={before:?} after={after:?} custody_floor={custody_floor:?}"
    );
}

fn assert_and_ack_exact_resident_releases(
    cleanup: &RangePrepublicationCleanupLedger,
    records: usize,
) -> Vec<RangePrepublicationCleanupEffect> {
    let step = cleanup.service(cleanup.ownership().slots);
    assert_eq!(step.effects.len(), records);
    assert_eq!(step.ready_remaining, 0);
    assert_eq!(cleanup.ownership().awaiting_acknowledgement, records);
    assert_eq!(
        step.effects
            .iter()
            .map(|effect| effect.token().id())
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        records
    );
    assert!(
        step.effects
            .iter()
            .any(|effect| matches!(effect, RangePrepublicationCleanupEffect::ReleasePage { .. }))
    );
    assert!(step.effects.iter().any(|effect| matches!(
        effect,
        RangePrepublicationCleanupEffect::ReleaseObjectPage { .. }
    )));
    for effect in &step.effects {
        assert_eq!(
            cleanup.acknowledge(effect.token()),
            gpui_text_input::RangePrepublicationCleanupAcknowledgement::Accepted
        );
    }
    assert_eq!(cleanup.ownership().active, 0);
    assert_eq!(cleanup.ownership().ready, 0);
    assert_eq!(cleanup.ownership().awaiting_acknowledgement, 0);
    step.effects
}

fn assert_cleanup_empty_and_reusable(
    cleanup: &RangePrepublicationCleanupLedger,
    environment: &RangePrepublicationEnvironment,
    restoration: RangeRestorationSeed,
    window: &mut gpui::VisualTestContext,
) {
    assert_eq!(cleanup.ownership().active, 0);
    assert_eq!(cleanup.ownership().ready, 0);
    assert_eq!(cleanup.ownership().awaiting_acknowledgement, 0);
    window.update(|window, _| {
        let mut reuse = RangePrepublicationSession::new(restoration, environment.clone()).unwrap();
        assert!(matches!(
            reuse.service(window.text_system()).effects.as_slice(),
            [RangePrepublicationEffect::ValidateOwner(_)]
        ));
        reuse.cancel();
        let _ = drain_cleanup(cleanup);
    });
}

fn drive_ordinary_publication_replacement(
    input: &gpui::Entity<RangeTextInput>,
    window: &mut gpui::VisualTestContext,
    source: &str,
    facts: &[InlineObjectFact],
    cleanup: &RangePrepublicationCleanupLedger,
    old_geometry: gpui_text_input::GeometryKey,
) {
    let mut page_id = 90_000;
    for _ in 0..512 {
        while let Some(request) = input.update(window, |input, _| input.take_request()) {
            match request {
                gpui_text_input::RangeTextInputRequest::Page(request) => {
                    page_id += 1;
                    let page = page_for(source, page_id, request);
                    window.update(|window, cx| {
                        input.update(cx, |input, cx| {
                            input.deliver_page(page, window, cx).unwrap()
                        })
                    });
                }
                gpui_text_input::RangeTextInputRequest::ObjectPage(request) => {
                    page_id += 1;
                    let page = object_page_for(page_id, request, facts);
                    window.update(|window, cx| {
                        input.update(cx, |input, cx| {
                            input
                                .deliver_object_page_in_window(page, window, cx)
                                .unwrap()
                        })
                    });
                }
                gpui_text_input::RangeTextInputRequest::ReleasePage(_)
                | gpui_text_input::RangeTextInputRequest::CancelPage(_)
                | gpui_text_input::RangeTextInputRequest::ReleaseObjectPage(_)
                | gpui_text_input::RangeTextInputRequest::CancelObjectPage(_) => {}
                other => panic!("unexpected replacement request: {other:?}"),
            }
            if cleanup.ownership().ready != 0 {
                input.read_with(window, |input, _| {
                    assert_ne!(input.surface().unwrap().geometry_key(), old_geometry)
                });
                return;
            }
        }
        window.update(|window, app| window.draw(app).clear());
        window.run_until_parked();
        if cleanup.ownership().ready != 0 {
            input.read_with(window, |input, _| {
                assert_ne!(input.surface().unwrap().geometry_key(), old_geometry)
            });
            return;
        }
    }
    panic!(
        "ordinary publication replacement did not commit: {:?}",
        input.read_with(window, |input, _| input.realization_diagnostics())
    );
}
