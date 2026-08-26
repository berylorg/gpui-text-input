use super::*;

fn presentation_config(source: &str) -> RangeTextInputConfig {
    let mut configuration = config(2 * 1024 * 1024, 32_768);
    configuration.binding = RangeBinding::new(
        BindingId::new(210),
        SourceRevision::new(1),
        LogicalExtent::new(source.len() as u64, 100),
    );
    configuration.geometry_limits = ExactGeometryLimits::new(32, 8, 512 * 1024, 8192).unwrap();
    configuration.viewport_extent = px(32.);
    configuration.overscan = Pixels::ZERO;
    configuration.limits.max_realization_work_per_frame = 1;
    configuration
}

fn stage_terminal_target_text_response(
    input: &gpui::Entity<RangeTextInput>,
    cx: &mut gpui::VisualTestContext,
    source: &str,
) -> RangePage {
    drive_surface_for_source(input, cx, source);
    input.update(cx, |input, cx| {
        for (index, start) in (0..source.len()).step_by(4).enumerate() {
            install_empty_object_page_for_range(
                input,
                500_000 + index as u64,
                ObjectPurpose::GeometryTarget,
                ByteRange::from_u64(start as u64, (start + 4).min(source.len()) as u64).unwrap(),
            );
        }
        input.begin_realization_frame();
        input.request_absolute_scroll(px(160.), cx).unwrap()
    });
    for id in 220_000..221_000 {
        let Some(request) = input.update(cx, |input, _| input.take_request()) else {
            cx.update(|window, app| {
                input.update(app, |input, cx| {
                    input.begin_realization_frame();
                    input
                        .service_geometry_until_external_boundary(window, cx)
                        .unwrap();
                })
            });
            continue;
        };
        match request {
            RangeTextInputRequest::Page(request)
                if request.key().purpose() == PagePurpose::GeometryTarget =>
            {
                let page = page_for_source(request, id, source);
                if page.range().end().get() == 24 {
                    return page;
                }
                cx.update(|window, app| {
                    input.update(app, |input, cx| {
                        input.deliver_page(page, window, cx).unwrap()
                    })
                });
            }
            RangeTextInputRequest::Page(request) => {
                let page = page_for_source(request, id, source);
                cx.update(|window, app| {
                    input.update(app, |input, cx| {
                        input.deliver_page(page, window, cx).unwrap()
                    })
                });
            }
            RangeTextInputRequest::ObjectPage(request) => {
                let page = empty_terminal_object_response(request.key());
                cx.update(|window, app| {
                    input.update(app, |input, cx| {
                        input
                            .deliver_object_page_in_window(page, window, cx)
                            .unwrap()
                    })
                });
            }
            RangeTextInputRequest::ReleasePage(_)
            | RangeTextInputRequest::ReleaseObjectPage(_)
            | RangeTextInputRequest::CancelPage(_)
            | RangeTextInputRequest::CancelObjectPage(_) => {}
            other => panic!("unexpected target request: {other:?}"),
        }
    }
    panic!("target text response was not requested")
}

#[gpui::test]
fn exhausted_target_and_presentation_intents_compose_in_both_orders(cx: &mut gpui::TestAppContext) {
    let source = "line\n".repeat(100);
    for reverse in [false, true] {
        let configuration = presentation_config(&source);
        let (input, cx) = cx.add_window_view(move |window, cx| {
            RangeTextInput::new(configuration, window, cx).unwrap()
        });
        drive_surface_for_source(&input, cx, &source);
        let position =
            SourcePosition::new(ByteOffset::new(30), crate::InlineObjectGap::no_objects());
        input.update(cx, |input, cx| {
            input.begin_realization_frame();
            input.spend_realization_credit();
            if reverse {
                input
                    .set_presentation_generation(PresentationGeneration::new(2), cx)
                    .unwrap();
            }
            input
                .publish_source_selection(RangeSourceSelection::caret(position), None, None, cx)
                .unwrap();
            input.request_absolute_scroll(px(96.), cx).unwrap();
            input
                .set_presentation_generation(PresentationGeneration::new(3), cx)
                .unwrap();
            let pending = input.pending_target_intent.unwrap().desired;
            assert_eq!(
                pending.source_selection,
                Some(RangeSourceSelection::caret(position))
            );
            assert_eq!(pending.target_block, px(96.));
            assert_eq!(
                input.pending_presentation_intent,
                Some(PresentationGeneration::new(3))
            );
        });
        drive_surface_for_source(&input, cx, &source);
        input.read_with(cx, |input, _| {
            let surface = input.surface().unwrap();
            assert_eq!(
                input.config.presentation_generation,
                PresentationGeneration::new(3)
            );
            assert_eq!(surface.selection(), RangeSourceSelection::caret(position));
            assert_eq!(surface.scroll_block(), px(96.));
            assert!(input.pending_target_intent.is_none());
            assert!(input.pending_presentation_intent.is_none());
            assert!(input.is_quiescent());
        });
    }
}

#[gpui::test]
fn constructor_admits_complete_initial_peak_exactly(cx: &mut gpui::TestAppContext) {
    let exact_probe = std::rc::Rc::new(std::cell::Cell::new(None));
    let ownership_probe = std::rc::Rc::new(std::cell::Cell::new(None));
    let components_probe = std::rc::Rc::new(std::cell::Cell::new(None));
    let exact = {
        let exact_capture = exact_probe.clone();
        let ownership_capture = ownership_probe.clone();
        let components_capture = components_probe.clone();
        let (input, cx) = cx.add_window_view(move |window, cx| {
            let input = RangeTextInput::new(config(2 * 1024 * 1024, 32_768), window, cx).unwrap();
            exact_capture.set(input.last_surface_admission_charge());
            ownership_capture.set(Some(input.current_realization_ownership()));
            components_capture.set(input.last_widget_admission_components.get());
            input
        });
        input.read_with(cx, |_, _| {
            let exact = exact_probe.get().unwrap();
            let ownership = ownership_probe.get().unwrap();
            assert!(
                ownership.owned_bytes <= exact.bytes,
                "{ownership:?} {exact:?}"
            );
            assert!(
                ownership.owned_items <= exact.items,
                "{ownership:?} {exact:?} {:?}",
                components_probe.get()
            );
            exact
        })
    };
    let accepted_probe = std::rc::Rc::new(std::cell::Cell::new(None));
    let accepted_capture = accepted_probe.clone();
    let (accepted, cx) = cx.add_window_view(move |window, cx| {
        let mut configuration = config(exact.bytes, exact.items);
        configuration.limits.max_surface_bytes = exact.bytes;
        configuration.limits.max_surface_items = exact.items;
        let input = RangeTextInput::new(configuration, window, cx).unwrap();
        accepted_capture.set(input.last_surface_admission_charge());
        input
    });
    accepted.read_with(cx, |input, _| {
        assert_eq!(accepted_probe.get(), Some(exact));
        assert!(input.mounted);
    });
    for (bytes, items) in [
        (exact.bytes - 1, exact.items),
        (exact.bytes, exact.items - 1),
    ] {
        let (_, _cx) = cx.add_window_view(move |window, cx| {
            let mut configuration = config(2 * 1024 * 1024, 32_768);
            configuration.limits.max_surface_bytes = bytes;
            configuration.limits.max_surface_items = items;
            let result = RangeTextInput::new(configuration, window, cx);
            assert!(
                matches!(
                    &result,
                    Err(RangeTextInputError::SurfaceCapacity | RangeTextInputError::InvalidLimits)
                ),
                "under=({bytes}, {items}) exact={exact:?} result={:?}",
                result
                    .as_ref()
                    .map(|input| input.last_surface_admission_charge())
            );
            RangeTextInput::new(config(2 * 1024 * 1024, 32_768), window, cx).unwrap()
        });
    }
}

#[gpui::test]
fn direct_terminal_text_capacity_fallback_retains_custody_until_commit(
    cx: &mut gpui::TestAppContext,
) {
    let source = "line\n".repeat(20);
    let mut configuration = presentation_config(&source);
    configuration.binding = RangeBinding::new(
        BindingId::new(211),
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
    let page = stage_terminal_target_text_response(&input, cx, &source);
    let key = page.key();
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.begin_realization_frame();
            input.spend_realization_credit();
            input
                .defer_geometry_response(
                    super::super::geometry::DeferredGeometryResponse::TargetPage(page),
                    cx,
                )
                .unwrap();
            assert!(input.deferred_geometry_response.is_some());
            assert!(input.dispatched_pages.contains(&key));

            input.config.limits.max_surface_bytes = 1;
            input.config.limits.max_surface_items = 1;
            input.begin_realization_frame();
            let result = input.service_deferred_geometry_response(window, cx);
            assert!(
                matches!(result, Err(RangeTextInputError::Pending)),
                "{result:?}, desired={:?}, pending={}",
                input
                    .surface_candidate
                    .as_ref()
                    .map(|candidate| candidate.desired),
                input.pending_target_intent.is_some()
            );
            assert!(input.deferred_geometry_response.is_some());
            assert!(input.dispatched_pages.contains(&key));
            assert!(input.pending_target_intent.is_some());

            input.begin_realization_frame();
            assert!(matches!(
                input.service_pending_target_intent(cx),
                Err(RangeTextInputError::SurfaceCapacity)
            ));
            assert!(input.deferred_geometry_response.is_some());
            assert!(input.dispatched_pages.contains(&key));

            input.config.limits.max_surface_bytes = 2 * 1024 * 1024;
            input.config.limits.max_surface_items = 32_768;
            input.begin_realization_frame();
            assert!(input.service_pending_target_intent(cx).unwrap().is_some());
            assert!(input.deferred_geometry_response.is_none());
            assert!(!input.dispatched_pages.contains(&key));
        })
    });
}

#[gpui::test]
fn terminal_publication_reclamps_viewport_reflow_and_content_shrink(cx: &mut gpui::TestAppContext) {
    let source = format!("{}line", "line\n".repeat(99));
    let mut configuration = presentation_config(&source);
    configuration.limits.max_realization_work_per_frame = 8;
    let (input, cx) = cx
        .add_window_view(move |window, cx| RangeTextInput::new(configuration, window, cx).unwrap());
    cx.simulate_resize(gpui::size(px(800.), px(32.)));
    drive_surface_for_source(&input, cx, &source);
    input.read_with(cx, |input, _| {
        assert_eq!(input.desired.viewport_extent, px(32.));
    });
    input.update(cx, |input, cx| {
        input.request_absolute_scroll(px(10_000.), cx).unwrap()
    });
    input.read_with(cx, |input, _| {
        assert_eq!(input.target_intent_desired().target_block, px(1_568.));
    });
    drive_surface_for_source(&input, cx, &source);
    input.read_with(cx, |input, _| {
        let surface = input.surface().unwrap();
        assert_eq!(
            surface.scroll_block(),
            (surface.content_height() - px(32.)).max(Pixels::ZERO),
            "desired={:?} priority={:?}",
            input.desired,
            input.desired.priority()
        );
    });

    cx.simulate_resize(gpui::size(px(800.), px(80.)));
    drive_surface_for_source(&input, cx, &source);
    input.read_with(cx, |input, _| {
        let surface = input.surface().unwrap();
        assert_eq!(
            surface.scroll_block(),
            (surface.content_height() - px(80.)).max(Pixels::ZERO)
        );
    });

    let (mut layout, style) = input.read_with(cx, |input, _| {
        (input.config.layout.clone(), input.config.style.clone())
    });
    layout.line_height = px(8.);
    input.update(cx, |input, cx| input.set_layout(layout, style, cx).unwrap());
    drive_surface_for_source(&input, cx, &source);
    input.read_with(cx, |input, _| {
        let surface = input.surface().unwrap();
        assert_eq!(
            surface.scroll_block(),
            (surface.content_height() - px(80.)).max(Pixels::ZERO)
        );
    });

    let short = "short";
    let binding = RangeBinding::new(
        BindingId::new(210),
        SourceRevision::new(2),
        LogicalExtent::new(short.len() as u64, 1),
    );
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.rebind(binding, None, window, cx).unwrap()
        })
    });
    drive_surface_for_source(&input, cx, short);
    input.read_with(cx, |input, _| {
        assert_eq!(input.surface().unwrap().scroll_block(), Pixels::ZERO);
    });
}
