use gpui::{
    App, Bounds, ContentMask, CursorStyle, Element, ElementId, ElementInputHandler, Entity,
    GlobalElementId, IntoElement, LayoutId, MouseButton, Pixels, Point, Style, TextAlign, TextRun,
    Window, WrappedLine, div, point, prelude::*, px, relative, size,
};
use gpui_scrollbar::{Axis, ScrollDirection, ScrollbarScrollState, render_scrollbar};

use super::PendingScroll;
use crate::actions::TEXT_INPUT_KEY_CONTEXT;
use crate::{RangeTextInput, RangeTextInputError};

impl Render for RangeTextInput {
    fn render(&mut self, _window: &mut Window, cx: &mut gpui::Context<Self>) -> impl IntoElement {
        let input = cx.entity();
        let focus = self.enabled.then(|| self.focus_handle.clone());
        div()
            .relative()
            .w_full()
            .min_w(px(0.))
            .h_full()
            .overflow_hidden()
            .when(self.enabled, |element| {
                let focus = focus.as_ref().expect("enabled input has a focus handle");
                element
                    .key_context(TEXT_INPUT_KEY_CONTEXT)
                    .track_focus(focus)
                    .tab_stop(true)
                    .cursor(CursorStyle::IBeam)
                    .on_action(cx.listener(Self::backspace))
                    .on_action(cx.listener(Self::delete))
                    .on_action(cx.listener(Self::delete_word_backward))
                    .on_action(cx.listener(Self::delete_word_forward))
                    .on_action(cx.listener(Self::move_left))
                    .on_action(cx.listener(Self::move_right))
                    .on_action(cx.listener(Self::move_up))
                    .on_action(cx.listener(Self::move_down))
                    .on_action(cx.listener(Self::move_word_left))
                    .on_action(cx.listener(Self::move_word_right))
                    .on_action(cx.listener(Self::select_left))
                    .on_action(cx.listener(Self::select_right))
                    .on_action(cx.listener(Self::select_up))
                    .on_action(cx.listener(Self::select_down))
                    .on_action(cx.listener(Self::select_word_left))
                    .on_action(cx.listener(Self::select_word_right))
                    .on_action(cx.listener(Self::move_home))
                    .on_action(cx.listener(Self::move_end))
                    .on_action(cx.listener(Self::select_home))
                    .on_action(cx.listener(Self::select_end))
                    .on_action(cx.listener(Self::move_to_start))
                    .on_action(cx.listener(Self::move_to_end))
                    .on_action(cx.listener(Self::select_to_start))
                    .on_action(cx.listener(Self::select_to_end))
                    .on_action(cx.listener(Self::select_all))
                    .on_action(cx.listener(Self::enter))
                    .on_action(cx.listener(Self::space))
                    .on_action(cx.listener(Self::insert_newline))
                    .on_action(cx.listener(Self::copy))
                    .on_action(cx.listener(Self::cut))
                    .on_action(cx.listener(Self::paste))
                    .on_action(cx.listener(Self::undo))
                    .on_action(cx.listener(Self::redo))
                    .on_mouse_down(MouseButton::Left, cx.listener(Self::pointer_down))
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|input, _, _, _| input.pointer_anchor = None),
                    )
                    .on_mouse_up_out(
                        MouseButton::Left,
                        cx.listener(|input, _, _, _| input.pointer_anchor = None),
                    )
                    .on_mouse_move(cx.listener(Self::pointer_move))
                    .on_scroll_wheel(cx.listener(Self::scroll_wheel))
            })
            .child(RangeTextInputElement {
                input: input.clone(),
            })
            .when(self.enabled, |element| {
                element.when_some(self.render_scrollbar(input), |element, scrollbar| {
                    element.child(scrollbar)
                })
            })
    }
}

impl RangeTextInput {
    fn render_scrollbar(&self, input: Entity<Self>) -> Option<gpui::AnyElement> {
        let state = self.scrollbar_state()?;
        self.scrollbar.model.set(Some(state));
        render_scrollbar(
            ("gpui-range-text-input-scrollbar", input.entity_id()),
            self.scrollbar.state.clone(),
            Axis::Vertical,
            self.config.scrollbar_style,
            self.scrollbar
                .state
                .managed(self.scrollbar.on_visibility_update.clone()),
            self.scrollbar.interaction.clone(),
        )
    }

    fn scrollbar_state(&self) -> Option<ScrollbarScrollState> {
        let surface = self.interactive_surface()?;
        let bounds = self.last_bounds?;
        let max = (surface.content_height() - bounds.size.height).max(Pixels::ZERO);
        (max > Pixels::ZERO).then_some(ScrollbarScrollState {
            owner: self.scrollbar.owner,
            viewport_bounds: bounds,
            content_size: bounds.size + size(Pixels::ZERO, max),
            scroll_offset: point(
                Pixels::ZERO,
                surface.scroll_block().clamp(Pixels::ZERO, max),
            ),
            page_distance: bounds.size,
        })
    }

    pub(super) fn apply_scrollbar(
        &mut self,
        request: PendingScroll,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        if !self.enabled {
            return;
        }
        let Some(state) = self.scrollbar_state() else {
            return;
        };
        let current = self.target_intent_desired();
        let next = match request {
            PendingScroll::Set(offset) => offset,
            PendingScroll::Page(ScrollDirection::Backward, distance) => {
                current.target_block - distance
            }
            PendingScroll::Page(ScrollDirection::Forward, distance) => {
                current.target_block + distance
            }
        }
        .clamp(
            Pixels::ZERO,
            (state.content_size.height - state.viewport_bounds.size.height).max(Pixels::ZERO),
        );
        if next == current.target_block {
            return;
        }
        let mut desired = current;
        desired.target_block = next;
        desired.realization_anchor_block = next;
        desired.scroll.intra_anchor = Pixels::ZERO;
        desired.preserve_scroll_anchor = false;
        desired.reveal_caret = false;
        let Ok(_) = self.request_target_intent(
            super::realization::PendingTargetIntent::ordinary(desired),
            cx,
        ) else {
            return;
        };
        self.note_scroll_activity(window, cx);
    }

    fn note_scroll_activity(&self, window: &mut Window, cx: &mut gpui::Context<Self>) {
        self.scrollbar
            .state
            .managed(self.scrollbar.on_visibility_update.clone())
            .record_viewport_activity(self.scrollbar.owner, window, cx);
        cx.notify();
    }

    pub(super) fn scroll_wheel(
        &mut self,
        event: &gpui::ScrollWheelEvent,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        if !self.enabled {
            cx.propagate();
            return;
        }
        let Some(state) = self.scrollbar_state() else {
            cx.propagate();
            return;
        };
        let delta = event.delta.pixel_delta(window.line_height());
        let max = (state.content_size.height - state.viewport_bounds.size.height).max(Pixels::ZERO);
        let current = self.target_intent_desired();
        let next = (current.target_block - delta.y).clamp(Pixels::ZERO, max);
        if next == current.target_block {
            cx.propagate();
            return;
        }
        let mut desired = current;
        desired.target_block = next;
        desired.realization_anchor_block = next;
        desired.preserve_scroll_anchor = false;
        desired.reveal_caret = false;
        let Ok(_) = self.request_target_intent(
            super::realization::PendingTargetIntent::ordinary(desired),
            cx,
        ) else {
            return;
        };
        self.note_scroll_activity(window, cx);
    }
}

struct RangeTextInputElement {
    input: Entity<RangeTextInput>,
}

struct RangePrepaint {
    origin: Point<Pixels>,
    placeholder: Option<WrappedLine>,
}

impl IntoElement for RangeTextInputElement {
    type Element = Self;
    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for RangeTextInputElement {
    type RequestLayoutState = ();
    type PrepaintState = RangePrepaint;
    fn id(&self) -> Option<ElementId> {
        None
    }
    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }
    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = relative(1.).into();
        (window.request_layout(style, [], cx), ())
    }
    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> RangePrepaint {
        self.input.update(cx, |input, cx| {
            if !input.mounted {
                return;
            }
            input.begin_realization_frame();
            let _ = input.service_response_custody(window, cx);
            let _ = input.service_pending_configuration_intent(cx);
            let _ = input.service_pending_rebind_intent(window, cx);
            if bounds.size.height > Pixels::ZERO && f32::from(bounds.size.height).is_finite() {
                match input.set_realization_viewport_extent(bounds.size.height, cx) {
                    Ok(())
                    | Err(RangeTextInputError::Busy)
                    | Err(RangeTextInputError::SurfaceCapacity) => {}
                    Err(error) => {
                        debug_assert!(false, "finite widget bounds rejected: {error}");
                    }
                }
            }
            let _ = input.service_pending_target_intent(cx);
            let _ = input.service_admitted_geometry_for_prepaint(window, cx);
        });
        let input = self.input.read(cx);
        let Some(surface) = input.surface() else {
            return RangePrepaint {
                origin: bounds.origin,
                placeholder: None,
            };
        };
        let origin = bounds.origin - point(Pixels::ZERO, surface.scroll_block());
        let placeholder = surface.placeholder().and_then(|placeholder| {
            let text_style = window.text_style();
            let run = TextRun {
                len: placeholder.len(),
                font: text_style.font(),
                color: input.config.theme.placeholder,
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            window
                .text_system()
                .shape_text(
                    placeholder.clone(),
                    input.config.layout.font_size,
                    &[run],
                    Some(input.config.layout.wrap_width),
                    None,
                )
                .ok()
                .and_then(|mut lines| lines.pop())
        });
        RangePrepaint {
            origin,
            placeholder,
        }
    }
    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        prepaint: &mut RangePrepaint,
        window: &mut Window,
        cx: &mut App,
    ) {
        let (focus, enabled) = {
            let input = self.input.read(cx);
            (input.focus_handle.clone(), input.enabled)
        };
        if enabled {
            window.handle_input(
                &focus,
                ElementInputHandler::new(bounds, self.input.clone()),
                cx,
            );
        }
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            self.input.update(cx, |input, cx| {
                let Some(surface) = input.surface() else {
                    return;
                };
                for selection in surface.selection_bounds() {
                    window.paint_quad(gpui::fill(
                        Bounds::new(prepaint.origin + selection.origin, selection.size),
                        input.config.theme.selection,
                    ));
                }
                for fragment in surface.fragments() {
                    let _ = match fragment {
                        gpui::StreamingLayoutFragment::Text(fragment) => {
                            fragment.paint_background(prepaint.origin, window, cx)
                        }
                        gpui::StreamingLayoutFragment::OversizeAtom(fragment) => {
                            fragment.paint_background(prepaint.origin, window)
                        }
                        gpui::StreamingLayoutFragment::InlineObject(fragment) => {
                            fragment.paint_background(prepaint.origin, window)
                        }
                        gpui::StreamingLayoutFragment::Boundary(_) => Ok(()),
                    };
                }
                if let Some(active) = input.active_object {
                    if active.anchor.binding == surface.binding()
                        && active.anchor.presentation_generation
                            == surface.geometry_key().presentation_generation()
                        && active.anchor.layout_epoch == surface.geometry_key().epoch()
                    {
                        window.paint_quad(gpui::fill(
                            Bounds::new(
                                prepaint.origin + active.anchor.bounds.origin,
                                active.anchor.bounds.size,
                            ),
                            input.config.theme.selection,
                        ));
                    }
                }
                for fragment in surface.fragments() {
                    let _ = match fragment {
                        gpui::StreamingLayoutFragment::Text(fragment) => {
                            fragment.paint(prepaint.origin, window, cx)
                        }
                        gpui::StreamingLayoutFragment::OversizeAtom(fragment) => {
                            fragment.paint(prepaint.origin, window, cx)
                        }
                        gpui::StreamingLayoutFragment::InlineObject(fragment) => {
                            fragment.paint(prepaint.origin, window, cx)
                        }
                        gpui::StreamingLayoutFragment::Boundary(_) => Ok(()),
                    };
                }
            });
            if let Some(placeholder) = prepaint.placeholder.take() {
                let _ = placeholder.paint(
                    prepaint.origin,
                    self.input.read(cx).config.layout.line_height,
                    TextAlign::default(),
                    Some(bounds),
                    window,
                    cx,
                );
            }
            self.input.update(cx, |input, _| {
                let Some(surface) = input.surface() else {
                    return;
                };
                for marked in surface.composition_bounds() {
                    window.paint_quad(gpui::fill(
                        Bounds::new(
                            prepaint.origin
                                + point(
                                    marked.origin.x,
                                    marked.origin.y + marked.size.height - px(1.),
                                ),
                            size(marked.size.width, px(1.)),
                        ),
                        input.config.theme.marked_underline,
                    ));
                }
                if input.enabled && input.focus_handle.is_focused(window) {
                    if let Some(caret) = surface.caret_bounds(input.config.layout.line_height) {
                        window.paint_quad(gpui::fill(
                            Bounds::new(prepaint.origin + caret.origin, caret.size),
                            input.config.theme.caret,
                        ));
                    }
                }
            });
        });
        self.input.update(cx, |input, cx| {
            let width_changed = bounds.size.width > Pixels::ZERO
                && bounds.size.width != input.config.layout.wrap_width;
            input.last_bounds = Some(bounds);
            if width_changed {
                let mut layout = input.config.layout.clone();
                layout.wrap_width = bounds.size.width;
                let style = input.config.style.clone();
                let _ = input.set_layout(layout, style, cx);
            }
        });
    }
}
