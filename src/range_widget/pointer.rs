use gpui::{Context, MouseButton, MouseDownEvent, MouseMoveEvent, Pixels, Window};

use crate::{
    InlineObjectInputOrigin, RangeSurfaceHit, RangeTextInput, SegmentationDirection,
    SegmentationKind,
};

use super::interaction::PendingBoundaryAction;

impl RangeTextInput {
    pub(super) fn pointer_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if event.button != MouseButton::Left || !self.enabled {
            return;
        }
        self.focus(window);
        let Some(surface) = self.interactive_surface() else {
            return;
        };
        let local =
            event.position - self.last_origin() + gpui::point(Pixels::ZERO, surface.scroll_block());
        let Some(hit) = surface.hit_test_composite(local) else {
            let viewport_block = event.position.y - self.last_origin().y;
            let _ = self.request_filler_reanchor(viewport_block, cx);
            return;
        };
        let byte_offset = match hit {
            RangeSurfaceHit::Gap(position) => position.byte_offset,
            RangeSurfaceHit::Object(object) => object.leading().byte_offset,
        };
        let prior_anchor = surface.selection().anchor;
        if !event.modifiers.shift
            && event.click_count == 1
            && let RangeSurfaceHit::Gap(position) = hit
            && matches!(position.gap, crate::InlineObjectGap::NoObjects)
            && let Some(atom) = surface.atom_at(position.byte_offset)
        {
            cx.emit(crate::RangeTextInputEvent::InlineAtomClicked(atom.id()));
        }
        if !event.modifiers.shift && event.click_count >= 2 {
            self.retire_surface_candidate();
            if let Some(continuation) = self.segmentation.take() {
                let pending = *continuation.pending_request();
                let _ = self.residency.cancel(pending);
                self.cancel_page_dispatch(pending);
                self.segmentation_action = None;
            }
            if let RangeSurfaceHit::Object(object) = hit {
                let selection = crate::RangeSourceSelection {
                    anchor: object.leading(),
                    head: object.trailing(),
                };
                let _ =
                    self.publish_pointer_source_selection(selection, Some(object), None, None, cx);
                return;
            }
            let RangeSurfaceHit::Gap(position) = hit else {
                return;
            };
            if !matches!(position.gap, crate::InlineObjectGap::NoObjects) {
                return;
            }
            let kind = if event.click_count == 2 {
                SegmentationKind::Word
            } else {
                SegmentationKind::LogicalLine
            };
            let _ = self.begin_boundary_from(
                byte_offset,
                kind,
                SegmentationDirection::Reverse,
                PendingBoundaryAction::SelectPointStart {
                    origin: byte_offset,
                    kind,
                },
                window,
                cx,
            );
            return;
        }
        let pointer_anchor = Some(if event.modifiers.shift {
            prior_anchor
        } else {
            match hit {
                RangeSurfaceHit::Gap(position) => position,
                RangeSurfaceHit::Object(object) => object.leading(),
            }
        });
        match hit {
            RangeSurfaceHit::Gap(position) => {
                let selection = if event.modifiers.shift {
                    crate::RangeSourceSelection {
                        anchor: prior_anchor,
                        head: position,
                    }
                } else {
                    crate::RangeSourceSelection::caret(position)
                };
                let selected_object = surface.object_selected_by(selection);
                let _ = self.publish_pointer_source_selection(
                    selection,
                    selected_object,
                    None,
                    pointer_anchor,
                    cx,
                );
            }
            RangeSurfaceHit::Object(object) => {
                let selection = if event.modifiers.shift {
                    let anchor = prior_anchor;
                    let head = if anchor
                        .compare_in_revision(object.leading())
                        .is_some_and(|ordering| ordering.is_le())
                    {
                        object.trailing()
                    } else {
                        object.leading()
                    };
                    crate::RangeSourceSelection { anchor, head }
                } else {
                    crate::RangeSourceSelection {
                        anchor: object.leading(),
                        head: object.trailing(),
                    }
                };
                let selected_object = selection
                    .range()
                    .ok()
                    .filter(|range| {
                        range.start() == object.leading() && range.end() == object.trailing()
                    })
                    .map(|_| object);
                let activation = (!event.modifiers.shift)
                    .then_some(InlineObjectInputOrigin::Pointer { point: local });
                let _ = self.publish_pointer_source_selection(
                    selection,
                    selected_object,
                    activation,
                    pointer_anchor,
                    cx,
                );
            }
        }
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
        if let Some(hit) = surface.hit_test_composite(local) {
            let position = match hit {
                RangeSurfaceHit::Gap(position) => position,
                RangeSurfaceHit::Object(object) => object.trailing(),
            };
            self.select_source_position(position, true, window, cx);
        }
    }

    pub(super) fn last_origin(&self) -> gpui::Point<Pixels> {
        self.last_bounds
            .map_or(gpui::point(Pixels::ZERO, Pixels::ZERO), |bounds| {
                bounds.origin
            })
    }
}
