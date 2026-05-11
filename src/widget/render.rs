use gpui::{
    App, ContentMask, CursorStyle, Element, ElementId, ElementInputHandler, Entity,
    GlobalElementId, IntoElement, LayoutId, MouseButton, Style, Window, div, prelude::*, relative,
};

use crate::actions::TEXT_INPUT_KEY_CONTEXT;

use super::{layout, *};

impl Render for TextInput {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mode = self.state.mode();
        let input = cx.entity();
        let focus_handle = self.focus_handle.clone();

        div()
            .w_full()
            .min_w(px(0.0))
            .when(mode == TextInputMode::SingleLine, |this| {
                this.h(window.line_height())
            })
            .when(mode == TextInputMode::Multiline, |this| this.h_full())
            .overflow_hidden()
            .when(self.enabled, |this| {
                this.key_context(TEXT_INPUT_KEY_CONTEXT)
                    .track_focus(&focus_handle)
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
            .on_action(cx.listener(Self::insert_newline_action))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::undo))
            .on_action(cx.listener(Self::redo))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_scroll_wheel(cx.listener(Self::on_scroll_wheel))
            .child(TextInputElement { input })
    }
}

struct TextInputElement {
    input: Entity<TextInput>,
}

struct PrepaintState {
    lines: Vec<layout::InputLineLayout>,
    cursor: Option<gpui::PaintQuad>,
    selection: Vec<gpui::PaintQuad>,
    scroll_x: Pixels,
    scroll_y: Pixels,
    content_height: Pixels,
    visible_range: Range<usize>,
}

impl IntoElement for TextInputElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for TextInputElement {
    type RequestLayoutState = ();
    type PrepaintState = PrepaintState;

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
    ) -> (LayoutId, Self::RequestLayoutState) {
        let multiline = self.input.read(cx).state().mode() == TextInputMode::Multiline;
        let mut style = Style::default();
        style.size.width = relative(1.0).into();
        style.size.height = if multiline {
            relative(1.0).into()
        } else {
            window.line_height().into()
        };
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let input = self.input.read(cx);
        let layout = layout::build_input_layout(
            input.state(),
            &input.placeholder,
            &input.theme,
            bounds,
            input.scroll_x,
            input.scroll_y,
            input.reveal_cursor,
            input.enabled && input.focus_handle.is_focused(window),
            window,
        );
        PrepaintState {
            lines: layout.lines,
            cursor: layout.cursor,
            selection: layout.selection,
            scroll_x: layout.scroll_x,
            scroll_y: layout.scroll_y,
            content_height: layout.content_height,
            visible_range: layout.visible_range,
        }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let (focus_handle, enabled) = {
            let input = self.input.read(cx);
            (input.focus_handle.clone(), input.enabled)
        };
        if enabled {
            window.handle_input(
                &focus_handle,
                ElementInputHandler::new(bounds, self.input.clone()),
                cx,
            );
        }

        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            for selection in prepaint.selection.drain(..) {
                window.paint_quad(selection);
            }
            layout::paint_lines(&prepaint.lines, window, cx);
            if let Some(cursor) = prepaint.cursor.take() {
                window.paint_quad(cursor);
            }
        });

        let lines = std::mem::take(&mut prepaint.lines);
        self.input.update(cx, |input, _cx| {
            input.last_layout = lines;
            input.last_bounds = Some(bounds);
            input.scroll_x = prepaint.scroll_x;
            input.scroll_y = prepaint.scroll_y;
            input.content_height = prepaint.content_height;
            input.visible_range = prepaint.visible_range.clone();
            input.reveal_cursor = false;
        });
    }
}
