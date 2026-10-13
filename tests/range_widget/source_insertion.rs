use super::*;
use gpui::Focusable;
use gpui_text_input::{MutationPageItem, RangeTextInputError};

#[gpui::test]
fn source_insertion_uses_unfocused_selection_head_and_exact_replay(cx: &mut gpui::TestAppContext) {
    let source = "aébc";
    let selected = RangeSourceSelection {
        anchor: ordinary_position(0),
        head: ordinary_position(3),
    };
    let (input, cx) = cx
        .add_window_view(|window, cx| RangeTextInput::new(config(source, 1), window, cx).unwrap());
    assert!(drive_pages(&input, cx, source).is_empty());
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input
                .rebind(binding(source, 1), Some(selected), window, cx)
                .unwrap();
        })
    });
    assert!(drive_pages(&input, cx, source).is_empty());
    let focus = input.read_with(cx, |input, app| input.focus_handle(app));
    cx.update(|window, _| assert!(!focus.is_focused(window)));
    let exact = "**é**\r\n末\r\n";
    let key = input.update(cx, |input, cx| {
        admit_ordinary_edit_positions(input, source, 1, &[0, 3]);
        input.insert_text_at_caret(exact.into(), cx).unwrap()
    });
    let begin = match input.update(cx, |input, _| input.take_request()) {
        Some(RangeTextInputRequest::MutationBegin(begin)) => begin,
        request => panic!("source insertion begin: {request:?}"),
    };
    assert_eq!(begin.proposal().key(), key);
    assert_eq!(begin.proposal().kind(), MutationKind::Edit);
    assert_eq!(
        begin.proposal().predecessor(),
        MutationPositions::new(selected.head, selected.anchor, selected.head)
    );
    assert_eq!(
        begin.proposal().replacement(),
        SourceRange::new(selected.head, selected.head).unwrap()
    );
    input.update(cx, |input, cx| {
        assert_eq!(input.surface().unwrap().selection(), selected);
        let pass = input.request_mutation_evidence(key).unwrap();
        let (page, finish) = input.local_mutation_evidence(pass).unwrap();
        let page = page.unwrap().clone();
        assert!(matches!(&page.items()[0], MutationPageItem::Utf8 { inserted_offset: 0, text } if text.as_ref() == exact));
        assert_eq!(finish.intended(), MutationPositions::collapsed(ordinary_position(3 + exact.len() as u64)));
        let acknowledgement = input.submit_mutation_evidence_page(pass, page.clone(), cx).unwrap();
        input.acknowledge_mutation_evidence_page(acknowledgement).unwrap();
        input.submit_mutation_evidence_finish(pass, finish).unwrap();
        input.accept_mutation_preflight(key, cx).unwrap();
        let staging = input.mutation_restart(key).unwrap();
        input.acknowledge_mutation_restart(staging, cx).unwrap();
        assert!(matches!(input.take_request(), Some(RangeTextInputRequest::MutationProposalPage(request)) if request.page() == &page));
        assert!(matches!(input.take_request(), Some(RangeTextInputRequest::MutationFinishInput(actual)) if actual == finish));
    });
    cx.update(|window, _| assert!(!focus.is_focused(window)));
}

#[gpui::test]
fn source_insertion_capacity_refusal_preserves_selection_and_has_no_request(
    cx: &mut gpui::TestAppContext,
) {
    let source = "abc";
    let (input, cx) = cx
        .add_window_view(|window, cx| RangeTextInput::new(config(source, 1), window, cx).unwrap());
    assert!(drive_pages(&input, cx, source).is_empty());
    input.update(cx, |input, cx| {
        let before = input.surface().unwrap().selection();
        assert!(matches!(
            input.insert_text_at_caret("x".repeat(257), cx),
            Err(RangeTextInputError::SurfaceCapacity)
        ));
        assert_eq!(input.surface().unwrap().selection(), before);
        assert_eq!(input.surface().unwrap().binding(), binding(source, 1));
        assert!(input.take_request().is_none());
        assert!(input.is_semantically_quiescent());
        input.set_read_only(true, cx);
        assert!(matches!(
            input.insert_text_at_caret("x".into(), cx),
            Err(RangeTextInputError::ReadOnly)
        ));
        assert_eq!(input.surface().unwrap().selection(), before);
        assert!(input.take_request().is_none());
    });
}

#[gpui::test]
fn source_insertion_preserves_same_anchor_object_gap(cx: &mut gpui::TestAppContext) {
    let source = "ab";
    let facts = [object_fact(401, 1, 10), object_fact(402, 1, 20)];
    let at = SourcePosition::new(
        ByteOffset::new(1),
        InlineObjectGap::between(object_neighbor(401, 10), object_neighbor(402, 20)).unwrap(),
    );
    let (input, cx) = cx
        .add_window_view(|window, cx| RangeTextInput::new(config(source, 1), window, cx).unwrap());
    drive_pages_with_objects(&input, cx, source, &facts);
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input
                .rebind(
                    binding(source, 1),
                    Some(RangeSourceSelection::caret(at)),
                    window,
                    cx,
                )
                .unwrap();
        })
    });
    drive_pages_with_objects(&input, cx, source, &facts);
    input.update(cx, |input, cx| {
        let key = input.insert_text_at_caret("x".into(), cx).unwrap();
        let begin = match input.take_request() {
            Some(RangeTextInputRequest::MutationBegin(begin)) => begin,
            request => panic!("object-gap insertion begin: {request:?}"),
        };
        assert_eq!(begin.proposal().replacement(), SourceRange::new(at, at).unwrap());
        let pass = input.request_mutation_evidence(key).unwrap();
        let (page, finish) = input.local_mutation_evidence(pass).unwrap();
        assert_eq!(page.unwrap().items().len(), 1);
        assert!(matches!(&page.unwrap().items()[0], MutationPageItem::Utf8 { text, .. } if text.as_ref() == "x"));
        assert_eq!(finish.intended().caret().byte_offset, ByteOffset::new(2));
        assert_eq!(finish.intended().caret().gap, InlineObjectGap::Before(object_neighbor(402, 20)));
        assert_eq!(input.surface().unwrap().realized_objects().len(), 2);
    });
}
