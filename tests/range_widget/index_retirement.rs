use super::*;

fn queue_index(
    input: &gpui::Entity<RangeTextInput>,
    cx: &mut gpui::VisualTestContext,
    source: &str,
) {
    for _ in 0..256 {
        if input.read_with(cx, |input, _| {
            let current = input.realization_diagnostics().current;
            current.active_geometry_jobs == 1
                && current.pending_index_intents == 0
                && input.is_surface_current_and_interactive()
                && current.queued_requests > 0
        }) {
            return;
        }
        match input.update(cx, |input, _| input.take_request()) {
            Some(RangeTextInputRequest::Page(request)) => {
                assert_eq!(request.key().purpose(), PagePurpose::GeometryTarget);
                let page = page_for(source, request.key().id().get(), request);
                cx.update(|window, app| {
                    input.update(app, |input, cx| {
                        input.deliver_page(page, window, cx).unwrap()
                    })
                });
            }
            Some(RangeTextInputRequest::ObjectPage(request)) => {
                let page = restoration_object_page(request, &[], request.key().id().get());
                cx.update(|window, app| {
                    input.update(app, |input, cx| {
                        input
                            .deliver_object_page_in_window(page, window, cx)
                            .unwrap()
                    })
                });
            }
            Some(RangeTextInputRequest::ReleasePage(_))
            | Some(RangeTextInputRequest::ReleaseObjectPage(_))
            | Some(RangeTextInputRequest::CancelPage(_))
            | Some(RangeTextInputRequest::CancelObjectPage(_)) => {}
            None => {
                cx.update(|window, app| window.draw(app).clear());
                cx.run_until_parked();
            }
            other => panic!("unexpected request before index: {other:?}"),
        }
    }
    panic!(
        "index did not queue: {:?}",
        input.read_with(cx, |input, _| input.realization_diagnostics())
    );
}

fn take_index(
    input: &gpui::Entity<RangeTextInput>,
    cx: &mut gpui::VisualTestContext,
) -> gpui_text_input::PageRequest {
    loop {
        match take_request_after_scheduled_frames(input, cx, "index") {
            RangeTextInputRequest::Page(request) => {
                assert_eq!(request.key().purpose(), PagePurpose::GeometryIndex);
                return request;
            }
            RangeTextInputRequest::ReleasePage(_) | RangeTextInputRequest::ReleaseObjectPage(_) => {
            }
            other => panic!("unexpected request before index: {other:?}"),
        }
    }
}

fn replace_index_on_focus_loss(
    source: &'static str,
    dispatch: bool,
    cx: &mut gpui::TestAppContext,
) {
    let configuration = config(source, 1);
    let layout = configuration.layout.clone();
    let style = configuration.style.clone();
    let (input, cx) = cx.add_window_view(|window, cx| {
        let input = RangeTextInput::new(configuration, window, cx).unwrap();
        input.focus(window);
        input
    });
    assert!(drive_pages(&input, cx, source).is_empty());
    assert!(input.read_with(cx, |input, _| input.is_quiescent()));
    input.update(cx, |input, cx| input.set_layout(layout, style, cx).unwrap());
    queue_index(&input, cx, source);
    let old = if dispatch {
        Some(take_index(&input, cx))
    } else {
        let current = input.read_with(cx, |input, _| input.realization_diagnostics().current);
        assert_eq!(current.active_geometry_jobs, 1);
        assert!(current.queued_requests > 0);
        None
    };
    cx.update(|window, _| window.blur());
    cx.run_until_parked();

    let mut cancelled = 0;
    let mut successor = None;
    for _ in 0..32 {
        match input.update(cx, |input, _| input.take_request()) {
            Some(RangeTextInputRequest::CancelPage(key)) => {
                if let Some(old) = old {
                    assert_eq!(key, old.key());
                    cancelled += 1;
                }
            }
            Some(RangeTextInputRequest::ReleasePage(_))
            | Some(RangeTextInputRequest::ReleaseObjectPage(_)) => {}
            Some(RangeTextInputRequest::Page(request)) => {
                successor = Some(request);
                break;
            }
            None => {
                cx.update(|window, app| window.draw(app).clear());
                cx.run_until_parked();
            }
            other => panic!("unexpected replacement request: {other:?}"),
        }
    }
    assert_eq!(cancelled, usize::from(dispatch));
    let successor = successor.expect("replacement must restart geometry");
    assert_eq!(
        successor.key().purpose(),
        if source.is_empty() {
            PagePurpose::GeometryIndex
        } else {
            PagePurpose::GeometryTarget
        },
    );
    if let Some(old) = old {
        assert_ne!(successor.key(), old.key());
        let late = page_for(source, 90_001, old);
        let result = cx.update(|window, app| {
            input.update(app, |input, cx| input.deliver_page(late, window, cx))
        });
        assert!(matches!(result,
            Err(gpui_text_input::RangeTextInputError::PageResponseRejected(released))
                if released.key() == old.key()));
    }
    let page = page_for(source, 90_002, successor);
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.deliver_page(page, window, cx).unwrap()
        })
    });
    assert!(drive_pages(&input, cx, source).is_empty());
    input.read_with(cx, |input, _| {
        assert!(
            input.is_quiescent(),
            "{:?}",
            input.realization_diagnostics()
        );
        assert_eq!(input.surface().unwrap().binding(), binding(source, 1));
        assert_eq!(
            input
                .realization_diagnostics()
                .current
                .pending_index_intents,
            0
        );
        assert_eq!(
            input.realization_diagnostics().current.active_geometry_jobs,
            0
        );
    });
}

#[gpui::test]
fn empty_local_target_retires_dispatched_index(cx: &mut gpui::TestAppContext) {
    replace_index_on_focus_loss("", true, cx);
}

#[gpui::test]
fn empty_local_target_retires_queued_index(cx: &mut gpui::TestAppContext) {
    replace_index_on_focus_loss("", false, cx);
}

#[gpui::test]
fn nonterminal_local_target_retires_dispatched_index(cx: &mut gpui::TestAppContext) {
    replace_index_on_focus_loss("resident text", true, cx);
}

#[gpui::test]
fn nonterminal_local_target_retires_queued_index(cx: &mut gpui::TestAppContext) {
    replace_index_on_focus_loss("resident text", false, cx);
}

#[gpui::test]
fn deferred_target_preserves_dispatched_index(cx: &mut gpui::TestAppContext) {
    let source = "resident text";
    let configuration = config(source, 1);
    let layout = configuration.layout.clone();
    let style = configuration.style.clone();
    let (input, cx) =
        cx.add_window_view(|window, cx| RangeTextInput::new(configuration, window, cx).unwrap());
    assert!(drive_pages(&input, cx, source).is_empty());
    input.update(cx, |input, cx| input.set_layout(layout, style, cx).unwrap());
    queue_index(&input, cx, source);
    let old = take_index(&input, cx);
    input.update(cx, |input, cx| {
        input.request_absolute_scroll(px(0.), cx).unwrap()
    });
    cx.run_until_parked();
    input.read_with(cx, |input, _| {
        assert_eq!(
            input
                .realization_diagnostics()
                .current
                .dispatched_page_requests,
            1
        );
        assert_eq!(
            input.realization_diagnostics().current.active_geometry_jobs,
            1
        );
    });
    let page = page_for(source, 91_001, old);
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.deliver_page(page, window, cx).unwrap()
        })
    });
    assert!(drive_pages(&input, cx, source).is_empty());
    assert!(input.read_with(cx, |input, _| input.is_quiescent()));
}

#[gpui::test]
fn local_target_retires_dispatched_index_object(cx: &mut gpui::TestAppContext) {
    let source = "resident text";
    let configuration = config(source, 1);
    let layout = configuration.layout.clone();
    let style = configuration.style.clone();
    let (input, cx) = cx.add_window_view(|window, cx| {
        let input = RangeTextInput::new(configuration, window, cx).unwrap();
        input.focus(window);
        input
    });
    assert!(drive_pages(&input, cx, source).is_empty());
    input.update(cx, |input, cx| input.set_layout(layout, style, cx).unwrap());
    queue_index(&input, cx, source);
    let old = take_index(&input, cx);
    let page = page_for(source, 92_001, old);
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.deliver_page(page, window, cx).unwrap()
        })
    });
    let old = loop {
        match take_request_after_scheduled_frames(&input, cx, "index object") {
            RangeTextInputRequest::ObjectPage(request) => break request,
            RangeTextInputRequest::ReleasePage(_) | RangeTextInputRequest::ReleaseObjectPage(_) => {
            }
            other => panic!("unexpected index object request: {other:?}"),
        }
    };
    assert_eq!(old.key().purpose(), ObjectPurpose::GeometryIndex);
    cx.update(|window, _| window.blur());
    cx.run_until_parked();
    let cancellation = take_request_after_scheduled_frames(&input, cx, "object cancellation");
    assert!(
        matches!(cancellation, RangeTextInputRequest::CancelObjectPage(key) if key == old.key())
    );
    let late = restoration_object_page(old, &[], 92_002);
    let result = cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.deliver_object_page_in_window(late, window, cx)
        })
    });
    assert!(
        matches!(result, Err(gpui_text_input::RangeTextInputError::ObjectResponseRejected(released))
        if released.key() == old.key())
    );
    assert!(drive_pages(&input, cx, source).is_empty());
    input.read_with(cx, |input, _| {
        assert!(input.is_quiescent());
        assert_eq!(
            input
                .realization_diagnostics()
                .current
                .dispatched_object_requests,
            0
        );
        assert_eq!(
            input
                .realization_diagnostics()
                .current
                .pending_object_requests,
            0
        );
    });
}

#[cfg(feature = "test-support")]
#[gpui::test]
fn refused_local_target_preserves_dispatched_index(cx: &mut gpui::TestAppContext) {
    let source = "";
    let configuration = config(source, 1);
    let layout = configuration.layout.clone();
    let style = configuration.style.clone();
    let (input, cx) = cx.add_window_view(|window, cx| {
        let input = RangeTextInput::new(configuration, window, cx).unwrap();
        input.focus(window);
        input
    });
    assert!(drive_pages(&input, cx, source).is_empty());
    input.update(cx, |input, cx| input.set_layout(layout, style, cx).unwrap());
    queue_index(&input, cx, source);
    let old = take_index(&input, cx);
    let before = input.read_with(cx, |input, _| {
        (
            input.surface().unwrap().geometry_key(),
            input.realization_diagnostics().current,
        )
    });
    input.update(cx, |input, _| {
        input
            .lower_max_surface_items_for_test(NonZeroUsize::new(1).unwrap())
            .unwrap();
    });
    cx.update(|window, _| window.blur());
    cx.run_until_parked();
    input.read_with(cx, |input, _| {
        let current = input.realization_diagnostics().current;
        assert_eq!(input.surface().unwrap().geometry_key(), before.0);
        assert_eq!(current.active_geometry_jobs, before.1.active_geometry_jobs);
        assert_eq!(
            current.pending_geometry_pages,
            before.1.pending_geometry_pages
        );
        assert_eq!(
            current.dispatched_page_requests,
            before.1.dispatched_page_requests
        );
        assert_eq!(current.queued_requests, before.1.queued_requests);
        assert!(!input.is_quiescent());
    });
    let cancellations =
        cx.update(|window, app| input.update(app, |input, cx| input.dispose(window, cx)));
    assert_eq!(
        cancellations
            .iter()
            .filter(|request| matches!(request,
        RangeTextInputRequest::CancelPage(key) if *key == old.key()))
            .count(),
        1
    );
}
