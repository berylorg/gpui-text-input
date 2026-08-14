use gpui::{Context, Window};

use crate::actions::{
    Backspace, Copy, Cut, Delete, DeleteWordBackward, DeleteWordForward, Enter, InsertNewline,
    MoveDown, MoveEnd, MoveHome, MoveLeft, MoveRight, MoveToEnd, MoveToStart, MoveUp, MoveWordLeft,
    MoveWordRight, Paste, Redo, SelectAll, SelectDown, SelectEnd, SelectHome, SelectLeft,
    SelectRight, SelectToEnd, SelectToStart, SelectUp, SelectWordLeft, SelectWordRight, Undo,
};
use crate::{
    ByteOffset, ClipboardKind, MutationKind, RangeSelection, RangeTextInput, SegmentationDirection,
    SegmentationKind,
};

impl RangeTextInput {
    fn move_boundary(
        &mut self,
        kind: SegmentationKind,
        direction: SegmentationDirection,
        extend: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let _ = self.begin_boundary(
            kind,
            direction,
            super::interaction::PendingBoundaryAction::Move { extend },
            window,
            cx,
        );
    }

    fn delete_boundary(
        &mut self,
        kind: SegmentationKind,
        direction: SegmentationDirection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.enabled || self.read_only {
            return;
        }
        let Some(surface) = self.interactive_surface() else {
            return;
        };
        if !surface.selection().range().is_empty() {
            let _ = self.begin_replacement(
                surface.selection().range(),
                String::new(),
                MutationKind::Edit,
                cx,
            );
            return;
        }
        let _ = self.begin_boundary(
            kind,
            direction,
            super::interaction::PendingBoundaryAction::Delete,
            window,
            cx,
        );
    }

    pub(super) fn backspace(&mut self, _: &Backspace, window: &mut Window, cx: &mut Context<Self>) {
        self.delete_boundary(
            SegmentationKind::Grapheme,
            SegmentationDirection::Reverse,
            window,
            cx,
        );
    }

    pub(super) fn delete(&mut self, _: &Delete, window: &mut Window, cx: &mut Context<Self>) {
        self.delete_boundary(
            SegmentationKind::Grapheme,
            SegmentationDirection::Forward,
            window,
            cx,
        );
    }

    pub(super) fn delete_word_backward(
        &mut self,
        _: &DeleteWordBackward,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.delete_boundary(
            SegmentationKind::Word,
            SegmentationDirection::Reverse,
            window,
            cx,
        );
    }

    pub(super) fn delete_word_forward(
        &mut self,
        _: &DeleteWordForward,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.delete_boundary(
            SegmentationKind::Word,
            SegmentationDirection::Forward,
            window,
            cx,
        );
    }

    pub(super) fn move_left(&mut self, _: &MoveLeft, window: &mut Window, cx: &mut Context<Self>) {
        self.move_boundary(
            SegmentationKind::Grapheme,
            SegmentationDirection::Reverse,
            false,
            window,
            cx,
        );
    }
    pub(super) fn move_right(
        &mut self,
        _: &MoveRight,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.move_boundary(
            SegmentationKind::Grapheme,
            SegmentationDirection::Forward,
            false,
            window,
            cx,
        );
    }
    pub(super) fn select_left(
        &mut self,
        _: &SelectLeft,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.move_boundary(
            SegmentationKind::Grapheme,
            SegmentationDirection::Reverse,
            true,
            window,
            cx,
        );
    }
    pub(super) fn select_right(
        &mut self,
        _: &SelectRight,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.move_boundary(
            SegmentationKind::Grapheme,
            SegmentationDirection::Forward,
            true,
            window,
            cx,
        );
    }
    pub(super) fn move_word_left(
        &mut self,
        _: &MoveWordLeft,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.move_boundary(
            SegmentationKind::Word,
            SegmentationDirection::Reverse,
            false,
            window,
            cx,
        );
    }
    pub(super) fn move_word_right(
        &mut self,
        _: &MoveWordRight,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.move_boundary(
            SegmentationKind::Word,
            SegmentationDirection::Forward,
            false,
            window,
            cx,
        );
    }
    pub(super) fn select_word_left(
        &mut self,
        _: &SelectWordLeft,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.move_boundary(
            SegmentationKind::Word,
            SegmentationDirection::Reverse,
            true,
            window,
            cx,
        );
    }
    pub(super) fn select_word_right(
        &mut self,
        _: &SelectWordRight,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.move_boundary(
            SegmentationKind::Word,
            SegmentationDirection::Forward,
            true,
            window,
            cx,
        );
    }
    pub(super) fn move_home(&mut self, _: &MoveHome, window: &mut Window, cx: &mut Context<Self>) {
        self.move_boundary(
            SegmentationKind::LogicalLine,
            SegmentationDirection::Reverse,
            false,
            window,
            cx,
        );
    }
    pub(super) fn move_end(&mut self, _: &MoveEnd, window: &mut Window, cx: &mut Context<Self>) {
        self.move_boundary(
            SegmentationKind::LogicalLine,
            SegmentationDirection::Forward,
            false,
            window,
            cx,
        );
    }
    pub(super) fn select_home(
        &mut self,
        _: &SelectHome,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.move_boundary(
            SegmentationKind::LogicalLine,
            SegmentationDirection::Reverse,
            true,
            window,
            cx,
        );
    }
    pub(super) fn select_end(
        &mut self,
        _: &SelectEnd,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.move_boundary(
            SegmentationKind::LogicalLine,
            SegmentationDirection::Forward,
            true,
            window,
            cx,
        );
    }

    fn select_document_offset(&mut self, offset: ByteOffset, extend: bool, cx: &mut Context<Self>) {
        if !self.enabled {
            return;
        }
        let Some(surface) = self.interactive_surface() else {
            return;
        };
        self.desired.selection = if extend {
            RangeSelection {
                anchor: surface.selection().anchor,
                head: offset,
            }
        } else {
            RangeSelection::caret(offset)
        };
        self.desired.composition = None;
        self.desired.reveal_caret = true;
        let _ = self.start_target();
        cx.notify();
    }

    pub(super) fn move_to_start(
        &mut self,
        _: &MoveToStart,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_document_offset(ByteOffset::new(0), false, cx);
    }
    pub(super) fn move_to_end(&mut self, _: &MoveToEnd, _: &mut Window, cx: &mut Context<Self>) {
        self.select_document_offset(
            ByteOffset::new(self.config.binding.extent().byte_len()),
            false,
            cx,
        );
    }
    pub(super) fn select_to_start(
        &mut self,
        _: &SelectToStart,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_document_offset(ByteOffset::new(0), true, cx);
    }
    pub(super) fn select_to_end(
        &mut self,
        _: &SelectToEnd,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_document_offset(
            ByteOffset::new(self.config.binding.extent().byte_len()),
            true,
            cx,
        );
    }
    pub(super) fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        if !self.enabled {
            return;
        }
        self.desired.selection = RangeSelection {
            anchor: ByteOffset::new(0),
            head: ByteOffset::new(self.config.binding.extent().byte_len()),
        };
        self.desired.composition = None;
        self.desired.reveal_caret = true;
        let _ = self.start_target();
        cx.notify();
    }

    fn move_vertical(&mut self, delta: i8, extend: bool, cx: &mut Context<Self>) {
        if !self.enabled {
            return;
        }
        let Some(surface) = self.interactive_surface() else {
            return;
        };
        let Some(mut point) = surface.position_for_offset(surface.caret()) else {
            return;
        };
        point.y += self.config.layout.line_height * f32::from(delta);
        if let Some(offset) = surface.hit_test(point) {
            self.desired.selection = if extend {
                RangeSelection {
                    anchor: surface.selection().anchor,
                    head: offset,
                }
            } else {
                RangeSelection::caret(offset)
            };
            self.desired.composition = None;
            self.desired.reveal_caret = true;
            let _ = self.start_target();
            cx.notify();
        }
    }
    pub(super) fn move_up(&mut self, _: &MoveUp, _: &mut Window, cx: &mut Context<Self>) {
        self.move_vertical(-1, false, cx);
    }
    pub(super) fn move_down(&mut self, _: &MoveDown, _: &mut Window, cx: &mut Context<Self>) {
        self.move_vertical(1, false, cx);
    }
    pub(super) fn select_up(&mut self, _: &SelectUp, _: &mut Window, cx: &mut Context<Self>) {
        self.move_vertical(-1, true, cx);
    }
    pub(super) fn select_down(&mut self, _: &SelectDown, _: &mut Window, cx: &mut Context<Self>) {
        self.move_vertical(1, true, cx);
    }

    pub(super) fn enter(&mut self, _: &Enter, _: &mut Window, cx: &mut Context<Self>) {
        let _ = self.insert_text("\n".to_owned(), cx);
    }
    pub(super) fn insert_newline(
        &mut self,
        _: &InsertNewline,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let _ = self.insert_text("\n".to_owned(), cx);
    }
    pub(super) fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        let _ = self.begin_clipboard(ClipboardKind::Copy, cx);
    }
    pub(super) fn cut(&mut self, _: &Cut, _: &mut Window, cx: &mut Context<Self>) {
        let _ = self.begin_clipboard(ClipboardKind::Cut, cx);
    }
    pub(super) fn paste(&mut self, _: &Paste, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            let _ = self.insert_text(text, cx);
        }
    }
    pub(super) fn undo(&mut self, _: &Undo, _: &mut Window, cx: &mut Context<Self>) {
        self.request_history(MutationKind::Undo, cx);
    }
    pub(super) fn redo(&mut self, _: &Redo, _: &mut Window, cx: &mut Context<Self>) {
        self.request_history(MutationKind::Redo, cx);
    }
}
