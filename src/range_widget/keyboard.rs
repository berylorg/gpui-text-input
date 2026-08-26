use gpui::{Context, Window};

use crate::actions::{
    Backspace, Copy, Cut, Delete, DeleteWordBackward, DeleteWordForward, Enter, InsertNewline,
    MoveDown, MoveEnd, MoveHome, MoveLeft, MoveRight, MoveToEnd, MoveToStart, MoveUp, MoveWordLeft,
    MoveWordRight, Paste, Redo, SelectAll, SelectDown, SelectEnd, SelectHome, SelectLeft,
    SelectRight, SelectToEnd, SelectToStart, SelectUp, SelectWordLeft, SelectWordRight, Space,
    Undo,
};
use crate::{
    ByteOffset, ClipboardKind, InlineObjectActivationKey, MutationKind, RangeSourceSelection,
    RangeTextInput, SegmentationDirection, SegmentationKind,
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
        let Some(surface) = self.interactive_surface() else {
            return;
        };
        let selection = surface.selection();
        if !extend && !selection.range().is_ok_and(|range| range.is_empty()) {
            let position = match direction {
                SegmentationDirection::Forward => selection.range().ok().map(|range| range.end()),
                SegmentationDirection::Reverse => selection.range().ok().map(|range| range.start()),
            };
            if let Some(position) = position {
                let _ = self.publish_source_selection(
                    RangeSourceSelection::caret(position),
                    None,
                    None,
                    cx,
                );
                return;
            }
        }
        if let Some(object) = surface.adjacent_object(selection.head, direction) {
            let head = match direction {
                SegmentationDirection::Forward => object.trailing(),
                SegmentationDirection::Reverse => object.leading(),
            };
            let anchor = if extend {
                selection.anchor
            } else {
                match direction {
                    SegmentationDirection::Forward => object.leading(),
                    SegmentationDirection::Reverse => object.trailing(),
                }
            };
            let next = RangeSourceSelection { anchor, head };
            let activates_object = next.range().ok().is_some_and(|range| {
                range.start() == object.leading() && range.end() == object.trailing()
            });
            let _ =
                self.publish_source_selection(next, activates_object.then_some(object), None, cx);
            return;
        }
        let _ = self.begin_boundary(
            kind,
            direction,
            super::interaction::PendingBoundaryAction::Move { extend, direction },
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
        if !surface
            .selection()
            .range()
            .is_ok_and(|range| range.is_empty())
        {
            let _ = self.begin_source_replacement(
                surface.selection().range().expect("coherent selection"),
                String::new(),
                MutationKind::Edit,
                cx,
            );
            return;
        }
        if let Some(object) = surface.adjacent_object(surface.caret(), direction) {
            let range = crate::SourceRange::new(object.leading(), object.trailing())
                .expect("realized object has ordered edges");
            let _ = self.begin_source_replacement(range, String::new(), MutationKind::Edit, cx);
            return;
        }
        let _ = self.begin_boundary(
            kind,
            direction,
            super::interaction::PendingBoundaryAction::Delete { direction },
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

    fn select_document_offset(
        &mut self,
        offset: ByteOffset,
        direction: SegmentationDirection,
        extend: bool,
        cx: &mut Context<Self>,
    ) {
        if !self.enabled {
            return;
        }
        let Some(surface) = self.interactive_surface() else {
            return;
        };
        let position = surface
            .source_position_for_byte(offset, direction)
            .or_else(|| {
                let endpoints = self.geometry.index()?.document_selection();
                match direction {
                    SegmentationDirection::Forward => Some(endpoints.anchor),
                    SegmentationDirection::Reverse => Some(endpoints.head),
                }
                .filter(|position| position.byte_offset == offset)
            });
        let Some(position) = position else {
            return;
        };
        let selection = if extend {
            RangeSourceSelection {
                anchor: surface.selection().anchor,
                head: position,
            }
        } else {
            RangeSourceSelection::caret(position)
        };
        let selected_object = surface.object_selected_by(selection);
        let _ = self.publish_source_selection(selection, selected_object, None, cx);
    }

    pub(super) fn move_to_start(
        &mut self,
        _: &MoveToStart,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_document_offset(
            ByteOffset::new(0),
            SegmentationDirection::Forward,
            false,
            cx,
        );
    }
    pub(super) fn move_to_end(&mut self, _: &MoveToEnd, _: &mut Window, cx: &mut Context<Self>) {
        self.select_document_offset(
            ByteOffset::new(self.config.binding.extent().byte_len()),
            SegmentationDirection::Reverse,
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
        self.select_document_offset(ByteOffset::new(0), SegmentationDirection::Forward, true, cx);
    }
    pub(super) fn select_to_end(
        &mut self,
        _: &SelectToEnd,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_document_offset(
            ByteOffset::new(self.config.binding.extent().byte_len()),
            SegmentationDirection::Reverse,
            true,
            cx,
        );
    }
    pub(super) fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        if !self.enabled {
            return;
        }
        if self.reject_restoration_task(cx).is_err() {
            return;
        }
        let extent = self.config.binding.extent().byte_len();
        let surface_selection = self.interactive_surface().and_then(|surface| {
            Some(RangeSourceSelection {
                anchor: surface
                    .source_position_for_byte(ByteOffset::new(0), SegmentationDirection::Forward)?,
                head: surface.source_position_for_byte(
                    ByteOffset::new(extent),
                    SegmentationDirection::Reverse,
                )?,
            })
        });
        let selection = surface_selection.or_else(|| {
            self.geometry
                .index()
                .map(|index| index.document_selection())
        });
        if let Some(selection) = selection {
            let selected_object = self
                .interactive_surface()
                .and_then(|surface| surface.object_selected_by(selection));
            let _ = self.publish_source_selection(selection, selected_object, None, cx);
            return;
        }

        let was_pending = self.pending_select_all;
        self.pending_select_all = true;
        if self.active_geometry.is_none() {
            let started = self.start_index();
            if started.is_err() {
                self.pending_select_all = was_pending;
            } else {
                cx.notify();
            }
        }
    }

    fn move_vertical(&mut self, delta: i8, extend: bool, cx: &mut Context<Self>) {
        if !self.enabled {
            return;
        }
        let Some(surface) = self.interactive_surface() else {
            return;
        };
        let Some(mut point) = surface
            .caret_bounds(self.config.layout.line_height)
            .map(|bounds| bounds.origin)
        else {
            let filler = if delta.is_positive() {
                surface.fillers().last()
            } else {
                surface.fillers().next()
            };
            if let Some(filler) = filler {
                let block = filler.block_start() - surface.scroll_block();
                let _ = self.request_filler_reanchor(block, cx);
            }
            return;
        };
        point.y += self.config.layout.line_height * f32::from(delta);
        let crossed_filler = surface.fillers().find(|filler| {
            point.y + self.config.layout.line_height >= filler.block_start()
                && point.y < filler.block_end()
        });
        if let Some(filler) = crossed_filler {
            let block = filler.block_start() - surface.scroll_block();
            let _ = self.request_filler_reanchor(block, cx);
            return;
        }
        if let Some(crate::RangeSurfaceHit::Gap(position)) = surface.hit_test_composite(point) {
            let selection = if extend {
                RangeSourceSelection {
                    anchor: surface.selection().anchor,
                    head: position,
                }
            } else {
                RangeSourceSelection::caret(position)
            };
            let selected_object = surface.object_selected_by(selection);
            let _ = self.publish_source_selection(selection, selected_object, None, cx);
        } else if let Some(filler) = if delta.is_positive() {
            surface.fillers().last()
        } else {
            surface.fillers().next()
        } {
            let block = filler.block_start() - surface.scroll_block();
            let _ = self.request_filler_reanchor(block, cx);
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
        if self
            .activate_current_object(InlineObjectActivationKey::Enter, cx)
            .consumes_key()
        {
            return;
        }
        if self.config.enter_key == crate::TextInputEnterKey::Propagate {
            cx.emit(crate::RangeTextInputEvent::CommandPropagated(
                crate::TextInputCommand::Enter,
            ));
            return;
        }
        let _ = self.insert_text("\n".to_owned(), cx);
    }
    pub(super) fn space(&mut self, _: &Space, _: &mut Window, cx: &mut Context<Self>) {
        if !self
            .activate_current_object(InlineObjectActivationKey::Space, cx)
            .consumes_key()
        {
            let _ = self.insert_text(" ".to_owned(), cx);
        }
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
        if self.config.rich_paste_policy == crate::TextInputRichPastePolicy::Propagate {
            cx.emit(crate::RangeTextInputEvent::CommandPropagated(
                crate::TextInputCommand::Paste,
            ));
            return;
        }
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
