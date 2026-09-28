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
