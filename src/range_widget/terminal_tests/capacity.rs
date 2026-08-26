use super::*;

#[gpui::test]
fn nonzero_filler_coordinates_route_pointer_keyboard_and_scroll_to_exact_successor(
    cx: &mut gpui::TestAppContext,
) {
    let source = (0..160)
        .map(|line| format!("filler-{line:03}\n"))
        .collect::<String>();
    let mut configuration = config(2 * 1024 * 1024, 32_768);
    configuration.binding = RangeBinding::new(
        BindingId::new(90),
        SourceRevision::new(1),
        LogicalExtent::new(source.len() as u64, 160),
    );
    configuration.viewport_extent = px(80.);
    configuration.limits.max_realized_block_extent = px(16.);
    let (input, cx) = cx
        .add_window_view(move |window, cx| RangeTextInput::new(configuration, window, cx).unwrap());
    cx.simulate_resize(gpui::size(px(800.), px(80.)));
    drive_surface_for_source(&input, cx, &source);
    input.update(cx, |input, cx| {
        input.request_absolute_scroll(px(160.), cx).unwrap();
    });
    drive_surface_for_source(&input, cx, &source);
    let (scroll, filler, origin) = input.read_with(cx, |input, _| {
        let surface = input.surface().unwrap();
        (
            surface.scroll_block(),
            surface.filler().unwrap(),
            input.last_origin(),
        )
    });
    assert!(scroll > Pixels::ZERO);
    assert_eq!(filler.block_start(), scroll + px(16.));
    assert_eq!(filler.block_end(), (scroll + px(80.)).min(px(2_576.)));

    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.pointer_down(
                &MouseDownEvent {
                    position: gpui::point(
                        origin.x + px(500.),
                        origin.y + filler.block_start() - scroll + px(8.),
                    ),
                    modifiers: Modifiers::none(),
                    button: MouseButton::Left,
                    click_count: 1,
                    first_mouse: false,
                },
                window,
                cx,
            );
            assert_eq!(
                input
                    .pending_target_intent
                    .map_or(input.desired, |intent| intent.desired)
                    .realization_anchor_block,
                filler.successor_block()
            );
        })
    });
    drive_surface_for_source(&input, cx, &source);

    let exact_origin = input.read_with(cx, |input, _| input.last_origin());
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.pointer_down(
                &MouseDownEvent {
                    position: gpui::point(exact_origin.x + px(1.), exact_origin.y + px(1.)),
                    modifiers: Modifiers::none(),
                    button: MouseButton::Left,
                    click_count: 1,
                    first_mouse: false,
                },
                window,
                cx,
            );
        })
    });
    drive_surface_for_source(&input, cx, &source);

    if input.read_with(cx, |input, _| input.surface().unwrap().filler().is_none()) {
        input.update(cx, |input, cx| {
            input.request_absolute_scroll(scroll, cx).unwrap();
        });
        drive_surface_for_source(&input, cx, &source);
    }

    let mut keyboard_reanchored = false;
    let mut last_caret_block = None;
    let mut last_filler_start = Pixels::ZERO;
    for _ in 0..32 {
        let (keyboard_successor, caret_block, filler_start) = input.read_with(cx, |input, _| {
            let surface = input.surface().unwrap();
            (
                surface.filler().unwrap().successor_block(),
                surface
                    .caret_bounds(input.config.layout.line_height)
                    .map(|bounds| bounds.origin.y),
                surface.filler().unwrap().block_start(),
            )
        });
        last_caret_block = caret_block;
        last_filler_start = filler_start;
        cx.update(|window, app| {
            input.update(app, |input, cx| {
                input.move_down(&crate::MoveDown, window, cx)
            })
        });
        if input.read_with(cx, |input, _| {
            input
                .pending_target_intent
                .map_or(input.desired, |intent| intent.desired)
                .realization_anchor_block
                == keyboard_successor
        }) {
            keyboard_reanchored = true;
            break;
        }
        drive_surface_for_source(&input, cx, &source);
    }
    assert!(
        keyboard_reanchored,
        "keyboard did not reach filler from caret {last_caret_block:?} before {last_filler_start:?}"
    );
    drive_surface_for_source(&input, cx, &source);
    input.update(cx, |input, cx| {
        input.request_absolute_scroll(Pixels::ZERO, cx).unwrap();
    });
    drive_surface_for_source(&input, cx, &source);
    input.update(cx, |input, cx| {
        input.config.limits.max_realized_block_extent = px(64.);
        input.set_realization_viewport_extent(px(79.), cx).unwrap();
    });
    drive_surface_for_source(&input, cx, &source);
    let (mut layout, style) = input.read_with(cx, |input, _| {
        (input.config.layout.clone(), input.config.style.clone())
    });
    layout.line_height = px(24.);
    input.update(cx, |input, cx| input.set_layout(layout, style, cx).unwrap());
    drive_surface_for_source(&input, cx, &source);
    input.read_with(cx, |input, _| {
        let surface = input.surface().unwrap();
        let filler = surface.filler().unwrap();
        assert_eq!(filler.block_start(), surface.scroll_block() + px(64.));
        assert!(filler.block_end() <= surface.content_height());
    });
}

#[gpui::test]
fn huge_finite_viewport_publishes_one_filler_and_exact_extent(cx: &mut gpui::TestAppContext) {
    let source = (0..256)
        .map(|line| format!("line-{line:03}\n"))
        .collect::<String>();
    let mut configuration = config(2 * 1024 * 1024, 32_768);
    configuration.binding = RangeBinding::new(
        BindingId::new(71),
        SourceRevision::new(1),
        LogicalExtent::new(source.len() as u64, 256),
    );
    configuration.limits.max_realized_block_extent = px(64.);
    let (input, cx) = cx
        .add_window_view(move |window, cx| RangeTextInput::new(configuration, window, cx).unwrap());
    cx.simulate_resize(gpui::size(px(800.), px(1_000_000.)));
    drive_surface_for_source(&input, cx, &source);
    input.read_with(cx, |input, _| {
        assert_eq!(input.desired.viewport_extent, px(1_000_000.));
        assert_eq!(input.desired.realization_extent, px(64.));
        let surface = input.surface().unwrap();
        assert_eq!(
            surface.capacity_state(),
            RangeRealizationCapacityState::ViewportExceedsRenderingCapacity
        );
        assert_eq!(surface.filler_count(), 1);
        assert_eq!(surface.content_height(), px(4_112.));
        let filler = surface.filler().unwrap();
        assert_eq!(filler.block_start(), surface.scroll_block() + px(64.));
        assert_eq!(filler.block_end(), surface.content_height());
        let diagnostics = input.realization_diagnostics();
        assert_eq!(diagnostics.filler_count, 1);
        assert!(
            diagnostics.current.resident_pages
                <= input.config.residency_limits.max_resident_pages()
        );
        assert!(
            diagnostics.current.resident_objects
                <= input.config.object_residency_limits.max_resident_objects()
        );
        assert!(diagnostics.current.candidates <= 1);
    });
    let filler = input.read_with(cx, |input, _| input.surface().unwrap().filler().unwrap());
    input.update(cx, |input, cx| {
        input
            .request_filler_reanchor(filler.block_start(), cx)
            .unwrap();
        assert_eq!(input.desired.target_block, Pixels::ZERO);
        let diagnostics = input.realization_diagnostics();
        assert_eq!(diagnostics.current.candidates, 1);
        assert_eq!(diagnostics.current.candidate_items, 1);
        assert_eq!(
            diagnostics.current.candidate_bytes,
            std::mem::size_of::<SurfaceCandidate>()
        );
        assert_eq!(diagnostics.high_water.candidate_items, 1);
        assert_eq!(
            diagnostics.high_water.candidate_bytes,
            std::mem::size_of::<SurfaceCandidate>()
        );
    });
    rebind_revision(&input, cx, 2);
    input.read_with(cx, |input, _| {
        let diagnostics = input.realization_diagnostics();
        assert_eq!(diagnostics.filler_count, 1);
        assert_eq!(diagnostics.current.resident_pages, 8);
        assert_eq!(diagnostics.current.resident_objects, 0);
        assert!(diagnostics.current.candidates <= 1);
    });
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            let _ = input.dispose(window, cx);
            let diagnostics = input.realization_diagnostics();
            assert_eq!(diagnostics.filler_count, 0);
            assert_eq!(diagnostics.current.resident_pages, 0);
            assert_eq!(diagnostics.current.resident_objects, 0);
            assert_eq!(diagnostics.current.queued_requests, 0);
            assert_eq!(diagnostics.current.candidates, 0);
            assert!(diagnostics.high_water.resident_pages > 0);
        })
    });
}

#[gpui::test]
fn multi_megabyte_generated_host_keeps_all_realization_owners_bounded(
    cx: &mut gpui::TestAppContext,
) {
    const LOGICAL_BYTES: u64 = 2 * 1024 * 1024;
    const PAGE_BYTES: u64 = 4 * 1024;
    let mut configuration = config(8 * 1024 * 1024, 131_072);
    configuration.binding = RangeBinding::new(
        BindingId::new(96),
        SourceRevision::new(1),
        LogicalExtent::new(LOGICAL_BYTES, 1),
    );
    configuration.geometry_limits =
        ExactGeometryLimits::new(PAGE_BYTES, 8, 4 * 1024 * 1024, 65_536).unwrap();
    configuration.residency_limits =
        ResidencyLimits::new(2, (PAGE_BYTES * 2) as usize, 2, PAGE_BYTES * 2).unwrap();
    configuration.limits.page_bytes = PAGE_BYTES;
    configuration.layout.limits.segment_bytes = PAGE_BYTES as usize;
    configuration.layout.limits.glyphs = (PAGE_BYTES * 2) as usize;
    configuration.layout.limits.wraps = (PAGE_BYTES * 2) as usize;
    configuration.layout.limits.maps = (PAGE_BYTES * 2 + 1) as usize;
    configuration.layout.limits.retained_items = (PAGE_BYTES * 16) as usize;
    configuration.layout.limits.retained_bytes = 4 * 1024 * 1024;
    let (input, cx) = cx
        .add_window_view(move |window, cx| RangeTextInput::new(configuration, window, cx).unwrap());
    let mut text_pages = 0usize;
    let mut max_generated_page = 0usize;
    let mut sampled = [false; 3];
    let sample_offsets = [0, LOGICAL_BYTES / 2, LOGICAL_BYTES - 1];
    let max_steps = usize::try_from(LOGICAL_BYTES / PAGE_BYTES)
        .unwrap()
        .checked_add(1)
        .and_then(|pages| pages.checked_mul(8))
        .unwrap();
    for step in 0..max_steps {
        let id = u64::try_from(step).unwrap() + 1;
        cx.update(|window, app| {
            input.update(app, |input, cx| {
                input.begin_realization_frame();
                input
                    .service_admitted_geometry_for_prepaint(window, cx)
                    .unwrap();
            })
        });
        match input.update(cx, |input, _| input.take_request()) {
            Some(RangeTextInputRequest::Page(request)) => {
                let (start, end) = match request.key().demand() {
                    PageDemandEnvelope::Adjacent {
                        anchor,
                        direction: PageDirection::Forward,
                        max_payload_bytes,
                    } => (
                        anchor.get(),
                        anchor
                            .get()
                            .saturating_add(max_payload_bytes)
                            .min(LOGICAL_BYTES),
                    ),
                    PageDemandEnvelope::Adjacent {
                        anchor,
                        direction: PageDirection::Backward,
                        max_payload_bytes,
                    } => (anchor.get().saturating_sub(max_payload_bytes), anchor.get()),
                    PageDemandEnvelope::Validation { .. } => {
                        panic!("generated scale source does not validate a whole value")
                    }
                };
                let generated_len = usize::try_from(end - start).unwrap();
                for (seen, sample) in sampled.iter_mut().zip(sample_offsets) {
                    *seen |= start <= sample && sample < end;
                }
                max_generated_page = max_generated_page.max(generated_len);
                text_pages += 1;
                let page = RangePage::new(
                    PageId::new(id),
                    request.key(),
                    ByteRange::from_u64(start, end).unwrap(),
                    "a".repeat(generated_len),
                    vec![],
                    if start == 0 {
                        PageEdgeFact::DocumentBoundary
                    } else {
                        PageEdgeFact::Continues
                    },
                    if end == LOGICAL_BYTES {
                        PageEdgeFact::DocumentBoundary
                    } else {
                        PageEdgeFact::Continues
                    },
                    end == LOGICAL_BYTES,
                )
                .unwrap();
                cx.update(|window, app| {
                    input.update(app, |input, cx| {
                        input.deliver_page(page, window, cx).unwrap()
                    })
                });
            }
            Some(RangeTextInputRequest::ObjectPage(request)) => {
                let page = ObjectPage::new(
                    ObjectPageId::new(id),
                    request.key(),
                    vec![],
                    ObjectPageEdgeFact::EnvelopeBoundary,
                    ObjectPageEdgeFact::EnvelopeBoundary,
                    true,
                    None,
                )
                .unwrap();
                cx.update(|window, app| {
                    input.update(app, |input, cx| {
                        input
                            .deliver_object_page_in_window(page, window, cx)
                            .unwrap()
                    })
                });
            }
            Some(
                RangeTextInputRequest::ReleasePage(_)
                | RangeTextInputRequest::ReleaseObjectPage(_)
                | RangeTextInputRequest::CancelPage(_)
                | RangeTextInputRequest::CancelObjectPage(_),
            ) => {}
            Some(request) => panic!("unexpected generated-host request: {request:?}"),
            None if input.read_with(cx, |input, _| input.is_quiescent()) => break,
            None => {}
        }
    }
    input.read_with(cx, |input, _| {
        assert!(input.is_quiescent());
        assert!(text_pages >= usize::try_from(LOGICAL_BYTES / PAGE_BYTES).unwrap());
        assert_eq!(sampled, [true; 3]);
        assert!(max_generated_page <= PAGE_BYTES as usize);
        let index = input.geometry.index().expect("terminal exact index");
        assert_eq!(
            index.document_selection().head.byte_offset,
            ByteOffset::new(LOGICAL_BYTES)
        );
        assert!(input.surface().is_some());
        let diagnostics = input.realization_diagnostics();
        assert!(diagnostics.high_water.resident_pages <= diagnostics.max_owned_pages);
        assert!(diagnostics.high_water.resident_objects <= diagnostics.max_owned_objects);
        assert!(
            diagnostics.high_water.pending_page_requests <= diagnostics.max_pending_page_requests
        );
        assert!(
            diagnostics.high_water.pending_object_requests
                <= diagnostics.max_pending_object_requests
        );
        assert!(diagnostics.high_water.candidates <= 1);
        assert!(diagnostics.high_water.checkpoints <= diagnostics.max_checkpoints);
    });
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            assert!(input.dispose(window, cx).is_empty());
        })
    });
    input.read_with(cx, |input, _| {
        let current = input.realization_diagnostics().current;
        assert_eq!(current.resident_pages, 0);
        assert_eq!(current.resident_objects, 0);
        assert_eq!(current.pending_page_requests, 0);
        assert_eq!(current.pending_object_requests, 0);
        assert_eq!(current.dispatched_page_requests, 0);
        assert_eq!(current.dispatched_object_requests, 0);
        assert_eq!(current.active_geometry_jobs, 0);
        assert_eq!(current.pending_geometry_pages, 0);
        assert_eq!(current.pending_geometry_objects, 0);
        assert_eq!(current.deferred_geometry_responses, 0);
        assert_eq!(current.pending_target_intents, 0);
        assert_eq!(current.pending_presentation_intents, 0);
        assert_eq!(current.scheduled_continuations, 0);
        assert_eq!(current.queued_requests, 0);
        assert_eq!(current.candidates, 0);
        assert_eq!(current.checkpoints, 0);
    });
}

#[gpui::test]
fn generated_large_source_keeps_realization_owners_within_configured_caps(
    cx: &mut gpui::TestAppContext,
) {
    let source = "0123456789abcdef\n".repeat(3_856);
    let (input, cx) = cx.add_window_view(|window, cx| {
        let mut configuration = config(2 * 1024 * 1024, 32_768);
        configuration.binding = RangeBinding::new(
            BindingId::new(81),
            SourceRevision::new(1),
            LogicalExtent::new(source.len() as u64, 3_856),
        );
        RangeTextInput::new(configuration, window, cx).unwrap()
    });
    drive_surface_for_source(&input, cx, &source);
    input.read_with(cx, |input, _| {
        let diagnostics = input.realization_diagnostics();
        assert!(
            diagnostics.current.resident_pages
                <= input.config.residency_limits.max_resident_pages()
        );
        assert!(
            diagnostics.current.resident_objects
                <= input.config.object_residency_limits.max_resident_objects()
        );
        assert!(diagnostics.current.queued_requests <= 1);
        assert!(diagnostics.current.candidates <= 1);
        assert!(diagnostics.current.checkpoints <= input.config.geometry_limits.max_checkpoints());
        assert!(
            diagnostics.high_water.resident_pages
                <= input.config.residency_limits.max_resident_pages()
        );
        assert!(
            diagnostics.high_water.resident_objects
                <= input.config.object_residency_limits.max_resident_objects()
        );
        assert!(
            diagnostics.high_water.queued_requests
                <= input
                    .config
                    .residency_limits
                    .max_pending_requests()
                    .saturating_add(input.config.object_residency_limits.max_pending_requests(),)
        );
        assert!(diagnostics.high_water.candidates <= 1);
        assert!(
            diagnostics.high_water.checkpoints <= input.config.geometry_limits.max_checkpoints()
        );
    });
}
