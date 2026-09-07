use super::*;

#[gpui::test]
fn evidence_late_transport_after_commit_emits_no_second_settlement(cx: &mut gpui::TestAppContext) {
    let (input, cx) = cx.add_window_view(|window, cx| {
        let input = RangeTextInput::new(config("abc", 1), window, cx).unwrap();
        input.focus(window);
        input
    });
    assert!(drive_pages(&input, cx, "abc").is_empty());
    let events = restoration_events(&input, cx);
    input.update(cx, |input, _| {
        admit_ordinary_edit_positions(input, "abc", 1, &[0])
    });
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.replace_text_in_range(Some(0..0), "x", window, cx);
        })
    });
    let begin = drive_pages(&input, cx, "abc")
        .into_iter()
        .find_map(|request| match request {
            RangeTextInputRequest::MutationBegin(begin) => Some(begin),
            _ => None,
        })
        .unwrap();
    let key = begin.proposal().key();
    let (evidence, staging, page, finish) = input.update(cx, |input, cx| {
        let evidence = input.request_mutation_evidence(key).unwrap();
        let (page, finish) = input.local_mutation_evidence(evidence).unwrap();
        let page = page.unwrap().clone();
        let ack = input
            .submit_mutation_evidence_page(evidence, page.clone(), cx)
            .unwrap();
        input.acknowledge_mutation_evidence_page(ack).unwrap();
        input
            .submit_mutation_evidence_finish(evidence, finish)
            .unwrap();
        input.accept_mutation_preflight(key, cx).unwrap();
        let staging = input.mutation_restart(key).unwrap();
        input.acknowledge_mutation_restart(staging, cx).unwrap();
        assert!(matches!(
            input.take_request(),
            Some(RangeTextInputRequest::MutationProposalPage(_))
        ));
        assert!(matches!(
            input.take_request(),
            Some(RangeTextInputRequest::MutationFinishInput(_))
        ));
        input.accept_mutation_finish(key, cx).unwrap();
        assert!(matches!(
            input.take_request(),
            Some(RangeTextInputRequest::MutationCommit(_))
        ));
        (evidence, staging, page, finish)
    });
    let (text, objects) = admitted_sources("xabc", 2, &[finish.intended().caret()]);
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input
                .settle_committed_mutation(
                    key,
                    binding("xabc", 2),
                    finish.intended(),
                    &text,
                    &objects,
                    window,
                    cx,
                )
                .unwrap();
        })
    });
    drive_pages(&input, cx, "xabc");
    assert_eq!(
        events
            .borrow()
            .iter()
            .filter(|event| matches!(event, RangeTextInputEvent::MutationSettled { .. }))
            .count(),
        1
    );
    assert!(events.borrow().iter().any(|event| matches!(
        event,
        RangeTextInputEvent::MutationSettled {
            outcome: gpui_text_input::MutationOutcome::Committed(_),
            ..
        }
    )));
    input.update(cx, |input, cx| {
        assert!(
            input
                .submit_mutation_evidence_page(evidence, page.clone(), cx)
                .is_err()
        );
        assert!(input.submit_mutation_pass_page(staging, page, cx).is_err());
        assert!(
            input
                .submit_mutation_pass_finish(staging, finish, cx)
                .is_err()
        );
    });
    assert_eq!(
        events
            .borrow()
            .iter()
            .filter(|event| matches!(event, RangeTextInputEvent::MutationSettled { .. }))
            .count(),
        1
    );
    assert_eq!(
        input.read_with(cx, |input, _| input.surface().unwrap().binding()),
        binding("xabc", 2)
    );
}
