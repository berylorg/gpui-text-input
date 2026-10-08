use std::{cell::RefCell, collections::VecDeque, rc::Rc};

use gpui::prelude::*;
use gpui::{
    ClipboardItem, Entity, EntityInputHandler, IntoElement, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, Render, ScrollDelta, ScrollWheelEvent, TestAppContext, div,
};

use super::*;
use crate::{actions::*, state::EditSnapshot};

#[derive(Debug, PartialEq)]
struct EditingSnapshot {
    current: EditSnapshot,
    undo: VecDeque<EditSnapshot>,
    redo: VecDeque<EditSnapshot>,
    scroll: Point<Pixels>,
    reveal_cursor: bool,
    is_selecting: bool,
}

fn snapshot(input: &TextInput) -> EditingSnapshot {
    EditingSnapshot {
        current: EditSnapshot {
            text: input.state.text.clone(),
            selected_range: input.state.selected_range.clone(),
            selection_reversed: input.state.selection_reversed,
            marked_range: input.state.marked_range.clone(),
            atoms: input.state.atoms.clone(),
        },
        undo: input.state.undo_stack.clone(),
        redo: input.state.redo_stack.clone(),
        scroll: point(input.scroll_x, input.scroll_y),
        reveal_cursor: input.reveal_cursor,
        is_selecting: input.is_selecting,
    }
}

#[gpui::test]
fn disabled_native_delivery_preserves_editing_state_before_repaint(cx: &mut TestAppContext) {
    let (input, cx) = cx.add_window_view(|window, cx| {
        let mut input = TextInput::multiline("alpha\nbeta", "Body", cx);
        input.focus(window, cx);
        input
    });
    let events = Rc::new(RefCell::new(Vec::new()));
    let recorded = events.clone();
    cx.cx.update(|app| {
        app.subscribe(&input, move |_, event: &TextInputEvent, _| {
            recorded.borrow_mut().push(event.clone());
        })
        .detach();
    });

    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.replace_text_in_range(Some(0..5), "first", window, cx);
            input.replace_and_mark_text_in_range(Some(6..10), "候補", Some(0..1), window, cx);
            assert_eq!(input.text(), "first\n候補");
            assert!(input.state.marked_range().is_some());
            assert!(!input.state.undo_stack.is_empty());
            input.scroll_x = px(7.0);
            input.scroll_y = px(11.0);
            input.reveal_cursor = false;
        });
    });
    events.borrow_mut().clear();

    cx.update(|window, app| {
        input.update(app, |input, cx| {
            let before = snapshot(input);
            input.set_enabled(false, cx);
            input.replace_text_in_range(Some(0..1), "rejected", window, cx);
            assert_eq!(snapshot(input), before);
            input.replace_and_mark_text_in_range(None, "rejected", Some(0..2), window, cx);
            assert_eq!(snapshot(input), before);
            input.unmark_text(window, cx);
            assert_eq!(snapshot(input), before);
        });
    });
    assert!(events.borrow().is_empty());

    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.set_enabled(true, cx);
            assert_eq!(input.text(), "first\n候補");
            input.unmark_text(window, cx);
            assert!(input.state.marked_range().is_none());
            input.replace_text_in_range(Some(0..5), "new", window, cx);
            assert_eq!(input.text(), "new\n候補");
            input.replace_and_mark_text_in_range(Some(4..6), "次", Some(1..1), window, cx);
            assert_eq!(input.text(), "new\n次");
            assert!(input.state.marked_range().is_some());
        });
    });
    assert!(!events.borrow().is_empty());
}

struct ScrollableInput {
    input: Entity<TextInput>,
}

impl Render for ScrollableInput {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().w(px(200.0)).h(px(70.0)).child(self.input.clone())
    }
}

#[gpui::test]
fn disabled_retained_drag_and_wheel_delivery_preserves_selection_and_scroll(
    cx: &mut TestAppContext,
) {
    let (view, cx) = cx.add_window_view(|window, cx| {
        let input = cx.new(|cx| {
            let mut input = TextInput::multiline("alpha beta\n".repeat(50), "Body", cx);
            input.focus(window, cx);
            input
        });
        ScrollableInput { input }
    });
    let input = view.read_with(cx, |view, _| view.input.clone());

    cx.update(|window, app| {
        input.update(app, |input, cx| {
            let bounds = input.last_bounds.expect("painted input");
            assert!(layout::max_scroll_y(input.content_height, bounds) > px(0.0));
            let start = bounds.origin + point(px(2.0), px(2.0));
            let end = bounds.origin + point(px(120.0), px(35.0));
            let drag = MouseMoveEvent {
                position: end,
                pressed_button: Some(MouseButton::Left),
                ..Default::default()
            };
            let wheel = ScrollWheelEvent {
                position: start,
                delta: ScrollDelta::Pixels(point(
                    px(0.0),
                    if input.scroll_y > px(0.0) {
                        px(20.0)
                    } else {
                        px(-20.0)
                    },
                )),
                ..Default::default()
            };
            input.on_mouse_down(
                &MouseDownEvent {
                    position: start,
                    button: MouseButton::Left,
                    click_count: 1,
                    ..Default::default()
                },
                window,
                cx,
            );
            assert!(input.is_selecting);
            let before = snapshot(input);
            input.set_enabled(false, cx);
            input.on_mouse_move(&drag, window, cx);
            assert_eq!(snapshot(input), before);
            input.on_scroll_wheel(&wheel, window, cx);
            assert_eq!(snapshot(input), before);
            input.on_mouse_up(&MouseUpEvent::default(), window, cx);
            assert_eq!(snapshot(input), before);

            input.set_enabled(true, cx);
            assert_eq!(snapshot(input), before);
            input.on_mouse_move(&drag, window, cx);
            assert_ne!(input.state.selection(), before.current.selected_range);
            let before_scroll = input.scroll_y;
            input.on_scroll_wheel(&wheel, window, cx);
            assert_ne!(input.scroll_y, before_scroll);
            input.on_mouse_up(&MouseUpEvent::default(), window, cx);
            assert!(!input.is_selecting);
        });
    });
}

#[gpui::test]
fn disabled_action_delivery_preserves_history_selection_scroll_and_clipboard(
    cx: &mut TestAppContext,
) {
    let (input, cx) = cx.add_window_view(|window, cx| {
        let mut input = TextInput::multiline("alpha\nbeta", "Body", cx);
        input.focus(window, cx);
        input
    });
    let events = Rc::new(RefCell::new(Vec::new()));
    let recorded = events.clone();
    cx.cx.update(|app| {
        app.subscribe(&input, move |_, event: &TextInputEvent, _| {
            recorded.borrow_mut().push(event.clone());
        })
        .detach();
    });
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.replace_text_in_range(None, "!", window, cx);
            input.replace_text_in_range(None, "?", window, cx);
            input.undo(&Undo, window, cx);
            assert!(!input.state.undo_stack.is_empty());
            assert!(!input.state.redo_stack.is_empty());
            input.select_all(&SelectAll, window, cx);
            input.state.selection_reversed = true;
            input.scroll_x = px(5.0);
            input.scroll_y = px(13.0);
            input.reveal_cursor = false;
            cx.write_to_clipboard(ClipboardItem::new_string("clipboard".to_string()));
        });
    });
    events.borrow_mut().clear();

    cx.update(|window, app| {
        input.update(app, |input, cx| {
            let before = snapshot(input);
            input.set_enabled(false, cx);
            macro_rules! deliver {
                ($handler:ident, $action:ident) => {
                    input.$handler(&$action, window, cx);
                    assert_eq!(snapshot(input), before, stringify!($action));
                    assert_eq!(
                        cx.read_from_clipboard().and_then(|item| item.text()),
                        Some("clipboard".to_string()),
                        stringify!($action)
                    );
                };
            }
            deliver!(backspace, Backspace);
            deliver!(delete, Delete);
            deliver!(delete_word_backward, DeleteWordBackward);
            deliver!(delete_word_forward, DeleteWordForward);
            deliver!(enter, Enter);
            deliver!(insert_newline_action, InsertNewline);
            deliver!(copy, Copy);
            deliver!(cut, Cut);
            deliver!(paste, Paste);
            deliver!(undo, Undo);
            deliver!(redo, Redo);
            deliver!(move_left, MoveLeft);
            deliver!(move_right, MoveRight);
            deliver!(move_up, MoveUp);
            deliver!(move_down, MoveDown);
            deliver!(move_word_left, MoveWordLeft);
            deliver!(move_word_right, MoveWordRight);
            deliver!(select_left, SelectLeft);
            deliver!(select_right, SelectRight);
            deliver!(select_up, SelectUp);
            deliver!(select_down, SelectDown);
            deliver!(select_word_left, SelectWordLeft);
            deliver!(select_word_right, SelectWordRight);
            deliver!(move_home, MoveHome);
            deliver!(move_end, MoveEnd);
            deliver!(select_home, SelectHome);
            deliver!(select_end, SelectEnd);
            deliver!(move_to_start, MoveToStart);
            deliver!(move_to_end, MoveToEnd);
            deliver!(select_to_start, SelectToStart);
            deliver!(select_to_end, SelectToEnd);
            deliver!(select_all, SelectAll);
        });
    });
    assert!(events.borrow().is_empty());

    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.set_enabled(true, cx);
            assert_eq!(input.text(), "alpha\nbeta!");
            input.cut(&Cut, window, cx);
            assert_eq!(input.text(), "");
            assert_eq!(
                cx.read_from_clipboard().and_then(|item| item.text()),
                Some("alpha\nbeta!".to_string())
            );
            input.paste(&Paste, window, cx);
            assert_eq!(input.text(), "alpha\nbeta!");
            input.undo(&Undo, window, cx);
            assert_eq!(input.text(), "");
            input.redo(&Redo, window, cx);
            assert_eq!(input.text(), "alpha\nbeta!");
            input.move_to_start(&MoveToStart, window, cx);
            assert_eq!(input.cursor_offset(), 0);
            input.select_all(&SelectAll, window, cx);
            assert_eq!(input.state.selection(), 0..input.text().len());
            input.enter(&Enter, window, cx);
            assert_eq!(input.text(), "\n");
        });
    });
    assert!(!events.borrow().is_empty());
}
