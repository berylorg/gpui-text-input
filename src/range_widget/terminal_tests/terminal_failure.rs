use super::*;

#[derive(Debug, PartialEq)]
struct PreservedTerminalState {
    surface: Option<String>,
    desired: DesiredFingerprint,
    admission: Option<RangeSurfaceCharge>,
    next_id: u64,
    index: Option<String>,
    text_pages: Vec<String>,
    object_pages: Vec<String>,
    active_object: Option<ActiveInlineObject>,
    restoration_seed: Option<crate::RangeRestorationSeed>,
    published_restoration: Option<crate::RangeRestorationSeed>,
    pending_select_all: bool,
}

fn preserved_terminal_state(input: &RangeTextInput) -> PreservedTerminalState {
    PreservedTerminalState {
        surface: input.surface.as_ref().map(|surface| format!("{surface:?}")),
        desired: desired_fingerprint(input.desired),
        admission: input.last_surface_admission,
        next_id: input.next_id,
        index: input.geometry.index().map(|index| format!("{index:?}")),
        text_pages: input
            .residency
            .resident_pages()
            .map(|page| format!("{page:?}"))
            .collect(),
        object_pages: input
            .object_residency
            .resident_pages()
            .map(|page| format!("{page:?}"))
            .collect(),
        active_object: input.active_object,
        restoration_seed: input.restoration_seed,
        published_restoration: input.published_restoration,
        pending_select_all: input.pending_select_all,
    }
}

fn assert_terminal_failure_closed(input: &RangeTextInput) {
    assert!(input.active_geometry.is_none());
    assert!(input.pending_geometry_page.is_none());
    assert!(input.pending_geometry_object.is_none());
    assert!(input.pending_target_intent.is_none());
    assert!(input.response_custody.is_empty());
    assert_eq!(input.residency.pending_requests().count(), 0);
    assert_eq!(input.object_residency.pending_requests().count(), 0);

    let current = input.realization_diagnostics().current;
    assert_eq!(current.active_geometry_jobs, 0);
    assert_eq!(current.pending_geometry_pages, 0);
    assert_eq!(current.pending_geometry_objects, 0);
    assert_eq!(current.pending_page_requests, 0);
    assert_eq!(current.pending_object_requests, 0);
    assert_eq!(current.deferred_geometry_responses, 0);
    assert_eq!(current.response_custody_count, 0);
    assert_eq!(current.pending_target_intents, 0);
}

#[gpui::test]
fn impossible_terminal_object_retarget_closes_once_without_retry(cx: &mut gpui::TestAppContext) {
    let source = "line\n".repeat(24);
    let mut configuration = config(2 * 1024 * 1024, 32_768);
    configuration.binding = RangeBinding::new(
        BindingId::new(231),
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
    let before = input.update(cx, |input, _| {
        let current = input.desired.source_selection.unwrap().head;
        input.restoration_seed = Some(crate::RangeRestorationSeed {
            binding: input.config.binding,
            caret: current,
            selection: RangeSourceSelection::caret(current),
            scroll: crate::RangeRestorationScrollAnchor {
                position: current,
                intra_anchor: Pixels::ZERO,
            },
            history: None,
        });
        let endpoint = SourcePosition::new(
            ByteOffset::new(source.len() as u64 + 1),
            crate::InlineObjectGap::no_objects(),
        );
        let candidate = input.surface_candidate.as_mut().unwrap();
        candidate.desired.source_selection = Some(RangeSourceSelection::caret(endpoint));
        candidate.desired.reveal_caret = true;
        preserved_terminal_state(input)
    });
    let result = cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.deliver_object_page_in_window(empty_terminal_object_response(key), window, cx)
        })
    });
    assert!(
        matches!(result, Err(RangeTextInputError::IncompleteSurface)),
        "{result:?}"
    );
    input.update(cx, |input, _| {
        assert_eq!(preserved_terminal_state(input), before);
        assert!(input.surface_candidate.is_none());
        assert_terminal_failure_closed(input);
        assert!(!input.dispatched_object_pages.contains(&key));
        assert_eq!(
            input
                .requests
                .iter()
                .filter(|request| matches!(request, RangeTextInputRequest::ReleaseObjectPage(released) if *released == key))
                .count(),
            1
        );
    });
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.begin_realization_frame();
            assert!(!input.service_response_custody(window, cx).unwrap());
            assert!(matches!(
                input.take_request(),
                Some(RangeTextInputRequest::ReleaseObjectPage(released)) if released == key
            ));
            assert!(input.take_request().is_none());
            assert_terminal_failure_closed(input);
            assert_eq!(
                input
                    .realization_diagnostics()
                    .current
                    .dispatched_object_requests,
                0
            );
        })
    });
}

#[gpui::test]
fn delivered_text_then_resident_terminal_incomplete_surface_closes_once(
    cx: &mut gpui::TestAppContext,
) {
    let source = "line\n".repeat(20);
    let mut configuration = super::seal::presentation_config(&source);
    configuration.binding = RangeBinding::new(
        BindingId::new(232),
        SourceRevision::new(1),
        LogicalExtent::new(source.len() as u64, 20),
    );
    configuration.geometry_limits = ExactGeometryLimits::new(4, 8, 512 * 1024, 8192).unwrap();
    configuration.residency_limits = ResidencyLimits::new(1, 128 * 1024, 8, 256).unwrap();
    configuration.object_residency_limits =
        ObjectResidencyLimits::new(32, 32, 128 * 1024, 64 * 1024, 8, 32, 128 * 1024).unwrap();
    configuration.limits.page_bytes = 4;
    let (input, cx) = cx
        .add_window_view(move |window, cx| RangeTextInput::new(configuration, window, cx).unwrap());
    let page = super::seal::stage_terminal_target_text_response(&input, cx, &source);
    let key = page.key();
    input.update(cx, |input, _| {
        input
            .surface_candidate
            .as_mut()
            .unwrap()
            .desired
            .inline_object_interaction = Some(DesiredInlineObjectInteraction::Set {
            object_id: InlineObjectId::new(900_001),
            order: InlineObjectOrder::new(1),
            activation_eligible: true,
            origin: None,
        });
    });
    cx.update(|window, app| input.update(app, |input, cx| input.deliver_page(page, window, cx)))
        .unwrap();
    let (before, object_key) = input.read_with(cx, |input, _| {
        (
            preserved_terminal_state(input),
            input
                .pending_geometry_object
                .as_ref()
                .expect("terminal text delivery advances to resident object validation")
                .request
                .key(),
        )
    });
    let result = cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.begin_realization_frame();
            input.service_geometry_page(window, cx)
        })
    });
    assert!(
        matches!(result, Err(RangeTextInputError::IncompleteSurface)),
        "{result:?}"
    );
    input.update(cx, |input, _| {
        assert_eq!(preserved_terminal_state(input), before);
        assert!(input.surface_candidate.is_none());
        assert_terminal_failure_closed(input);
        assert!(!input.dispatched_pages.contains(&key));
        assert_eq!(
            input
                .requests
                .iter()
                .filter(|request| matches!(request, RangeTextInputRequest::ReleasePage(released) if *released == key))
                .count(),
            1
        );
        assert!(!input.requests.iter().any(
            |request| matches!(request, RangeTextInputRequest::ReleaseObjectPage(released) if *released == object_key)
        ));
    });
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.begin_realization_frame();
            assert!(!input.service_response_custody(window, cx).unwrap());
            assert!(matches!(
                input.take_request(),
                Some(RangeTextInputRequest::ReleasePage(released)) if released == key
            ));
            assert!(input.take_request().is_none());
            assert_terminal_failure_closed(input);
            assert_eq!(
                input
                    .realization_diagnostics()
                    .current
                    .dispatched_page_requests,
                0
            );
        })
    });
}

#[gpui::test]
fn ordinary_terminal_object_publication_still_succeeds(cx: &mut gpui::TestAppContext) {
    let source = "line\n".repeat(24);
    let mut configuration = config(2 * 1024 * 1024, 32_768);
    configuration.binding = RangeBinding::new(
        BindingId::new(233),
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
            input
                .deliver_object_page_in_window(empty_terminal_object_response(key), window, cx)
                .unwrap();
            assert!(input.active_geometry.is_none());
            assert!(input.pending_geometry_object.is_none());
            assert!(input.surface_candidate.is_none());
            assert!(input.response_custody.is_empty());
            assert!(!input.dispatched_object_pages.contains(&key));
            assert!(input.is_surface_current_and_interactive());
            assert_eq!(
                input
                    .requests
                    .iter()
                    .filter(|request| matches!(request, RangeTextInputRequest::ReleaseObjectPage(released) if *released == key))
                    .count(),
                1
            );
        })
    });
}
