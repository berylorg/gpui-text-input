use gpui::{
    App, Bounds, ContentMask, CursorStyle, Element, ElementId, ElementInputHandler, Entity,
    GlobalElementId, IntoElement, LayoutId, MouseButton, Pixels, Point, Style, TextAlign, TextRun,
    Window, WrappedLine, div, point, prelude::*, px, relative, size,
};
use gpui_scrollbar::{Axis, ScrollDirection, ScrollbarScrollState, render_scrollbar};

use super::PendingScroll;
use crate::RangeTextInput;
use crate::actions::TEXT_INPUT_KEY_CONTEXT;

impl Render for RangeTextInput {
    fn render(&mut self, _window: &mut Window, cx: &mut gpui::Context<Self>) -> impl IntoElement {
        let input = cx.entity();
        let focus = self.focus_handle.clone();
        let scrollbar = self.render_scrollbar(input.clone());
        div()
            .relative()
            .w_full()
            .min_w(px(0.))
            .h_full()
            .overflow_hidden()
            .when(self.enabled, |element| {
                element
                    .key_context(TEXT_INPUT_KEY_CONTEXT)
                    .track_focus(&focus)
                    .tab_stop(true)
                    .cursor(CursorStyle::IBeam)
            })
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
            .child(RangeTextInputElement { input })
            .when_some(scrollbar, |element, scrollbar| element.child(scrollbar))
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
        let next = match request {
            PendingScroll::Set(offset) => offset,
            PendingScroll::Page(ScrollDirection::Backward, distance) => {
                self.desired.target_block - distance
            }
            PendingScroll::Page(ScrollDirection::Forward, distance) => {
                self.desired.target_block + distance
            }
        }
        .clamp(
            Pixels::ZERO,
            (state.content_size.height - state.viewport_bounds.size.height).max(Pixels::ZERO),
        );
        if next == self.desired.target_block {
            return;
        }
        self.desired.target_block = next;
        self.desired.scroll.intra_anchor = Pixels::ZERO;
        self.desired.preserve_scroll_anchor = false;
        self.desired.reveal_caret = false;
        if self.geometry.index().is_some() {
            let _ = self.start_target();
        }
        self.note_scroll_activity(window, cx);
    }

    fn note_scroll_activity(&self, window: &mut Window, cx: &mut gpui::Context<Self>) {
        self.scrollbar
            .state
            .managed(self.scrollbar.on_visibility_update.clone())
            .record_viewport_activity(self.scrollbar.owner, window, cx);
        cx.notify();
    }

    fn scroll_wheel(
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
        let next = (self.desired.target_block - delta.y).clamp(Pixels::ZERO, max);
        if next == self.desired.target_block {
            cx.propagate();
            return;
        }
        self.desired.target_block = next;
        self.desired.preserve_scroll_anchor = false;
        self.desired.reveal_caret = false;
        if self.geometry.index().is_some() {
            let _ = self.start_target();
        }
        self.note_scroll_activity(window, cx);
    }
}

struct RangeTextInputElement {
    input: Entity<RangeTextInput>,
}

struct RangePrepaint {
    origin: Point<Pixels>,
    selections: Vec<gpui::PaintQuad>,
    caret: Option<gpui::PaintQuad>,
    marked: Vec<gpui::PaintQuad>,
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
        let _ = self
            .input
            .update(cx, |input, cx| input.service_geometry_page(window, cx));
        let input = self.input.read(cx);
        let Some(surface) = input.surface() else {
            return RangePrepaint {
                origin: bounds.origin,
                selections: Vec::new(),
                caret: None,
                marked: Vec::new(),
                placeholder: None,
            };
        };
        let origin = bounds.origin - point(Pixels::ZERO, surface.scroll_block());
        let selections = surface
            .selection_bounds(
                input.config.layout.line_height,
                input.config.layout.wrap_width,
            )
            .into_iter()
            .map(|bounds| {
                gpui::fill(
                    Bounds::new(origin + bounds.origin, bounds.size),
                    input.config.theme.selection,
                )
            })
            .collect();
        let caret = (input.enabled && input.focus_handle.is_focused(window))
            .then(|| surface.caret_bounds(input.config.layout.line_height))
            .flatten()
            .map(|bounds| {
                gpui::fill(
                    Bounds::new(origin + bounds.origin, bounds.size),
                    input.config.theme.caret,
                )
            });
        let marked = surface
            .composition()
            .into_iter()
            .flat_map(|range| {
                surface.bounds_for_range(
                    range,
                    input.config.layout.line_height,
                    input.config.layout.wrap_width,
                )
            })
            .map(|bounds| {
                gpui::fill(
                    Bounds::new(
                        origin
                            + point(
                                bounds.origin.x,
                                bounds.origin.y + bounds.size.height - px(1.),
                            ),
                        size(bounds.size.width, px(1.)),
                    ),
                    input.config.theme.marked_underline,
                )
            })
            .collect();
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
            selections,
            caret,
            marked,
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
            for quad in prepaint.selections.drain(..) {
                window.paint_quad(quad);
            }
            self.input.update(cx, |input, cx| {
                let Some(surface) = input.surface() else {
                    return;
                };
                for fragment in surface.fragments() {
                    let _ = match fragment {
                        gpui::StreamingLayoutFragment::Text(fragment) => {
                            fragment.paint_background(prepaint.origin, window, cx)
                        }
                        gpui::StreamingLayoutFragment::OversizeAtom(fragment) => {
                            fragment.paint_background(prepaint.origin, window)
                        }
                    };
                }
                for fragment in surface.fragments() {
                    let _ = match fragment {
                        gpui::StreamingLayoutFragment::Text(fragment) => {
                            fragment.paint(prepaint.origin, window, cx)
                        }
                        gpui::StreamingLayoutFragment::OversizeAtom(fragment) => {
                            fragment.paint(prepaint.origin, window, cx)
                        }
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
            for quad in prepaint.marked.drain(..) {
                window.paint_quad(quad);
            }
            if let Some(caret) = prepaint.caret.take() {
                window.paint_quad(caret);
            }
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
