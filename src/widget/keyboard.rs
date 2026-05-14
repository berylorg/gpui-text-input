use gpui::{ClipboardEntry, ClipboardItem, MouseButton, ScrollWheelEvent};

use crate::{
    TextInputMode,
    actions::{
        Backspace, Copy, Cut, Delete, DeleteWordBackward, DeleteWordForward, Enter, InsertNewline,
        MoveDown, MoveEnd, MoveHome, MoveLeft, MoveRight, MoveToEnd, MoveToStart, MoveUp,
        MoveWordLeft, MoveWordRight, Paste, Redo, SelectAll, SelectDown, SelectEnd, SelectHome,
        SelectLeft, SelectRight, SelectToEnd, SelectToStart, SelectUp, SelectWordLeft,
        SelectWordRight, Undo,
    },
};

use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum VerticalDirection {
    Up,
    Down,
}

impl TextInput {
    pub(super) fn backspace(&mut self, _: &Backspace, _: &mut Window, cx: &mut Context<Self>) {
        let change = self.state.backspace();
        self.finish_text_command(TextInputCommand::Backspace, change, cx);
    }

    pub(super) fn delete(&mut self, _: &Delete, _: &mut Window, cx: &mut Context<Self>) {
        let change = self.state.delete();
        self.finish_text_command(TextInputCommand::Delete, change, cx);
    }

    pub(super) fn delete_word_backward(
        &mut self,
        _: &DeleteWordBackward,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let change = self.state.delete_word_backward();
        self.finish_text_command(TextInputCommand::DeleteWordBackward, change, cx);
    }

    pub(super) fn delete_word_forward(
        &mut self,
        _: &DeleteWordForward,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let change = self.state.delete_word_forward();
        self.finish_text_command(TextInputCommand::DeleteWordForward, change, cx);
    }

    pub(super) fn move_left(&mut self, _: &MoveLeft, _: &mut Window, cx: &mut Context<Self>) {
        let changed = self.state.move_left();
        self.finish_selection_command(TextInputCommand::MoveLeft, changed, cx);
    }

    pub(super) fn move_right(&mut self, _: &MoveRight, _: &mut Window, cx: &mut Context<Self>) {
        let changed = self.state.move_right();
        self.finish_selection_command(TextInputCommand::MoveRight, changed, cx);
    }

    pub(super) fn move_up(&mut self, _: &MoveUp, _: &mut Window, cx: &mut Context<Self>) {
        if self.state.mode() == TextInputMode::SingleLine
            && self.single_line_vertical_key == TextInputSingleLineVerticalKey::Propagate
        {
            cx.propagate();
            return;
        }

        let changed = self.move_vertically(VerticalDirection::Up, false);
        self.finish_selection_command(TextInputCommand::MoveUp, changed, cx);
    }

    pub(super) fn move_down(&mut self, _: &MoveDown, _: &mut Window, cx: &mut Context<Self>) {
        if self.state.mode() == TextInputMode::SingleLine
            && self.single_line_vertical_key == TextInputSingleLineVerticalKey::Propagate
        {
            cx.propagate();
            return;
        }

        let changed = self.move_vertically(VerticalDirection::Down, false);
        self.finish_selection_command(TextInputCommand::MoveDown, changed, cx);
    }

    pub(super) fn move_word_left(
        &mut self,
        _: &MoveWordLeft,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let changed = self.state.move_word_left();
        self.finish_selection_command(TextInputCommand::MoveWordLeft, changed, cx);
    }

    pub(super) fn move_word_right(
        &mut self,
        _: &MoveWordRight,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let changed = self.state.move_word_right();
        self.finish_selection_command(TextInputCommand::MoveWordRight, changed, cx);
    }

    pub(super) fn select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        let changed = self.state.select_left();
        self.finish_selection_command(TextInputCommand::SelectLeft, changed, cx);
    }

    pub(super) fn select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        let changed = self.state.select_right();
        self.finish_selection_command(TextInputCommand::SelectRight, changed, cx);
    }

    pub(super) fn select_up(&mut self, _: &SelectUp, _: &mut Window, cx: &mut Context<Self>) {
        let changed = self.move_vertically(VerticalDirection::Up, true);
        self.finish_selection_command(TextInputCommand::SelectUp, changed, cx);
    }

    pub(super) fn select_down(&mut self, _: &SelectDown, _: &mut Window, cx: &mut Context<Self>) {
        let changed = self.move_vertically(VerticalDirection::Down, true);
        self.finish_selection_command(TextInputCommand::SelectDown, changed, cx);
    }

    pub(super) fn select_word_left(
        &mut self,
        _: &SelectWordLeft,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let changed = self.state.select_word_left();
        self.finish_selection_command(TextInputCommand::SelectWordLeft, changed, cx);
    }

    pub(super) fn select_word_right(
        &mut self,
        _: &SelectWordRight,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let changed = self.state.select_word_right();
        self.finish_selection_command(TextInputCommand::SelectWordRight, changed, cx);
    }

    pub(super) fn move_home(&mut self, _: &MoveHome, _: &mut Window, cx: &mut Context<Self>) {
        let changed = self.state.move_home();
        self.finish_selection_command(TextInputCommand::MoveHome, changed, cx);
    }

    pub(super) fn move_end(&mut self, _: &MoveEnd, _: &mut Window, cx: &mut Context<Self>) {
        let changed = self.state.move_end();
        self.finish_selection_command(TextInputCommand::MoveEnd, changed, cx);
    }

    pub(super) fn select_home(&mut self, _: &SelectHome, _: &mut Window, cx: &mut Context<Self>) {
        let changed = self.state.select_home();
        self.finish_selection_command(TextInputCommand::SelectHome, changed, cx);
    }

    pub(super) fn select_end(&mut self, _: &SelectEnd, _: &mut Window, cx: &mut Context<Self>) {
        let changed = self.state.select_end();
        self.finish_selection_command(TextInputCommand::SelectEnd, changed, cx);
    }

    pub(super) fn move_to_start(
        &mut self,
        _: &MoveToStart,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let changed = self.state.move_to_start();
        self.finish_selection_command(TextInputCommand::MoveToStart, changed, cx);
    }

    pub(super) fn move_to_end(&mut self, _: &MoveToEnd, _: &mut Window, cx: &mut Context<Self>) {
        let changed = self.state.move_to_end();
        self.finish_selection_command(TextInputCommand::MoveToEnd, changed, cx);
    }

    pub(super) fn select_to_start(
        &mut self,
        _: &SelectToStart,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let changed = self.state.select_to_start();
        self.finish_selection_command(TextInputCommand::SelectToStart, changed, cx);
    }

    pub(super) fn select_to_end(
        &mut self,
        _: &SelectToEnd,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let changed = self.state.select_to_end();
        self.finish_selection_command(TextInputCommand::SelectToEnd, changed, cx);
    }

    pub(super) fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        let changed = self.state.select_all();
        self.finish_selection_command(TextInputCommand::SelectAll, changed, cx);
    }

    pub(super) fn enter(&mut self, _: &Enter, _: &mut Window, cx: &mut Context<Self>) {
        if self.enter_key == TextInputEnterKey::Propagate {
            cx.propagate();
            return;
        }

        self.perform_insert_newline_action(cx);
    }

    pub(super) fn insert_newline_action(
        &mut self,
        _: &InsertNewline,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.perform_insert_newline_action(cx);
    }

    fn perform_insert_newline_action(&mut self, cx: &mut Context<Self>) {
        let changed = self.state.insert_newline();
        if changed.is_none() {
            cx.propagate();
            return;
        }
        self.finish_text_command(TextInputCommand::InsertNewline, changed, cx);
    }

    pub(super) fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(selection) = self.state.selection_export() {
            if self.atom_clipboard_policy == TextInputAtomClipboardPolicy::Propagate
                && selection.has_atoms()
            {
                cx.propagate();
                return;
            }
            cx.write_to_clipboard(ClipboardItem::new_string(selection.copy_text().to_string()));
        }
        cx.emit(TextInputEvent::CommandHandled(TextInputCommand::Copy));
    }

    pub(super) fn cut(&mut self, _: &Cut, _: &mut Window, cx: &mut Context<Self>) {
        if self.atom_clipboard_policy == TextInputAtomClipboardPolicy::Propagate
            && self
                .state
                .selection_export()
                .is_some_and(|selection| selection.has_atoms())
        {
            cx.propagate();
            return;
        }

        if let Some((text, change)) = self.state.cut_selection() {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
            self.finish_text_command(TextInputCommand::Cut, Some(change), cx);
        } else {
            cx.emit(TextInputEvent::CommandHandled(TextInputCommand::Cut));
        }
    }

    pub(super) fn paste(&mut self, _: &Paste, _: &mut Window, cx: &mut Context<Self>) {
        let Some(item) = cx.read_from_clipboard() else {
            cx.emit(TextInputEvent::CommandHandled(TextInputCommand::Paste));
            return;
        };
        if self.rich_paste_policy == TextInputRichPastePolicy::Propagate
            && clipboard_item_should_propagate(&item, self.state.marked_range().is_none())
        {
            cx.propagate();
            return;
        }

        let change = item.text().and_then(|text| self.state.paste(&text));
        self.finish_text_command(TextInputCommand::Paste, change, cx);
    }

    pub(super) fn undo(&mut self, _: &Undo, _: &mut Window, cx: &mut Context<Self>) {
        let change = self.state.undo();
        self.finish_text_command(TextInputCommand::Undo, change, cx);
    }

    pub(super) fn redo(&mut self, _: &Redo, _: &mut Window, cx: &mut Context<Self>) {
        let change = self.state.redo();
        self.finish_text_command(TextInputCommand::Redo, change, cx);
    }

    pub(super) fn on_mouse_down(
        &mut self,
        event: &gpui::MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.enabled {
            return;
        }

        window.focus(&self.focus_handle);
        let offset = self.index_for_mouse_position(event.position);
        if !event.modifiers.shift
            && event.click_count == 1
            && let Some(atom) = self.state.atom_at_offset(offset)
        {
            self.is_selecting = false;
            cx.emit(TextInputEvent::InlineAtomClicked {
                atom_id: atom.id().to_string(),
                position: event.position,
            });
            return;
        }

        let changed = if event.modifiers.shift {
            self.state.select_to_offset(offset)
        } else if event.click_count >= 3 {
            match self.state.mode() {
                TextInputMode::SingleLine => self.state.select_all(),
                TextInputMode::Multiline => self.state.select_line_at_offset(offset),
            }
        } else if event.click_count == 2 {
            self.state.select_word_at_offset(offset)
        } else {
            self.state.move_to_offset(offset)
        };
        self.is_selecting = event.button == MouseButton::Left && event.click_count == 1;
        self.finish_selection_change(changed, cx);
    }

    pub(super) fn on_mouse_up(
        &mut self,
        _: &gpui::MouseUpEvent,
        _: &mut Window,
        _: &mut Context<Self>,
    ) {
        self.is_selecting = false;
    }

    pub(super) fn on_mouse_move(
        &mut self,
        event: &gpui::MouseMoveEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.is_selecting || !event.dragging() {
            return;
        }

        let changed = self
            .state
            .select_to_offset(self.index_for_mouse_position(event.position));
        self.finish_selection_change(changed, cx);
    }

    pub(super) fn on_scroll_wheel(
        &mut self,
        event: &ScrollWheelEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.state.mode() != TextInputMode::Multiline {
            cx.propagate();
            return;
        }
        let Some(bounds) = self.last_bounds else {
            cx.propagate();
            return;
        };
        let delta = event.delta.pixel_delta(window.line_height());
        let max_scroll = layout::max_scroll_y(self.content_height, bounds);
        let next = (self.scroll_y - delta.y).clamp(px(0.0), max_scroll);
        if next == self.scroll_y {
            cx.propagate();
            return;
        }

        self.scroll_y = next;
        self.reveal_cursor = false;
        cx.notify();
    }

    fn finish_text_command(
        &mut self,
        command: TextInputCommand,
        change: Option<TextInputChange>,
        cx: &mut Context<Self>,
    ) {
        if let Some(change) = change {
            self.reveal_cursor = true;
            self.emit_selection_changed(cx);
            cx.emit(TextInputEvent::Changed(change));
            cx.notify();
        }
        cx.emit(TextInputEvent::CommandHandled(command));
    }

    fn finish_selection_command(
        &mut self,
        command: TextInputCommand,
        changed: bool,
        cx: &mut Context<Self>,
    ) {
        self.finish_selection_change(changed, cx);
        cx.emit(TextInputEvent::CommandHandled(command));
    }

    fn move_vertically(&mut self, direction: VerticalDirection, extend_selection: bool) -> bool {
        if self.state.mode() != TextInputMode::Multiline {
            return false;
        }

        if !extend_selection && !self.state.selection().is_empty() {
            let selection = self.state.selection();
            let offset = match direction {
                VerticalDirection::Up => selection.start,
                VerticalDirection::Down => selection.end,
            };
            return self.state.move_to_offset(offset);
        }

        let Some(target_offset) =
            layout::move_visual_line(&self.last_layout, self.state.cursor_offset(), direction)
        else {
            return false;
        };

        if extend_selection {
            self.state.select_to_offset(target_offset)
        } else {
            self.state.move_to_offset(target_offset)
        }
    }

    fn index_for_mouse_position(&self, position: Point<Pixels>) -> usize {
        let Some(bounds) = self.last_bounds else {
            return self.state.text().len();
        };

        layout::index_for_position(&self.last_layout, bounds, position)
    }
}

fn clipboard_item_should_propagate(item: &ClipboardItem, allow_metadata_only: bool) -> bool {
    item.entries()
        .iter()
        .any(|entry| !matches!(entry, ClipboardEntry::String(_)))
        || (allow_metadata_only && item.metadata().is_some())
}
