use super::*;

fn deliver(
    session: &mut RangePrepublicationSession,
    source: &str,
    id: u64,
    effect: &RangePrepublicationEffect,
) -> RangePrepublicationDelivery {
    match effect {
        RangePrepublicationEffect::ValidateOwner(request) => {
            session.deliver_validation(RangePrepublicationValidationResponse {
                key: request.key,
                binding: request.binding,
                history: request.history,
                current: true,
            })
        }
        RangePrepublicationEffect::Page {
            generation,
            request,
            ..
        } => session.deliver_page(*generation, page_for(source, id, *request)),
        RangePrepublicationEffect::ObjectPage {
            generation,
            request,
            ..
        } => session.deliver_object_page(*generation, empty_object_page(id, *request)),
    }
}

#[gpui::test]
fn admitted_geometry_response_retains_exact_custody_until_commit(cx: &mut TestAppContext) {
    let source = "first line\nsecond line\nthird line";
    let window = cx.add_empty_window();
    window.update(|window, _| {
        for object in [false, true] {
            for item_limit in [false, true] {
                for outcome in 0..4 {
                    let mut config = config(source, 1, 16);
                    config.limits.max_realization_work_per_frame = 1;
                    let (environment, cleanup) = make_environment(73, config, window.text_system());
                    let mut session =
                        RangePrepublicationSession::new(seed(source, 1, 0), environment).unwrap();
                    let mut selected = None;
                    for id in 1..200 {
                        let step = session.service(window.text_system());
                        assert!(!matches!(step.status, RangePrepublicationStatus::Failed(_)));
                        for effect in step.effects {
                            let wanted = match effect {
                                RangePrepublicationEffect::Page { request, .. } => {
                                    !object && request.key().purpose() == PagePurpose::GeometryIndex
                                }
                                RangePrepublicationEffect::ObjectPage { request, .. } => {
                                    object
                                        && request.key().purpose() == ObjectPurpose::GeometryIndex
                                }
                                _ => false,
                            };
                            assert_eq!(
                                deliver(&mut session, source, id, &effect),
                                RangePrepublicationDelivery::Accepted
                            );
                            if wanted {
                                selected = Some((id, effect));
                            }
                        }
                        let _ = drain_cleanup(&cleanup);
                        if selected.is_some() {
                            break;
                        }
                    }
                    let (id, effect) = selected.expect("geometry response delivered");
                    let admitted = session.service(window.text_system());
                    assert_eq!(admitted.spent, 1);
                    assert!(admitted.effects.is_empty());
                    assert_eq!(admitted.status, RangePrepublicationStatus::Advancing);
                    assert!(drain_cleanup(&cleanup).is_empty());
                    let before = session.ownership();
                    #[cfg(feature = "test-support")]
                    let successor_ids =
                        gpui_text_input::preparation_test_support::session_response_successor_ids(
                            &session,
                        )
                        .expect("admitted response retains successor identities");
                    let records = cleanup.ownership().active;
                    session.set_available_capacity(if item_limit {
                        RangeSurfaceCharge {
                            bytes: usize::MAX,
                            items: before.items - 1,
                        }
                    } else {
                        RangeSurfaceCharge {
                            bytes: before.bytes - 1,
                            items: usize::MAX,
                        }
                    });
                    assert_eq!(session.status(), RangePrepublicationStatus::CapacityBlocked);
                    for _ in 0..3 {
                        let blocked = session.service(window.text_system());
                        assert_eq!(blocked.status, RangePrepublicationStatus::CapacityBlocked);
                        assert_eq!(blocked.spent, 0);
                        assert!(blocked.effects.is_empty());
                        let after = session.ownership();
                        assert_eq!(
                            (
                                after.bytes,
                                after.items,
                                after.resident_pages,
                                after.resident_object_pages
                            ),
                            (
                                before.bytes,
                                before.items,
                                before.resident_pages,
                                before.resident_object_pages
                            )
                        );
                        assert_eq!(cleanup.ownership().active, records);
                        #[cfg(feature = "test-support")]
                        assert_eq!(
                            gpui_text_input::preparation_test_support::session_response_successor_ids(
                                &session,
                            ),
                            Some(successor_ids)
                        );
                        assert!(drain_cleanup(&cleanup).is_empty());
                    }
                    match outcome {
                        0 => session.cancel(),
                        1 => {}
                        2 => {
                            session.set_available_capacity(RangeSurfaceCharge {
                                bytes: usize::MAX,
                                items: usize::MAX,
                            });
                            let committed = session.service(window.text_system());
                            assert_eq!(committed.spent, 1);
                            assert!(committed.effects.is_empty());
                            assert!(!matches!(
                                committed.status,
                                RangePrepublicationStatus::Failed(_)
                            ));
                            // The consumed identity cannot be admitted a second time after commit.
                            assert_eq!(
                                deliver(&mut session, source, id, &effect),
                                RangePrepublicationDelivery::Obsolete
                            );
                            let mut ready = false;
                            for next in 201..1000 {
                                let step = session.service(window.text_system());
                                assert!(!matches!(
                                    step.status,
                                    RangePrepublicationStatus::Failed(_)
                                ));
                                for effect in step.effects {
                                    assert_eq!(
                                        deliver(&mut session, source, next, &effect),
                                        RangePrepublicationDelivery::Accepted
                                    );
                                }
                                if step.status == RangePrepublicationStatus::Ready {
                                    ready = true;
                                    break;
                                }
                            }
                            assert!(ready);
                        }
                        3 => assert_eq!(
                            deliver(&mut session, source, id, &effect),
                            RangePrepublicationDelivery::Terminal(
                                RangePrepublicationFailure::ExactKeyCollision
                            )
                        ),
                        _ => unreachable!(),
                    }
                    #[cfg(feature = "test-support")]
                    if outcome != 1 {
                        assert_eq!(
                            gpui_text_input::preparation_test_support::session_response_successor_ids(
                                &session,
                            ),
                            None
                        );
                    }
                    drop(session);
                    let released = drain_cleanup(&cleanup);
                    let exact = released
                        .iter()
                        .filter(|release| match (&effect, release) {
                            (
                                RangePrepublicationEffect::Page { request, .. },
                                RangePrepublicationCleanupEffect::ReleasePage { key, .. },
                            ) => request.key() == *key,
                            (
                                RangePrepublicationEffect::ObjectPage { request, .. },
                                RangePrepublicationCleanupEffect::ReleaseObjectPage { key, .. },
                            ) => request.key() == *key,
                            _ => false,
                        })
                        .count();
                    assert_eq!(exact, 1);
                    assert_eq!(cleanup.ownership().active, 0);
                }
            }
        }
    });
}

#[cfg(feature = "test-support")]
#[gpui::test]
fn admitted_response_successor_ids_are_stable_and_never_reused(cx: &mut TestAppContext) {
    use gpui_text_input::preparation_test_support::session_response_successor_ids;

    let source = &format!("{}TARGET{}", "😀".repeat(24), "é".repeat(24));
    let window = cx.add_empty_window();
    window.update(|window, _| {
        let mut config = config(source, 1, 32);
        config.limits.max_realization_work_per_frame = 1;
        config.layout.limits.segment_bytes = 8;
        let (environment, cleanup) = make_environment(74, config, window.text_system());
        let mut session = RangePrepublicationSession::new(seed(source, 1, 0), environment).unwrap();
        let mut previous_delivery = false;
        let mut previous_id = 0;
        let mut resident_reservations = 0;
        let mut delivered_reservations = 0;
        let mut ready = false;
        for id in 1..2000 {
            let step = session.service(window.text_system());
            assert!(!matches!(step.status, RangePrepublicationStatus::Failed(_)));
            if let Some(ids) = session_response_successor_ids(&session) {
                assert!(step.effects.is_empty());
                assert!(ids[0] > previous_id);
                assert_eq!(ids, [ids[0], ids[0] + 1, ids[0] + 2]);
                previous_id = ids[2];
                if previous_delivery {
                    delivered_reservations += 1;
                } else {
                    resident_reservations += 1;
                }
                let before = session.ownership();
                for items in [false, true] {
                    session.set_available_capacity(if items {
                        RangeSurfaceCharge {
                            bytes: usize::MAX,
                            items: before.items - 1,
                        }
                    } else {
                        RangeSurfaceCharge {
                            bytes: before.bytes - 1,
                            items: usize::MAX,
                        }
                    });
                    for _ in 0..3 {
                        let blocked = session.service(window.text_system());
                        assert_eq!(blocked.status, RangePrepublicationStatus::CapacityBlocked);
                        assert_eq!(blocked.spent, 0);
                        assert!(blocked.effects.is_empty());
                        assert_eq!(session_response_successor_ids(&session), Some(ids));
                        assert_eq!(session.ownership().bytes, before.bytes);
                        assert_eq!(session.ownership().items, before.items);
                    }
                }
                session.set_available_capacity(RangeSurfaceCharge {
                    bytes: usize::MAX,
                    items: usize::MAX,
                });
                let committed = session.service(window.text_system());
                assert!(!matches!(
                    committed.status,
                    RangePrepublicationStatus::Failed(_)
                ));
                assert_eq!(committed.spent, 1);
                assert!(committed.effects.is_empty());
                assert_eq!(session_response_successor_ids(&session), None);
            }
            previous_delivery = !step.effects.is_empty();
            for effect in step.effects {
                assert_eq!(
                    deliver(&mut session, source, id, &effect),
                    RangePrepublicationDelivery::Accepted
                );
            }
            let _ = drain_cleanup(&cleanup);
            if session.status() == RangePrepublicationStatus::Ready {
                ready = true;
                break;
            }
        }
        assert!(ready);
        assert!(resident_reservations > 0);
        assert!(delivered_reservations > 0);
        drop(session);
        let _ = drain_cleanup(&cleanup);
        assert_eq!(cleanup.ownership().active, 0);
    });
}
