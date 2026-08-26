use super::*;
use crate::PageRequestKey;

fn adjacent_key(id: u64, purpose: PagePurpose, max_payload_bytes: u64) -> PageRequestKey {
    PageRequestKey::adjacent(
        PageRequestId::new(id),
        BindingId::new(71),
        SourceRevision::new(1),
        purpose,
        ByteOffset::new(0),
        PageDirection::Forward,
        max_payload_bytes,
    )
    .unwrap()
}

#[gpui::test]
fn deferred_terminal_capacity_fallback_retains_custody_until_replacement_commits(
    cx: &mut gpui::TestAppContext,
) {
    let source = "line\n".repeat(24);
    let mut configuration = config(2 * 1024 * 1024, 32_768);
    configuration.binding = RangeBinding::new(
        BindingId::new(190),
        SourceRevision::new(1),
        LogicalExtent::new(source.len() as u64, 24),
    );
    configuration.geometry_limits =
        ExactGeometryLimits::new(source.len() as u64, 8, 512 * 1024, 8192).unwrap();
    configuration.limits.page_bytes = source.len() as u64;
    configuration.limits.max_realized_block_extent = px(80.);
    configuration.viewport_extent = px(160.);
    configuration.overscan = Pixels::ZERO;
    let (input, cx) = cx
        .add_window_view(move |window, cx| RangeTextInput::new(configuration, window, cx).unwrap());
    let key = stage_terminal_target_object_response(&input, cx, &source);
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.config.limits.max_realization_work_per_frame = 1;
            input.begin_realization_frame();
            input.spend_realization_credit();
            input
                .defer_geometry_response(
                    super::super::geometry::DeferredGeometryResponse::TargetObject(
                        empty_terminal_object_response(key),
                    ),
                    cx,
                )
                .unwrap();
            assert!(input.deferred_geometry_response.is_some());
            input.config.limits.max_surface_bytes = 1;
            input.config.limits.max_surface_items = 1;

            input.begin_realization_frame();
            assert!(matches!(
                input.service_deferred_geometry_response(window, cx),
                Err(RangeTextInputError::Pending)
            ));
            assert!(input.deferred_geometry_response.is_some());
            assert!(input.dispatched_object_pages.contains(&key));
            assert!(input.pending_target_intent.is_some());

            input.begin_realization_frame();
            assert!(matches!(
                input.service_pending_target_intent(cx),
                Err(RangeTextInputError::SurfaceCapacity)
            ));
            assert!(input.deferred_geometry_response.is_some());
            assert!(input.dispatched_object_pages.contains(&key));

            input.config.limits.max_surface_bytes = 2 * 1024 * 1024;
            input.config.limits.max_surface_items = 32_768;
            input.begin_realization_frame();
            assert!(input.service_pending_target_intent(cx).unwrap().is_some());
            assert!(input.deferred_geometry_response.is_none());
            assert!(!input.dispatched_object_pages.contains(&key));
        })
    });
}

#[gpui::test]
fn page_alias_capacity_waits_peak_and_disposal_are_exact(cx: &mut gpui::TestAppContext) {
    let (input, cx) = cx.add_window_view(|window, cx| {
        RangeTextInput::new(config(2 * 1024 * 1024, 32_768), window, cx).unwrap()
    });
    drive_initial_surface(&input, cx);
    let first_key = adjacent_key(200_000, PagePurpose::PlatformRange, SOURCE.len() as u64);
    let mut delivered = Vec::new();
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            let envelope = PageDemandEnvelope::Adjacent {
                anchor: ByteOffset::new(0),
                direction: PageDirection::Forward,
                max_payload_bytes: SOURCE.len() as u64,
            };
            let first = input
                .residency
                .demand(first_key.id(), first_key.purpose(), envelope)
                .unwrap();
            assert!(matches!(first, PageDemand::Requested(_)));
            input
                .accept_page_demand(PageRequest::new(first_key), first, cx)
                .unwrap();
            assert!(matches!(
                input.take_request(),
                Some(RangeTextInputRequest::Page(request)) if request.key() == first_key
            ));

            for (id, purpose) in [
                (200_001, PagePurpose::PlatformRange),
                (200_002, PagePurpose::PlatformRange),
            ] {
                let key = adjacent_key(id, purpose, SOURCE.len() as u64);
                let demand = input
                    .residency
                    .demand(key.id(), key.purpose(), envelope)
                    .unwrap();
                assert!(matches!(demand, PageDemand::Coalesced(source) if source == first_key));
                input
                    .accept_page_demand(PageRequest::new(key), demand, cx)
                    .unwrap();
            }
            let diagnostics = input.realization_diagnostics();
            assert_eq!(diagnostics.current.page_alias_waits, 2);
            assert_eq!(diagnostics.current.pending_page_requests, 1);
            assert_eq!(diagnostics.current.dispatched_page_requests, 1);
            assert_eq!(
                diagnostics.current.page_alias_storage_bytes,
                input.pending_page_aliases.capacity()
                    * std::mem::size_of::<super::super::page_delivery::PendingPageAlias>()
            );
            assert_eq!(
                diagnostics.current.page_alias_storage_items,
                input.pending_page_aliases.capacity()
            );

            let third_key = adjacent_key(200_003, PagePurpose::PlatformRange, SOURCE.len() as u64);
            let replacement_charge = RangeSurfaceCharge {
                bytes: 3 * std::mem::size_of::<super::super::page_delivery::PendingPageAlias>(),
                items: 3,
            };
            let current = input.current_realization_ownership();
            let exact = RangeSurfaceCharge {
                bytes: current.owned_bytes + replacement_charge.bytes,
                items: current.owned_items + replacement_charge.items,
            };
            input.config.limits.max_surface_bytes = exact.bytes - 1;
            let before = input.pending_page_aliases.len();
            assert!(matches!(
                input.accept_page_demand(
                    PageRequest::new(third_key),
                    PageDemand::Coalesced(first_key),
                    cx
                ),
                Err(RangeTextInputError::SurfaceCapacity)
            ));
            assert_eq!(input.pending_page_aliases.len(), before);
            input.config.limits.max_surface_bytes = exact.bytes;
            input.config.limits.max_surface_items = exact.items;
            input
                .accept_page_demand(
                    PageRequest::new(third_key),
                    PageDemand::Coalesced(first_key),
                    cx,
                )
                .unwrap();
            let diagnostics = input.realization_diagnostics();
            assert_eq!(diagnostics.current.page_alias_waits, 3);
            assert!(diagnostics.high_water.owned_bytes >= exact.bytes);
            assert!(
                diagnostics.high_water.page_alias_storage_bytes
                    >= diagnostics.current.page_alias_storage_bytes
            );

            let page = page_for(PageRequest::new(first_key), 200_000);
            input.config.limits.max_surface_bytes = 2 * 1024 * 1024;
            input.config.limits.max_surface_items = 32_768;
            input.config.limits.max_realization_work_per_frame = 1;
            input
                .admit_response_custody(
                    super::super::response_custody::RangeResponseCustody::AliasFanout(
                        super::super::response_custody::AliasFanout {
                            page,
                            cursor: 0,
                            matched: false,
                        },
                    ),
                )
                .unwrap();
            assert!(matches!(
                input.response_custody.front(),
                Some(super::super::response_custody::RangeResponseCustody::AliasFanout(fanout))
                    if fanout.page.key() == first_key
            ));
            assert!(
                input
                    .pending_page_aliases
                    .iter()
                    .any(|alias| alias.source == first_key)
            );
            let current = input.current_realization_ownership();
            let retained = input
                .response_custody
                .front()
                .and_then(|response| response.page())
                .unwrap()
                .retained_charge();
            let exact_service = RangeSurfaceCharge {
                bytes: current.owned_bytes
                    + retained.bytes()
                    + (retained.bytes() - std::mem::size_of::<RangePage>())
                    + retained.bytes(),
                items: current.owned_items
                    + retained.items()
                    + retained.items().saturating_sub(1)
                    + retained.items(),
            };
            input.config.limits.max_surface_bytes = exact_service.bytes - 1;
            input.config.limits.max_surface_items = exact_service.items;
            input.begin_realization_frame();
            assert!(matches!(
                input.service_response_custody(window, cx),
                Err(RangeTextInputError::SurfaceCapacity)
            ));
            assert_eq!(input.realization_diagnostics().frame.spent, 0);
            assert_eq!(input.pending_page_aliases.len(), 3);
            assert_eq!(input.response_custody.len(), 1);

            input.config.limits.max_surface_bytes = exact_service.bytes;
            input.begin_realization_frame();
            assert!(input.service_response_custody(window, cx).unwrap());
            assert_eq!(input.realization_diagnostics().frame.spent, 1);
            assert_eq!(input.pending_page_aliases.len(), 2);
            assert_eq!(input.response_custody.len(), 2);
            assert!(input.realization_diagnostics().high_water.owned_bytes >= exact_service.bytes);
            input.config.limits.max_surface_bytes = 2 * 1024 * 1024;
            input.config.limits.max_surface_items = 32_768;
            let response = input.response_custody.pop_front().unwrap();
            let super::super::response_custody::RangeResponseCustody::ResidentPage(page) = response
            else {
                unreachable!()
            };
            delivered.push(page.key());
            input.obsolete_realization_continuation();
            while !input.pending_page_aliases.is_empty() {
                let before = input.pending_page_aliases.len();
                input.begin_realization_frame();
                assert!(
                    input
                        .service_response_custody(window, cx)
                        .unwrap_or_else(|error| panic!("alias waits {before}: {error:?}"))
                );
                assert_eq!(input.realization_diagnostics().frame.spent, 1);
                assert_eq!(input.pending_page_aliases.len(), before - 1);
                let response = input.response_custody.pop_front().unwrap();
                let super::super::response_custody::RangeResponseCustody::ResidentPage(page) =
                    response
                else {
                    panic!("one alias clone must precede the compact fan-out continuation")
                };
                delivered.push(page.key());
                assert_eq!(
                    input
                        .response_custody
                        .iter()
                        .filter(|response| matches!(
                            response,
                            super::super::response_custody::RangeResponseCustody::AliasFanout(_)
                        ))
                        .count(),
                    usize::from(before > 1)
                );
            }
            input.obsolete_realization_continuation();
        })
    });
    assert_eq!(delivered.len(), 3);
    assert!(delivered.iter().all(|key| key != &first_key));
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            let _ = input.dispose(window, cx);
            let diagnostics = input.realization_diagnostics();
            assert_eq!(diagnostics.current.page_alias_waits, 0);
            assert_eq!(diagnostics.current.page_alias_storage_bytes, 0);
            assert_eq!(diagnostics.current.page_alias_storage_items, 0);
            assert_eq!(diagnostics.current.pending_page_requests, 0);
        })
    });
}

#[gpui::test]
fn transition_destination_queue_is_preallocated_and_swapped_atomically(
    cx: &mut gpui::TestAppContext,
) {
    let (input, cx) = cx.add_window_view(|window, cx| {
        RangeTextInput::new(config(2 * 1024 * 1024, 32_768), window, cx).unwrap()
    });
    drive_initial_surface(&input, cx);
    input.update(cx, |input, cx| {
        let key = adjacent_key(220_000, PagePurpose::Viewport, SOURCE.len() as u64);
        input
            .push_request(RangeTextInputRequest::ReleasePage(key), cx)
            .unwrap();
        let before = format!("{:?}", input.requests);
        let before_capacity = input.requests.capacity();
        input.prepare_focus_loss_transition().unwrap();
        let components = input.last_widget_admission_components.get().unwrap();
        assert!(components.destination_request_storage.bytes > 0);
        assert!(components.destination_request_storage.items >= input.requests.len());
        let exact = components.checked_total().unwrap();

        input.config.limits.max_surface_bytes = exact.bytes - 1;
        input.config.limits.max_surface_items = exact.items;
        assert!(matches!(
            input.prepare_focus_loss_transition(),
            Err(RangeTextInputError::SurfaceCapacity)
        ));
        assert_eq!(format!("{:?}", input.requests), before);
        assert_eq!(input.requests.capacity(), before_capacity);

        input.config.limits.max_surface_bytes = exact.bytes;
        let candidate = input.prepare_focus_loss_transition().unwrap();
        assert_eq!(format!("{:?}", input.requests), before);
        assert_eq!(input.requests.capacity(), before_capacity);
        input.commit_widget_transition_internal(candidate);
        assert!(matches!(
            input.requests.front(),
            Some(RangeTextInputRequest::ReleasePage(request)) if *request == key
        ));
        assert_eq!(
            input
                .requests
                .iter()
                .filter(|request| matches!(request, RangeTextInputRequest::Page(_)))
                .count(),
            1
        );
        assert_eq!(
            input.requests.capacity(),
            components.destination_request_storage.items
        );
        assert!(input.realization_diagnostics().high_water.owned_bytes >= exact.bytes);
    });
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.config.limits.max_surface_bytes = 2 * 1024 * 1024;
            let _ = input.dispose(window, cx);
            let diagnostics = input.realization_diagnostics();
            assert_eq!(diagnostics.current.request_storage_bytes, 0);
            assert_eq!(diagnostics.current.request_storage_items, 0);
        })
    });
}

#[gpui::test]
fn exact_absolute_scroll_clamps_to_content_minus_composed_viewport(cx: &mut gpui::TestAppContext) {
    let source = "line\n".repeat(100);
    let mut configuration = config(2 * 1024 * 1024, 32_768);
    configuration.binding = RangeBinding::new(
        BindingId::new(191),
        SourceRevision::new(1),
        LogicalExtent::new(source.len() as u64, 100),
    );
    configuration.viewport_extent = px(32.);
    let (input, cx) = cx
        .add_window_view(move |window, cx| RangeTextInput::new(configuration, window, cx).unwrap());
    drive_surface_for_source(&input, cx, &source);
    input.update(cx, |input, cx| {
        let viewport = input.target_intent_desired().viewport_extent;
        input.request_absolute_scroll(px(10_000.), cx).unwrap();
        let expected = (input.surface().unwrap().content_height() - viewport).max(Pixels::ZERO);
        assert!(expected > Pixels::ZERO);
        assert_eq!(input.target_intent_desired().target_block, expected);
        input.begin_realization_frame();
        input.spend_realization_credit();
        input
            .set_realization_viewport_extent(px(2_000.), cx)
            .unwrap();
        input.request_absolute_scroll(px(10_000.), cx).unwrap();
        assert_eq!(input.target_intent_desired().target_block, Pixels::ZERO);
    });
}

#[gpui::test]
fn exhausted_focus_loss_composes_pending_selection_and_scroll_last_wins(
    cx: &mut gpui::TestAppContext,
) {
    let source = "line\n".repeat(100);
    let mut configuration = config(2 * 1024 * 1024, 32_768);
    configuration.binding = RangeBinding::new(
        BindingId::new(192),
        SourceRevision::new(1),
        LogicalExtent::new(source.len() as u64, 100),
    );
    configuration.viewport_extent = px(32.);
    configuration.limits.max_realization_work_per_frame = 1;
    let (input, cx) = cx
        .add_window_view(move |window, cx| RangeTextInput::new(configuration, window, cx).unwrap());
    drive_surface_for_source(&input, cx, &source);
    input.update(cx, |input, cx| {
        input.begin_realization_frame();
        input.spend_realization_credit();
        let position =
            SourcePosition::new(ByteOffset::new(5), crate::InlineObjectGap::no_objects());
        input
            .publish_source_selection(RangeSourceSelection::caret(position), None, None, cx)
            .unwrap();
        input.request_absolute_scroll(px(10_000.), cx).unwrap();
        input
            .pending_target_intent
            .as_mut()
            .unwrap()
            .desired
            .composition = Some(ByteRange::from_u64(0, 1).unwrap());
        let intent = input.focus_loss_intent();
        input.request_target_intent(intent, cx).unwrap();
        let pending = input.pending_target_intent.unwrap().desired;
        assert_eq!(
            pending.source_selection,
            Some(RangeSourceSelection::caret(position))
        );
        assert_eq!(
            pending.target_block,
            (input.surface().unwrap().content_height() - pending.viewport_extent).max(Pixels::ZERO)
        );
        assert_eq!(pending.composition, None);
        assert_ne!(input.desired.source_selection, pending.source_selection);
    });
}

#[gpui::test]
fn restoration_index_admission_rejection_preserves_complete_validation_and_retries(
    cx: &mut gpui::TestAppContext,
) {
    let (input, cx) = cx.add_window_view(|window, cx| {
        RangeTextInput::new(config(2 * 1024 * 1024, 32_768), window, cx).unwrap()
    });
    drive_initial_surface(&input, cx);
    input.update(cx, |input, cx| {
        let position =
            SourcePosition::new(ByteOffset::new(0), crate::InlineObjectGap::no_objects());
        let seed = crate::RangeRestorationSeed {
            binding: input.config.binding,
            caret: position,
            selection: RangeSourceSelection::caret(position),
            scroll: crate::RangeRestorationScrollAnchor {
                position,
                intra_anchor: Pixels::ZERO,
            },
            history: None,
        };
        input.restoration =
            Some(super::super::restoration::RestorationValidation::complete_for_test(seed));
        let desired = input.desired;
        let active = input.active_geometry;
        input.config.limits.max_surface_bytes = 1;
        input.config.limits.max_surface_items = 1;
        input.begin_realization_frame();
        assert!(matches!(
            input.service_pending_restoration_completion(cx),
            Err(RangeTextInputError::SurfaceCapacity)
        ));
        assert_eq!(input.desired, desired);
        assert_eq!(input.active_geometry, active);
        assert!(input.restoration.is_some());
        assert!(input.restoration_seed.is_none());
        assert!(matches!(
            input.request_absolute_scroll(px(16.), cx),
            Err(RangeTextInputError::Busy)
        ));
        assert_eq!(input.desired, desired);

        input.config.limits.max_surface_bytes = 2 * 1024 * 1024;
        input.config.limits.max_surface_items = 32_768;
        input.begin_realization_frame();
        assert!(input.service_pending_restoration_completion(cx).unwrap());
        assert!(input.restoration.is_none());
        assert_eq!(input.restoration_seed, Some(seed));
        assert_eq!(input.desired.source_selection, Some(seed.selection));
        assert!(input.active_geometry.is_some());
    });
}
