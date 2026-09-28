use super::*;
use gpui_text_input::preparation_test_support::session_delivered_object_preparation;

const SOURCE: &str =
    "first line of text spanning pages\nsecond line of text spanning pages\nthird line";

fn replacement_config(nonempty: bool) -> RangeTextInputConfig {
    let mut config = config(SOURCE, 1, 32);
    config.limits.max_realization_work_per_frame = 1;
    config.object_residency_limits = ObjectResidencyLimits::new(
        4,
        if nonempty { 3 } else { 1 },
        256 * 1024,
        64 * 1024,
        4,
        64,
        256 * 1024,
    )
    .unwrap();
    config
}

fn reach_replacement(
    session: &mut RangePrepublicationSession,
    text_system: &Arc<WindowTextSystem>,
    cleanup: &RangePrepublicationCleanupLedger,
    nonempty: bool,
) {
    for id in 1..300 {
        let step = session.service(text_system);
        assert!(
            !matches!(step.status, RangePrepublicationStatus::Failed(_)),
            "{:?}",
            step.status
        );
        for effect in step.effects {
            let replacement = matches!(effect,
                RangePrepublicationEffect::ObjectPage { request, .. }
                if request.key().purpose() == ObjectPurpose::GeometryIndex
                    && if nonempty { request.key().demand().contains_anchor(ByteOffset::new(34)) } else { session.ownership().resident_object_pages >= 2 });
            assert_eq!(
                deliver_objects(session, id, &effect, nonempty),
                RangePrepublicationDelivery::Accepted
            );
            if replacement {
                return;
            }
        }
        let _ = drain_cleanup(cleanup);
    }
    panic!("adjacent replacement response not reached");
}

#[gpui::test]
fn delivered_objects_preserve_custody_until_storage_fits(cx: &mut TestAppContext) {
    let window = cx.add_empty_window();
    window.update(|window, _| {
        for (nonempty, cancel) in [(false, false), (false, true), (true, false), (true, true)] {
            let (environment, cleanup) =
                make_environment(96, replacement_config(nonempty), window.text_system());
            let mut session =
                RangePrepublicationSession::new(seed(SOURCE, 1, 0), environment).unwrap();
            reach_replacement(&mut session, window.text_system(), &cleanup, nonempty);
            let proof = session_delivered_object_preparation(&session).unwrap();
            let (_, _, current, exact) = proof;
            assert!(exact.bytes > current.bytes);
            assert!(exact.items > current.items);
            let before = session.ownership();
            let records = cleanup.ownership().active;
            for available in [
                RangeSurfaceCharge { bytes: 0, items: 0 },
                current,
                RangeSurfaceCharge {
                    bytes: exact.bytes - 1,
                    ..exact
                },
                RangeSurfaceCharge {
                    items: exact.items - 1,
                    ..exact
                },
            ] {
                session.set_available_capacity(available);
                for _ in 0..2 {
                    let step = session.service(window.text_system());
                    assert_eq!(step.status, RangePrepublicationStatus::CapacityBlocked);
                    assert_eq!(step.spent, 0);
                    assert!(step.effects.is_empty());
                    assert_eq!(session.ownership(), before);
                    assert_eq!(session_delivered_object_preparation(&session), Some(proof));
                    assert_eq!(cleanup.ownership().active, records);
                    assert!(drain_cleanup(&cleanup).is_empty());
                    assert!(session.high_water().bytes >= exact.bytes);
                    assert!(session.high_water().items >= exact.items);
                }
            }
            if !cancel {
                session.set_available_capacity(exact);
                let step = session.service(window.text_system());
                assert_eq!(step.status, RangePrepublicationStatus::Advancing);
                assert_eq!(step.spent, 1);
                assert!(step.effects.is_empty());
                assert!(session.ownership().resident_object_pages >= before.resident_object_pages);
                assert!(session_delivered_object_preparation(&session).is_none());
                let _ = drain_cleanup(&cleanup);
                session.set_available_capacity(RangeSurfaceCharge {
                    bytes: usize::MAX,
                    items: usize::MAX,
                });
                finish(&mut session, window.text_system(), &cleanup, nonempty);
            }
            drop(session);
            let _ = drain_cleanup(&cleanup);
            assert_eq!(cleanup.ownership().active, 0);
        }
    });
}

fn deliver_objects(
    session: &mut RangePrepublicationSession,
    id: u64,
    effect: &RangePrepublicationEffect,
    nonempty: bool,
) -> RangePrepublicationDelivery {
    if let RangePrepublicationEffect::ObjectPage {
        generation,
        request,
        ..
    } = effect
    {
        let facts = if nonempty {
            vec![
                object_fact(1, 34, 1),
                object_fact(2, 34, 2),
                object_fact(3, 35, 1),
            ]
        } else {
            vec![]
        };
        session.deliver_object_page(*generation, object_page_for(id, *request, &facts))
    } else {
        super::admitted_response::deliver(session, SOURCE, id, effect)
    }
}

fn finish(
    session: &mut RangePrepublicationSession,
    text_system: &Arc<WindowTextSystem>,
    cleanup: &RangePrepublicationCleanupLedger,
    nonempty: bool,
) {
    for id in 1000..2000 {
        let step = session.service(text_system);
        assert!(
            !matches!(step.status, RangePrepublicationStatus::Failed(_)),
            "{:?}",
            step.status
        );
        for effect in step.effects {
            assert_eq!(
                deliver_objects(session, id, &effect, nonempty),
                RangePrepublicationDelivery::Accepted
            );
        }
        let _ = drain_cleanup(cleanup);
        if step.status == RangePrepublicationStatus::Ready {
            drop(session.take_candidate().unwrap());
            return;
        }
    }
    panic!("candidate not reached");
}

#[gpui::test]
fn delivered_object_failures_precede_zero_host_capacity(cx: &mut TestAppContext) {
    let window = cx.add_empty_window();
    window.update(|window, _| {
        for malformed in [false, true] {
            let source = "éabc";
            let mut config = config(source, 1, 16);
            config.limits.max_realization_work_per_frame = 1;
            if !malformed {
                config.object_residency_limits =
                    ObjectResidencyLimits::new(4, 1, 16384, 1, 4, 4, 16384).unwrap();
            }
            let (environment, cleanup) = make_environment(98, config, window.text_system());
            let mut session =
                RangePrepublicationSession::new(seed(source, 1, 0), environment).unwrap();
            let mut selected = false;
            for id in 1..100 {
                let step = session.service(window.text_system());
                assert!(
                    !matches!(step.status, RangePrepublicationStatus::Failed(_)),
                    "{:?}",
                    step.status
                );
                for effect in step.effects {
                    if let RangePrepublicationEffect::ObjectPage {
                        generation,
                        request,
                        ..
                    } = effect
                    {
                        if request.key().purpose() == ObjectPurpose::GeometryIndex {
                            let facts = vec![object_fact(1, if malformed { 1 } else { 2 }, 1)];
                            assert_eq!(
                                session.deliver_object_page(
                                    generation,
                                    object_page_for(id, request, &facts)
                                ),
                                RangePrepublicationDelivery::Accepted
                            );
                            selected = true;
                            break;
                        }
                    }
                    assert_eq!(
                        super::admitted_response::deliver(&mut session, source, id, &effect),
                        RangePrepublicationDelivery::Accepted
                    );
                }
                if selected {
                    break;
                }
                let _ = drain_cleanup(&cleanup);
            }
            assert!(selected);
            session.set_available_capacity(RangeSurfaceCharge { bytes: 0, items: 0 });
            let step = session.service(window.text_system());
            assert_eq!(
                step.status,
                RangePrepublicationStatus::Failed(if malformed {
                    RangePrepublicationFailure::MalformedResponse
                } else {
                    RangePrepublicationFailure::TerminalCapacity
                })
            );
            assert!(step.effects.is_empty());
            drop(session);
            let _ = drain_cleanup(&cleanup);
            assert_eq!(cleanup.ownership().active, 0);
        }
    });
}
