use super::*;

#[gpui::test]
fn resident_geometry_request_refusal_preserves_custody_and_resumes(cx: &mut TestAppContext) {
    let source = &format!("{}TARGET{}", "😀".repeat(24), "é".repeat(24));
    let window = cx.add_empty_window();
    window.update(|window, _| {
        for mode in 0..4 {
            let mut config = config(source, 1, 32);
            config.limits.max_realization_work_per_frame = 1;
            config.layout.limits.segment_bytes = 8;
            let (environment, cleanup) = make_environment(71, config, window.text_system());
            let mut session =
                RangePrepublicationSession::new(seed(source, 1, 0), environment).unwrap();
            let mut response_id = 1;
            let mut geometry = false;
            let mut delivered = false;
            let mut refusals = 0;
            let mut ready = false;
            for _ in 0..2000 {
                let before = session.ownership();
                if geometry
                    && !delivered
                    && before.pending_pages == 0
                    && before.pending_object_pages == 0
                    && before.resident_pages > 0
                {
                    session.set_available_capacity(if mode % 2 == 0 {
                        RangeSurfaceCharge {
                            bytes: if mode == 0 { 0 } else { before.bytes - 1 },
                            items: usize::MAX,
                        }
                    } else {
                        RangeSurfaceCharge {
                            bytes: usize::MAX,
                            items: if mode == 1 { 0 } else { before.items },
                        }
                    });
                    let step = session.service(window.text_system());
                    assert_eq!(step.status, RangePrepublicationStatus::CapacityBlocked);
                    assert_eq!(step.spent, 0);
                    assert!(step.effects.is_empty());
                    let after = session.ownership();
                    assert_eq!((after.bytes, after.items), (before.bytes, before.items));
                    let records = cleanup.ownership().active;
                    let retry = session.service(window.text_system());
                    assert_eq!(retry.status, RangePrepublicationStatus::CapacityBlocked);
                    assert_eq!(retry.spent, 0);
                    assert!(retry.effects.is_empty());
                    assert_eq!(cleanup.ownership().active, records);
                    assert_eq!(session.ownership().bytes, before.bytes);
                    assert_eq!(session.ownership().items, before.items);
                    refusals += 1;
                }
                session.set_available_capacity(RangeSurfaceCharge {
                    bytes: usize::MAX,
                    items: usize::MAX,
                });
                let step = session.service(window.text_system());
                delivered = false;
                for effect in step.effects {
                    response_id += 1;
                    let result = match effect {
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
                            geometry |= matches!(
                                request.key().purpose(),
                                PagePurpose::GeometryIndex | PagePurpose::GeometryTarget
                            );
                            session.deliver_page(generation, page_for(source, response_id, request))
                        }
                        RangePrepublicationEffect::ObjectPage {
                            generation,
                            request,
                            ..
                        } => session.deliver_object_page(
                            generation,
                            empty_object_page(response_id, request),
                        ),
                    };
                    assert_eq!(result, RangePrepublicationDelivery::Accepted);
                    delivered = true;
                }
                let _ = drain_cleanup(&cleanup);
                if step.status == RangePrepublicationStatus::Ready {
                    ready = true;
                    break;
                }
                assert!(
                    !matches!(step.status, RangePrepublicationStatus::Failed(_)),
                    "{:?}",
                    step.status
                );
            }
            assert!(ready);
            assert!(refusals > 0);
            drop(session);
            let _ = drain_cleanup(&cleanup);
            assert_eq!(cleanup.ownership().active, 0);
        }
    });
}
