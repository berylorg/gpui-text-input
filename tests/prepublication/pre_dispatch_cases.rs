use super::*;

fn one_step_config(source: &str, revision: u64) -> RangeTextInputConfig {
    let mut value = config(source, revision, 32);
    value.limits.max_realization_work_per_frame = 1;
    value
}

fn sum(charges: impl IntoIterator<Item = RangeSurfaceCharge>) -> RangeSurfaceCharge {
    charges
        .into_iter()
        .fold(RangeSurfaceCharge::default(), |total, charge| {
            RangeSurfaceCharge {
                bytes: total.bytes.checked_add(charge.bytes).unwrap(),
                items: total.items.checked_add(charge.items).unwrap(),
            }
        })
}

fn record_charge(text_system: &Arc<WindowTextSystem>) -> RangeSurfaceCharge {
    let one = RangePrepublicationCleanupLedger::new(text_system, 1)
        .unwrap()
        .ownership()
        .retained_charge;
    let two = RangePrepublicationCleanupLedger::new(text_system, 2)
        .unwrap()
        .ownership()
        .retained_charge;
    RangeSurfaceCharge {
        bytes: two.bytes - one.bytes,
        items: two.items - one.items,
    }
}

fn effect_storage(capacity: usize) -> RangeSurfaceCharge {
    RangeSurfaceCharge {
        bytes: capacity * std::mem::size_of::<RangePrepublicationEffect>(),
        items: capacity,
    }
}

fn complete_validation(
    session: &mut RangePrepublicationSession,
    text_system: &Arc<WindowTextSystem>,
) {
    let step = session.service(text_system);
    let RangePrepublicationEffect::ValidateOwner(request) = step.effects[0] else {
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
    assert!(session.service(text_system).effects.is_empty());
}

fn complete_restoration_text(
    session: &mut RangePrepublicationSession,
    source: &str,
    text_system: &Arc<WindowTextSystem>,
    page_id: u64,
) {
    let step = session.service(text_system);
    let (generation, request) = step
        .effects
        .into_iter()
        .find_map(|effect| match effect {
            RangePrepublicationEffect::Page {
                generation,
                request,
                ..
            } if request.key().purpose() == PagePurpose::Restoration => Some((generation, request)),
            _ => None,
        })
        .expect("restoration page expected");
    assert_eq!(
        session.deliver_page(generation, page_for(source, page_id, request)),
        RangePrepublicationDelivery::Accepted
    );
    assert!(session.service(text_system).effects.is_empty());
}

fn complete_restoration_object(
    session: &mut RangePrepublicationSession,
    facts: &[InlineObjectFact],
    text_system: &Arc<WindowTextSystem>,
    cleanup: &RangePrepublicationCleanupLedger,
    page_id: u64,
) {
    let step = session.service(text_system);
    let (generation, request) = step
        .effects
        .into_iter()
        .find_map(|effect| match effect {
            RangePrepublicationEffect::ObjectPage {
                generation,
                request,
                ..
            } if request.key().purpose() == ObjectPurpose::Restoration => {
                Some((generation, request))
            }
            _ => None,
        })
        .expect("restoration object page expected");
    assert_eq!(
        session.deliver_object_page(generation, object_page_for(page_id, request, facts)),
        RangePrepublicationDelivery::Accepted
    );
    assert!(session.service(text_system).effects.is_empty());
    assert!(session.service(text_system).effects.is_empty());
    let _ = drain_cleanup(cleanup);
}

#[gpui::test]
fn external_effect_admission_is_exact_and_one_under_is_side_effect_free(cx: &mut TestAppContext) {
    let source = "ab";
    let fact = object_fact(77, 1, 1);
    let neighbor = InlineObjectNeighbor::new(fact.id(), fact.order());
    let object_position =
        SourcePosition::new(ByteOffset::new(1), InlineObjectGap::before(neighbor));
    let mut restoration = seed(source, 10, 1);
    restoration.caret = object_position;
    restoration.selection = RangeSourceSelection::caret(object_position);
    restoration.scroll.position = object_position;
    let window = cx.add_empty_window();

    window.update(|window, _| {
        let config = one_step_config(source, 10);
        let cleanup_record = record_charge(window.text_system());

        let (environment, cleanup) = make_environment(40, config.clone(), window.text_system());
        let mut probe = RangePrepublicationSession::new(restoration, environment).unwrap();
        let validation_base = probe.ownership();
        let validation = probe.service(window.text_system());
        assert!(matches!(
            validation.effects.as_slice(),
            [RangePrepublicationEffect::ValidateOwner(_)]
        ));
        let validation_required = sum([
            RangeSurfaceCharge {
                bytes: validation_base.bytes,
                items: validation_base.items,
            },
            cleanup_record,
            effect_storage(validation.effects.capacity()),
        ]);
        drop(probe);
        let _ = drain_cleanup(&cleanup);

        for (id, available, expect_effect) in [
            (41, validation_required, true),
            (
                42,
                RangeSurfaceCharge {
                    bytes: validation_required.bytes - 1,
                    items: validation_required.items,
                },
                false,
            ),
            (
                43,
                RangeSurfaceCharge {
                    bytes: validation_required.bytes,
                    items: validation_required.items - 1,
                },
                false,
            ),
        ] {
            let (environment, cleanup) = make_environment(id, config.clone(), window.text_system());
            let mut session = RangePrepublicationSession::new(restoration, environment).unwrap();
            session.set_available_capacity(available);
            let step = session.service(window.text_system());
            assert_eq!(!step.effects.is_empty(), expect_effect);
            assert_eq!(cleanup.ownership().active, usize::from(expect_effect));
            if !expect_effect {
                assert_eq!(step.status, RangePrepublicationStatus::CapacityBlocked);
            }
            drop(session);
            let _ = drain_cleanup(&cleanup);
        }

        let (environment, cleanup) = make_environment(44, config.clone(), window.text_system());
        let mut page_probe = RangePrepublicationSession::new(restoration, environment).unwrap();
        complete_validation(&mut page_probe, window.text_system());
        let page_base = page_probe.ownership();
        let page_step = page_probe.service(window.text_system());
        assert!(matches!(
            page_step.effects.as_slice(),
            [RangePrepublicationEffect::Page { request, .. }]
                if request.key().purpose() == PagePurpose::Restoration
        ));
        let page_required = sum([
            RangeSurfaceCharge {
                bytes: page_base.bytes,
                items: page_base.items,
            },
            cleanup_record,
            effect_storage(page_step.effects.capacity()),
            RangeSurfaceCharge {
                bytes: usize::try_from(config.limits.page_bytes).unwrap(),
                items: 2,
            },
        ]);
        drop(page_probe);
        let _ = drain_cleanup(&cleanup);

        for (id, available, expect_effect) in [
            (45, page_required, true),
            (
                46,
                RangeSurfaceCharge {
                    bytes: page_required.bytes - 1,
                    items: page_required.items,
                },
                false,
            ),
            (
                47,
                RangeSurfaceCharge {
                    bytes: page_required.bytes,
                    items: page_required.items - 1,
                },
                false,
            ),
        ] {
            let (environment, cleanup) = make_environment(id, config.clone(), window.text_system());
            let mut session = RangePrepublicationSession::new(restoration, environment).unwrap();
            complete_validation(&mut session, window.text_system());
            let active_before = cleanup.ownership().active;
            session.set_available_capacity(available);
            let step = session.service(window.text_system());
            assert_eq!(!step.effects.is_empty(), expect_effect);
            assert_eq!(
                cleanup.ownership().active,
                active_before + usize::from(expect_effect)
            );
            assert_eq!(
                session.ownership().pending_pages,
                usize::from(expect_effect)
            );
            drop(session);
            let _ = drain_cleanup(&cleanup);
        }

        let (environment, cleanup) = make_environment(48, config.clone(), window.text_system());
        let mut object_probe = RangePrepublicationSession::new(restoration, environment).unwrap();
        complete_validation(&mut object_probe, window.text_system());
        complete_restoration_text(&mut object_probe, source, window.text_system(), 70_001);
        let object_base = object_probe.ownership();
        let object_step = object_probe.service(window.text_system());
        assert!(matches!(
            object_step.effects.as_slice(),
            [RangePrepublicationEffect::ObjectPage { request, .. }]
                if request.key().purpose() == ObjectPurpose::Restoration
        ));
        let object_required = sum([
            RangeSurfaceCharge {
                bytes: object_base.bytes,
                items: object_base.items,
            },
            cleanup_record,
            effect_storage(object_step.effects.capacity()),
            RangeSurfaceCharge {
                bytes: config.object_residency_limits.max_resident_bytes(),
                items: config.object_residency_limits.max_resident_objects() + 2,
            },
        ]);
        drop(object_probe);
        let _ = drain_cleanup(&cleanup);

        for (id, available, expect_effect) in [
            (49, object_required, true),
            (
                50,
                RangeSurfaceCharge {
                    bytes: object_required.bytes - 1,
                    items: object_required.items,
                },
                false,
            ),
            (
                51,
                RangeSurfaceCharge {
                    bytes: object_required.bytes,
                    items: object_required.items - 1,
                },
                false,
            ),
        ] {
            let (environment, cleanup) = make_environment(id, config.clone(), window.text_system());
            let mut session = RangePrepublicationSession::new(restoration, environment).unwrap();
            complete_validation(&mut session, window.text_system());
            complete_restoration_text(&mut session, source, window.text_system(), id + 70_000);
            let active_before = cleanup.ownership().active;
            session.set_available_capacity(available);
            let step = session.service(window.text_system());
            assert_eq!(!step.effects.is_empty(), expect_effect);
            assert_eq!(
                cleanup.ownership().active,
                active_before + usize::from(expect_effect)
            );
            assert_eq!(
                session.ownership().pending_object_pages,
                usize::from(expect_effect)
            );
            drop(session);
            let _ = drain_cleanup(&cleanup);
        }

        let facts = [fact];
        let (environment, cleanup) = make_environment(52, config.clone(), window.text_system());
        let mut geometry_probe = RangePrepublicationSession::new(restoration, environment).unwrap();
        complete_validation(&mut geometry_probe, window.text_system());
        complete_restoration_text(&mut geometry_probe, source, window.text_system(), 80_001);
        complete_restoration_object(
            &mut geometry_probe,
            &facts,
            window.text_system(),
            &cleanup,
            80_002,
        );
        let geometry_base = geometry_probe.ownership();
        let geometry_step = geometry_probe.service(window.text_system());
        assert!(matches!(
            geometry_step.effects.as_slice(),
            [RangePrepublicationEffect::Page { request, .. }]
                if request.key().purpose() == PagePurpose::GeometryIndex
        ));
        let geometry_required = sum([
            RangeSurfaceCharge {
                bytes: geometry_base.bytes,
                items: geometry_base.items,
            },
            cleanup_record,
            effect_storage(geometry_step.effects.capacity()),
            RangeSurfaceCharge {
                bytes: usize::try_from(config.limits.page_bytes).unwrap()
                    + std::mem::size_of::<gpui_text_input::PageRequestKey>(),
                items: 3,
            },
        ]);
        drop(geometry_probe);
        let _ = drain_cleanup(&cleanup);

        for (id, available, expect_effect) in [
            (53, geometry_required, true),
            (
                54,
                RangeSurfaceCharge {
                    bytes: geometry_required.bytes - 1,
                    items: geometry_required.items,
                },
                false,
            ),
            (
                55,
                RangeSurfaceCharge {
                    bytes: geometry_required.bytes,
                    items: geometry_required.items - 1,
                },
                false,
            ),
        ] {
            let (environment, cleanup) = make_environment(id, config.clone(), window.text_system());
            let mut session = RangePrepublicationSession::new(restoration, environment).unwrap();
            complete_validation(&mut session, window.text_system());
            complete_restoration_text(&mut session, source, window.text_system(), id + 80_000);
            complete_restoration_object(
                &mut session,
                &facts,
                window.text_system(),
                &cleanup,
                id + 90_000,
            );
            let active_before = cleanup.ownership().active;
            session.set_available_capacity(available);
            let step = session.service(window.text_system());
            assert_eq!(!step.effects.is_empty(), expect_effect);
            assert_eq!(
                cleanup.ownership().active,
                active_before + usize::from(expect_effect)
            );
            assert_eq!(
                session.ownership().pending_pages,
                usize::from(expect_effect)
            );
            drop(session);
            let _ = drain_cleanup(&cleanup);
        }
    });
}
