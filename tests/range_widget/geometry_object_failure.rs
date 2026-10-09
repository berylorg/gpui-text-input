use super::*;

fn geometry_object(
    input: &gpui::Entity<RangeTextInput>,
    cx: &mut gpui::VisualTestContext,
    source: &str,
    purpose: ObjectPurpose,
) -> gpui_text_input::ObjectRequest {
    for _ in 0..256 {
        match input.update(cx, |input, _| input.take_request()) {
            Some(RangeTextInputRequest::ObjectPage(request))
                if request.key().purpose() == purpose =>
            {
                return request;
            }
            Some(RangeTextInputRequest::Page(request)) => {
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
            other => panic!("unexpected geometry demand: {other:?}"),
        }
    }
    panic!("original geometry object did not queue");
}

fn failed_object_retires_geometry(cx: &mut gpui::TestAppContext, target: bool) {
    let source = "x".repeat(120);
    let configuration = config(&source, 1);
    let layout = configuration.layout.clone();
    let style = configuration.style.clone();
    let (input, cx) =
        cx.add_window_view(|window, cx| RangeTextInput::new(configuration, window, cx).unwrap());
    let purpose = if target {
        ObjectPurpose::GeometryTarget
    } else {
        assert!(drive_pages(&input, cx, &source).is_empty());
        input.update(cx, |input, cx| {
            input.set_layout(layout.clone(), style.clone(), cx).unwrap()
        });
        ObjectPurpose::GeometryIndex
    };
    let request = geometry_object(&input, cx, &source, purpose);
    let key = request.key();
    let foreign = gpui_text_input::ObjectRequestKey::new(
        ObjectRequestId::new(key.id().get() + 100_000),
        key.binding(),
        key.revision(),
        key.presentation_generation(),
        key.purpose(),
        key.demand(),
    )
    .unwrap();
    input.update(cx, |input, cx| {
        let before = input.realization_diagnostics().current;
        assert_eq!(before.active_geometry_jobs, 1);
        assert_eq!(before.pending_geometry_objects, 1);
        assert_eq!(before.dispatched_object_requests, 1);
        assert!(matches!(
            input.fail_object_page(foreign, gpui_text_input::ObjectPageFailure::Unavailable, cx),
            Err(gpui_text_input::RangeTextInputError::Stale)
        ));
        assert_eq!(input.realization_diagnostics().current, before);
        assert!(matches!(
            input.fail_object_page(key, gpui_text_input::ObjectPageFailure::Unavailable, cx),
            Err(gpui_text_input::RangeTextInputError::Stale)
        ));
        let after = input.realization_diagnostics().current;
        assert_eq!(after.active_geometry_jobs, 0);
        assert_eq!(after.pending_geometry_objects, 0);
        assert_eq!(after.pending_object_requests, 0);
        assert_eq!(after.dispatched_object_requests, 0);
        assert_eq!(after.pending_index_intents, 0);
        assert!(matches!(
            input.fail_object_page(key, gpui_text_input::ObjectPageFailure::Unavailable, cx),
            Err(gpui_text_input::RangeTextInputError::Stale)
        ));
        assert_eq!(input.realization_diagnostics().current, after);
        for _ in 0..input.realization_diagnostics().max_queued_requests {
            let request = input.take_request_if(|request| {
                matches!(
                    request,
                    RangeTextInputRequest::ReleasePage(_)
                        | RangeTextInputRequest::ReleaseObjectPage(_)
                        | RangeTextInputRequest::CancelPage(_)
                        | RangeTextInputRequest::CancelObjectPage(_)
                )
            });
            if request.is_none() {
                break;
            }
        }
        assert!(
            input.is_quiescent(),
            "{:?}",
            input.realization_diagnostics()
        );
    });
    input.update(cx, |input, cx| input.set_layout(layout, style, cx).unwrap());
    let successor = geometry_object(&input, cx, &source, ObjectPurpose::GeometryTarget);
    assert_ne!(successor.key(), key);
    input.update(cx, |input, cx| {
        let before = input.realization_diagnostics().current;
        assert!(matches!(
            input.fail_object_page(key, gpui_text_input::ObjectPageFailure::Unavailable, cx),
            Err(gpui_text_input::RangeTextInputError::Stale)
        ));
        assert_eq!(input.realization_diagnostics().current, before);
    });
    let page = restoration_object_page(successor, &[], successor.key().id().get());
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input
                .deliver_object_page_in_window(page, window, cx)
                .unwrap()
        })
    });
    assert!(drive_pages(&input, cx, &source).is_empty());
    input.read_with(cx, |input, _| {
        assert!(input.is_quiescent());
        assert!(input.is_surface_current_and_interactive());
        assert_eq!(input.surface().unwrap().binding(), binding(&source, 1));
    });
}

#[gpui::test]
fn exact_target_object_failure_retires_index_intent_and_resumes_explicit_layout(
    cx: &mut gpui::TestAppContext,
) {
    failed_object_retires_geometry(cx, true);
}

#[gpui::test]
fn exact_index_object_failure_preserves_foreign_job_and_resumes_explicit_layout(
    cx: &mut gpui::TestAppContext,
) {
    failed_object_retires_geometry(cx, false);
}
