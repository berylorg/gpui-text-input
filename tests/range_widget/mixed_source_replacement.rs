use super::*;
use gpui_text_input::{
    LocalMutationEvidence, MutationPage, MutationPageItem, MutationPass, ObjectChange,
};

const SOURCE: &str = "a\r\nbcd";

fn native_begin<'a>(
    cx: &'a mut gpui::TestAppContext,
    facts: &[InlineObjectFact],
    action: &str,
) -> (
    gpui::Entity<RangeTextInput>,
    &'a mut gpui::VisualTestContext,
    MutationBeginRequest,
) {
    cx.update(ensure_text_input_bindings);
    let mut configuration = config(SOURCE, 1);
    configuration.mutation_limits = MutationLimits::new(2, 1024).unwrap();
    let (input, cx) = cx.add_window_view(|window, cx| {
        let input = RangeTextInput::new(configuration, window, cx).unwrap();
        input.focus(window);
        input
    });
    drive_pages_with_objects(&input, cx, SOURCE, facts);
    cx.simulate_keystrokes("ctrl-a");
    drive_pages_with_objects(&input, cx, SOURCE, facts);
    input.update(cx, |input, _| {
        assert!(input.is_surface_current_and_interactive());
        assert_eq!(
            input.surface().unwrap().selection(),
            RangeSourceSelection {
                anchor: ordinary_position(0),
                head: ordinary_position(SOURCE.len() as u64),
            }
        );
        let (text, objects) = admitted_sources_with_facts(
            SOURCE,
            1,
            &[ordinary_position(0), ordinary_position(SOURCE.len() as u64)],
            facts,
        );
        input
            .admit_edit_positions(
                &[ordinary_position(0), ordinary_position(SOURCE.len() as u64)],
                &text,
                &objects,
            )
            .unwrap();
    });
    cx.simulate_keystrokes(action);
    for _ in 0..1024 {
        let request = input.update(cx, |input, _| input.take_request());
        if let Some(RangeTextInputRequest::MutationBegin(begin)) = request {
            assert_eq!(
                begin.proposal().replacement(),
                SourceRange::new(ordinary_position(0), ordinary_position(SOURCE.len() as u64))
                    .unwrap()
            );
            assert_eq!(
                begin.proposal().predecessor().selection_anchor(),
                ordinary_position(0)
            );
            assert_eq!(
                begin.proposal().predecessor().selection_head(),
                ordinary_position(SOURCE.len() as u64)
            );
            return (input, cx, begin);
        }
        service(request, &input, cx, facts);
    }
    panic!("native selected-range replacement did not begin");
}

fn service(
    request: Option<RangeTextInputRequest>,
    input: &gpui::Entity<RangeTextInput>,
    cx: &mut gpui::VisualTestContext,
    facts: &[InlineObjectFact],
) {
    match request {
        Some(RangeTextInputRequest::Page(request)) => {
            let page = page_for(SOURCE, request.key().id().get(), request);
            cx.update(|window, app| {
                input.update(app, |input, cx| {
                    input.deliver_page(page, window, cx).unwrap()
                })
            });
        }
        Some(RangeTextInputRequest::ObjectPage(request)) => {
            let page = restoration_object_page(request, facts, request.key().id().get());
            cx.update(|window, app| {
                input.update(app, |input, cx| {
                    input
                        .deliver_object_page_in_window(page, window, cx)
                        .unwrap()
                })
            });
        }
        Some(RangeTextInputRequest::ReleasePage(_))
        | Some(RangeTextInputRequest::CancelPage(_))
        | Some(RangeTextInputRequest::ReleaseObjectPage(_))
        | Some(RangeTextInputRequest::CancelObjectPage(_)) => {}
        None => {
            cx.update(|window, app| window.draw(app).clear());
            cx.run_until_parked();
        }
        Some(request) => panic!("unexpected replacement request: {request:?}"),
    }
}

fn evidence(
    input: &gpui::Entity<RangeTextInput>,
    cx: &mut gpui::VisualTestContext,
    facts: &[InlineObjectFact],
    pass: MutationPass,
) -> (Vec<MutationPage>, gpui_text_input::MutationFinishInput) {
    let mut pages = Vec::new();
    for _ in 0..4096 {
        let step = input.update(cx, |input, cx| {
            match input.local_mutation_evidence(pass, cx).unwrap() {
                LocalMutationEvidence::Pending => (None, None),
                LocalMutationEvidence::Page(page) => (Some(page.clone()), None),
                LocalMutationEvidence::Finish(finish) => (None, Some(finish)),
            }
        });
        if let Some(finish) = step.1 {
            return (pages, finish);
        }
        if let Some(page) = step.0 {
            assert!(page.items().len() <= 2);
            input.update(cx, |input, cx| {
                let ack = input
                    .submit_mutation_evidence_page(pass, page.clone(), cx)
                    .unwrap();
                input.acknowledge_mutation_evidence_page(ack).unwrap();
                let diagnostics = input.realization_diagnostics();
                assert!(diagnostics.current.owned_bytes <= diagnostics.max_surface_bytes);
                assert!(diagnostics.current.owned_items <= diagnostics.max_surface_items);
                assert!(input.surface().unwrap().binding() == binding(SOURCE, 1));
            });
            pages.push(page);
        }
        let request = input.update(cx, |input, _| input.take_request());
        service(request, input, cx, facts);
    }
    panic!("bounded evidence producer did not close");
}

#[gpui::test]
fn mixed_source_native_deletion_streams_same_anchor_removals_and_replays_exactly(
    cx: &mut gpui::TestAppContext,
) {
    let facts = (1..=9).map(|id| object_fact(id, 3, id)).collect::<Vec<_>>();
    let (input, cx, begin) = native_begin(cx, &facts, "backspace");
    let key = begin.proposal().key();
    let pass = input.update(cx, |input, _| input.request_mutation_evidence(key).unwrap());
    let (pages, finish) = evidence(&input, cx, &facts, pass);
    assert!(pages.len() > 2);
    let removed = pages
        .iter()
        .flat_map(|page| page.items())
        .filter_map(|item| match item {
            MutationPageItem::Object(ObjectChange::Remove { target }) => Some(target.id()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        removed,
        facts.iter().map(InlineObjectFact::id).collect::<Vec<_>>()
    );
    assert_eq!(finish.proposal().totals.objects, 9);
    assert_eq!(finish.intended_extent(), LogicalExtent::new(0, 0));
    assert_eq!(
        finish.intended(),
        gpui_text_input::MutationPositionsIntent::ReplacementEnd {
            replacement: begin.proposal().replacement(),
            inserted_utf8_bytes: 0,
        }
    );
    input.update(cx, |input, cx| {
        input.submit_mutation_evidence_finish(pass, finish).unwrap();
        input.accept_mutation_preflight(key, cx).unwrap();
        let staging = input.mutation_restart(key).unwrap();
        input.acknowledge_mutation_restart(staging, cx).unwrap();
    });
    let mut replay = Vec::new();
    let mut closed = false;
    for _ in 0..4096 {
        let request = input.update(cx, |input, _| input.take_request());
        match request {
            Some(RangeTextInputRequest::MutationProposalPage(page)) => {
                replay.push(page.page().clone())
            }
            Some(RangeTextInputRequest::MutationFinishInput(actual)) => {
                assert_eq!(actual, finish);
                closed = true;
                break;
            }
            request => service(request, &input, cx, &facts),
        }
    }
    assert!(closed, "staging must reproduce explicit EOF");
    assert_eq!(replay, pages);
    input.update(cx, |input, cx| {
        input.accept_mutation_finish(key, cx).unwrap()
    });
    assert!(
        matches!(input.update(cx, |input, _| input.take_request()), Some(RangeTextInputRequest::MutationCommit(commit)) if commit.key() == key)
    );
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            assert!(matches!(
                input
                    .settle_mutation(key, gpui_text_input::MutationOutcome::Rejected, window, cx)
                    .unwrap(),
                gpui_text_input::MutationSettlement::Current(
                    gpui_text_input::MutationOutcome::Rejected
                )
            ));
            assert_eq!(input.surface().unwrap().binding(), binding(SOURCE, 1));
            assert_eq!(
                input.surface().unwrap().selection().anchor,
                ordinary_position(0)
            );
            assert_eq!(
                input.surface().unwrap().selection().head,
                ordinary_position(SOURCE.len() as u64)
            );
        })
    });
}

#[gpui::test]
fn mixed_source_pending_object_cancellation_rejects_late_response_and_releases_producer(
    cx: &mut gpui::TestAppContext,
) {
    let facts = vec![object_fact(1, 3, 1), object_fact(2, 3, 2)];
    let (input, cx, begin) = native_begin(cx, &facts, "delete");
    let key = begin.proposal().key();
    let pass = input.update(cx, |input, _| input.request_mutation_evidence(key).unwrap());
    input.update(cx, |input, cx| {
        assert!(matches!(
            input.local_mutation_evidence(pass, cx).unwrap(),
            LocalMutationEvidence::Pending
        ))
    });
    let pending = pending_predecessor_request(&input, cx, &facts);
    let late = restoration_object_page(pending, &facts, 99_111);
    input.update(cx, |input, cx| {
        input.cancel_mutation(key, cx).unwrap();
    });
    let rejected = cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.deliver_object_page_in_window(late, window, cx)
        })
    });
    assert!(matches!(
        rejected,
        Err(gpui_text_input::RangeTextInputError::ObjectResponseRejected(_))
    ));
    input.update(cx, |input, cx| {
        assert!(input.local_mutation_evidence(pass, cx).is_err());
        assert_eq!(input.surface().unwrap().binding(), binding(SOURCE, 1));
        while input.take_request().is_some() {}
        assert!(input.is_semantically_quiescent());
        assert_eq!(
            input
                .realization_diagnostics()
                .current
                .pending_object_requests,
            0
        );
    });
}

fn pending_predecessor_request(
    input: &gpui::Entity<RangeTextInput>,
    cx: &mut gpui::VisualTestContext,
    facts: &[InlineObjectFact],
) -> gpui_text_input::ObjectRequest {
    for _ in 0..128 {
        let request = input.update(cx, |input, _| input.take_request());
        if let Some(RangeTextInputRequest::ObjectPage(request)) = request
            && request.key().purpose() == ObjectPurpose::MutationPredecessor
        {
            return request;
        }
        service(request, input, cx, facts);
    }
    panic!("bounded predecessor object request was not dispatched");
}

#[gpui::test]
fn mixed_source_pending_disposal_and_rebind_retire_exact_object_custody(
    cx: &mut gpui::TestAppContext,
) {
    for dispose in [true, false] {
        let facts = vec![object_fact(1, 3, 1), object_fact(2, 3, 2)];
        let (input, cx, begin) = native_begin(cx, &facts, "backspace");
        let key = begin.proposal().key();
        let pass = input.update(cx, |input, _| input.request_mutation_evidence(key).unwrap());
        input.update(cx, |input, cx| {
            assert!(matches!(
                input.local_mutation_evidence(pass, cx).unwrap(),
                LocalMutationEvidence::Pending
            ))
        });
        let pending = pending_predecessor_request(&input, cx, &facts);
        let late = restoration_object_page(pending, &facts, 99_112);
        let mut retired_object = false;
        let mut retired_mutation = false;
        if dispose {
            let effects =
                cx.update(|window, app| input.update(app, |input, cx| input.dispose(window, cx)));
            retired_object = effects.iter().any(|effect| matches!(effect, RangeTextInputRequest::CancelObjectPage(object) if *object == pending.key()));
            retired_mutation = effects.iter().any(|effect| matches!(effect, RangeTextInputRequest::CancelMutation(mutation) if mutation.key() == key));
            input.read_with(cx, |input, _| assert!(input.surface().is_none()));
        } else {
            cx.update(|window, app| {
                input.update(app, |input, cx| {
                    input.rebind(binding(SOURCE, 2), None, window, cx).unwrap()
                })
            });
            for _ in 0..1024 {
                let request = input.update(cx, |input, _| input.take_request());
                match request {
                    Some(RangeTextInputRequest::CancelObjectPage(object))
                        if object == pending.key() =>
                    {
                        retired_object = true
                    }
                    Some(RangeTextInputRequest::CancelMutation(mutation))
                        if mutation.key() == key =>
                    {
                        retired_mutation = true
                    }
                    request => service(request, &input, cx, &facts),
                }
                if input.read_with(cx, |input, _| input.is_quiescent()) {
                    break;
                }
            }
            input.read_with(cx, |input, _| {
                assert_eq!(input.surface().unwrap().binding(), binding(SOURCE, 2))
            });
        }
        assert!(
            retired_object && retired_mutation,
            "retirement must name the exact producer object and mutation keys"
        );
        let rejected = cx.update(|window, app| {
            input.update(app, |input, cx| {
                input.deliver_object_page_in_window(late, window, cx)
            })
        });
        assert!(matches!(
            rejected,
            Err(gpui_text_input::RangeTextInputError::ObjectResponseRejected(_))
        ));
        input.update(cx, |input, cx| {
            assert!(input.local_mutation_evidence(pass, cx).is_err());
            let diagnostics = input.realization_diagnostics();
            assert_eq!(diagnostics.current.response_custody_count, 0);
            assert_eq!(diagnostics.current.pending_object_requests, 0);
        });
    }
}

#[gpui::test]
fn mixed_source_changed_staging_objects_refuse_evidence_closure_without_adoption(
    cx: &mut gpui::TestAppContext,
) {
    let facts = vec![object_fact(1, 3, 1), object_fact(2, 3, 2)];
    let (input, cx, begin) = native_begin(cx, &facts, "backspace");
    let events = restoration_events(&input, cx);
    let key = begin.proposal().key();
    let pass = input.update(cx, |input, _| input.request_mutation_evidence(key).unwrap());
    let (_, finish) = evidence(&input, cx, &facts, pass);
    input.update(cx, |input, cx| {
        input.submit_mutation_evidence_finish(pass, finish).unwrap();
        input.accept_mutation_preflight(key, cx).unwrap();
        let staging = input.mutation_restart(key).unwrap();
        input.acknowledge_mutation_restart(staging, cx).unwrap();
    });
    let changed = vec![object_fact(11, 3, 1), object_fact(2, 3, 2)];
    let mut refused = false;
    for _ in 0..1024 {
        let request = input.update(cx, |input, _| input.take_request());
        match request {
            Some(RangeTextInputRequest::ObjectPage(request))
                if request.key().purpose() == ObjectPurpose::MutationPredecessor =>
            {
                let page = restoration_object_page(request, &changed, request.key().id().get());
                let result = cx.update(|window, app| {
                    input.update(app, |input, cx| {
                        input.deliver_object_page_in_window(page, window, cx)
                    })
                });
                if matches!(
                    result,
                    Err(gpui_text_input::RangeTextInputError::Mutation(
                        gpui_text_input::MutationError::ReplayMismatch
                            | gpui_text_input::MutationError::FinishMismatch
                    ))
                ) {
                    refused = true;
                    break;
                }
                result.unwrap();
            }
            Some(RangeTextInputRequest::MutationProposalPage(_)) => {}
            Some(
                RangeTextInputRequest::MutationFinishInput(_)
                | RangeTextInputRequest::MutationCommit(_),
            ) => panic!("changed replay cannot close or commit"),
            request => service(request, &input, cx, &facts),
        }
        if events.borrow().iter().any(|event| matches!(event,
            RangeTextInputEvent::MutationSettled { key: settled, outcome: gpui_text_input::MutationOutcome::Rejected } if *settled == key)) {
            refused = true;
            break;
        }
    }
    assert!(
        refused,
        "changed staged object identities must refuse the frozen closure"
    );
    input.update(cx, |input, cx| {
        assert_eq!(input.surface().unwrap().binding(), binding(SOURCE, 1));
        assert_eq!(
            input.surface().unwrap().selection().anchor,
            ordinary_position(0)
        );
        assert_eq!(
            input.surface().unwrap().selection().head,
            ordinary_position(SOURCE.len() as u64)
        );
        assert!(input.local_mutation_evidence(pass, cx).is_err());
        while input.take_request().is_some() {}
        assert!(input.is_semantically_quiescent());
    });
}
