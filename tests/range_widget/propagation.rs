use super::*;
use gpui::ClipboardItem;
use gpui_text_input::{TextInputCommand, TextInputEnterKey, TextInputRichPastePolicy};

fn propagated_commands(events: &Rc<RefCell<Vec<RangeTextInputEvent>>>) -> Vec<TextInputCommand> {
    events
        .borrow()
        .iter()
        .filter_map(|event| match event {
            RangeTextInputEvent::CommandPropagated(command) => Some(*command),
            _ => None,
        })
        .collect()
}

#[gpui::test]
fn enter_propagates_without_mutation_while_shift_enter_inserts_newline(
    cx: &mut gpui::TestAppContext,
) {
    cx.update(ensure_text_input_bindings);
    let source = "alpha";
    let mut configuration = config(source, 1);
    configuration.enter_key = TextInputEnterKey::Propagate;
    let (input, cx) = cx.add_window_view(|window, cx| {
        let input = RangeTextInput::new(configuration, window, cx).unwrap();
        input.focus(window);
        input
    });
    assert!(drive_pages(&input, cx, source).is_empty());
    input.update(cx, |input, _| {
        admit_ordinary_edit_positions(input, source, 1, &[0]);
    });
    let events = restoration_events(&input, cx);
    let before = range_publication_fingerprint(&input, cx);
    let seed = input.read_with(cx, |input, _| input.export_restoration(None).unwrap());

    cx.simulate_keystrokes("enter");

    assert_eq!(propagated_commands(&events), vec![TextInputCommand::Enter]);
    let request = input.update(cx, |input, _| input.take_request());
    assert!(request.is_none(), "propagated Enter queued {request:?}");
    assert_eq!(range_publication_fingerprint(&input, cx), before);
    input.read_with(cx, |input, _| {
        assert_eq!(input.export_restoration(None).unwrap(), seed);
    });

    cx.simulate_keystrokes("shift-enter");
    assert!(matches!(
        input.update(cx, |input, _| input.take_request()),
        Some(RangeTextInputRequest::MutationBegin(_))
    ));
    assert_eq!(propagated_commands(&events), vec![TextInputCommand::Enter]);
}

#[gpui::test]
fn rich_paste_propagates_without_read_or_mutation_and_plain_text_paste_stays_internal(
    cx: &mut gpui::TestAppContext,
) {
    cx.update(ensure_text_input_bindings);
    let source = "alpha";
    let mut propagated = config(source, 1);
    propagated.rich_paste_policy = TextInputRichPastePolicy::Propagate;
    let (input, cx) = cx.add_window_view(|window, cx| {
        let input = RangeTextInput::new(propagated, window, cx).unwrap();
        input.focus(window);
        input
    });
    assert!(drive_pages(&input, cx, source).is_empty());
    input.update(cx, |input, _| {
        admit_ordinary_edit_positions(input, source, 1, &[0]);
    });
    let events = restoration_events(&input, cx);
    let before = range_publication_fingerprint(&input, cx);
    let seed = input.read_with(cx, |input, _| input.export_restoration(None).unwrap());
    cx.write_to_clipboard(ClipboardItem::new_string("pasted".to_owned()));

    cx.simulate_keystrokes("ctrl-v");

    assert_eq!(propagated_commands(&events), vec![TextInputCommand::Paste]);
    assert!(input.update(cx, |input, _| input.take_request()).is_none());
    assert_eq!(range_publication_fingerprint(&input, cx), before);
    input.read_with(cx, |input, _| {
        assert_eq!(input.export_restoration(None).unwrap(), seed);
    });

    let (plain, cx) = cx.add_window_view(|window, cx| {
        let input = RangeTextInput::new(config(source, 2), window, cx).unwrap();
        input.focus(window);
        input
    });
    assert!(drive_pages(&plain, cx, source).is_empty());
    plain.update(cx, |input, _| {
        admit_ordinary_edit_positions(input, source, 2, &[0]);
    });
    cx.simulate_keystrokes("ctrl-v");
    assert!(matches!(
        plain.update(cx, |input, _| input.take_request()),
        Some(RangeTextInputRequest::MutationBegin(_))
    ));
}

#[gpui::test]
fn atom_cut_propagates_after_bounded_classification_without_write_or_deletion(
    cx: &mut gpui::TestAppContext,
) {
    cx.update(ensure_text_input_bindings);
    let source = "ab";
    let object = object_fact(901, 1, 1);
    let mut configuration = config(source, 1);
    configuration.atom_clipboard_policy = TextInputAtomClipboardPolicy::Propagate;
    let (input, cx) = cx.add_window_view(|window, cx| {
        let input = RangeTextInput::new(configuration, window, cx).unwrap();
        input.focus(window);
        input
    });
    drive_pages_with_objects(&input, cx, source, std::slice::from_ref(&object));
    cx.simulate_keystrokes("ctrl-a");
    drive_pages_with_objects(&input, cx, source, std::slice::from_ref(&object));
    let start = ordinary_position(0);
    let end = ordinary_position(source.len() as u64);
    let (surface_proves_start_gap, surface_proves_end_gap) = input.read_with(cx, |input, _| {
        let surface = input.surface().unwrap();
        assert_eq!(surface.source_caret(), end);
        assert_eq!(
            surface.source_selection(),
            RangeSourceSelection {
                anchor: start,
                head: end,
            }
        );
        assert_eq!(surface.scroll_position(), end);
        assert_eq!(
            surface.binding().extent().byte_len(),
            end.byte_offset.get()
        );
        assert!(source.is_char_boundary(start.byte_offset.get() as usize));
        assert!(source.is_char_boundary(end.byte_offset.get() as usize));
        assert_eq!(start.gap, InlineObjectGap::NoObjects);
        assert_eq!(end.gap, InlineObjectGap::NoObjects);
        assert_eq!(object.anchor(), ByteOffset::new(1));

        assert_eq!(surface.object_pages().len(), 1);
        let page = &surface.object_pages()[0];
        assert!(page.objects().is_empty());
        assert_eq!(
            page.preceding(),
            ObjectPageEdgeFact::Continues(object.cursor())
        );
        assert_eq!(page.following(), ObjectPageEdgeFact::EnvelopeBoundary);
        (
            page.key().demand().contains_anchor(start.byte_offset),
            page.key().demand().contains_anchor(end.byte_offset)
                && object.anchor() < end.byte_offset,
        )
    });
    assert!(!surface_proves_start_gap);
    assert!(surface_proves_end_gap);
    assert!(matches!(
        input.read_with(cx, |input, _| input.export_restoration(None)),
        Err(gpui_text_input::RangeTextInputError::IncompleteSurface)
    ));

    let distinct_edit_positions = [start, end];
    assert!(
        distinct_edit_positions
            .iter()
            .all(|position| position.byte_offset != object.anchor())
    );
    let (text, objects) = admitted_sources_with_facts(
        source,
        1,
        &distinct_edit_positions,
        std::slice::from_ref(&object),
    );
    input.update(cx, |input, _| {
        input
            .admit_edit_positions(&distinct_edit_positions, &text, &objects)
            .unwrap();
    });
    let events = restoration_events(&input, cx);
    let before = range_publication_fingerprint(&input, cx);
    let seed = input.read_with(cx, |input, _| input.export_restoration(None).unwrap());
    assert_eq!(seed.caret, end);
    assert_eq!(
        seed.selection,
        RangeSourceSelection {
            anchor: start,
            head: end,
        }
    );
    assert_eq!(seed.scroll.position, end);

    cx.simulate_keystrokes("ctrl-x");
    drive_pages_with_objects(&input, cx, source, std::slice::from_ref(&object));

    assert_eq!(propagated_commands(&events), vec![TextInputCommand::Cut]);
    assert!(input.update(cx, |input, _| input.take_request()).is_none());
    assert_eq!(range_publication_fingerprint(&input, cx), before);
    input.read_with(cx, |input, _| {
        assert!(input.is_quiescent());
        assert_eq!(input.export_restoration(None).unwrap(), seed);
    });
}

#[gpui::test]
fn atom_policy_keeps_text_only_copy_and_cut_on_the_bounded_internal_path(
    cx: &mut gpui::TestAppContext,
) {
    cx.update(ensure_text_input_bindings);
    let source = "plain text";
    let mut configuration = config(source, 1);
    configuration.atom_clipboard_policy = TextInputAtomClipboardPolicy::Propagate;
    let (input, cx) = cx.add_window_view(|window, cx| {
        let input = RangeTextInput::new(configuration, window, cx).unwrap();
        input.focus(window);
        input
    });
    assert!(drive_pages(&input, cx, source).is_empty());
    cx.simulate_keystrokes("ctrl-a");
    assert!(drive_pages(&input, cx, source).is_empty());
    cx.simulate_keystrokes("ctrl-c");
    let copy_requests = drive_pages(&input, cx, source);
    let copy = copy_requests
        .iter()
        .find_map(|request| match request {
            RangeTextInputRequest::ClipboardWrite(write) => Some(write),
            _ => None,
        })
        .expect("text-only copy stays internal");
    assert_eq!(copy.text(), source);
    input.update(cx, |input, cx| {
        input
            .settle_clipboard_write(copy.key(), ClipboardWriteOutcome::Written, cx)
            .unwrap();
    });

    let before_cut = range_publication_fingerprint(&input, cx);
    cx.simulate_keystrokes("ctrl-x");
    let cut_requests = drive_pages(&input, cx, source);
    let cut = cut_requests
        .iter()
        .find_map(|request| match request {
            RangeTextInputRequest::ClipboardWrite(write) => Some(write),
            _ => None,
        })
        .expect("text-only cut stays internal");
    assert_eq!(cut.text(), source);
    assert_eq!(range_publication_fingerprint(&input, cx), before_cut);
    input.update(cx, |input, cx| {
        input
            .settle_clipboard_write(cut.key(), ClipboardWriteOutcome::Written, cx)
            .unwrap();
    });
    assert!(matches!(
        input.update(cx, |input, _| input.take_request()),
        Some(RangeTextInputRequest::MutationBegin(_))
    ));
}
