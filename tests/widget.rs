use std::{cell::RefCell, rc::Rc};

use gpui::ClipboardItem;
use gpui_text_input::{
    TextInput, TextInputEnterKey, TextInputEvent, TextInputMode, TextInputOptions,
    TextInputRetainedCounts, TextInputSingleLineVerticalKey, ensure_text_input_bindings,
};

#[gpui::test]
fn widget_accepts_typed_text_and_emits_changes(cx: &mut gpui::TestAppContext) {
    cx.update(ensure_text_input_bindings);
    let (input, cx) = cx.add_window_view(|window, cx| {
        let mut input = TextInput::new("", "Name", cx);
        input.focus(window, cx);
        input
    });
    let events = Rc::new(RefCell::new(Vec::new()));
    let captured = events.clone();

    cx.cx.update(|cx| {
        cx.subscribe(&input, move |_, event: &TextInputEvent, _| {
            captured.borrow_mut().push(event.clone());
        })
        .detach();
    });

    cx.simulate_input("abc");

    input.read_with(cx, |input, _| assert_eq!(input.text(), "abc"));
    assert!(
        events
            .borrow()
            .iter()
            .any(|event| matches!(event, TextInputEvent::Changed(change) if change.text_changed))
    );
}

#[gpui::test]
fn lib_docs_widget_example_is_covered_by_nextest(cx: &mut gpui::TestAppContext) {
    cx.update(ensure_text_input_bindings);
    let (input, _) = cx.add_window_view(|_, cx| {
        let mut input = TextInput::new("", "Value", cx);
        input.set_enter_key(TextInputEnterKey::Propagate);
        input.set_single_line_vertical_key(TextInputSingleLineVerticalKey::Propagate);
        input
    });

    input.read_with(cx, |input, _| {
        assert_eq!(input.text(), "");
        assert_eq!(input.state().mode(), TextInputMode::SingleLine);
    });
}

#[gpui::test]
fn widget_retained_counts_forward_state_and_include_layout_cache(cx: &mut gpui::TestAppContext) {
    let (input, _) = cx.add_window_view(|_, cx| TextInput::new("abc", "Value", cx));

    input.read_with(cx, |input, _| {
        assert_eq!(
            input.retained_counts(),
            TextInputRetainedCounts {
                current_text_bytes: "abc".len(),
                widget_layout_line_count: Some(1),
                widget_visual_line_count: Some(1),
                widget_visible_text_bytes: Some("abc".len()),
                ..TextInputRetainedCounts::default()
            }
        );
    });
}

#[gpui::test]
fn widget_clear_edit_history_forwards_to_state(cx: &mut gpui::TestAppContext) {
    let (input, cx) = cx.add_window_view(|window, cx| {
        let mut input = TextInput::new("", "Value", cx);
        input.focus(window, cx);
        input
    });

    cx.simulate_input("draft");
    input.update(cx, |input, cx| {
        input.clear_edit_history();
        cx.notify();
    });

    cx.simulate_keystrokes("ctrl-z");
    input.read_with(cx, |input, _| assert_eq!(input.text(), "draft"));
}

#[gpui::test]
fn clipboard_actions_use_plain_text(cx: &mut gpui::TestAppContext) {
    cx.update(ensure_text_input_bindings);
    let (input, cx) = cx.add_window_view(|window, cx| {
        let mut input = TextInput::new("", "Name", cx);
        input.focus(window, cx);
        input
    });

    cx.simulate_input("hello");
    cx.simulate_keystrokes("ctrl-a ctrl-c");
    assert_eq!(
        cx.read_from_clipboard().and_then(|item| item.text()),
        Some("hello".to_string())
    );

    cx.simulate_keystrokes("ctrl-x");
    input.read_with(cx, |input, _| assert_eq!(input.text(), ""));

    cx.write_to_clipboard(ClipboardItem::new_string("world".to_string()));
    cx.simulate_keystrokes("ctrl-v");
    input.read_with(cx, |input, _| assert_eq!(input.text(), "world"));
}

#[gpui::test]
fn multiline_enter_inserts_newline(cx: &mut gpui::TestAppContext) {
    cx.update(ensure_text_input_bindings);
    let (input, cx) = cx.add_window_view(|window, cx| {
        let mut input = TextInput::multiline("alpha", "Body", cx);
        input.focus(window, cx);
        input
    });

    cx.simulate_keystrokes("enter");
    cx.simulate_input("beta");

    input.read_with(cx, |input, _| assert_eq!(input.text(), "alpha\nbeta"));
}

#[gpui::test]
fn multiline_enter_can_propagate_while_shift_enter_inserts_newline(cx: &mut gpui::TestAppContext) {
    cx.update(ensure_text_input_bindings);
    let (input, cx) = cx.add_window_view(|window, cx| {
        let mut input = TextInput::multiline("alpha", "Body", cx);
        input.set_enter_key(TextInputEnterKey::Propagate);
        input.focus(window, cx);
        input
    });

    cx.simulate_keystrokes("enter");
    input.read_with(cx, |input, _| assert_eq!(input.text(), "alpha"));

    cx.simulate_keystrokes("shift-enter");
    cx.simulate_input("beta");
    input.read_with(cx, |input, _| assert_eq!(input.text(), "alpha\nbeta"));
}

#[gpui::test]
fn read_only_widget_copies_but_rejects_mutation(cx: &mut gpui::TestAppContext) {
    cx.update(ensure_text_input_bindings);
    let (input, cx) = cx.add_window_view(|window, cx| {
        let mut input = TextInput::new_with_options(
            "locked",
            "Name",
            TextInputOptions::single_line_read_only(),
            cx,
        );
        input.focus(window, cx);
        input
    });

    cx.simulate_keystrokes("ctrl-a ctrl-c");
    assert_eq!(
        cx.read_from_clipboard().and_then(|item| item.text()),
        Some("locked".to_string())
    );

    cx.write_to_clipboard(ClipboardItem::new_string("open".to_string()));
    cx.simulate_keystrokes("ctrl-v backspace delete ctrl-x");
    input.read_with(cx, |input, _| assert_eq!(input.text(), "locked"));
}
