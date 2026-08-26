use super::*;

#[gpui::test]
fn exhausted_frame_coalesces_wheel_scrollbar_selection_and_ime_without_preparing_geometry(
    cx: &mut gpui::TestAppContext,
) {
    let source = (0..100)
        .map(|line| format!("line-{line:03}\n"))
        .collect::<String>();
    let mut configuration = config(2 * 1024 * 1024, 32_768);
    configuration.binding = RangeBinding::new(
        BindingId::new(71),
        SourceRevision::new(1),
        LogicalExtent::new(source.len() as u64, 100),
    );
    let (input, cx) =
        cx.add_window_view(|window, cx| RangeTextInput::new(configuration, window, cx).unwrap());
    drive_surface_for_source(&input, cx, &source);

    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.config.limits.max_realization_work_per_frame = 1;
            input.begin_realization_frame();
            input.request_absolute_scroll(px(16.), cx).unwrap();
            assert_eq!(input.last_realization_step.spent, 1);
            let prepared = (
                input.next_id,
                input.geometry.counts(),
                input.active_geometry,
                input
                    .surface_candidate
                    .as_ref()
                    .map(|candidate| candidate.job),
            );

            input.apply_scrollbar(PendingScroll::Set(px(32.)), window, cx);
            assert_eq!(
                input.pending_target_intent.unwrap().desired.target_block,
                px(32.)
            );
            assert_eq!(
                (
                    input.next_id,
                    input.geometry.counts(),
                    input.active_geometry,
                    input
                        .surface_candidate
                        .as_ref()
                        .map(|candidate| candidate.job),
                ),
                prepared
            );

            input.scroll_wheel(
                &gpui::ScrollWheelEvent {
                    position: gpui::point(px(1.), px(1.)),
                    delta: gpui::ScrollDelta::Pixels(gpui::point(px(0.), px(-16.))),
                    ..Default::default()
                },
                window,
                cx,
            );
            assert!(input.pending_target_intent.unwrap().desired.target_block > px(32.));
            assert_eq!(
                (
                    input.next_id,
                    input.geometry.counts(),
                    input.active_geometry,
                    input
                        .surface_candidate
                        .as_ref()
                        .map(|candidate| candidate.job),
                ),
                prepared
            );

            let selection = RangeSourceSelection::caret(SourcePosition::new(
                ByteOffset::new(200),
                crate::InlineObjectGap::NoObjects,
            ));
            input
                .publish_optional_source_selection(Some(selection), None, None, cx)
                .unwrap();
            assert_eq!(
                input
                    .pending_target_intent
                    .unwrap()
                    .desired
                    .source_selection,
                Some(selection)
            );
            assert_eq!(
                (
                    input.next_id,
                    input.geometry.counts(),
                    input.active_geometry,
                    input
                        .surface_candidate
                        .as_ref()
                        .map(|candidate| candidate.job),
                ),
                prepared
            );

            input
                .pending_target_intent
                .as_mut()
                .unwrap()
                .desired
                .composition = Some(ByteRange::from_u64(190, 200).unwrap());
            assert!(input.clear_composition(cx).unwrap());
            assert_eq!(
                input.pending_target_intent.unwrap().desired.composition,
                None
            );
            assert_eq!(input.last_realization_step.spent, 1);
            assert_eq!(
                (
                    input.next_id,
                    input.geometry.counts(),
                    input.active_geometry,
                    input
                        .surface_candidate
                        .as_ref()
                        .map(|candidate| candidate.job),
                ),
                prepared
            );

            input.begin_realization_frame();
            input.service_pending_target_intent(cx).unwrap();
            assert_eq!(input.last_realization_step.spent, 1);
            assert!(input.pending_target_intent.is_none());
            assert_ne!(input.next_id, prepared.0);
        })
    });
}

#[gpui::test]
fn diagnostics_preserve_exact_residency_across_surface_transfer_and_disposal(
    cx: &mut gpui::TestAppContext,
) {
    let (input, cx) = cx.add_window_view(|window, cx| {
        RangeTextInput::new(config(2 * 1024 * 1024, 32_768), window, cx).unwrap()
    });
    let RangeTextInputRequest::Page(request) =
        input.update(cx, |input, _| input.take_request()).unwrap()
    else {
        panic!("initial geometry page")
    };
    let page = page_for(request, 92_001);
    let page_charge = page.retained_charge();
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.deliver_page(page, window, cx).unwrap()
        })
    });
    input.read_with(cx, |input, _| {
        assert_eq!(input.residency.counts().resident_pages, 1);
        let diagnostics = input.realization_diagnostics();
        assert_eq!(diagnostics.current.resident_pages, 1);
        assert_eq!(diagnostics.current.resident_page_bytes, page_charge.bytes());
        assert_eq!(
            diagnostics.high_water.resident_page_bytes,
            page_charge.bytes()
        );
    });
    drive_initial_surface(&input, cx);
    input.read_with(cx, |input, _| {
        let surface_pages = input.surface().unwrap().pages().len();
        assert!(surface_pages > 0);
        assert_eq!(input.residency.counts().resident_pages, 0);
        let diagnostics = input.realization_diagnostics();
        assert_eq!(diagnostics.current.resident_pages, surface_pages);
        assert_eq!(diagnostics.current.resident_page_bytes, page_charge.bytes());
        assert_eq!(
            diagnostics.high_water.resident_page_bytes,
            page_charge.bytes()
        );
        assert!(diagnostics.high_water.resident_pages >= surface_pages);
    });
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            let _ = input.dispose(window, cx);
        })
    });
    input.read_with(cx, |input, _| {
        let current = input.realization_diagnostics().current;
        assert_eq!(current.resident_pages, 0);
        assert_eq!(current.resident_page_bytes, 0);
        assert_eq!(current.resident_objects, 0);
        assert_eq!(current.resident_object_bytes, 0);
        assert_eq!(current.deferred_geometry_responses, 0);
        assert_eq!(current.deferred_response_bytes, 0);
        assert_eq!(current.deferred_response_items, 0);
        assert_eq!(current.candidates, 0);
    });
}

#[gpui::test]
fn deferred_realization_callback_is_inert_after_disposal(cx: &mut gpui::TestAppContext) {
    let (input, cx) = cx.add_window_view(|window, cx| {
        RangeTextInput::new(config(2 * 1024 * 1024, 32_768), window, cx).unwrap()
    });
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.defer_realization_continuation(window, cx);
            assert!(input.realization_continuation_scheduled);
            let _ = input.dispose(window, cx);
            assert!(!input.realization_continuation_scheduled);
        })
    });
    cx.run_until_parked();
    input.read_with(cx, |input, _| {
        assert!(!input.realization_continuation_scheduled);
        assert_eq!(input.last_realization_step.remaining, 0);
        assert_eq!(
            input.realization_diagnostics().current.active_geometry_jobs,
            0
        );
        assert_eq!(
            input
                .realization_diagnostics()
                .current
                .deferred_geometry_responses,
            0
        );
    });
}

#[gpui::test]
fn short_document_has_no_filler_and_reanchor_is_rejected(cx: &mut gpui::TestAppContext) {
    let (input, cx) = cx.add_window_view(|window, cx| {
        let mut configuration = config(2 * 1024 * 1024, 32_768);
        configuration.limits.max_realized_block_extent = px(64.);
        RangeTextInput::new(configuration, window, cx).unwrap()
    });
    drive_initial_surface(&input, cx);
    input.read_with(cx, |input, _| {
        let surface = input.surface().unwrap();
        assert_eq!(surface.content_height(), px(16.));
        assert_eq!(surface.filler_count(), 0);
        assert_eq!(
            surface.capacity_state(),
            RangeRealizationCapacityState::Normal
        );
    });
    input.update(cx, |input, cx| {
        assert!(matches!(
            input.request_filler_reanchor(px(1.), cx),
            Err(RangeTextInputError::IncompleteSurface)
        ));
    });
}

#[gpui::test]
fn same_cap_viewport_retargets_are_generation_safe_and_last_wins(cx: &mut gpui::TestAppContext) {
    let source = (0..128)
        .map(|line| format!("line-{line:03}\n"))
        .collect::<String>();
    let mut configuration = config(2 * 1024 * 1024, 32_768);
    configuration.binding = RangeBinding::new(
        BindingId::new(83),
        SourceRevision::new(1),
        LogicalExtent::new(source.len() as u64, 128),
    );
    configuration.limits.max_realized_block_extent = px(64.);
    let (input, cx) = cx
        .add_window_view(move |window, cx| RangeTextInput::new(configuration, window, cx).unwrap());
    drive_surface_for_source(&input, cx, &source);
    let late_page = cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.config.limits.max_realization_work_per_frame = 1;
            input.begin_realization_frame();
            input
                .set_realization_viewport_extent(px(1_000.), cx)
                .unwrap();
            assert_eq!(input.last_realization_step.remaining, 0);
            let first_job = input.surface_candidate.as_ref().unwrap().job;
            let stale_request = loop {
                match input.take_request() {
                    Some(RangeTextInputRequest::Page(request)) => break request,
                    Some(
                        RangeTextInputRequest::CancelPage(_)
                        | RangeTextInputRequest::ReleasePage(_)
                        | RangeTextInputRequest::CancelObjectPage(_)
                        | RangeTextInputRequest::ReleaseObjectPage(_),
                    ) => {}
                    Some(request) => panic!("unexpected viewport request: {request:?}"),
                    None => panic!("first viewport target must request a page"),
                }
            };
            input
                .set_realization_viewport_extent(px(2_000.), cx)
                .unwrap();
            let candidate = input.surface_candidate.as_ref().unwrap();
            assert_eq!(candidate.job, first_job);
            assert_eq!(candidate.desired.viewport_extent, px(1_000.));
            assert_eq!(candidate.desired.realization_extent, px(64.));
            assert_eq!(input.desired.viewport_extent, px(1_000.));
            assert_eq!(
                input.pending_target_intent.unwrap().desired.viewport_extent,
                px(2_000.)
            );
            let stale_page = page_for_source(stale_request, 90_001, &source);
            let late_page = stale_page.clone();
            input.deliver_page(stale_page, window, cx).unwrap();
            assert_eq!(input.response_custody.len(), 1);
            assert_eq!(input.desired.viewport_extent, px(1_000.));
            assert_eq!(
                input.pending_target_intent.unwrap().desired.viewport_extent,
                px(2_000.)
            );
            input.begin_realization_frame();
            input.service_pending_target_intent(cx).unwrap();
            assert!(input.response_custody.is_empty());
            assert_eq!(input.desired.viewport_extent, px(2_000.));
            let candidate = input.surface_candidate.as_ref().unwrap();
            assert_ne!(candidate.job, first_job);
            assert_eq!(candidate.desired.viewport_extent, px(2_000.));
            late_page
        })
    });
    let stale = cx.update(|window, app| {
        input.update(app, |input, cx| input.deliver_page(late_page, window, cx))
    });
    assert!(matches!(
        stale,
        Err(RangeTextInputError::PageResponseRejected(_))
    ));
    drive_surface_for_source(&input, cx, &source);
    input.read_with(cx, |input, _| {
        assert_ne!(input.desired.viewport_extent, px(1_000.));
        assert_eq!(input.surface().unwrap().filler_count(), 1);
        assert!(input.realization_diagnostics().current.candidates <= 1);
    });
}
