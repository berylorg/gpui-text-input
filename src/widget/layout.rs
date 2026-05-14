use std::ops::Range;

use gpui::{
    App, Bounds, PaintQuad, Pixels, Point, SharedString, TextAlign, Window, point, px, size,
};

use crate::{TextInputAtom, TextInputMode, TextInputState};

use super::{TextInputTheme, keyboard::VerticalDirection};

mod geometry;
mod shape;

pub use shape::wrapped_visual_line_count_for_width;

pub(super) use geometry::{
    bounds_for_range, cursor_bounds, index_for_position, max_scroll_y, vertical_target_offset,
};

use geometry::{
    cursor_quad, desired_horizontal_scroll, desired_vertical_scroll, selection_quads, visible_range,
};
use shape::shape_logical_lines;

#[derive(Clone)]
pub(super) struct InputLineLayout {
    pub(super) range: Range<usize>,
    pub(super) line: gpui::WrappedLine,
    pub(super) origin: Point<Pixels>,
    pub(super) bounds: Bounds<Pixels>,
}

pub(super) struct BuiltInputLayout {
    pub(super) lines: Vec<InputLineLayout>,
    pub(super) selection: Vec<PaintQuad>,
    pub(super) cursor: Option<PaintQuad>,
    pub(super) scroll_x: Pixels,
    pub(super) scroll_y: Pixels,
    pub(super) revealed_scroll_y: Pixels,
    pub(super) content_height: Pixels,
    pub(super) visible_range: Range<usize>,
}

pub(super) fn build_input_layout(
    state: &TextInputState,
    placeholder: &SharedString,
    theme: &TextInputTheme,
    bounds: Bounds<Pixels>,
    current_scroll_x: Pixels,
    current_scroll_y: Pixels,
    reveal_cursor: bool,
    show_caret: bool,
    window: &mut Window,
) -> BuiltInputLayout {
    let text = state.text().to_string();
    let showing_placeholder = text.is_empty();
    let mode = state.mode();
    let style = window.text_style();
    let text_color = if showing_placeholder {
        theme.placeholder
    } else {
        theme.text.unwrap_or(style.color)
    };
    let font_size = style.font_size.to_pixels(window.rem_size());
    let line_height = window.line_height();
    let wrap_width = (mode == TextInputMode::Multiline).then_some(bounds.size.width.max(px(1.0)));
    let atom_ranges = if showing_placeholder {
        Vec::new()
    } else {
        state
            .atoms()
            .iter()
            .map(TextInputAtom::range)
            .collect::<Vec<_>>()
    };
    let shaped = shape_logical_lines(
        &text,
        placeholder,
        state.marked_range().as_ref(),
        &atom_ranges,
        text_color,
        theme.marked_underline,
        theme.atom_text,
        theme.atom_background,
        style.font(),
        font_size,
        line_height,
        wrap_width,
        showing_placeholder,
        window,
    );
    let content_height = shaped
        .iter()
        .fold(px(0.0), |height, line| height + line.block_height)
        .max(line_height);
    let scroll_x =
        desired_horizontal_scroll(mode, &shaped, state, current_scroll_x, bounds, line_height);
    let clamped_scroll_y = desired_vertical_scroll(
        mode,
        &shaped,
        state,
        current_scroll_y,
        bounds,
        line_height,
        false,
    );
    let revealed_scroll_y = desired_vertical_scroll(
        mode,
        &shaped,
        state,
        current_scroll_y,
        bounds,
        line_height,
        true,
    );
    let scroll_y = if reveal_cursor {
        revealed_scroll_y
    } else {
        clamped_scroll_y
    };
    let top = match mode {
        TextInputMode::SingleLine => centered_line_bounds(bounds, line_height).top(),
        TextInputMode::Multiline => bounds.top() - scroll_y,
    };
    let mut next_top = top;
    let mut lines = Vec::with_capacity(shaped.len());

    for shaped_line in shaped {
        let origin = point(bounds.left() - scroll_x, next_top);
        let line_bounds = Bounds::new(
            point(bounds.left(), next_top),
            size(bounds.size.width, shaped_line.block_height),
        );
        next_top += shaped_line.block_height;
        lines.push(InputLineLayout {
            range: shaped_line.range,
            line: shaped_line.line,
            origin,
            bounds: line_bounds,
        });
    }

    let selection = if showing_placeholder || state.selection().is_empty() {
        Vec::new()
    } else {
        selection_quads(&lines, &state.selection(), line_height, theme.selection)
    };
    let cursor = (show_caret && state.selection().is_empty())
        .then(|| cursor_quad(&lines, state.cursor_offset(), line_height, theme.caret))
        .flatten();
    let visible_range = visible_range(&lines, bounds).unwrap_or_else(|| {
        let cursor = state.cursor_offset();
        cursor..cursor
    });

    BuiltInputLayout {
        lines,
        selection,
        cursor,
        scroll_x,
        scroll_y,
        revealed_scroll_y,
        content_height,
        visible_range,
    }
}

pub(super) fn paint_lines(lines: &[InputLineLayout], window: &mut Window, cx: &mut App) {
    for line in lines {
        line.line
            .paint(
                line.origin,
                line_height_for(line),
                TextAlign::default(),
                Some(line.bounds),
                window,
                cx,
            )
            .expect("input text should paint");
    }
}

pub(super) fn line_height_for(line: &InputLineLayout) -> Pixels {
    line.bounds.size.height / (line.line.wrap_boundaries().len() + 1) as f32
}

fn centered_line_bounds(bounds: Bounds<Pixels>, line_height: Pixels) -> Bounds<Pixels> {
    let line_height = line_height.min(bounds.size.height);
    let vertical_inset = ((bounds.size.height - line_height) / 2.0).max(px(0.0));

    Bounds::new(
        point(bounds.left(), bounds.top() + vertical_inset),
        size(bounds.size.width, line_height),
    )
}

pub(super) fn move_visual_line(
    lines: &[InputLineLayout],
    cursor_offset: usize,
    direction: VerticalDirection,
) -> Option<usize> {
    vertical_target_offset(lines, cursor_offset, direction)
}
