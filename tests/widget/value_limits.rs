use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use gpui::prelude::*;
use gpui::{ClipboardItem, EntityInputHandler, Point};

use super::*;
use gpui_text_input::TextInputState;

#[derive(Debug, PartialEq)]
struct StateFacts {
    text: String,
    selection: std::ops::Range<usize>,
    reversed: bool,
    marked: Option<std::ops::Range<usize>>,
    atoms: Vec<TextInputAtom>,
}

fn state_facts(state: &TextInputState) -> StateFacts {
    StateFacts {
        text: state.text().to_owned(),
        selection: state.selection(),
        reversed: state.selection_reversed(),
        marked: state.marked_range(),
        atoms: state.atoms().to_vec(),
    }
}

fn history(state: &TextInputState, undo: bool) -> Vec<StateFacts> {
    let mut state = state.clone();
    state.set_value_byte_limit(None).unwrap();
    state.set_read_only(false);
    let mut values = Vec::new();
    while if undo { state.undo() } else { state.redo() }.is_some() {
        values.push(state_facts(&state));
    }
    values
}

#[derive(Debug, PartialEq)]
struct EditorFacts {
    current: StateFacts,
    undo: Vec<StateFacts>,
    redo: Vec<StateFacts>,
    scroll: Point<Pixels>,
    visible: std::ops::Range<usize>,
    retained: TextInputRetainedCounts,
}

fn facts(input: &TextInput) -> EditorFacts {
    EditorFacts {
        current: state_facts(input.state()),
        undo: history(input.state(), true),
        redo: history(input.state(), false),
        scroll: input.scroll_offset(),
        visible: input.visible_range(),
        retained: input.retained_counts(),
    }
}

#[gpui::test]
fn capped_platform_atom_and_host_replacements_leave_all_editor_facts_unchanged(
    cx: &mut gpui::TestAppContext,
) {
    let (input, cx) =
        fixed_multiline_input(cx, "éalpha\nbeta\n".repeat(40), input_bounds(200.0, 70.0));
    let events = Rc::new(RefCell::new(Vec::new()));
    let captured = events.clone();
    cx.cx.update(|app| {
        app.subscribe(&input, move |_, event: &TextInputEvent, _| {
            captured.borrow_mut().push(event.clone())
        })
        .detach();
    });
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input
                .set_atoms([TextInputAtom::new("atom", 0..2, "accent")], cx)
                .unwrap();
            input.replace_and_mark_text_in_range(Some(2..3), "候", Some(0..0), window, cx);
            input.set_selection(3..6, true, cx);
            input
                .set_value_byte_limit(Some(input.text().len()), cx)
                .unwrap();
        });
    });
    {
        let setup = events.borrow();
        assert_eq!(
            setup
                .iter()
                .filter(|event| matches!(event, TextInputEvent::Changed(_)))
                .count(),
            1
        );
        assert!(
            setup.iter().all(|event| matches!(
                event,
                TextInputEvent::Changed(_) | TextInputEvent::SelectionChanged(_)
            )),
            "setup events: {setup:?}"
        );
    }
    let position = input.read_with(cx, |input, _| {
        input.geometry().unwrap().bounds.origin + point(px(4.0), px(4.0))
    });
    cx.simulate_event(ScrollWheelEvent {
        position,
        delta: ScrollDelta::Pixels(point(px(0.0), px(-30.0))),
        ..Default::default()
    });
    input.read_with(cx, |input, _| assert!(input.scroll_offset().y > px(0.0)));
    events.borrow_mut().clear();
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            let before = facts(input);
            let payload = "🌳".repeat(20_000);
            assert!(!input.set_text_and_select(payload.clone(), cx));
            assert_eq!(facts(input), before);
            assert!(!input.set_text(payload.clone(), cx));
            assert_eq!(facts(input), before);
            input.replace_text_in_range(None, &payload, window, cx);
            assert_eq!(facts(input), before);
            input.replace_and_mark_text_in_range(None, &payload, Some(0..2), window, cx);
            assert_eq!(facts(input), before);
            assert!(!input.insert_text_at_offset(0, &payload, cx));
            assert_eq!(facts(input), before);
            assert!(
                !input
                    .replace_text_range_with_atom(0..2, &payload, "rejected", "copy", cx)
                    .unwrap()
            );
            assert_eq!(facts(input), before);
            assert!(
                !input
                    .replace_selected_text_with_atoms(&payload, [], cx)
                    .unwrap()
            );
            assert_eq!(facts(input), before);
        });
    });
    let events = events.borrow();
    assert_eq!(events.len(), 7);
    assert!(events.iter().all(|event| matches!(event, TextInputEvent::ValueLimitExceeded { limit, attempted_bytes } if attempted_bytes > limit)));
}

#[gpui::test]
fn mounted_typing_paste_and_history_refusals_emit_only_bounded_failure_events(
    cx: &mut gpui::TestAppContext,
) {
    cx.update(ensure_text_input_bindings);
    let (input, cx) = cx.add_window_view(|window, cx| {
        let mut input = TextInput::new("", "Name", cx);
        input.set_value_byte_limit(Some(4), cx).unwrap();
        input.focus(window, cx);
        input
    });
    let events = Rc::new(RefCell::new(Vec::new()));
    let captured = events.clone();
    cx.cx.update(|app| {
        app.subscribe(&input, move |_, event: &TextInputEvent, _| {
            captured.borrow_mut().push(event.clone())
        })
        .detach();
    });
    cx.simulate_input("🌳");
    input.read_with(cx, |input, _| assert_eq!(input.text(), "🌳"));
    events.borrow_mut().clear();
    cx.simulate_input("é");
    cx.cx
        .update(|app| app.write_to_clipboard(ClipboardItem::new_string("oversized".to_owned())));
    cx.simulate_keystrokes("ctrl-v");
    input.read_with(cx, |input, _| assert_eq!(input.text(), "🌳"));
    assert_eq!(
        events
            .borrow()
            .iter()
            .filter(|event| matches!(event, TextInputEvent::ValueLimitExceeded { .. }))
            .count(),
        2
    );
    assert!(!events.borrow().iter().any(|event| matches!(
        event,
        TextInputEvent::Changed(_) | TextInputEvent::SelectionChanged(_)
    )));

    cx.simulate_keystrokes("ctrl-z");
    input.update(cx, |input, cx| {
        input.set_value_byte_limit(Some(0), cx).unwrap()
    });
    let before = input.read_with(cx, |input, _| facts(input));
    events.borrow_mut().clear();
    cx.simulate_keystrokes("ctrl-y");
    input.read_with(cx, |input, _| assert_eq!(facts(input), before));
    assert!(events.borrow().iter().any(|event| matches!(
        event,
        TextInputEvent::ValueLimitExceeded {
            limit: 0,
            attempted_bytes: 4
        }
    )));
    input.update(cx, |input, cx| {
        input.set_value_byte_limit(None, cx).unwrap()
    });
    cx.simulate_keystrokes("ctrl-y");
    input.read_with(cx, |input, _| assert_eq!(input.text(), "🌳"));
}

#[gpui::test]
fn dynamic_read_only_rejects_retained_ime_and_edit_callbacks_without_resetting_the_input(
    cx: &mut gpui::TestAppContext,
) {
    cx.update(ensure_text_input_bindings);
    let (input, cx) = cx.add_window_view(|window, cx| {
        let mut input = TextInput::multiline("abc", "Body", cx);
        input.focus(window, cx);
        input
    });
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.replace_and_mark_text_in_range(Some(1..2), "候", Some(0..0), window, cx);
            let before = facts(input);
            input.set_read_only(true, cx);
            assert_eq!(facts(input), before);
            input.replace_text_in_range(None, "blocked", window, cx);
            input.replace_and_mark_text_in_range(None, "blocked", None, window, cx);
            input.unmark_text(window, cx);
            assert_eq!(facts(input), before);
        });
    });
    let before = input.read_with(cx, |input, _| facts(input));
    cx.cx
        .update(|app| app.write_to_clipboard(ClipboardItem::new_string("blocked".to_owned())));
    cx.simulate_keystrokes("ctrl-z");
    cx.simulate_keystrokes("ctrl-v");
    input.read_with(cx, |input, _| assert_eq!(facts(input), before));
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.set_read_only(false, cx);
            input.replace_text_in_range(None, "x", window, cx);
            assert_eq!(input.text(), "axc");
            assert!(input.state().marked_range().is_none());
        })
    });
    cx.simulate_keystrokes("ctrl-z");
    input.read_with(cx, |input, _| {
        assert_eq!(state_facts(input.state()), before.current)
    });
}

struct EnterParent {
    input: gpui::Entity<TextInput>,
    propagated: Rc<Cell<usize>>,
}

impl Render for EnterParent {
    fn render(&mut self, _: &mut Window, cx: &mut gpui::Context<Self>) -> impl IntoElement {
        div()
            .on_action(cx.listener(|view, _: &gpui_text_input::Enter, _, _| {
                view.propagated.set(view.propagated.get() + 1);
            }))
            .child(self.input.clone())
    }
}

#[gpui::test]
fn exact_cap_enter_reports_once_without_change_or_parent_propagation(
    cx: &mut gpui::TestAppContext,
) {
    cx.update(ensure_text_input_bindings);
    let propagated = Rc::new(Cell::new(0));
    let observed = propagated.clone();
    let (view, cx) = cx.add_window_view(|window, cx| {
        let input = cx.new(|cx| {
            let mut input = TextInput::multiline("x", "Body", cx);
            input.set_value_byte_limit(Some(1), cx).unwrap();
            input.focus(window, cx);
            input
        });
        EnterParent {
            input,
            propagated: observed,
        }
    });
    let input = view.read_with(cx, |view, _| view.input.clone());
    let events = Rc::new(RefCell::new(Vec::new()));
    let captured = events.clone();
    cx.cx.update(|app| {
        app.subscribe(&input, move |_, event: &TextInputEvent, _| {
            captured.borrow_mut().push(event.clone())
        })
        .detach()
    });
    let before = input.read_with(cx, |input, _| facts(input));
    cx.simulate_keystrokes("enter");
    assert_eq!(propagated.get(), 0);
    assert_eq!(
        &*events.borrow(),
        &[TextInputEvent::ValueLimitExceeded {
            limit: 1,
            attempted_bytes: 2
        }]
    );
    input.read_with(cx, |input, _| assert_eq!(facts(input), before));
    input.update(cx, |input, cx| {
        input.set_value_byte_limit(Some(2), cx).unwrap()
    });
    events.borrow_mut().clear();
    cx.simulate_keystrokes("enter");
    input.read_with(cx, |input, _| assert_eq!(input.text(), "x\n"));
    assert_eq!(propagated.get(), 0);
    assert!(
        !events
            .borrow()
            .iter()
            .any(|event| matches!(event, TextInputEvent::ValueLimitExceeded { .. }))
    );
}
