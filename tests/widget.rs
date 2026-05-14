use std::{cell::RefCell, rc::Rc};

use gpui::prelude::*;
use gpui::{
    Bounds, ClipboardItem, Entity, IntoElement, Pixels, Render, Window, div, point, px, size,
};
use gpui_text_input::{
    TextInput, TextInputAtom, TextInputEnterKey, TextInputEvent, TextInputGeometry, TextInputMode,
    TextInputOptions, TextInputRetainedCounts, TextInputSingleLineVerticalKey,
    ensure_text_input_bindings,
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

#[gpui::test]
fn geometry_measurement_wraps_and_remeasures_for_width(cx: &mut gpui::TestAppContext) {
    let text = "alpha beta gamma delta epsilon zeta eta theta";
    let (input, cx) = cx.add_window_view(|_, cx| TextInput::multiline(text, "Body", cx));
    let narrow_bounds = input_bounds(80.0, 400.0);
    let wide_bounds = input_bounds(800.0, 400.0);

    let (narrow, wide, line_height, expected_narrow_lines) = cx.update(|window, app| {
        let input = input.read(app);
        (
            input.measure_geometry(narrow_bounds, window),
            input.measure_geometry(wide_bounds, window),
            window.line_height(),
            gpui_text_input::wrapped_visual_line_count_for_width(
                text,
                narrow_bounds.size.width,
                window,
            ),
        )
    });

    assert_eq!(narrow.visual_line_count, expected_narrow_lines);
    assert!(narrow.visual_line_count > wide.visual_line_count);
    assert_eq!(
        narrow.content_height,
        line_height * narrow.visual_line_count as f32
    );
    assert!(narrow.content_height > wide.content_height);
}

#[gpui::test]
fn geometry_reports_caret_bounds_after_soft_wrap(cx: &mut gpui::TestAppContext) {
    let text = "alpha beta gamma delta epsilon";
    let (input, cx) = cx.add_window_view(|_, cx| TextInput::multiline(text, "Body", cx));
    let bounds = input_bounds(70.0, 400.0);

    let geometry = cx.update(|window, app| input.read(app).measure_geometry(bounds, window));
    let caret = geometry.caret_bounds.expect("caret bounds");
    let endpoint = geometry
        .bounds_for_range(text.len()..text.len())
        .expect("endpoint bounds");

    assert!(geometry.visual_line_count > 1);
    assert!(caret.top() > bounds.top());
    assert_eq!(geometry.active_selection_endpoint_bounds, Some(endpoint));
}

#[gpui::test]
fn geometry_reveals_multiline_endpoint_under_capped_bounds(cx: &mut gpui::TestAppContext) {
    let text = "one two three four five six seven eight nine ten eleven twelve";
    let (input, cx) = cx.add_window_view(|_, cx| TextInput::multiline(text, "Body", cx));
    input.update(cx, |input, cx| {
        input.set_selection(0..0, false, cx);
        input.set_selection(text.len()..text.len(), false, cx);
    });
    let bounds = input_bounds(70.0, 40.0);

    let geometry = cx.update(|window, app| input.read(app).measure_geometry(bounds, window));
    let reveal = geometry.vertical_reveal.expect("multiline reveal data");

    assert!(geometry.visual_line_count > 2);
    assert!(geometry.scroll_limits.max_y > px(0.0));
    assert!(geometry.scroll_offset.y <= reveal.max_scroll_y);
    assert_eq!(reveal.scroll_y, reveal.max_scroll_y);
}

#[gpui::test]
fn geometry_reveals_active_endpoint_after_bounds_only_wrap_change(cx: &mut gpui::TestAppContext) {
    let text = "alpha beta gamma delta epsilon zeta eta theta iota kappa lambda ".repeat(6);
    let (input, cx) = cx.add_window_view(|_, cx| TextInput::multiline(text, "Body", cx));
    let bounds = input_bounds(90.0, 48.0);

    let geometry = cx.update(|window, app| input.read(app).measure_geometry(bounds, window));
    let reveal = geometry.vertical_reveal.expect("multiline reveal data");

    assert!(geometry.visual_line_count > 2);
    assert!(geometry.scroll_limits.max_y > px(0.0));
    assert_eq!(geometry.scroll_offset.y, reveal.scroll_y);
    assert_eq!(reveal.scroll_y, reveal.max_scroll_y);
    assert_active_endpoint_visible(&geometry);
}

#[gpui::test]
fn keyboard_newline_reveals_active_endpoint_under_capped_overflow(cx: &mut gpui::TestAppContext) {
    cx.update(ensure_text_input_bindings);
    let text = "alpha beta gamma delta epsilon zeta eta theta iota kappa lambda ".repeat(120);
    let (input, cx) = capped_multiline_input(cx, text);

    cx.simulate_keystrokes("enter");

    input.read_with(cx, |input, _| {
        assert!(input.text().ends_with('\n'));
        let geometry = input.geometry().expect("painted geometry");
        let reveal = geometry.vertical_reveal.expect("multiline reveal data");

        assert!(geometry.scroll_limits.max_y > px(0.0));
        assert_eq!(geometry.scroll_offset.y, reveal.scroll_y);
        assert_active_endpoint_visible(&geometry);
    });
}

#[gpui::test]
fn keyboard_paste_reveals_active_endpoint_under_capped_overflow(cx: &mut gpui::TestAppContext) {
    cx.update(ensure_text_input_bindings);
    let (input, cx) = capped_multiline_input(cx, "");
    let text = "pasted alpha beta gamma delta epsilon zeta eta theta iota kappa ".repeat(120);

    cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
    cx.simulate_keystrokes("ctrl-v");

    input.read_with(cx, |input, _| {
        assert_eq!(input.text(), text);
        let geometry = input.geometry().expect("painted geometry");
        let reveal = geometry.vertical_reveal.expect("multiline reveal data");

        assert!(geometry.scroll_limits.max_y > px(0.0));
        assert_eq!(geometry.scroll_offset.y, reveal.scroll_y);
        assert_active_endpoint_visible(&geometry);
    });
}

#[gpui::test]
fn keyboard_redo_reveals_active_endpoint_under_capped_overflow(cx: &mut gpui::TestAppContext) {
    cx.update(ensure_text_input_bindings);
    let (input, cx) = capped_multiline_input(cx, "");
    let text = "redo alpha beta gamma delta epsilon zeta eta theta iota kappa ".repeat(120);

    cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
    cx.simulate_keystrokes("ctrl-v ctrl-z ctrl-y");

    input.read_with(cx, |input, _| {
        assert_eq!(input.text(), text);
        let geometry = input.geometry().expect("painted geometry");
        let reveal = geometry.vertical_reveal.expect("multiline reveal data");

        assert!(geometry.scroll_limits.max_y > px(0.0));
        assert_eq!(geometry.scroll_offset.y, reveal.scroll_y);
        assert_active_endpoint_visible(&geometry);
    });
}

#[gpui::test]
fn geometry_combines_explicit_newlines_and_soft_wrap(cx: &mut gpui::TestAppContext) {
    let text = "alpha beta gamma\ndelta epsilon zeta";
    let second_line_start = text.find('\n').unwrap() + 1;
    let (input, cx) = cx.add_window_view(|_, cx| TextInput::multiline(text, "Body", cx));
    let bounds = input_bounds(70.0, 400.0);

    let geometry = cx.update(|window, app| input.read(app).measure_geometry(bounds, window));
    let first_line = geometry.bounds_for_range(0..0).expect("first line bounds");
    let second_line = geometry
        .bounds_for_range(second_line_start..second_line_start)
        .expect("second line bounds");

    assert!(geometry.visual_line_count >= 4);
    assert!(second_line.top() > first_line.top());
}

#[gpui::test]
fn geometry_reports_atom_range_bounds(cx: &mut gpui::TestAppContext) {
    let text = "See [A] here";
    let (input, cx) = cx.add_window_view(|_, cx| TextInput::multiline(text, "Body", cx));
    input.update(cx, |input, cx| {
        input
            .set_atoms(
                vec![TextInputAtom::new("atom-a", 4..7, "[Attachment A]")],
                cx,
            )
            .unwrap();
    });
    let bounds = input_bounds(300.0, 100.0);

    let geometry = cx.update(|window, app| input.read(app).measure_geometry(bounds, window));
    let atom_bounds = geometry.bounds_for_range(4..7).expect("atom range bounds");

    assert!(atom_bounds.size.width > px(0.0));
    assert!(atom_bounds.size.height > px(0.0));
}

#[gpui::test]
fn cached_geometry_exposes_last_painted_range_and_scroll_data(cx: &mut gpui::TestAppContext) {
    let text = "alpha beta gamma delta epsilon";
    let (input, cx) = cx.add_window_view(|_, cx| TextInput::multiline(text, "Body", cx));

    input.read_with(cx, |input, _| {
        let geometry = input.geometry().expect("painted geometry");
        assert_eq!(
            input.bounds_for_range(0..5),
            geometry.bounds_for_range(0..5)
        );
        assert_eq!(input.caret_bounds(), geometry.caret_bounds);
        assert_eq!(
            input.active_selection_endpoint_bounds(),
            geometry.active_selection_endpoint_bounds
        );
        assert_eq!(input.scroll_limits(), Some(geometry.scroll_limits));
    });
}

fn input_bounds(width: f32, height: f32) -> Bounds<Pixels> {
    Bounds::new(point(px(0.0), px(0.0)), size(px(width), px(height)))
}

fn capped_multiline_input(
    cx: &mut gpui::TestAppContext,
    text: impl Into<String>,
) -> (Entity<TextInput>, &mut gpui::VisualTestContext) {
    let text = text.into();
    let (view, cx) = cx.add_window_view(|window, cx| {
        let input = cx.new(|cx| {
            let mut input = TextInput::multiline(text, "Body", cx);
            input.focus(window, cx);
            input
        });

        FixedInputView {
            input,
            bounds: input_bounds(90.0, 48.0),
        }
    });
    let input = view.read_with(cx, |view, _| view.input.clone());

    (input, cx)
}

fn assert_active_endpoint_visible(geometry: &TextInputGeometry) {
    let endpoint = geometry
        .active_selection_endpoint_bounds
        .expect("active endpoint bounds");

    assert!(endpoint.top() >= geometry.bounds.top());
    assert!(endpoint.bottom() <= geometry.bounds.bottom());
}

struct FixedInputView {
    input: Entity<TextInput>,
    bounds: Bounds<Pixels>,
}

impl Render for FixedInputView {
    fn render(&mut self, _: &mut Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        div()
            .w(self.bounds.size.width)
            .h(self.bounds.size.height)
            .child(self.input.clone())
    }
}
