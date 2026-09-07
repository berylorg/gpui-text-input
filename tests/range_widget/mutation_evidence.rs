use super::*;
use gpui_text_input::{MutationError, MutationPassKind, RangeTextInputError};

fn begin_local(
    input: &gpui::Entity<RangeTextInput>,
    cx: &mut gpui::VisualTestContext,
) -> MutationBeginRequest {
    input.update(cx, |input, _| {
        admit_ordinary_edit_positions(input, "abc", 1, &[0])
    });
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.replace_text_in_range(Some(0..0), "x", window, cx);
        })
    });
    drive_pages(input, cx, "abc")
        .into_iter()
        .find_map(|request| match request {
            RangeTextInputRequest::MutationBegin(begin) => Some(begin),
            _ => None,
        })
        .expect("local insertion begin")
}

#[gpui::test]
fn evidence_local_page_survives_async_admission_and_replays_exact_bytes(
    cx: &mut gpui::TestAppContext,
) {
    let (input, cx) = cx.add_window_view(|window, cx| {
        let input = RangeTextInput::new(config("abc", 1), window, cx).unwrap();
        input.focus(window);
        input
    });
    assert!(drive_pages(&input, cx, "abc").is_empty());
    let begin = begin_local(&input, cx);
    input.update(cx, |input, cx| {
        let key = begin.proposal().key();
        let evidence = input.request_mutation_evidence(key).unwrap();
        assert_eq!(evidence.producer(), begin.producer().unwrap());
        let (page, finish) = input.local_mutation_evidence(evidence).unwrap();
        let page = page.unwrap().clone();
        assert_eq!(page.payload_owner_count(), 2);
        let acknowledgement = input.submit_mutation_evidence_page(evidence, page.clone(), cx).unwrap();
        assert!(matches!(input.accept_mutation_preflight(key, cx), Err(RangeTextInputError::Mutation(MutationError::MissingFinishInput))));
        assert!(matches!(input.submit_mutation_evidence_finish(evidence, finish), Err(RangeTextInputError::Mutation(MutationError::EvidenceAcknowledgementPending))));
        assert!(input.take_request().is_none());
        input.acknowledge_mutation_evidence_page(acknowledgement).unwrap();
        input.submit_mutation_evidence_finish(evidence, finish).unwrap();
        input.accept_mutation_preflight(key, cx).unwrap();
        assert!(input.take_request().is_none());
        let staging = input.mutation_restart(key).unwrap();
        assert_eq!(staging.kind(), MutationPassKind::Staging);
        assert!(matches!(input.acknowledge_mutation_restart(evidence, cx), Err(RangeTextInputError::Mutation(MutationError::WrongMutationPass))));
        input.acknowledge_mutation_restart(staging, cx).unwrap();
        assert!(matches!(input.acknowledge_mutation_evidence_page(acknowledgement), Err(RangeTextInputError::Mutation(MutationError::WrongMutationPass))));
        assert!(input.local_mutation_evidence(evidence).is_err());
        let replay = input.take_request().unwrap();
        assert!(matches!(&replay, RangeTextInputRequest::MutationProposalPage(request)
            if request.page() == &page && request.pass() == Some(staging)));
        assert!(matches!(input.take_request(), Some(RangeTextInputRequest::MutationFinishInput(actual)) if actual == finish));
        drop(replay);
        assert_eq!(page.payload_owner_count(), 1);
        input.accept_mutation_finish(key, cx).unwrap();
        assert!(matches!(input.take_request(), Some(RangeTextInputRequest::MutationCommit(request)) if request.key() == key));
    });
    cx.update(|window, app| input.update(app, |input, cx| {
        let requests = input.dispose(window, cx);
        assert!(requests.iter().any(|request| matches!(request, RangeTextInputRequest::DetachedMutation(key) if *key == begin.proposal().key())));
    }));
}

#[gpui::test]
fn evidence_local_disposal_releases_replay_payload_and_late_ack(cx: &mut gpui::TestAppContext) {
    let (input, cx) = cx.add_window_view(|window, cx| {
        let input = RangeTextInput::new(config("abc", 1), window, cx).unwrap();
        input.focus(window);
        input
    });
    assert!(drive_pages(&input, cx, "abc").is_empty());
    let begin = begin_local(&input, cx);
    let (page, acknowledgement) = input.update(cx, |input, cx| {
        let pass = input
            .request_mutation_evidence(begin.proposal().key())
            .unwrap();
        let page = input
            .local_mutation_evidence(pass)
            .unwrap()
            .0
            .unwrap()
            .clone();
        let acknowledgement = input
            .submit_mutation_evidence_page(pass, page.clone(), cx)
            .unwrap();
        (page, acknowledgement)
    });
    assert_eq!(page.payload_owner_count(), 2);
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.dispose(window, cx);
            assert!(
                input
                    .acknowledge_mutation_evidence_page(acknowledgement)
                    .is_err()
            );
            assert!(
                input
                    .local_mutation_evidence(acknowledgement.pass())
                    .is_err()
            );
        })
    });
    assert_eq!(page.payload_owner_count(), 1);
}
