use super::*;

fn service_frame(
    input: &mut RangeTextInput,
    window: &mut gpui::Window,
    cx: &mut gpui::Context<RangeTextInput>,
) -> Result<(), (&'static str, RangeTextInputError)> {
    input.begin_realization_frame();
    if input.pending_boundary_move.is_some() {
        input.defer_realization_continuation(window, cx);
    }
    match input.service_response_custody(window, cx) {
        super::super::response_custody::ResponseCustodyProgress::Rejected(error) => {
            return Err(("response_custody", error));
        }
        super::super::response_custody::ResponseCustodyProgress::Idle
        | super::super::response_custody::ResponseCustodyProgress::Progressed
        | super::super::response_custody::ResponseCustodyProgress::AcceptedTerminal => {}
        progress => panic!("unexpected response custody progress: {progress:?}"),
    }
    input
        .service_pending_configuration_intent(cx)
        .map_err(|error| ("pending_configuration", error))?;
    input
        .service_pending_rebind_intent(window, cx)
        .map_err(|error| ("pending_rebind", error))?;
    let bounds = input
        .last_bounds
        .expect("initial surface has realized bounds");
    if bounds.size.height > Pixels::ZERO && f32::from(bounds.size.height).is_finite() {
        input
            .set_realization_viewport_extent(bounds.size.height, cx)
            .map_err(|error| ("viewport_extent", error))?;
    }
    input
        .service_pending_target_intent(cx)
        .map_err(|error| ("pending_target", error))?;
    input
        .service_admitted_geometry_for_prepaint(window, cx)
        .map_err(|error| ("resident_geometry", error))
}

fn assert_bounds(input: &RangeTextInput) {
    let diagnostics = input.realization_diagnostics();
    assert!(
        diagnostics.current.owned_bytes <= diagnostics.max_surface_bytes,
        "{diagnostics:?}"
    );
    assert!(
        diagnostics.current.owned_items <= diagnostics.max_surface_items,
        "{diagnostics:?}"
    );
    assert!(
        diagnostics.high_water.owned_bytes <= diagnostics.max_surface_bytes,
        "{diagnostics:?}"
    );
    assert!(
        diagnostics.high_water.owned_items <= diagnostics.max_surface_items,
        "{diagnostics:?}"
    );
    assert!(
        diagnostics.frame.spent <= diagnostics.max_realization_work_per_frame,
        "{diagnostics:?}"
    );
    assert_eq!(
        diagnostics.frame.spent + diagnostics.frame.remaining,
        diagnostics.max_realization_work_per_frame,
        "{diagnostics:?}"
    );
}

fn complete_layout(
    input: &gpui::Entity<RangeTextInput>,
    cx: &mut gpui::VisualTestContext,
    source: &str,
) {
    for _ in 0..64 {
        let result = cx
            .update(|window, app| input.update(app, |input, cx| service_frame(input, window, cx)));
        if let Err((stage, error)) = result {
            input.read_with(cx, |input, _| {
                panic!(
                    "layout service failed at {stage}: {error:?}; state={:?}; diagnostics={:?}",
                    fingerprint(input),
                    input.realization_diagnostics(),
                );
            });
        }
        input.read_with(cx, |input, _| assert_bounds(input));
        let request = input.update(cx, |input, _| input.take_request());
        match request {
            Some(RangeTextInputRequest::Page(request)) => {
                let page = page_for_source(request, request.key().id().get(), source);
                cx.update(|window, app| {
                    input.update(app, |input, cx| {
                        input
                            .deliver_page(page, window, cx)
                            .unwrap_or_else(|error| {
                                panic!(
                                    "layout text delivery failed: {error:?}; state={:?}",
                                    fingerprint(input)
                                )
                            });
                    });
                });
            }
            Some(RangeTextInputRequest::ObjectPage(request)) => {
                let page = ObjectPage::new(
                    ObjectPageId::new(request.key().id().get()),
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
                            .unwrap_or_else(|error| {
                                panic!(
                                    "layout object delivery failed: {error:?}; state={:?}",
                                    fingerprint(input)
                                )
                            });
                    });
                });
            }
            Some(RangeTextInputRequest::ReleasePage(_))
            | Some(RangeTextInputRequest::ReleaseObjectPage(_))
            | Some(RangeTextInputRequest::CancelPage(_))
            | Some(RangeTextInputRequest::CancelObjectPage(_)) => {}
            None => {}
            other => panic!("unexpected source-preserving layout request: {other:?}"),
        }
        cx.run_until_parked();
        if input.read_with(cx, |input, _| {
            input.is_surface_current_and_interactive() && input.is_quiescent()
        }) {
            return;
        }
    }
    input.read_with(cx, |input, _| {
        panic!(
            "resident layout did not complete within bounded service frames; state={:?}; diagnostics={:?}",
            fingerprint(input),
            input.realization_diagnostics(),
        );
    });
}

#[gpui::test]
fn resident_layout_replacement_completes_after_exhausted_frame(cx: &mut gpui::TestAppContext) {
    let source = "saved draft 1";
    assert_eq!(source.len(), 13);
    let mut configuration = config(2 * 1024 * 1024, 32_768);
    configuration.binding = RangeBinding::new(
        BindingId::new(71),
        SourceRevision::new(1),
        LogicalExtent::new(source.len() as u64, 1),
    );
    configuration.limits.max_realization_work_per_frame = 64;
    let original_binding = configuration.binding;
    let (input, cx) =
        cx.add_window_view(|window, cx| RangeTextInput::new(configuration, window, cx).unwrap());
    let original_input = input.entity_id();
    drive_surface_for_source(&input, cx, source);
    for _ in 0..2 {
        input.update(cx, |input, cx| {
            assert!(input.is_surface_current_and_interactive());
            assert!(input.is_quiescent());
            let surface = input.surface().unwrap();
            assert_eq!(surface.binding(), original_binding);
            assert_eq!(surface.source_caret().byte_offset.get(), 0);
            let layout = input.resident_layout_snapshot();
            let diagnostics = input.realization_diagnostics();
            assert_eq!(diagnostics.max_realization_work_per_frame, 64);
            let requests = diagnostics.frame.remaining.checked_add(1).unwrap();
            assert!(requests <= diagnostics.max_realization_work_per_frame + 1);
            for _ in 0..requests {
                input
                    .set_layout(layout.layout.clone(), layout.style.clone(), cx)
                    .unwrap();
            }
            assert!(input.pending_layout_intent.is_some());
            assert!(!input.is_surface_current_and_interactive());
            assert_eq!(input.realization_diagnostics().frame.remaining, 0);
            assert_bounds(input);
        });
        complete_layout(&input, cx, source);
        assert_eq!(input.entity_id(), original_input);
        input.read_with(cx, |input, _| {
            assert_eq!(input.config.binding, original_binding);
            assert_eq!(input.surface().unwrap().binding(), original_binding);
            assert_eq!(input.surface().unwrap().source_caret().byte_offset.get(), 0);
            assert!(input.is_surface_current_and_interactive());
            assert!(input.is_quiescent());
            assert_eq!(input.realization_diagnostics().response_rejection_count, 0);
            assert_bounds(input);
        });
    }
}

#[derive(Default)]
struct ResizeSourceProgress {
    notifications: std::cell::Cell<usize>,
    callbacks: std::cell::Cell<usize>,
    requests: std::cell::Cell<usize>,
}

fn observe_resize_source(
    input: &gpui::Entity<RangeTextInput>,
    cx: &mut gpui::VisualTestContext,
    source: &'static str,
    progress: std::rc::Rc<ResizeSourceProgress>,
) -> gpui::Subscription {
    cx.update(|window, app| {
        let handle = window.window_handle();
        app.observe(input, move |input, app| {
            let notifications = progress.notifications.get() + 1;
            progress.notifications.set(notifications);
            assert!(notifications <= 128, "resize source notification bound");
            let input = input.downgrade();
            let progress = progress.clone();
            app.defer(move |app| {
                progress.callbacks.set(progress.callbacks.get() + 1);
                let input = input.upgrade().expect("resize input remains alive");
                handle
                    .update(app, |_, window, app| {
                        loop {
                            let request = input.update(app, |input, _| input.take_request());
                            let Some(request) = request else {
                                return;
                            };
                            let requests = progress.requests.get() + 1;
                            progress.requests.set(requests);
                            assert!(requests <= 128, "resize source request bound");
                            match request {
                                RangeTextInputRequest::Page(request) => {
                                    let page = page_for_source(
                                        request,
                                        request.key().id().get(),
                                        source,
                                    );
                                    input.update(app, |input, cx| {
                                        input.deliver_page(page, window, cx).unwrap_or_else(
                                            |error| {
                                                panic!(
                                                    "resize text delivery failed: {error:?}; state={:?}; diagnostics={:?}",
                                                    fingerprint(input),
                                                    input.realization_diagnostics(),
                                                )
                                            },
                                        );
                                    });
                                }
                                RangeTextInputRequest::ObjectPage(request) => {
                                    let page = ObjectPage::new(
                                        ObjectPageId::new(request.key().id().get()),
                                        request.key(),
                                        vec![],
                                        ObjectPageEdgeFact::EnvelopeBoundary,
                                        ObjectPageEdgeFact::EnvelopeBoundary,
                                        true,
                                        None,
                                    )
                                    .unwrap();
                                    input.update(app, |input, cx| {
                                        input
                                            .deliver_object_page_in_window(page, window, cx)
                                            .unwrap_or_else(|error| {
                                                panic!(
                                                    "resize object delivery failed: {error:?}; state={:?}; diagnostics={:?}",
                                                    fingerprint(input),
                                                    input.realization_diagnostics(),
                                                )
                                            });
                                    });
                                }
                                RangeTextInputRequest::ReleasePage(_)
                                | RangeTextInputRequest::ReleaseObjectPage(_)
                                | RangeTextInputRequest::CancelPage(_)
                                | RangeTextInputRequest::CancelObjectPage(_) => {}
                                request => panic!("unexpected resize source request: {request:?}"),
                            }
                            input.read_with(app, |input, _| assert_bounds(input));
                        }
                    })
                    .unwrap();
            });
        })
    })
}

#[gpui::test]
fn resized_layout_completes_with_automatic_continuation(cx: &mut gpui::TestAppContext) {
    let source = "saved draft 1";
    assert_eq!(source.len(), 13);
    let mut configuration = config(2 * 1024 * 1024, 32_768);
    configuration.binding = RangeBinding::new(
        BindingId::new(71),
        SourceRevision::new(1),
        LogicalExtent::new(source.len() as u64, 1),
    );
    configuration.limits.max_realization_work_per_frame = 64;
    let original_binding = configuration.binding;
    let (input, cx) =
        cx.add_window_view(|window, cx| RangeTextInput::new(configuration, window, cx).unwrap());
    let original_input = input.entity_id();
    drive_surface_for_source(&input, cx, source);
    let (original_width, original_generation) = input.read_with(cx, |input, _| {
        assert!(input.is_surface_current_and_interactive());
        assert!(input.is_quiescent());
        assert_eq!(input.surface().unwrap().binding(), original_binding);
        assert_eq!(input.surface().unwrap().source_caret().byte_offset.get(), 0);
        assert_eq!(
            input
                .realization_diagnostics()
                .max_realization_work_per_frame,
            64
        );
        assert_bounds(input);
        (
            input.config.layout.wrap_width,
            input.realization_frame_generation,
        )
    });
    let progress = std::rc::Rc::new(ResizeSourceProgress::default());
    let _source_subscription = observe_resize_source(&input, cx, source, progress.clone());
    let viewport = cx.update(|window, _| window.viewport_size());
    let resized = gpui::size(viewport.width + px(79.), viewport.height + px(17.));
    cx.simulate_resize(resized);
    cx.update(|window, app| window.draw(app).clear());
    input.read_with(cx, |input, _| {
        assert_ne!(input.config.layout.wrap_width, original_width);
        assert_eq!(input.config.layout.wrap_width, resized.width);
        assert_eq!(input.last_bounds.unwrap().size.width, resized.width);
        assert!(input.realization_frame_generation > original_generation);
        assert_bounds(input);
    });
    cx.run_until_parked();
    assert_eq!(input.entity_id(), original_input);
    input.read_with(cx, |input, _| {
        assert_eq!(input.config.binding, original_binding);
        assert_eq!(input.config.layout.wrap_width, resized.width);
        assert_eq!(input.realization_diagnostics().response_rejection_count, 0);
        assert_bounds(input);
        assert!(
            input.is_surface_current_and_interactive() && input.is_quiescent(),
            "resized layout did not complete through automatic continuation; notifications={}; callbacks={}; requests={}; state={:?}; diagnostics={:?}",
            progress.notifications.get(),
            progress.callbacks.get(),
            progress.requests.get(),
            fingerprint(input),
            input.realization_diagnostics(),
        );
        assert_eq!(input.surface().unwrap().binding(), original_binding);
        assert_eq!(input.surface().unwrap().source_caret().byte_offset.get(), 0);
    });
}

fn ready_measured_input(
    cx: &mut gpui::TestAppContext,
) -> (gpui::Entity<RangeTextInput>, &mut gpui::VisualTestContext) {
    let mut configuration = config(2 * 1024 * 1024, 32_768);
    configuration.binding = RangeBinding::new(
        BindingId::new(71),
        SourceRevision::new(1),
        LogicalExtent::new(13, 1),
    );
    configuration.limits.max_realization_work_per_frame = 64;
    let (input, cx) =
        cx.add_window_view(|window, cx| RangeTextInput::new(configuration, window, cx).unwrap());
    drive_surface_for_source(&input, cx, "saved draft 1");
    input.read_with(cx, |input, _| {
        assert!(input.is_surface_current_and_interactive());
        assert!(input.is_quiescent());
    });
    (input, cx)
}

fn on_next_resize(
    input: &gpui::Entity<RangeTextInput>,
    cx: &mut gpui::VisualTestContext,
    setup: impl FnOnce(&mut RangeTextInput, &mut gpui::Window, &mut gpui::Context<RangeTextInput>)
    + 'static,
) -> gpui::Subscription {
    let mut setup = Some(setup);
    cx.update(|window, app| {
        input.update(app, |_, cx| {
            cx.observe_window_bounds(window, move |input, window, cx| {
                if let Some(setup) = setup.take() {
                    setup(input, window, cx);
                }
            })
        })
    })
}

#[gpui::test]
fn measured_layout_coalesces_and_uses_latest_accepted_style(cx: &mut gpui::TestAppContext) {
    let (input, cx) = ready_measured_input(cx);
    let progress = std::rc::Rc::new(ResizeSourceProgress::default());
    let _source = observe_resize_source(&input, cx, "saved draft 1", progress.clone());
    let style = StreamingGeometryStyle::new(
        TextRun {
            len: 0,
            font: font(".SystemUIFont"),
            color: gpui::white(),
            background_color: None,
            underline: None,
            strikethrough: None,
        },
        StreamingOversizePresentation::new(
            SharedString::new(Arc::<str>::from("")),
            vec![],
            px(13.),
            px(17.),
            px(13.),
            None,
        ),
    );
    let expected_style = format!("{style:?}");
    let viewport = cx.update(|window, _| window.viewport_size());
    let resized = gpui::size(viewport.width + px(50.), viewport.height);
    let configured = Rc::new(std::cell::Cell::new(false));
    let observed = configured.clone();
    let _resize = on_next_resize(&input, cx, move |input, window, cx| {
        let original = input.config.layout.wrap_width;
        let generation = input.realization_frame_generation;
        input.record_measured_layout(original + px(20.), window, cx);
        input.record_measured_layout(original + px(30.), window, cx);
        assert_eq!(input.config.layout.wrap_width, original);
        assert_eq!(input.realization_frame_generation, generation);
        assert!(input.measured_layout_callback_scheduled);
        assert!(!input.is_surface_current_and_interactive());
        assert!(!input.is_quiescent());
        let mut explicit = input.config.layout.clone();
        explicit.font_size = px(13.);
        explicit.line_height = px(17.);
        input.set_layout(explicit, style, cx).unwrap();
        assert!(input.measured_layout.is_none());
        assert!(input.measured_layout_callback_scheduled);
        let latest = window.viewport_size().width;
        assert_eq!(latest, resized.width);
        input.record_measured_layout(original + px(40.), window, cx);
        input.record_measured_layout(latest, window, cx);
        observed.set(true);
    });
    cx.simulate_resize(resized);
    cx.run_until_parked();
    assert!(configured.get());
    input.read_with(cx, |input, _| {
        assert_eq!(input.config.layout.wrap_width, resized.width);
        assert_eq!(input.last_bounds.unwrap().size.width, resized.width);
        assert_eq!(input.config.layout.font_size, px(13.));
        assert_eq!(format!("{:?}", input.config.style), expected_style);
        assert!(input.measured_layout.is_none());
        assert!(!input.measured_layout_callback_scheduled);
        assert!(
            input.is_surface_current_and_interactive(),
            "{:?}",
            fingerprint(input)
        );
        assert!(input.is_quiescent(), "{:?}", fingerprint(input));
        assert_bounds(input);
    });
}

#[gpui::test]
fn measured_layout_rejection_is_terminal_and_keeps_coherent_paint(cx: &mut gpui::TestAppContext) {
    let (input, cx) = ready_measured_input(cx);
    let events = Rc::new(RefCell::new(Vec::new()));
    let observed = events.clone();
    let _subscription = cx.update(|_, app| {
        app.subscribe(&input, move |input, event, app| {
            if let RangeTextInputEvent::MeasuredLayoutRejected(receipt) = event {
                assert_eq!(input.read(app).measured_layout_rejection(), Some(*receipt));
                let mut events = observed.borrow_mut();
                assert!(events.len() < 2);
                events.push(*receipt);
            }
        })
    });
    let viewport = cx.update(|window, _| window.viewport_size());
    let resized = gpui::size(viewport.width + px(37.), viewport.height);
    let captured = Rc::new(RefCell::new(None));
    let observed = captured.clone();
    let _resize = on_next_resize(&input, cx, move |input, window, cx| {
        let geometry = input.surface().unwrap().geometry_key();
        let limits = input.config.limits;
        let ownership = input.current_realization_ownership();
        input.config.limits.max_surface_bytes = ownership.owned_bytes;
        input.config.limits.max_surface_items = ownership.owned_items;
        assert!(ownership.owned_bytes <= input.config.limits.max_surface_bytes);
        assert!(ownership.owned_items <= input.config.limits.max_surface_items);
        let width = window.viewport_size().width;
        assert_eq!(width, resized.width);
        let mut layout = input.config.layout.clone();
        layout.wrap_width = width;
        assert!(matches!(
            input.prepare_layout_transition(layout, input.config.style.clone(), None),
            Err(RangeTextInputError::SurfaceCapacity)
        ));
        input.record_measured_layout(width, window, cx);
        *observed.borrow_mut() = Some((width, geometry, limits));
    });
    cx.simulate_resize(resized);
    cx.run_until_parked();
    let (width, geometry, limits) = captured
        .borrow_mut()
        .take()
        .expect("physical resize setup ran");
    let receipt = input.read_with(cx, |input, _| {
        let receipt = input
            .measured_layout_rejection()
            .expect("actual admission refusal");
        assert_eq!(receipt.binding, input.config.binding);
        assert_eq!(receipt.wrap_width, width);
        assert_eq!(input.surface().unwrap().geometry_key(), geometry);
        assert!(!input.is_surface_current_and_interactive());
        assert!(input.is_quiescent(), "{:?}", fingerprint(input));
        let diagnostics = input.realization_diagnostics();
        assert!(diagnostics.current.owned_bytes <= diagnostics.max_surface_bytes);
        assert!(diagnostics.current.owned_items <= diagnostics.max_surface_items);
        receipt
    });
    assert_eq!(&*events.borrow(), &[receipt]);
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            let generation = input.realization_frame_generation;
            input.record_measured_layout(width, window, cx);
            input.record_measured_layout(width, window, cx);
            assert!(!input.measured_layout_callback_scheduled);
            assert_eq!(input.realization_frame_generation, generation);
            let layout = input.config.layout.clone();
            assert!(
                input
                    .set_layout(layout, input.config.style.clone(), cx)
                    .is_err()
            );
            assert_eq!(input.measured_layout_rejection(), Some(receipt));
            input.config.limits = limits;
            let layout = input.config.layout.clone();
            input
                .set_layout(layout, input.config.style.clone(), cx)
                .unwrap();
            assert!(input.measured_layout_rejection().is_none());
        })
    });
    drive_surface_for_source(&input, cx, "saved draft 1");
    assert_eq!(&*events.borrow(), &[receipt]);
    input.read_with(cx, |input, _| {
        assert!(input.is_surface_current_and_interactive());
        assert!(input.is_quiescent());
        assert_bounds(input);
    });
}

#[gpui::test]
fn superseded_measured_rejection_event_fails_authentication(cx: &mut gpui::TestAppContext) {
    let (input, cx) = ready_measured_input(cx);
    let events = Rc::new(RefCell::new(Vec::new()));
    let observed = events.clone();
    let _subscription = cx.update(|_, app| {
        app.subscribe(&input, move |input, event, app| {
            if let RangeTextInputEvent::MeasuredLayoutRejected(receipt) = event {
                observed.borrow_mut().push((
                    *receipt,
                    input.read(app).measured_layout_rejection() == Some(*receipt),
                ));
            }
        })
    });
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            let original = input.config.layout.wrap_width;
            let geometry = input.surface().unwrap().geometry_key();
            let limits = input.config.limits;
            let ownership = input.current_realization_ownership();
            input.config.limits.max_surface_bytes = ownership.owned_bytes;
            input.config.limits.max_surface_items = ownership.owned_items;
            input.record_measured_layout(original + px(23.), window, cx);
            input.service_measured_layout(window, cx);
            assert!(input.measured_layout_rejection().is_some());
            input.config.limits = limits;
            input.record_measured_layout(original, window, cx);
            assert!(input.measured_layout_rejection().is_none());
            assert_eq!(input.surface().unwrap().geometry_key(), geometry);
        })
    });
    cx.run_until_parked();
    let events = events.borrow();
    assert_eq!(events.len(), 1);
    assert!(!events[0].1);
    input.read_with(cx, |input, _| {
        assert!(!input.measured_layout_callback_scheduled);
        assert!(input.is_surface_current_and_interactive());
        assert!(input.is_quiescent());
    });
}

#[gpui::test]
fn measured_layout_waits_without_adding_frame_credit(cx: &mut gpui::TestAppContext) {
    let (input, cx) = ready_measured_input(cx);
    let progress = std::rc::Rc::new(ResizeSourceProgress::default());
    let _source = observe_resize_source(&input, cx, "saved draft 1", progress);
    let viewport = cx.update(|window, _| window.viewport_size());
    let resized = gpui::size(viewport.width + px(19.), viewport.height);
    let witnessed = Rc::new(std::cell::Cell::new(false));
    let observed = witnessed.clone();
    let _resize = on_next_resize(&input, cx, move |input, window, cx| {
        let original = input.config.layout.wrap_width;
        let remaining = input.realization_diagnostics().frame.remaining;
        for _ in 0..remaining {
            input.spend_realization_credit();
        }
        let width = window.viewport_size().width;
        assert_eq!(width, resized.width);
        input.record_measured_layout(width, window, cx);
        let input = cx.entity();
        cx.defer(move |app| {
            let input = input.read(app);
            assert_eq!(input.config.layout.wrap_width, original);
            assert!(input.has_pending_measured_layout());
            assert!(!input.measured_layout_callback_scheduled);
            assert!(input.realization_continuation_scheduled);
            assert_eq!(input.realization_diagnostics().frame.spent, 64);
            assert_eq!(input.realization_diagnostics().frame.remaining, 0);
            assert!(!input.is_surface_current_and_interactive());
            assert!(!input.is_quiescent());
            assert_bounds(input);
            observed.set(true);
        });
    });
    cx.simulate_resize(resized);
    cx.run_until_parked();
    assert!(witnessed.get());
    input.read_with(cx, |input, _| {
        assert_eq!(input.config.layout.wrap_width, resized.width);
        assert!(
            input.is_surface_current_and_interactive(),
            "{:?}",
            fingerprint(input)
        );
        assert!(input.is_quiescent(), "{:?}", fingerprint(input));
        assert_bounds(input);
    });
}

#[gpui::test]
fn measured_layout_respects_protection_and_stale_disposal(cx: &mut gpui::TestAppContext) {
    let (input, cx) = ready_measured_input(cx);
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.set_enabled(false, cx);
            let original = input.config.layout.wrap_width;
            input.record_measured_layout(original + px(19.), window, cx);
            assert!(matches!(
                input.protect_resident(cx),
                Err(RangeTextInputError::NotQuiescent)
            ));
            input.record_measured_layout(original, window, cx);
            input.service_measured_layout(window, cx);
            let protection = input.protect_resident(cx).unwrap();
            input.record_measured_layout(original + px(29.), window, cx);
            input.service_measured_layout(window, cx);
            assert!(input.measured_layout.is_none());
            assert_eq!(input.config.layout.wrap_width, original);
            assert!(input.resident_protection_is_current(protection));
            input.release_resident_protection(protection, cx).unwrap();
            input.record_measured_layout(original + px(39.), window, cx);
            assert!(input.has_pending_measured_layout());
            input.dispose(window, cx);
            assert!(input.measured_layout.is_none());
        })
    });
    cx.run_until_parked();
    input.read_with(cx, |input, _| {
        assert!(!input.mounted);
        assert!(!input.measured_layout_callback_scheduled);
        assert!(input.measured_layout_rejection().is_none());
    });
}

#[gpui::test]
fn accepted_rebind_retires_queued_measured_layout(cx: &mut gpui::TestAppContext) {
    let (input, cx) = ready_measured_input(cx);
    let successor = RangeBinding::new(
        BindingId::new(71),
        SourceRevision::new(2),
        LogicalExtent::new(13, 1),
    );
    let original_width = cx.update(|window, app| {
        input.update(app, |input, cx| {
            let width = input.config.layout.wrap_width;
            input.record_measured_layout(width + px(71.), window, cx);
            assert!(input.measured_layout_callback_scheduled);
            input.rebind(successor, None, window, cx).unwrap();
            assert!(input.measured_layout.is_none());
            assert!(input.measured_layout_callback_scheduled);
            width
        })
    });
    cx.run_until_parked();
    input.read_with(cx, |input, _| {
        assert_eq!(input.config.binding, successor);
        assert_eq!(input.config.layout.wrap_width, original_width);
        assert!(!input.measured_layout_callback_scheduled);
        assert!(input.measured_layout_rejection().is_none());
    });
    drive_surface_for_source(&input, cx, "saved draft 1");
    input.read_with(cx, |input, _| {
        assert_eq!(input.surface().unwrap().binding(), successor);
        assert!(input.is_surface_current_and_interactive());
        assert!(input.is_quiescent());
        assert_bounds(input);
    });
}

#[gpui::test]
fn measured_layout_waits_for_original_edit_settlement(cx: &mut gpui::TestAppContext) {
    let (input, cx) = ready_measured_input(cx);
    let current = input.read_with(cx, |input, _| input.surface().unwrap().source_caret());
    let (_, text, objects) = admitted_successor_sources("saved draft 1", 1, &[current]);
    let (key, original_width) = input.update(cx, |input, cx| {
        let binding = input.config.binding;
        let operation = input.lease_host_operation().unwrap();
        let key = crate::MutationKey::new(binding.binding(), binding.revision(), operation.operation());
        let proposal = crate::MutationProposal::new(
            key,
            crate::MutationKind::Edit,
            crate::MutationPositions::collapsed(current),
            crate::SourceRange::new(current, current).unwrap(),
            0,
        );
        let begin = crate::MutationBeginRequest::new(
            proposal,
            crate::MutationCursor::new(10),
            crate::MutationCursor::new(20),
        );
        input
            .begin_host_mutation(operation, begin, &[current], &text, &objects, cx)
            .unwrap();
        assert!(matches!(input.take_request(), Some(RangeTextInputRequest::MutationBegin(request)) if request == begin));
        (key, input.config.layout.wrap_width)
    });
    let viewport = cx.update(|window, _| window.viewport_size());
    let resized = gpui::size(viewport.width + px(43.), viewport.height);
    cx.simulate_resize(resized);
    cx.update(|window, app| window.draw(app).clear());
    cx.run_until_parked();
    input.read_with(cx, |input, _| {
        assert_eq!(input.edits.active_key(), Some(key));
        assert_eq!(input.config.layout.wrap_width, original_width);
        assert!(input.has_pending_measured_layout());
        assert!(!input.measured_layout_callback_scheduled);
        assert!(input.measured_layout_rejection().is_none());
        assert!(!input.is_surface_current_and_interactive());
        assert_bounds(input);
    });
    let progress = std::rc::Rc::new(ResizeSourceProgress::default());
    let _source = observe_resize_source(&input, cx, "saved draft 1", progress);
    input.update(cx, |input, cx| {
        assert_eq!(input.cancel_mutation(key, cx).unwrap(), crate::MutationCancellation::Cancelled);
        assert!(matches!(input.take_request(), Some(RangeTextInputRequest::CancelMutation(request)) if request.key() == key));
        assert!(input.take_request().is_none());
    });
    cx.run_until_parked();
    input.read_with(cx, |input, _| {
        assert!(input.edits.active_key().is_none());
        assert_eq!(input.config.layout.wrap_width, resized.width);
        assert!(input.measured_layout.is_none());
        assert!(
            input.is_surface_current_and_interactive(),
            "{:?}",
            fingerprint(input)
        );
        assert!(input.is_quiescent(), "{:?}", fingerprint(input));
        assert_bounds(input);
    });
}

#[gpui::test]
fn geometry_continuation_enrolls_despite_frame_needed_notification(cx: &mut gpui::TestAppContext) {
    let (input, cx) = ready_measured_input(cx);
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.schedule_realization_continuation(cx);
            assert!(input.realization_continuation_scheduled);
            assert!(input.realization_continuation_enrollment.is_none());
            input.defer_realization_continuation(window, cx);
            input.defer_realization_continuation(window, cx);
            assert_eq!(
                input.realization_continuation_enrollment,
                Some(input.realization_frame_generation)
            );
        })
    });
    cx.run_until_parked();
    input.read_with(cx, |input, _| {
        assert!(input.realization_continuation_enrollment.is_none());
        assert!(!input.realization_continuation_scheduled);
        assert!(input.is_surface_current_and_interactive());
        assert!(input.is_quiescent());
        assert_bounds(input);
    });
}

#[gpui::test]
fn geometry_continuation_retargets_one_queued_ticket(cx: &mut gpui::TestAppContext) {
    let (input, cx) = ready_measured_input(cx);
    let latest = cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.defer_realization_continuation(window, cx);
            let prior = input.realization_continuation_enrollment.unwrap();
            input.obsolete_realization_continuation();
            assert_eq!(input.realization_continuation_enrollment, Some(prior));
            input.begin_realization_frame();
            assert_eq!(input.realization_continuation_enrollment, Some(prior));
            input.defer_realization_continuation(window, cx);
            let latest = input.realization_frame_generation;
            assert_ne!(latest, prior);
            assert_eq!(input.realization_continuation_enrollment, Some(latest));
            for _ in 0..16 {
                input.defer_realization_continuation(window, cx);
            }
            latest
        })
    });
    cx.run_until_parked();
    input.read_with(cx, |input, _| {
        assert_eq!(input.realization_frame_generation, latest);
        assert!(input.realization_continuation_enrollment.is_none());
        assert!(!input.realization_continuation_scheduled);
        assert!(input.is_surface_current_and_interactive());
        assert!(input.is_quiescent());
        assert_bounds(input);
    });
}

#[gpui::test]
fn stale_geometry_callback_preserves_new_frame_demand(cx: &mut gpui::TestAppContext) {
    let (input, cx) = ready_measured_input(cx);
    let witnessed = Rc::new(std::cell::Cell::new(false));
    let observed = witnessed.clone();
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.defer_realization_continuation(window, cx);
            let prior = input.realization_continuation_enrollment.unwrap();
            input.obsolete_realization_continuation();
            input.schedule_realization_continuation(cx);
            assert_ne!(input.realization_frame_generation, prior);
            assert_eq!(input.realization_continuation_enrollment, Some(prior));
        });
        let input = input.clone();
        app.defer(move |app| {
            let input = input.read(app);
            assert!(input.realization_continuation_enrollment.is_none());
            assert!(input.realization_continuation_scheduled);
            observed.set(true);
        });
    });
    cx.run_until_parked();
    assert!(witnessed.get());
    input.read_with(cx, |input, _| {
        assert!(input.is_surface_current_and_interactive());
        assert!(input.is_quiescent());
        assert_bounds(input);
    });
}

#[gpui::test]
fn disposed_geometry_callback_releases_its_ticket_without_work(cx: &mut gpui::TestAppContext) {
    let (input, cx) = ready_measured_input(cx);
    let witnessed = Rc::new(std::cell::Cell::new(false));
    let observed = witnessed.clone();
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.defer_realization_continuation(window, cx);
            input.dispose(window, cx);
            assert!(input.realization_continuation_enrollment.is_some());
            assert!(!input.realization_continuation_scheduled);
        });
        let input = input.clone();
        app.defer(move |app| {
            let input = input.read(app);
            assert!(!input.mounted);
            assert!(input.realization_continuation_enrollment.is_none());
            assert!(!input.realization_continuation_scheduled);
            assert!(input.active_geometry.is_none());
            assert!(input.measured_layout_rejection().is_none());
            observed.set(true);
        });
    });
    cx.run_until_parked();
    assert!(witnessed.get());
}

#[gpui::test]
fn zero_credit_geometry_callback_notifies_without_minting_credit(cx: &mut gpui::TestAppContext) {
    let (input, cx) = ready_measured_input(cx);
    let notifications = Rc::new(std::cell::Cell::new(0));
    let observed = notifications.clone();
    let _subscription =
        cx.update(|_, app| app.observe(&input, move |_, _| observed.set(observed.get() + 1)));
    let witnessed = Rc::new(std::cell::Cell::new(false));
    let observed = witnessed.clone();
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            let remaining = input.realization_diagnostics().frame.remaining;
            for _ in 0..remaining {
                input.spend_realization_credit();
            }
            input.defer_realization_continuation(window, cx);
        });
        let input = input.clone();
        app.defer(move |app| {
            let input = input.read(app);
            assert!(input.realization_continuation_enrollment.is_none());
            assert!(input.realization_continuation_scheduled);
            assert_eq!(input.realization_diagnostics().frame.spent, 64);
            assert_eq!(input.realization_diagnostics().frame.remaining, 0);
            assert_bounds(input);
            observed.set(true);
        });
    });
    cx.run_until_parked();
    assert!(witnessed.get());
    assert_eq!(notifications.get(), 1);
    input.read_with(cx, |input, _| {
        assert!(input.is_surface_current_and_interactive());
        assert!(input.is_quiescent());
        assert_bounds(input);
    });
}

fn observe_replacement_source(
    input: &gpui::Entity<RangeTextInput>,
    window: &gpui::Window,
    app: &mut gpui::App,
    retired: std::rc::Rc<std::cell::Cell<Option<crate::ObjectRequestKey>>>,
    progress: std::rc::Rc<ResizeSourceProgress>,
) -> gpui::Subscription {
    let handle = window.window_handle();
    app.observe(input, move |input, app| {
        let notifications = progress.notifications.get() + 1;
        progress.notifications.set(notifications);
        assert!(
            notifications <= 128,
            "replacement source notification bound"
        );
        let input = input.downgrade();
        let retired = retired.clone();
        let progress = progress.clone();
        app.defer(move |app| {
            progress.callbacks.set(progress.callbacks.get() + 1);
            let input = input.upgrade().expect("replacement input remains alive");
            handle
                .update(app, |_, window, app| {
                    loop {
                        let request = input.update(app, |input, _| input.take_request());
                        let Some(request) = request else {
                            return;
                        };
                        let requests = progress.requests.get() + 1;
                        progress.requests.set(requests);
                        assert!(requests <= 128, "replacement source request bound");
                        match request {
                            RangeTextInputRequest::Page(request) => {
                                let page = page_for_source(
                                    request,
                                    request.key().id().get(),
                                    "saved draft 1",
                                );
                                input.update(app, |input, cx| {
                                    input.deliver_page(page, window, cx).unwrap();
                                });
                            }
                            RangeTextInputRequest::ObjectPage(request) => {
                                assert_ne!(Some(request.key()), retired.get());
                                let page = ObjectPage::new(
                                    ObjectPageId::new(request.key().id().get()),
                                    request.key(),
                                    vec![],
                                    ObjectPageEdgeFact::EnvelopeBoundary,
                                    ObjectPageEdgeFact::EnvelopeBoundary,
                                    true,
                                    None,
                                )
                                .unwrap();
                                input.update(app, |input, cx| {
                                    input
                                        .deliver_object_page_in_window(page, window, cx)
                                        .unwrap();
                                });
                            }
                            RangeTextInputRequest::CancelPage(_)
                            | RangeTextInputRequest::ReleasePage(_)
                            | RangeTextInputRequest::CancelObjectPage(_)
                            | RangeTextInputRequest::ReleaseObjectPage(_) => {}
                            request => panic!("unexpected replacement request: {request:?}"),
                        }
                        input.read_with(app, |input, _| assert_bounds(input));
                    }
                })
                .unwrap();
        });
    })
}

#[gpui::test]
fn queued_cleanup_preserves_automatic_resident_layout_progress(cx: &mut gpui::TestAppContext) {
    let source = "saved draft 1";
    assert_eq!(source.len(), 13);
    let mut configuration = config(2 * 1024 * 1024, 32_768);
    configuration.binding = RangeBinding::new(
        BindingId::new(71),
        SourceRevision::new(1),
        LogicalExtent::new(source.len() as u64, 1),
    );
    configuration.limits.max_realization_work_per_frame = 64;
    let original_binding = configuration.binding;
    let (input, cx) =
        cx.add_window_view(|window, cx| RangeTextInput::new(configuration, window, cx).unwrap());
    let original_input = input.entity_id();
    cx.update(|window, app| window.draw(app).clear());
    let viewport = cx.update(|window, _| window.viewport_size());
    let resized = gpui::size(viewport.width + px(79.), viewport.height);
    cx.simulate_resize(resized);
    let retired = std::rc::Rc::new(std::cell::Cell::new(None));
    let progress = std::rc::Rc::new(ResizeSourceProgress::default());
    let mut cleanup_count = 0;
    let _source = cx.update(|window, app| {
        let subscription = observe_replacement_source(
            &input,
            window,
            app,
            retired.clone(),
            progress.clone(),
        );
        let old_request = input.update(app, |input, cx| {
            let mut old_request = None;
            for _ in 0..16 {
                match input.take_request().expect("initial geometry request") {
                    RangeTextInputRequest::Page(request) => {
                        let page = page_for_source(request, request.key().id().get(), source);
                        input.deliver_page(page, window, cx).unwrap();
                    }
                    RangeTextInputRequest::ObjectPage(request) => {
                        assert!(matches!(
                            request.key().purpose(),
                            ObjectPurpose::GeometryTarget | ObjectPurpose::GeometryIndex
                        ));
                        assert!(input.dispatched_object_pages.contains(&request.key()));
                        assert_eq!(
                            input.pending_geometry_object.as_ref().unwrap().request.unwrap().key(),
                            request.key()
                        );
                        old_request = Some(request.key());
                        break;
                    }
                    RangeTextInputRequest::CancelPage(_)
                    | RangeTextInputRequest::ReleasePage(_)
                    | RangeTextInputRequest::CancelObjectPage(_)
                    | RangeTextInputRequest::ReleaseObjectPage(_) => {}
                    request => panic!("unexpected initial geometry request: {request:?}"),
                }
            }
            let old_request = old_request.expect("genuine dispatched predecessor object request");
            retired.set(Some(old_request));
            let old_job = input.active_geometry.expect("predecessor geometry job");
            let mut layout = input.config.layout.clone();
            layout.wrap_width = resized.width;
            layout.font_size += px(1.);
            input.set_layout(layout, input.config.style.clone(), cx).unwrap();
            assert_ne!(input.active_geometry, Some(old_job));
            assert!(!input.dispatched_object_pages.contains(&old_request));
            assert!(input.requests.iter().any(|request| {
                matches!(request, RangeTextInputRequest::CancelObjectPage(key) if *key == old_request)
            }));
            assert_bounds(input);
            old_request
        });
        window.draw(app).clear();
        input.update(app, |input, _| {
            assert_eq!(input.config.layout.wrap_width, resized.width);
            assert_eq!(input.last_bounds.unwrap().size.width, resized.width);
            assert!(input.last_realization_step.remaining > 0);
            assert!(input.last_realization_step.reached_external_boundary);
            assert!(input.pending_geometry_object.is_none());
            let pending = input.pending_geometry_page.as_ref().expect("resident target wait");
            assert_eq!(Some(pending.job), input.active_geometry);
            assert_eq!(pending.request.unwrap().key().purpose(), PagePurpose::GeometryTarget);
            let super::super::geometry::GeometryPageWait::Resident(page) = pending.wait else {
                panic!("replacement target must already have resident text");
            };
            assert!(input.residency.peek_page_by_id(page).is_some());
            assert!(!input.requests.is_empty());
            assert!(input.requests.iter().all(|request| matches!(
                request,
                RangeTextInputRequest::CancelPage(_)
                    | RangeTextInputRequest::ReleasePage(_)
                    | RangeTextInputRequest::CancelObjectPage(_)
                    | RangeTextInputRequest::ReleaseObjectPage(_)
            )));
            assert!(input.requests.iter().any(|request| {
                matches!(request, RangeTextInputRequest::CancelObjectPage(key) if *key == old_request)
            }));
            while let Some(request) = input.take_request() {
                assert!(matches!(
                    request,
                    RangeTextInputRequest::CancelPage(_)
                        | RangeTextInputRequest::ReleasePage(_)
                        | RangeTextInputRequest::CancelObjectPage(_)
                        | RangeTextInputRequest::ReleaseObjectPage(_)
                ));
                cleanup_count += 1;
                assert!(cleanup_count <= 16);
            }
            assert!(!input.is_surface_current_and_interactive());
            assert!(!input.is_quiescent());
            assert_bounds(input);
        });
        subscription
    });
    assert!(cleanup_count > 0);
    cx.run_until_parked();
    assert_eq!(input.entity_id(), original_input);
    input.read_with(cx, |input, _| {
        assert_eq!(input.config.binding, original_binding);
        assert_eq!(input.config.layout.wrap_width, resized.width);
        assert_bounds(input);
        assert!(
            input.is_surface_current_and_interactive() && input.is_quiescent(),
            "queued cleanup stranded resident work; notifications={}; callbacks={}; requests={}; state={:?}; diagnostics={:?}",
            progress.notifications.get(), progress.callbacks.get(), progress.requests.get(),
            fingerprint(input), input.realization_diagnostics(),
        );
        assert!(progress.requests.get() > 0);
        assert_eq!(input.surface().unwrap().binding(), original_binding);
        assert_eq!(input.surface().unwrap().source_caret().byte_offset.get(), 0);
        assert!(input.requests.is_empty());
        assert!(input.active_geometry.is_none());
        assert!(input.pending_geometry_page.is_none());
        assert!(input.pending_geometry_object.is_none());
        assert!(input.response_custody.is_empty());
        assert_eq!(input.dispatched_pages.len(), 0);
        assert_eq!(input.dispatched_object_pages.len(), 0);
        assert_eq!(input.residency.counts().pending_requests, 0);
        assert_eq!(input.object_residency.counts().pending_requests, 0);
        assert_eq!(input.realization_diagnostics().response_rejection_count, 0);
    });
}
