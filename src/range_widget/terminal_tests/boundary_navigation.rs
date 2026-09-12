use super::*;

#[gpui::test]
fn one_boundary_selection_resolves_a_nonresident_endpoint(cx: &mut gpui::TestAppContext) {
    let source = "word ".repeat(128);
    let mut configuration = config(2 * 1024 * 1024, 32_768);
    configuration.binding = RangeBinding::new(
        BindingId::new(800),
        SourceRevision::new(1),
        LogicalExtent::new(source.len() as u64, 1),
    );
    configuration.viewport_extent = px(80.);
    configuration.limits.max_realized_block_extent = px(16.);
    let (input, cx) = cx
        .add_window_view(move |window, cx| RangeTextInput::new(configuration, window, cx).unwrap());
    drive_surface_for_source(&input, cx, &source);
    input.read_with(cx, |input, _| {
        assert!(
            input
                .surface()
                .unwrap()
                .source_position_for_byte(
                    ByteOffset::new(640),
                    crate::SegmentationDirection::Forward
                )
                .is_none()
        );
    });
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input
                .begin_boundary_from(
                    ByteOffset::new(0),
                    crate::SegmentationKind::LogicalLine,
                    crate::SegmentationDirection::Forward,
                    super::super::interaction::PendingBoundaryAction::Move {
                        extend: true,
                        direction: crate::SegmentationDirection::Forward,
                        selection: input.surface().unwrap().selection(),
                    },
                    window,
                    cx,
                )
                .unwrap();
        })
    });
    drive_surface_for_source_from(&input, cx, &source, 10_000);
    input.read_with(cx, |input, _| {
        let surface = input.surface().unwrap();
        assert_eq!(surface.selection().anchor.byte_offset, ByteOffset::new(0));
        assert_eq!(surface.selection().head.byte_offset, ByteOffset::new(640));
        assert!(surface.caret_bounds(px(16.)).is_some());
        assert!(input.pending_boundary_move.is_none());
    });
}

fn request_move(
    input: &mut RangeTextInput,
    cx: &mut gpui::Context<RangeTextInput>,
) -> crate::ObjectRequestKey {
    input.begin_realization_frame();
    input
        .retain_boundary_move(
            ByteOffset::new(4),
            crate::SegmentationDirection::Forward,
            true,
            input.surface().unwrap().selection(),
            cx,
        )
        .unwrap();
    let Some(RangeTextInputRequest::ObjectPage(request)) = input.take_request() else {
        panic!("exact endpoint request");
    };
    request.key()
}

#[gpui::test]
fn disabling_a_pending_boundary_move_cancels_its_exact_request(cx: &mut gpui::TestAppContext) {
    let (input, cx) = cx.add_window_view(|window, cx| {
        RangeTextInput::new(config(2 * 1024 * 1024, 32_768), window, cx).unwrap()
    });
    drive_initial_surface(&input, cx);
    input.update(cx, |input, cx| {
        let original = input.surface().unwrap().selection();
        let key = request_move(input, cx);
        input.set_enabled(false, cx);
        assert!(input.pending_boundary_move.is_none());
        assert!(!input.dispatched_object_pages.contains(&key));
        assert!(matches!(input.take_request(), Some(RangeTextInputRequest::CancelObjectPage(cancelled)) if cancelled == key));
        input.set_enabled(true, cx);
        input.service_pending_boundary_move(cx).unwrap();
        assert_eq!(input.surface().unwrap().selection(), original);
    });
}

#[gpui::test]
fn rebinding_a_pending_boundary_move_cancels_once(cx: &mut gpui::TestAppContext) {
    let (input, cx) = cx.add_window_view(|window, cx| {
        RangeTextInput::new(config(2 * 1024 * 1024, 32_768), window, cx).unwrap()
    });
    drive_initial_surface(&input, cx);
    let key = input.update(cx, request_move);
    rebind_revision(&input, cx, 2);
    input.update(cx, |input, _| {
        assert!(input.pending_boundary_move.is_none());
        assert_eq!(input.requests.iter().filter(|request| matches!(request, RangeTextInputRequest::CancelObjectPage(cancelled) if *cancelled == key)).count(), 1);
    });
}

#[test]
fn partial_object_pages_prove_only_the_closed_directional_gap() {
    use crate::{InlineObjectGap, InlineObjectNeighbor, ObjectRequestKey, SegmentationDirection};
    let offset = ByteOffset::new(4);
    for direction in [ObjectDirection::Forward, ObjectDirection::Backward] {
        let key = ObjectRequestKey::new(
            ObjectRequestId::new(100),
            binding().binding(),
            binding().revision(),
            PresentationGeneration::new(1),
            ObjectPurpose::Selection,
            ObjectDemandEnvelope::anchor(offset, None, direction, 1, 16 * 1024).unwrap(),
        )
        .unwrap();
        let object = InlineObjectFact::new(
            InlineObjectId::new(2),
            offset,
            InlineObjectOrder::new(2),
            "object",
            InlineObjectPresentation::new(
                2,
                SharedString::new_static("object"),
                px(16.),
                px(16.),
                px(12.),
                None,
                0,
                true,
            )
            .unwrap(),
        );
        let cursor = object.cursor();
        let before = ObjectPageEdgeFact::Continues(cursor);
        let after = ObjectPageEdgeFact::Continues(cursor);
        let forward = direction == ObjectDirection::Forward;
        let page = ObjectPage::new(
            ObjectPageId::new(100),
            key,
            vec![object],
            if forward {
                ObjectPageEdgeFact::EnvelopeBoundary
            } else {
                before
            },
            if forward {
                after
            } else {
                ObjectPageEdgeFact::EnvelopeBoundary
            },
            false,
            Some(cursor),
        )
        .unwrap();
        let neighbor = InlineObjectNeighbor::new(InlineObjectId::new(2), InlineObjectOrder::new(2));
        let closed = if forward {
            SegmentationDirection::Forward
        } else {
            SegmentationDirection::Reverse
        };
        let open = if forward {
            SegmentationDirection::Reverse
        } else {
            SegmentationDirection::Forward
        };
        let expected = if forward {
            InlineObjectGap::Before(neighbor)
        } else {
            InlineObjectGap::After(neighbor)
        };
        assert_eq!(
            super::super::boundary_navigation::boundary_endpoint(&page, offset, closed),
            Some(SourcePosition::new(offset, expected))
        );
        assert_eq!(
            super::super::boundary_navigation::boundary_endpoint(&page, offset, open),
            None
        );
    }
}

#[gpui::test]
fn boundary_response_waits_in_custody_for_release_capacity(cx: &mut gpui::TestAppContext) {
    for windowless in [false, true] {
        let (input, cx) = cx.add_window_view(|window, cx| {
            RangeTextInput::new(config(2 * 1024 * 1024, 32_768), window, cx).unwrap()
        });
        drive_initial_surface(&input, cx);
        let key = input.update(cx, |input, cx| {
            let key = request_move(input, cx);
            let capacity = input.requests.capacity();
            input
                .requests
                .extend((0..capacity).map(|_| RangeTextInputRequest::ReleaseObjectPage(key)));
            key
        });
        let page = ObjectPage::new(
            ObjectPageId::new(100_000),
            key,
            vec![],
            ObjectPageEdgeFact::EnvelopeBoundary,
            ObjectPageEdgeFact::EnvelopeBoundary,
            true,
            None,
        )
        .unwrap();
        cx.update(|window, app| input.update(app, |input, cx| {
            if windowless { input.deliver_object_page(page, cx).unwrap(); }
            else { input.deliver_object_page_in_window(page, window, cx).unwrap(); }
            assert_eq!(input.response_custody.len(), 1);
            assert!(input.dispatched_object_pages.contains(&key));
            assert!(input.pending_boundary_move.is_some());
            assert_eq!(input.requests.len(), input.requests.capacity());
            input.requests.clear();
            input.begin_realization_frame();
            assert!(custody_progressed(input.service_response_custody(window, cx)));
            assert!(input.response_custody.is_empty());
            assert!(!input.dispatched_object_pages.contains(&key));
            assert_eq!(input.requests.iter().filter(|request| matches!(request, RangeTextInputRequest::ReleaseObjectPage(released) if *released == key)).count(), 1);
        }));
        drive_surface_for_source_from(&input, cx, SOURCE, 110_000);
        assert_eq!(
            input.read_with(cx, |input, _| input
                .surface()
                .unwrap()
                .selection()
                .head
                .byte_offset),
            ByteOffset::new(4)
        );
    }
}

#[gpui::test]
fn disable_then_enable_does_not_resume_inflight_boundary_segmentation(
    cx: &mut gpui::TestAppContext,
) {
    let (input, cx) = cx.add_window_view(|window, cx| {
        RangeTextInput::new(config(2 * 1024 * 1024, 32_768), window, cx).unwrap()
    });
    drive_initial_surface(&input, cx);
    cx.update(|window, app| input.update(app, |input, cx| {
        input.select_word_right(&crate::actions::SelectWordRight, window, cx);
        let Some(RangeTextInputRequest::Page(request)) = input.take_request() else { panic!("segmentation request"); };
        assert_eq!(request.key().purpose(), PagePurpose::Segmentation);
        input.set_enabled(false, cx);
        input.set_enabled(true, cx);
        assert!(input.segmentation.is_none());
        assert!(input.segmentation_action.is_none());
        assert!(matches!(input.take_request(), Some(RangeTextInputRequest::CancelPage(key)) if key == request.key()));
        let late = page_for(request, 120_000);
        assert!(input.deliver_page(late, window, cx).is_err());
        assert_eq!(input.surface().unwrap().selection().head.byte_offset, ByteOffset::new(0));
    }));
}

#[gpui::test]
fn a_late_boundary_endpoint_cannot_replace_a_newer_selection(cx: &mut gpui::TestAppContext) {
    let (input, cx) = cx.add_window_view(|window, cx| {
        RangeTextInput::new(config(2 * 1024 * 1024, 32_768), window, cx).unwrap()
    });
    drive_initial_surface(&input, cx);
    let replacement = RangeSourceSelection::caret(SourcePosition::new(
        ByteOffset::new(8),
        crate::InlineObjectGap::NoObjects,
    ));
    input.update(cx, |input, cx| {
        let key = request_move(input, cx);
        input
            .publish_source_selection(replacement, None, None, cx)
            .unwrap();
        input.begin_realization_frame();
        let page = ObjectPage::new(
            ObjectPageId::new(130_000),
            key,
            vec![],
            ObjectPageEdgeFact::EnvelopeBoundary,
            ObjectPageEdgeFact::EnvelopeBoundary,
            true,
            None,
        )
        .unwrap();
        assert!(matches!(
            input.deliver_object_page(page, cx),
            Err(RangeTextInputError::Stale)
        ));
        assert!(input.pending_boundary_move.is_none());
        assert!(!input.dispatched_object_pages.contains(&key));
    });
    drive_surface_for_source_from(&input, cx, SOURCE, 140_000);
    assert_eq!(
        input.read_with(cx, |input, _| input.surface().unwrap().selection()),
        replacement
    );
}
