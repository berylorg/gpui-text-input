use gpui::{Context, MouseDownEvent, MouseMoveEvent, Pixels, Window};

use crate::{RangeTextInput, RangeTextInputEvent, SegmentationDirection, SegmentationKind};

use super::interaction::PendingBoundaryAction;

impl RangeTextInput {
    pub(super) fn pointer_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.enabled {
            return;
        }
        self.focus(window);
        let Some(surface) = self.interactive_surface() else {
            return;
        };
        let local =
            event.position - self.last_origin() + gpui::point(Pixels::ZERO, surface.scroll_block());
        let Some(offset) = surface.hit_test(local) else {
            return;
        };
        if !event.modifiers.shift && event.click_count == 1 {
            if let Some(atom) = surface.atom_at(offset) {
                cx.emit(RangeTextInputEvent::InlineAtomClicked(atom.id()));
            }
        }
        if !event.modifiers.shift && event.click_count >= 2 {
            self.pointer_anchor = None;
            self.retire_surface_candidate();
            if let Some(continuation) = self.segmentation.take() {
                let pending = *continuation.pending_request();
                let _ = self.residency.cancel(pending);
                self.cancel_page_dispatch(pending);
                self.segmentation_action = None;
            }
            let kind = if event.click_count == 2 {
                SegmentationKind::Word
            } else {
                SegmentationKind::LogicalLine
            };
            let _ = self.begin_boundary_from(
                offset,
                kind,
                SegmentationDirection::Reverse,
                PendingBoundaryAction::SelectPointStart {
                    origin: offset,
                    kind,
                },
                window,
                cx,
            );
            return;
        }
        self.pointer_anchor = Some(if event.modifiers.shift {
            surface.selection().anchor
        } else {
            offset
        });
        self.select_offset(offset, event.modifiers.shift, window, cx);
    }

    pub(super) fn pointer_move(
        &mut self,
        event: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !event.dragging() || self.pointer_anchor.is_none() {
            return;
        }
        let Some(surface) = self.interactive_surface() else {
            return;
        };
        let local =
            event.position - self.last_origin() + gpui::point(Pixels::ZERO, surface.scroll_block());
        if let Some(offset) = surface.hit_test(local) {
            self.select_offset(offset, true, window, cx);
        }
    }

    pub(super) fn last_origin(&self) -> gpui::Point<Pixels> {
        self.last_bounds
            .map_or(gpui::point(Pixels::ZERO, Pixels::ZERO), |bounds| {
                bounds.origin
            })
    }
}
