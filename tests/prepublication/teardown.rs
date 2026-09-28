use super::*;

#[gpui::test]
fn active_geometry_teardown_releases_exact_request_once(cx: &mut TestAppContext) {
    let source = "first line\nsecond line\nthird line";
    let other = cx.add_empty_window();
    let wrong_text_system = other.update(|window, _| window.text_system().clone());
    let window = cx.add_empty_window();
    window.update(|window, _| {
        for object in [false, true] {
            for delivered in [false, true] {
                for outcome in 0..3 {
                    let mut config = config(source, 1, 16);
                    config.limits.max_realization_work_per_frame = 1;
                    let (environment, cleanup) = make_environment(99, config, window.text_system());
                    let mut session =
                        RangePrepublicationSession::new(seed(source, 1, 0), environment).unwrap();
                    let mut selected = None;
                    for id in 1..200 {
                        let step = session.service(window.text_system());
                        assert!(
                            !matches!(step.status, RangePrepublicationStatus::Failed(_)),
                            "{:?}",
                            step.status
                        );
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
                            if !wanted || delivered {
                                assert_eq!(
                                    super::admitted_response::deliver(
                                        &mut session,
                                        source,
                                        id,
                                        &effect
                                    ),
                                    RangePrepublicationDelivery::Accepted
                                );
                            }
                            if wanted {
                                selected = Some(effect);
                            }
                        }
                        let _ = drain_cleanup(&cleanup);
                        if selected.is_some() {
                            break;
                        }
                    }
                    let selected = selected.expect("active geometry request");
                    assert!(cleanup.ownership().active > 0);
                    if outcome == 0 {
                        session.cancel();
                        session.cancel();
                        let step = session.service(window.text_system());
                        assert_eq!(step.status, RangePrepublicationStatus::Cancelled);
                        assert!(step.effects.is_empty());
                    } else if outcome == 1 {
                        for _ in 0..2 {
                            let step = session.service(&wrong_text_system);
                            assert_eq!(
                                step.status,
                                RangePrepublicationStatus::Failed(
                                    RangePrepublicationFailure::Stale
                                )
                            );
                            assert!(step.effects.is_empty());
                        }
                    }
                    if outcome != 2 {
                        let ownership = session.ownership();
                        assert_eq!(ownership.pending_pages, 0);
                        assert_eq!(ownership.pending_object_pages, 0);
                        assert_eq!(ownership.resident_pages, 0);
                        assert_eq!(ownership.resident_object_pages, 0);
                        assert!(!ownership.candidate);
                    }
                    drop(session);
                    let effects = drain_cleanup(&cleanup);
                    let matching = effects
                        .iter()
                        .filter(|effect| match (&selected, **effect) {
                            (
                                RangePrepublicationEffect::Page {
                                    generation,
                                    request,
                                    ..
                                },
                                RangePrepublicationCleanupEffect::CancelPage {
                                    generation: found_generation,
                                    key,
                                    ..
                                },
                            ) => {
                                !delivered && *generation == found_generation && request.key() == key
                            }
                            (
                                RangePrepublicationEffect::Page {
                                    generation,
                                    request,
                                    ..
                                },
                                RangePrepublicationCleanupEffect::ReleasePage {
                                    generation: found_generation,
                                    key,
                                    ..
                                },
                            ) => {
                                delivered && *generation == found_generation && request.key() == key
                            }
                            (
                                RangePrepublicationEffect::ObjectPage {
                                    generation,
                                    request,
                                    ..
                                },
                                RangePrepublicationCleanupEffect::CancelObjectPage {
                                    generation: found_generation,
                                    key,
                                    ..
                                },
                            ) => {
                                !delivered && *generation == found_generation && request.key() == key
                            }
                            (
                                RangePrepublicationEffect::ObjectPage {
                                    generation,
                                    request,
                                    ..
                                },
                                RangePrepublicationCleanupEffect::ReleaseObjectPage {
                                    generation: found_generation,
                                    key,
                                    ..
                                },
                            ) => {
                                delivered && *generation == found_generation && request.key() == key
                            }
                            _ => false,
                        })
                        .count();
                    assert_eq!(matching, 1);
                    assert_eq!(cleanup.ownership().active, 0);
                    assert!(drain_cleanup(&cleanup).is_empty());
                }
            }
        }
    });
}
