use super::*;

#[test]
fn realization_priority_order_is_explicit_and_stable() {
    assert_eq!(
        RangeRealizationPriority::ORDERED,
        [
            RangeRealizationPriority::Caret,
            RangeRealizationPriority::Ime,
            RangeRealizationPriority::DirectedSelection,
            RangeRealizationPriority::ActiveInteraction,
            RangeRealizationPriority::ScrollAnchor,
            RangeRealizationPriority::NearbyContent,
        ]
    );
    let mut desired = DesiredSurface::origin(px(80.), px(64.), Pixels::ZERO);
    assert_eq!(desired.priority(), RangeRealizationPriority::NearbyContent);
    desired.preserve_scroll_anchor = true;
    assert_eq!(desired.priority(), RangeRealizationPriority::ScrollAnchor);
    desired.inline_object_interaction = Some(DesiredInlineObjectInteraction::Set {
        object_id: InlineObjectId::new(1),
        order: InlineObjectOrder::new(1),
        activation_eligible: true,
        origin: None,
    });
    assert_eq!(
        desired.priority(),
        RangeRealizationPriority::ActiveInteraction
    );
    let start = SourcePosition::new(ByteOffset::new(0), crate::InlineObjectGap::no_objects());
    let end = SourcePosition::new(ByteOffset::new(1), crate::InlineObjectGap::no_objects());
    desired.source_selection = Some(RangeSourceSelection {
        anchor: start,
        head: end,
    });
    assert_eq!(
        desired.priority(),
        RangeRealizationPriority::DirectedSelection
    );
    desired.composition = Some(ByteRange::from_u64(0, 1).unwrap());
    assert_eq!(desired.priority(), RangeRealizationPriority::Ime);
    desired.source_selection = Some(RangeSourceSelection::caret(end));
    assert_eq!(desired.priority(), RangeRealizationPriority::Caret);
}

#[gpui::test]
fn construction_revalidates_direct_surface_fields_and_frame_quantum(cx: &mut gpui::TestAppContext) {
    let (_input, _cx) = cx.add_window_view(|window, cx| {
        for invalid in [
            RangeTextInputLimits {
                max_surface_bytes: 0,
                ..config(1024, 128).limits
            },
            RangeTextInputLimits {
                max_surface_items: 0,
                ..config(1024, 128).limits
            },
            RangeTextInputLimits {
                max_realization_work_per_frame: 0,
                ..config(1024, 128).limits
            },
            RangeTextInputLimits {
                max_realized_block_extent: Pixels::ZERO,
                ..config(1024, 128).limits
            },
        ] {
            let mut configuration = config(1024, 128);
            configuration.limits = invalid;
            assert!(matches!(
                RangeTextInput::new(configuration, window, cx),
                Err(RangeTextInputError::InvalidLimits)
            ));
        }
        let mut overflow = config(1024, 128);
        overflow.viewport_extent = px(f32::MAX);
        overflow.limits.max_realized_block_extent = px(f32::MAX);
        overflow.overscan = px(f32::MAX);
        assert!(matches!(
            RangeTextInput::new(overflow, window, cx),
            Err(RangeTextInputError::InvalidLimits)
        ));
        let mut future_viewport_overflow = config(1024, 128);
        future_viewport_overflow.viewport_extent = px(1.);
        future_viewport_overflow.limits.max_realized_block_extent = px(3.0e38);
        future_viewport_overflow.overscan = px(1.0e38);
        assert!(matches!(
            RangeTextInput::new(future_viewport_overflow, window, cx),
            Err(RangeTextInputError::InvalidLimits)
        ));
        let mut aggregate_overflow = config(2 * 1024 * 1024, 32_768);
        aggregate_overflow.residency_limits =
            ResidencyLimits::new(2, 8 * 1024, 2, u64::MAX).unwrap();
        assert!(matches!(
            RangeTextInput::new(aggregate_overflow, window, cx),
            Err(RangeTextInputError::InvalidLimits)
        ));
        let owner = RangeTextInput::realization_owner_charge();
        for limits in [
            RangeTextInputLimits {
                max_surface_bytes: owner.bytes - 1,
                ..config(2 * 1024 * 1024, 32_768).limits
            },
            RangeTextInputLimits {
                max_surface_items: owner.items - 1,
                ..config(2 * 1024 * 1024, 32_768).limits
            },
        ] {
            let mut configuration = config(2 * 1024 * 1024, 32_768);
            configuration.limits = limits;
            assert!(matches!(
                RangeTextInput::new(configuration, window, cx),
                Err(RangeTextInputError::InvalidLimits)
            ));
        }
        RangeTextInput::new(config(2 * 1024 * 1024, 32_768), window, cx).unwrap()
    });
}

#[gpui::test]
fn diagnostics_count_queued_and_dispatched_reservations_after_visibility(
    cx: &mut gpui::TestAppContext,
) {
    let (input, cx) = cx.add_window_view(|window, cx| {
        RangeTextInput::new(config(2 * 1024 * 1024, 32_768), window, cx).unwrap()
    });
    let queued_ownership = input.read_with(cx, |input, _| {
        let diagnostics = input.realization_diagnostics();
        assert_eq!(diagnostics.max_surface_bytes, 2 * 1024 * 1024);
        assert_eq!(diagnostics.max_surface_items, 32_768);
        assert_eq!(diagnostics.max_resident_pages, 8);
        assert_eq!(diagnostics.max_resident_page_bytes, 128 * 1024);
        assert_eq!(diagnostics.max_owned_pages, 16);
        assert_eq!(diagnostics.max_pending_page_requests, 8);
        assert_eq!(diagnostics.max_pending_page_bytes, 256);
        assert_eq!(diagnostics.max_resident_object_pages, 4);
        assert_eq!(diagnostics.max_resident_objects, 32);
        assert_eq!(diagnostics.max_resident_object_bytes, 128 * 1024);
        assert_eq!(diagnostics.max_owned_objects, 64);
        assert_eq!(diagnostics.max_pending_object_requests, 4);
        assert_eq!(diagnostics.max_pending_object_bytes, 128 * 1024);
        assert_eq!(diagnostics.max_geometry_bytes, 512 * 1024);
        assert_eq!(diagnostics.max_geometry_items, 8192);
        assert_eq!(diagnostics.max_checkpoints, 8);
        assert_eq!(diagnostics.current.pending_page_requests, 1);
        assert_eq!(diagnostics.current.pending_page_bytes, 32);
        assert_eq!(diagnostics.current.queued_requests, 1);
        assert_eq!(diagnostics.current.dispatched_page_requests, 0);
        assert!(diagnostics.current.dispatched_record_bytes > 0);
        assert!(diagnostics.current.dispatched_record_items > 0);
        assert_eq!(
            diagnostics.current.request_storage_items,
            diagnostics.max_queued_requests
        );
        assert_eq!(
            diagnostics.current.request_storage_bytes,
            diagnostics.max_queued_requests * std::mem::size_of::<RangeTextInputRequest>()
        );
        diagnostics.current
    });
    let request = input.update(cx, |input, _| input.take_request()).unwrap();
    assert!(matches!(request, RangeTextInputRequest::Page(_)));
    input.read_with(cx, |input, _| {
        let diagnostics = input.realization_diagnostics();
        assert_eq!(diagnostics.current.pending_page_requests, 1);
        assert_eq!(diagnostics.current.pending_page_bytes, 32);
        assert_eq!(diagnostics.current.queued_requests, 0);
        assert_eq!(diagnostics.current.dispatched_page_requests, 1);
        assert_eq!(
            diagnostics.current.dispatched_record_bytes,
            queued_ownership.dispatched_record_bytes
        );
        assert_eq!(
            diagnostics.current.dispatched_record_items,
            queued_ownership.dispatched_record_items
        );
        assert_eq!(
            diagnostics.current.owned_bytes,
            queued_ownership.owned_bytes
        );
        assert_eq!(
            diagnostics.current.owned_items,
            queued_ownership.owned_items
        );
        assert_eq!(
            diagnostics.current.request_storage_items,
            diagnostics.max_queued_requests
        );
        assert_eq!(
            diagnostics.current.request_storage_bytes,
            diagnostics.max_queued_requests * std::mem::size_of::<RangeTextInputRequest>()
        );
        assert_eq!(diagnostics.high_water.queued_requests, 1);
        assert_eq!(diagnostics.high_water.dispatched_page_requests, 1);
        assert_eq!(
            diagnostics.high_water.dispatched_record_bytes,
            queued_ownership.dispatched_record_bytes
        );
        assert_eq!(
            diagnostics.high_water.dispatched_record_items,
            queued_ownership.dispatched_record_items
        );
        assert_eq!(diagnostics.high_water.pending_page_bytes, 32);
    });
}

#[gpui::test]
fn exhausted_frame_admits_one_response_without_geometry_progress_until_next_frame(
    cx: &mut gpui::TestAppContext,
) {
    let (input, cx) = cx.add_window_view(|window, cx| {
        RangeTextInput::new(config(2 * 1024 * 1024, 32_768), window, cx).unwrap()
    });
    let key = stage_terminal_target_object_response(&input, cx, SOURCE);
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.config.limits.max_realization_work_per_frame = 1;
            input.begin_realization_frame();
            input.spend_realization_credit();
            let prior_generation = input.realization_frame_generation;
            let before = input.geometry.counts();
            input
                .deliver_object_page_in_window(empty_terminal_object_response(key), window, cx)
                .unwrap();
            assert_eq!(input.geometry.counts(), before);
            let diagnostics = input.realization_diagnostics();
            assert_eq!(diagnostics.frame.spent, 1, "{diagnostics:?}");
            assert_eq!(diagnostics.frame.remaining, 0);
            assert_eq!(diagnostics.current.response_custody_count, 1);
            assert_eq!(diagnostics.current.scheduled_continuations, 1);
            assert_eq!(diagnostics.high_water.scheduled_continuations, 1);
            for _ in 0..3 {
                input
                    .service_geometry_until_external_boundary(window, cx)
                    .unwrap();
                assert_eq!(input.geometry.counts(), before);
                assert_eq!(input.last_realization_step.spent, 1);
            }
            input.begin_realization_frame();
            input
                .service_admitted_geometry_for_prepaint(window, cx)
                .unwrap();
            assert!(input.realization_frame_generation > prior_generation);
            assert!(input.dispatched_object_pages.contains(&key));
            assert_eq!(input.response_custody.len(), 1);
            for _ in 0..64 {
                if !input.dispatched_object_pages.contains(&key) {
                    break;
                }
                input.begin_realization_frame();
                let _ = input.service_response_custody(window, cx);
                let _ = input.service_pending_target_intent(cx);
                input
                    .service_admitted_geometry_for_prepaint(window, cx)
                    .unwrap();
                assert!(input.last_realization_step.spent <= 1);
            }
            assert!(
                !input.dispatched_object_pages.contains(&key),
                "{:?}",
                input.realization_diagnostics()
            );
            assert!(input.surface().is_some());
            assert_eq!(
                input
                    .realization_diagnostics()
                    .current
                    .response_custody_count,
                0
            );
        })
    });
}

#[gpui::test]
fn deferred_text_response_exact_fit_and_one_under_are_atomic_and_retryable(
    cx: &mut gpui::TestAppContext,
) {
    for under in [None, Some((1usize, 0usize)), Some((0usize, 1usize))] {
        let (input, cx) = cx.add_window_view(|window, cx| {
            RangeTextInput::new(config(2 * 1024 * 1024, 32_768), window, cx).unwrap()
        });
        let RangeTextInputRequest::Page(request) =
            input.update(cx, |input, _| input.take_request()).unwrap()
        else {
            panic!("initial geometry text request")
        };
        let key = request.key();
        let page = page_for(request, 91_100);
        let exact = input.read_with(cx, |input, _| {
            let current = input.current_realization_ownership();
            RangeSurfaceCharge {
                bytes: current.owned_bytes
                    + (page.retained_charge().bytes() - std::mem::size_of::<RangePage>())
                    + std::mem::size_of::<super::super::geometry::DeferredGeometryResponse>()
                    + page.retained_charge().bytes(),
                items: current.owned_items
                    + (page.retained_charge().items() - 1)
                    + 1
                    + page.retained_charge().items(),
            }
        });
        cx.update(|_window, app| {
            input.update(app, |input, cx| {
                input.config.limits.max_realization_work_per_frame = 1;
                input.begin_realization_frame();
                input.spend_realization_credit();
                input.config.limits.max_surface_bytes = exact
                    .bytes
                    .checked_sub(under.map_or(0, |value| value.0))
                    .unwrap();
                input.config.limits.max_surface_items = exact
                    .items
                    .checked_sub(under.map_or(0, |value| value.1))
                    .unwrap();
                let result = input.defer_geometry_response(
                    super::super::geometry::DeferredGeometryResponse::TargetPage(page.clone()),
                    cx,
                );
                if under.is_none() {
                    assert!(result.is_ok(), "exact={exact:?}: {result:?}");
                    assert!(input.deferred_geometry_response.is_some());
                    assert!(input.dispatched_pages.contains(&key));
                    assert_eq!(
                        input
                            .realization_diagnostics()
                            .current
                            .deferred_response_bytes,
                        page.retained_charge().bytes() - std::mem::size_of::<RangePage>()
                    );
                    assert_eq!(
                        input
                            .realization_diagnostics()
                            .current
                            .deferred_response_items,
                        page.retained_charge().items() - 1
                    );
                } else {
                    assert!(matches!(result, Err(RangeTextInputError::SurfaceCapacity)));
                    assert!(input.deferred_geometry_response.is_none());
                    assert!(input.dispatched_pages.contains(&key));
                    input.config.limits.max_surface_bytes = exact.bytes;
                    input.config.limits.max_surface_items = exact.items;
                    input
                        .defer_geometry_response(
                            super::super::geometry::DeferredGeometryResponse::TargetPage(page),
                            cx,
                        )
                        .unwrap();
                    assert!(input.deferred_geometry_response.is_some());
                    assert!(input.dispatched_pages.contains(&key));
                }
            })
        });
    }
}

#[gpui::test]
fn deferred_object_response_exact_fit_and_one_under_are_atomic_and_retryable(
    cx: &mut gpui::TestAppContext,
) {
    for under in [None, Some((1usize, 0usize)), Some((0usize, 1usize))] {
        let (input, cx) = cx.add_window_view(|window, cx| {
            RangeTextInput::new(config(2 * 1024 * 1024, 32_768), window, cx).unwrap()
        });
        let key = stage_terminal_target_object_response(&input, cx, SOURCE);
        let page = empty_terminal_object_response(key);
        let exact = input.read_with(cx, |input, _| {
            let current = input.current_realization_ownership();
            RangeSurfaceCharge {
                bytes: current.owned_bytes
                    + (page.retained_charge().bytes() - std::mem::size_of::<ObjectPage>())
                    + std::mem::size_of::<super::super::geometry::DeferredGeometryResponse>()
                    + page.retained_charge().bytes(),
                items: current.owned_items
                    + page.retained_charge().allocated_items()
                    + 1
                    + page.retained_charge().allocated_items()
                    + 1,
            }
        });
        cx.update(|_window, app| {
            input.update(app, |input, cx| {
                input.config.limits.max_realization_work_per_frame = 1;
                input.begin_realization_frame();
                input.spend_realization_credit();
                input.config.limits.max_surface_bytes = exact
                    .bytes
                    .checked_sub(under.map_or(0, |value| value.0))
                    .unwrap();
                input.config.limits.max_surface_items = exact
                    .items
                    .checked_sub(under.map_or(0, |value| value.1))
                    .unwrap();
                let result = input.defer_geometry_response(
                    super::super::geometry::DeferredGeometryResponse::TargetObject(page.clone()),
                    cx,
                );
                if under.is_none() {
                    assert!(result.is_ok(), "exact={exact:?}: {result:?}");
                    assert!(input.deferred_geometry_response.is_some());
                    assert!(input.dispatched_object_pages.contains(&key));
                    assert_eq!(
                        input
                            .realization_diagnostics()
                            .current
                            .deferred_response_bytes,
                        page.retained_charge().bytes() - std::mem::size_of::<ObjectPage>()
                    );
                    assert_eq!(
                        input
                            .realization_diagnostics()
                            .current
                            .deferred_response_items,
                        page.retained_charge().allocated_items()
                    );
                    assert_eq!(
                        input
                            .realization_diagnostics()
                            .current
                            .pending_geometry_record_items,
                        1
                    );
                    assert_eq!(
                        input
                            .realization_diagnostics()
                            .current
                            .pending_geometry_record_bytes,
                        std::mem::size_of::<super::super::geometry::PendingGeometryObject>()
                    );
                } else {
                    assert!(matches!(result, Err(RangeTextInputError::SurfaceCapacity)));
                    assert!(input.deferred_geometry_response.is_none());
                    assert!(input.dispatched_object_pages.contains(&key));
                    input.config.limits.max_surface_bytes = exact.bytes;
                    input.config.limits.max_surface_items = exact.items;
                    input
                        .defer_geometry_response(
                            super::super::geometry::DeferredGeometryResponse::TargetObject(page),
                            cx,
                        )
                        .unwrap();
                    assert!(input.deferred_geometry_response.is_some());
                    assert!(input.dispatched_object_pages.contains(&key));
                }
            })
        });
    }
}

#[gpui::test]
fn ordinary_target_replacement_retires_deferred_response_and_obsoletes_its_continuation(
    cx: &mut gpui::TestAppContext,
) {
    let (input, cx) = cx.add_window_view(|window, cx| {
        RangeTextInput::new(config(2 * 1024 * 1024, 32_768), window, cx).unwrap()
    });
    let key = stage_terminal_target_object_response(&input, cx, SOURCE);
    cx.update(|_window, app| {
        input.update(app, |input, cx| {
            input.config.limits.max_realization_work_per_frame = 1;
            input.begin_realization_frame();
            input.spend_realization_credit();
            let before = input.geometry.counts();
            input
                .defer_geometry_response(
                    super::super::geometry::DeferredGeometryResponse::TargetObject(
                        empty_terminal_object_response(key),
                    ),
                    cx,
                )
                .unwrap();
            let deferred_generation = input.realization_frame_generation;
            let deferred = input.realization_diagnostics().current;
            assert_eq!(deferred.deferred_geometry_responses, 1);
            assert_eq!(
                deferred.deferred_response_bytes,
                empty_terminal_object_response(key)
                    .retained_charge()
                    .bytes()
                    - std::mem::size_of::<ObjectPage>()
            );
            assert_eq!(deferred.deferred_response_items, 0);
            assert!(input.dispatched_object_pages.contains(&key));

            input.request_absolute_scroll(px(16.), cx).unwrap();
            let pending = input.realization_diagnostics().current;
            assert_eq!(input.geometry.counts(), before);
            assert_eq!(pending.deferred_geometry_responses, 1);
            assert_eq!(pending.deferred_response_bytes, 0);
            assert!(input.dispatched_object_pages.contains(&key));
            assert_eq!(input.object_residency.counts().pending_requests, 1);
            assert!(input.pending_target_intent.is_some());
            assert_eq!(input.realization_frame_generation, deferred_generation);
            assert_eq!(input.last_realization_step.spent, 1);

            let surface_bytes = input.config.limits.max_surface_bytes;
            input.config.limits.max_surface_bytes =
                RangeTextInput::realization_owner_charge().bytes;
            input.begin_realization_frame();
            assert!(matches!(
                input.service_pending_target_intent(cx),
                Err(RangeTextInputError::SurfaceCapacity)
            ));
            assert!(input.pending_target_intent.is_some());
            assert!(input.deferred_geometry_response.is_some());
            assert!(input.dispatched_object_pages.contains(&key));

            input.config.limits.max_surface_bytes = surface_bytes;
            input.begin_realization_frame();
            input.service_pending_target_intent(cx).unwrap();
            assert_eq!(input.last_realization_step.spent, 1);
            assert!(input.deferred_geometry_response.is_none());
            assert!(!input.dispatched_object_pages.contains(&key));
            assert!(input.realization_frame_generation > deferred_generation);
        })
    });
}
