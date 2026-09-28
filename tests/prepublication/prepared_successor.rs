use super::*;

#[gpui::test]
fn prepared_successor_waits_for_cleanup_admission_and_releases_exact_custody(
    cx: &mut TestAppContext,
) {
    let source = "first line\nsecond line\nthird line";
    let window = cx.add_empty_window();
    window.update(|window, _| {
        for outcome in 0..3 {
            let mut config = config(source, 1, 16);
            config.limits.max_realization_work_per_frame = 1;
            let (environment, cleanup) = make_environment(72, config, window.text_system());
            let mut session =
                RangePrepublicationSession::new(seed(source, 1, 0), environment).unwrap();
            let mut consumed = None;
            for id in 1..100 {
                let step = session.service(window.text_system());
                assert!(!matches!(step.status, RangePrepublicationStatus::Failed(_)));
                for effect in step.effects {
                    let delivery = match effect {
                        RangePrepublicationEffect::ValidateOwner(request) => session
                            .deliver_validation(RangePrepublicationValidationResponse {
                                key: request.key,
                                binding: request.binding,
                                history: request.history,
                                current: true,
                            }),
                        RangePrepublicationEffect::Page {
                            generation,
                            request,
                            ..
                        } => {
                            if request.key().purpose() == PagePurpose::GeometryIndex {
                                consumed = Some(request.key());
                            }
                            session.deliver_page(generation, page_for(source, id, request))
                        }
                        RangePrepublicationEffect::ObjectPage {
                            generation,
                            request,
                            ..
                        } => {
                            session.deliver_object_page(generation, empty_object_page(id, request))
                        }
                    };
                    assert_eq!(delivery, RangePrepublicationDelivery::Accepted);
                }
                let _ = drain_cleanup(&cleanup);
                if consumed.is_some() {
                    break;
                }
            }
            let consumed = consumed.expect("index page was delivered");
            let admitted = session.service(window.text_system());
            assert_eq!(admitted.spent, 1);
            assert!(admitted.effects.is_empty());
            let prepared = session.service(window.text_system());
            assert_eq!(prepared.spent, 1);
            assert!(prepared.effects.is_empty());
            assert_eq!(prepared.status, RangePrepublicationStatus::Advancing);
            let before = session.ownership();
            let records = cleanup.ownership().active;
            session.set_available_capacity(RangeSurfaceCharge::default());
            for _ in 0..3 {
                let blocked = session.service(window.text_system());
                assert_eq!(blocked.status, RangePrepublicationStatus::CapacityBlocked);
                assert_eq!(blocked.spent, 0);
                assert!(blocked.effects.is_empty());
                assert_eq!(
                    (session.ownership().bytes, session.ownership().items),
                    (before.bytes, before.items)
                );
                assert_eq!(cleanup.ownership().active, records);
                assert!(drain_cleanup(&cleanup).is_empty());
            }
            let exposed = if outcome == 2 {
                session.set_available_capacity(RangeSurfaceCharge {
                    bytes: usize::MAX,
                    items: usize::MAX,
                });
                let resumed = session.service(window.text_system());
                assert_eq!(resumed.effects.len(), 1);
                let RangePrepublicationEffect::ObjectPage { request, .. } = resumed.effects[0]
                else {
                    panic!("prepared successor is an object request");
                };
                assert_eq!(request.key().purpose(), ObjectPurpose::GeometryIndex);
                assert_eq!(cleanup.ownership().active, records + 1);
                Some(request.key())
            } else {
                None
            };
            if outcome != 1 {
                session.cancel();
                assert_eq!(session.status(), RangePrepublicationStatus::Cancelled);
            }
            drop(session);
            let released = drain_cleanup(&cleanup);
            assert_eq!(
                released
                    .iter()
                    .filter(|effect| matches!(effect,
                RangePrepublicationCleanupEffect::ReleasePage { key, .. } if *key == consumed))
                    .count(),
                1
            );
            let cancelled = released
                .iter()
                .filter_map(|effect| match effect {
                    RangePrepublicationCleanupEffect::CancelObjectPage { key, .. } => Some(*key),
                    _ => None,
                })
                .collect::<Vec<_>>();
            assert_eq!(cancelled, exposed.into_iter().collect::<Vec<_>>());
            assert_eq!(cleanup.ownership().active, 0);
        }
    });
}
