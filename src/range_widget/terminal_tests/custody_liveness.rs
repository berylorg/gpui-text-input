use super::*;

fn stale_text_page(input: &mut RangeTextInput, id: u64) -> RangePage {
    let key = crate::PageRequestKey::adjacent(
        PageRequestId::new(id),
        binding().binding(),
        binding().revision(),
        PagePurpose::Caret,
        ByteOffset::new(0),
        PageDirection::Forward,
        SOURCE.len() as u64,
    )
    .unwrap();
    input.dispatched_pages.insert(key);
    page_for(PageRequest::new(key), id)
}

fn requested_text_page(input: &mut RangeTextInput, id: u64) -> RangePage {
    let PageDemand::Requested(request) = input
        .residency
        .demand(
            PageRequestId::new(id),
            PagePurpose::Caret,
            PageDemandEnvelope::Adjacent {
                anchor: ByteOffset::new(0),
                direction: PageDirection::Forward,
                max_payload_bytes: SOURCE.len() as u64,
            },
        )
        .unwrap()
    else {
        panic!("fresh response must own a pending residency request")
    };
    input.dispatched_pages.insert(request.key());
    page_for(request, id)
}

fn stale_object_page(input: &mut RangeTextInput, id: u64) -> ObjectPage {
    let key = crate::ObjectRequestKey::new(
        ObjectRequestId::new(id),
        binding().binding(),
        binding().revision(),
        input.config.presentation_generation,
        ObjectPurpose::Restoration,
        ObjectDemandEnvelope::anchor(ByteOffset::new(0), None, ObjectDirection::Forward, 1, 4_096)
            .unwrap(),
    )
    .unwrap();
    input.dispatched_object_pages.insert(key);
    ObjectPage::new(
        ObjectPageId::new(id),
        key,
        vec![],
        ObjectPageEdgeFact::EnvelopeBoundary,
        ObjectPageEdgeFact::EnvelopeBoundary,
        true,
        None,
    )
    .unwrap()
}

fn drain_release_requests(input: &mut RangeTextInput) {
    while let Some(request) = input.take_request() {
        assert!(matches!(
            request,
            RangeTextInputRequest::ReleasePage(_) | RangeTextInputRequest::ReleaseObjectPage(_)
        ));
    }
}

#[gpui::test]
fn ordinary_eight_kib_geometry_target_capacity_failure_closes_exact_custody(
    cx: &mut gpui::TestAppContext,
) {
    let source = "a".repeat(8 * 1024);
    let mut configuration = config(8 * 1024 * 1024, 131_072);
    configuration.layout.wrap_width = px(320.);
    configuration.layout.limits.segment_bytes = 4096;
    configuration.layout.limits.glyphs = 4096;
    configuration.layout.limits.wraps = 256;
    configuration.layout.limits.maps = 4097;
    configuration.layout.limits.fragments = 4;
    configuration.geometry_limits =
        ExactGeometryLimits::new(source.len() as u64, 8, 2 * 1024 * 1024, 32_768).unwrap();
    configuration.residency_limits = ResidencyLimits::new(6, 384 * 1024, 8, 8 * 1024).unwrap();
    configuration.object_residency_limits =
        ObjectResidencyLimits::new(6, 4096, 384 * 1024, 128 * 1024, 8, 4096, 384 * 1024).unwrap();
    configuration.limits.max_realization_work_per_frame = 64;
    configuration.limits.max_realized_block_extent = px(64.);
    configuration.limits.page_bytes = 4096;
    configuration.viewport_extent = px(640.);
    configuration.overscan = Pixels::ZERO;
    let (input, cx) = cx
        .add_window_view(move |window, cx| RangeTextInput::new(configuration, window, cx).unwrap());
    drive_surface_for_source(&input, cx, SOURCE);
    let prior_publication = input.read_with(cx, |input, _| {
        format!("{:?}", input.surface.as_ref().expect("prior publication"))
    });
    let successor = RangeBinding::new(
        BindingId::new(610_100),
        SourceRevision::new(2),
        LogicalExtent::new(source.len() as u64, 1),
    );
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.rebind(successor, None, window, cx).unwrap()
        })
    });

    let mut terminal = None;
    for id in 1..1_536 {
        cx.update(|window, app| window.draw(app).clear());
        cx.run_until_parked();
        match input.update(cx, |input, _| input.take_request()) {
            Some(RangeTextInputRequest::Page(request)) => {
                let page = page_for_source(request, id, &source);
                cx.update(|window, app| {
                    input.update(app, |input, cx| {
                        input.deliver_page(page, window, cx).unwrap()
                    })
                });
            }
            Some(RangeTextInputRequest::ObjectPage(request)) => {
                let key = request.key();
                let page = ObjectPage::new(
                    ObjectPageId::new(id),
                    key,
                    vec![],
                    ObjectPageEdgeFact::EnvelopeBoundary,
                    ObjectPageEdgeFact::EnvelopeBoundary,
                    true,
                    None,
                )
                .unwrap();
                let prior = input.read_with(cx, |input, _| {
                    (
                        input.surface.as_ref().map(|surface| format!("{surface:?}")),
                        input
                            .residency
                            .resident_pages()
                            .map(|page| format!("{page:?}"))
                            .collect::<Vec<_>>(),
                        input
                            .object_residency
                            .resident_pages()
                            .map(|page| format!("{page:?}"))
                            .collect::<Vec<_>>(),
                    )
                });
                let result = cx.update(|window, app| {
                    input.update(app, |input, cx| {
                        input.deliver_object_page_in_window(page, window, cx)
                    })
                });
                result.unwrap();
                let accepted_terminal = input.read_with(cx, |input, _| {
                    input.active_geometry.is_none()
                        && input.realization_diagnostics().last_response_rejection
                            == Some(
                                RangeResponseRejectionClass::ExactGeometryLayoutCapacityExceeded,
                            )
                });
                if accepted_terminal {
                    terminal = Some((key, prior));
                    break;
                }
            }
            Some(RangeTextInputRequest::ReleasePage(_))
            | Some(RangeTextInputRequest::ReleaseObjectPage(_))
            | Some(RangeTextInputRequest::CancelPage(_))
            | Some(RangeTextInputRequest::CancelObjectPage(_)) => {}
            Some(request) => panic!("unexpected geometry request: {request:?}"),
            None => {}
        }
    }

    let (key, prior) = terminal.expect("ordinary fixture did not close terminal custody");
    assert_eq!(key.purpose(), ObjectPurpose::GeometryTarget);
    assert_eq!(prior.0.as_ref(), Some(&prior_publication));
    input.read_with(cx, |input, _| {
        let diagnostics = input.realization_diagnostics();
        assert_eq!(diagnostics.response_rejection_count, 1);
        assert_eq!(
            diagnostics.last_response_rejection,
            Some(RangeResponseRejectionClass::ExactGeometryLayoutCapacityExceeded)
        );
        assert_eq!(
            diagnostics.last_response_rejection_stage,
            Some(crate::ExactGeometryFailureStage::Scan)
        );
        assert_eq!(
            input.surface.as_ref().map(|surface| format!("{surface:?}")),
            prior.0
        );
        assert_eq!(
            input
                .residency
                .resident_pages()
                .map(|page| format!("{page:?}"))
                .collect::<Vec<_>>(),
            prior.1
        );
        assert_eq!(
            input
                .object_residency
                .resident_pages()
                .map(|page| format!("{page:?}"))
                .collect::<Vec<_>>(),
            prior.2
        );
        assert!(input.response_custody.is_empty());
        assert!(!input.dispatched_object_pages.contains(&key));
        assert!(input.pending_geometry_page.is_none());
        assert!(input.pending_geometry_object.is_none());
        assert!(input.active_geometry.is_none());
        assert!(input.surface_candidate.is_none());
        assert!(input.pending_target_intent.is_none());
        assert!(!input.pending_index_intent);
        assert_eq!(diagnostics.current.response_custody_count, 0);
        assert_eq!(diagnostics.current.pending_object_requests, 0);
        assert_eq!(diagnostics.current.dispatched_object_requests, 0);
        assert_eq!(diagnostics.current.active_geometry_jobs, 0);
        assert_eq!(diagnostics.current.pending_geometry_objects, 0);
        assert_eq!(diagnostics.current.candidates, 0);
        assert_eq!(diagnostics.current.pending_target_intents, 0);
        assert_eq!(diagnostics.current.pending_index_intents, 0);
        assert_eq!(diagnostics.capacity, RangeRealizationCapacityState::Normal);
        assert_eq!(
            input
                .requests
                .iter()
                .filter(|request| matches!(request, RangeTextInputRequest::ReleaseObjectPage(released) if *released == key))
                .count(),
            1
        );
        assert!(!input.requests.iter().any(|request| matches!(
            request,
            RangeTextInputRequest::Page(_) | RangeTextInputRequest::ObjectPage(_)
        )));
    });
    cx.update(|window, app| {
        input.update(app, |input, _| {
            assert!(matches!(
                input.take_request(),
                Some(RangeTextInputRequest::ReleaseObjectPage(released)) if released == key
            ));
            assert!(input.take_request().is_none());
        });
        for _ in 0..4 {
            window.draw(app).clear();
        }
    });
    cx.run_until_parked();
    input.update(cx, |input, _| {
        assert!(input.take_request().is_none());
        assert!(input.response_custody.is_empty());
        assert!(input.active_geometry.is_none());
        assert!(input.pending_target_intent.is_none());
        assert!(!input.pending_index_intent);
    });
}

#[gpui::test]
fn scheduled_frames_drain_mixed_text_object_and_alias_custody_tail(cx: &mut gpui::TestAppContext) {
    let (input, cx) = cx.add_window_view(|window, cx| {
        let mut configuration = config(2 * 1024 * 1024, 32_768);
        configuration.limits.max_realization_work_per_frame = 1;
        RangeTextInput::new(configuration, window, cx).unwrap()
    });
    drive_initial_surface(&input, cx);
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.begin_realization_frame();
            input.spend_realization_credit();
            let text = requested_text_page(input, 610_000);
            let object = stale_object_page(input, 610_001);
            let alias = stale_text_page(input, 610_002);
            input.dispatched_pages.remove(&alias.key());
            input
                .admit_response_custody(super::super::response_custody::RangeResponseCustody::Page(
                    text,
                ))
                .unwrap();
            input
                .admit_response_custody(
                    super::super::response_custody::RangeResponseCustody::Object(object),
                )
                .unwrap();
            input
                .admit_response_custody(
                    super::super::response_custody::RangeResponseCustody::AliasFanout(
                        super::super::response_custody::AliasFanout {
                            page: alias,
                            cursor: 0,
                            matched: true,
                        },
                    ),
                )
                .unwrap();
            assert!(custody_idle(input.service_response_custody(window, cx)));
            assert_eq!(input.response_custody.len(), 3);
            assert!(input.realization_continuation_scheduled);
            let frame_before = input.realization_frame_generation;
            let mut continuation_frames = 0;
            while !input.response_custody.is_empty() {
                assert!(continuation_frames < 8, "custody tail did not quiesce");
                assert!(
                    input.realization_continuation_scheduled,
                    "frame {continuation_frames}, custody {:?}",
                    input.response_custody
                );
                input.begin_realization_frame();
                let _ = input.service_response_custody(window, cx);
                assert!(input.realization_diagnostics().frame.spent <= 1);
                continuation_frames += 1;
            }
            assert_eq!(continuation_frames, 3);
            assert_eq!(
                input.realization_frame_generation,
                frame_before.wrapping_add(3)
            );
            assert!(!input.realization_continuation_scheduled);
            assert_eq!(input.dispatched_pages.len(), 0);
            assert_eq!(input.dispatched_object_pages.len(), 0);
            drain_release_requests(input);
            assert!(input.is_quiescent());
        })
    });
}

#[gpui::test]
fn windowless_object_custody_schedules_each_terminal_tail_unit(cx: &mut gpui::TestAppContext) {
    let (input, cx) = cx.add_window_view(|window, cx| {
        let mut configuration = config(2 * 1024 * 1024, 32_768);
        configuration.limits.max_realization_work_per_frame = 1;
        RangeTextInput::new(configuration, window, cx).unwrap()
    });
    drive_initial_surface(&input, cx);
    input.update(cx, |input, cx| {
        let first = stale_object_page(input, 610_100);
        let second = stale_object_page(input, 610_101);
        input
            .admit_response_custody(
                super::super::response_custody::RangeResponseCustody::Object(first),
            )
            .unwrap();
        input
            .admit_response_custody(
                super::super::response_custody::RangeResponseCustody::Object(second),
            )
            .unwrap();
        input.begin_realization_frame();
        assert!(matches!(
            input.service_object_response_custody(cx),
            super::super::response_custody::ResponseCustodyProgress::Rejected(
                RangeTextInputError::Stale
            )
        ));
        assert_eq!(input.response_custody.len(), 1);
        assert!(input.realization_continuation_scheduled);

        input.begin_realization_frame();
        assert!(matches!(
            input.service_object_response_custody(cx),
            super::super::response_custody::ResponseCustodyProgress::Rejected(
                RangeTextInputError::Stale
            )
        ));
        assert!(input.response_custody.is_empty());
        assert!(!input.realization_continuation_scheduled);
        assert_eq!(input.dispatched_object_pages.len(), 0);
        drain_release_requests(input);
        assert!(input.is_quiescent());
    });
}

#[gpui::test]
fn disposal_obsoletes_a_scheduled_mixed_custody_tail(cx: &mut gpui::TestAppContext) {
    let (input, cx) = cx.add_window_view(|window, cx| {
        let mut configuration = config(2 * 1024 * 1024, 32_768);
        configuration.limits.max_realization_work_per_frame = 1;
        RangeTextInput::new(configuration, window, cx).unwrap()
    });
    drive_initial_surface(&input, cx);
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            let alias = stale_text_page(input, 610_200);
            let object = stale_object_page(input, 610_201);
            input
                .admit_response_custody(
                    super::super::response_custody::RangeResponseCustody::AliasFanout(
                        super::super::response_custody::AliasFanout {
                            page: alias,
                            cursor: 0,
                            matched: false,
                        },
                    ),
                )
                .unwrap();
            input
                .admit_response_custody(
                    super::super::response_custody::RangeResponseCustody::Object(object),
                )
                .unwrap();
            input.begin_realization_frame();
            assert!(custody_progressed(
                input.service_response_custody(window, cx)
            ));
            assert_eq!(input.response_custody.len(), 2);
            assert!(input.realization_continuation_scheduled);
            let _ = input.dispose(window, cx);
            assert!(input.response_custody.is_empty());
            assert!(!input.realization_continuation_scheduled);
        })
    });
    cx.run_until_parked();
    input.read_with(cx, |input, _| {
        let current = input.realization_diagnostics().current;
        assert_eq!(current.response_custody_count, 0);
        assert_eq!(current.response_custody_bytes, 0);
        assert_eq!(current.response_custody_items, 0);
        assert_eq!(current.request_storage_bytes, 0);
        assert!(!input.realization_continuation_scheduled);
    });
}
