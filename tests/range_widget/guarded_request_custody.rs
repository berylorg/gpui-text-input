use super::*;

#[gpui::test]
fn guarded_empty_queue_does_not_invoke_admission(cx: &mut gpui::TestAppContext) {
    let source = "empty queue";
    let (input, cx) = cx
        .add_window_view(|window, cx| RangeTextInput::new(config(source, 1), window, cx).unwrap());
    assert!(drive_pages(&input, cx, source).is_empty());
    input.update(cx, |input, _| {
        let before = input.realization_diagnostics().current;
        assert!(
            input
                .take_request_if(|_| panic!("empty queue admission"))
                .is_none()
        );
        assert_eq!(input.realization_diagnostics().current, before);
    });
}

#[gpui::test]
fn guarded_rejected_page_preserves_front_and_original_dispatch_custody(
    cx: &mut gpui::TestAppContext,
) {
    let source = "original queued page";
    let (input, cx) = cx
        .add_window_view(|window, cx| RangeTextInput::new(config(source, 1), window, cx).unwrap());
    input.update(cx, |input, cx| {
        let before = input.realization_diagnostics().current;
        let mut original = None;
        assert!(
            input
                .take_request_if(|request| {
                    let RangeTextInputRequest::Page(page) = request else {
                        panic!("initial page demand");
                    };
                    original = Some(page.key());
                    false
                })
                .is_none()
        );
        assert_eq!(input.realization_diagnostics().current, before);
        let original = original.unwrap();
        assert!(input.take_request_if(|request| {
            assert!(matches!(request, RangeTextInputRequest::Page(page) if page.key() == original));
            false
        }).is_none());
        assert_eq!(input.realization_diagnostics().current, before);
        let accepted = input
            .take_request_if(|request| matches!(request, RangeTextInputRequest::Page(_)))
            .unwrap();
        let RangeTextInputRequest::Page(page) = accepted else {
            unreachable!()
        };
        assert_eq!(page.key(), original);
        let admitted = input.realization_diagnostics().current;
        assert_eq!(admitted.queued_requests + 1, before.queued_requests);
        assert_eq!(
            admitted.dispatched_page_requests,
            before.dispatched_page_requests + 1
        );
        input
            .fail_page(original, PageFailure::Unavailable, cx)
            .unwrap();
        assert!(matches!(
            input.fail_page(original, PageFailure::Unavailable, cx),
            Err(gpui_text_input::RangeTextInputError::Stale)
        ));
    });
}

#[gpui::test]
fn guarded_rejected_mutation_retains_publication_and_exact_cancellation(
    cx: &mut gpui::TestAppContext,
) {
    let source = "guarded mutation";
    let (input, cx) = cx
        .add_window_view(|window, cx| RangeTextInput::new(config(source, 1), window, cx).unwrap());
    assert!(drive_pages(&input, cx, source).is_empty());
    let current = input.read_with(cx, |input, _| input.surface().unwrap().selection().head);
    let (text, objects) = admitted_sources(source, 1, &[current]);
    let base = binding(source, 1);
    input.update(cx, |input, cx| {
        let operation = input.lease_host_operation().unwrap();
        let begin = MutationBeginRequest::new(
            MutationProposal::new(
                MutationKey::new(base.binding(), base.revision(), operation.operation()),
                MutationKind::Edit,
                MutationPositions::collapsed(current),
                SourceRange::new(current, current).unwrap(),
                0,
            ),
            MutationCursor::new(0),
            MutationCursor::new(0),
        );
        input.begin_host_mutation(operation, begin, &[current], &text, &objects, cx).unwrap();
        let key = begin.proposal().key();
        let before = input.realization_diagnostics().current;
        let publication = range_publication_fingerprint_from(input);
        assert!(input.take_request_if(|request| {
            assert!(matches!(request, RangeTextInputRequest::MutationBegin(request) if *request == begin));
            false
        }).is_none());
        assert_eq!(input.realization_diagnostics().current, before);
        assert_eq!(range_publication_fingerprint_from(input), publication);
        assert!(!input.is_semantically_quiescent());
        assert!(matches!(
            input.take_request_if(|request| matches!(request, RangeTextInputRequest::MutationBegin(_))),
            Some(RangeTextInputRequest::MutationBegin(request)) if request == begin
        ));
        assert!(matches!(input.cancel_mutation(key, cx), Ok(gpui_text_input::MutationCancellation::Cancelled)));
        let cancellation = input.realization_diagnostics().current;
        assert!(input.take_request_if(|request| matches!(request, RangeTextInputRequest::Page(_))).is_none());
        assert_eq!(input.realization_diagnostics().current, cancellation);
        assert!(!input.is_semantically_quiescent());
        assert!(matches!(
            input.take_request_if(|request| matches!(request, RangeTextInputRequest::CancelMutation(_))),
            Some(RangeTextInputRequest::CancelMutation(request)) if request.key() == key
        ));
        assert!(input.is_semantically_quiescent());
    });
}

#[gpui::test]
fn guarded_release_delivery_preserves_original_queue_front(cx: &mut gpui::TestAppContext) {
    let source = "resident release";
    let (input, cx) = cx
        .add_window_view(|window, cx| RangeTextInput::new(config(source, 1), window, cx).unwrap());
    let mut observed = false;
    for _ in 0..256 {
        let request = input.update(cx, |input, _| {
            input.take_request_if(|request| {
                !matches!(request, RangeTextInputRequest::ReleasePage(_))
            })
        });
        match request {
            Some(RangeTextInputRequest::Page(request)) => {
                let page = page_for(source, request.key().id().get(), request);
                cx.update(|window, app| {
                    input.update(app, |input, cx| {
                        input.deliver_page(page, window, cx).unwrap();
                    })
                });
            }
            Some(RangeTextInputRequest::ObjectPage(request)) => {
                let page = restoration_object_page(request, &[], request.key().id().get());
                cx.update(|window, app| {
                    input.update(app, |input, cx| {
                        input
                            .deliver_object_page_in_window(page, window, cx)
                            .unwrap();
                    })
                });
            }
            Some(RangeTextInputRequest::CancelPage(_))
            | Some(RangeTextInputRequest::CancelObjectPage(_))
            | Some(RangeTextInputRequest::ReleaseObjectPage(_)) => {}
            Some(_) => panic!("unexpected semantic request"),
            None => {
                observed = input.update(cx, |input, _| {
                    let before = input.realization_diagnostics().current;
                    let mut released = None;
                    assert!(input.take_request_if(|request| {
                        let RangeTextInputRequest::ReleasePage(key) = request else {
                            panic!("release stays at the front");
                        };
                        assert_eq!(key.binding(), binding(source, 1).binding());
                        released = Some(*key);
                        false
                    }).is_none());
                    assert_eq!(input.realization_diagnostics().current, before);
                    if released.is_none() {
                        return false;
                    }
                    assert!(matches!(
                        input.take_request_if(|request| matches!(request, RangeTextInputRequest::ReleasePage(_))),
                        Some(RangeTextInputRequest::ReleasePage(key)) if Some(key) == released
                    ));
                    true
                });
                if observed {
                    break;
                }
                cx.update(|window, app| window.draw(app).clear());
                cx.run_until_parked();
            }
        }
    }
    assert!(
        observed,
        "original Page delivery must emit its exact release"
    );
    assert!(drive_pages(&input, cx, source).is_empty());
    input.read_with(cx, |input, _| {
        assert_eq!(input.surface().unwrap().binding(), binding(source, 1));
        assert!(input.is_quiescent());
    });
}
