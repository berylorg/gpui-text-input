use std::ops::Range;

use gpui::{Bounds, Pixels, Point, Window, point, px};

use crate::{TextInputMode, TextInputState};

use super::{TextInput, layout};
use layout::{BuiltInputLayout, InputLineLayout};

/// Scroll extents for a measured text-input layout.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextInputScrollLimits {
    /// Largest horizontal scroll offset that still has content to reveal.
    pub max_x: Pixels,
    /// Largest vertical scroll offset that still has content to reveal.
    pub max_y: Pixels,
}

/// Vertical reveal data for a measured multiline layout.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextInputVerticalReveal {
    /// Scroll offset that reveals the active endpoint within the measured bounds.
    pub scroll_y: Pixels,
    /// Maximum scroll offset available for the measured bounds.
    pub max_scroll_y: Pixels,
}

/// Geometry snapshot for a text input measured from the rendering layout path.
#[derive(Clone)]
pub struct TextInputGeometry {
    /// Bounds used to measure the input.
    pub bounds: Bounds<Pixels>,
    /// Total shaped content height for the measured bounds.
    pub content_height: Pixels,
    /// Number of visual lines after soft wrapping.
    pub visual_line_count: usize,
    /// Byte range visible within the measured bounds.
    pub visible_range: Range<usize>,
    /// Scroll offset selected for the measured bounds.
    pub scroll_offset: Point<Pixels>,
    /// Scroll extents for the measured bounds.
    pub scroll_limits: TextInputScrollLimits,
    /// Caret bounds for the active endpoint.
    pub caret_bounds: Option<Bounds<Pixels>>,
    /// Bounds for the active selection endpoint.
    pub active_selection_endpoint_bounds: Option<Bounds<Pixels>>,
    /// Bounds for the active selection range when the selection is not empty.
    pub selection_bounds: Option<Bounds<Pixels>>,
    /// Vertical reveal data for multiline inputs.
    pub vertical_reveal: Option<TextInputVerticalReveal>,
    lines: Vec<InputLineLayout>,
    line_height: Pixels,
}

impl TextInputGeometry {
    pub(super) fn from_layout(
        state: &TextInputState,
        bounds: Bounds<Pixels>,
        layout: &BuiltInputLayout,
        line_height: Pixels,
    ) -> Self {
        let visual_line_count = layout
            .lines
            .iter()
            .map(|line| line.line.wrap_boundaries().len() + 1)
            .sum::<usize>()
            .max(1);
        let scroll_limits = TextInputScrollLimits {
            max_x: max_scroll_x(state.mode(), &layout.lines, bounds),
            max_y: layout::max_scroll_y(layout.content_height, bounds),
        };
        let caret_bounds = layout::cursor_bounds(&layout.lines, state.cursor_offset(), line_height);
        let active_selection_endpoint_bounds = layout::bounds_for_range(
            &layout.lines,
            state.cursor_offset()..state.cursor_offset(),
            line_height,
        );
        let selection = state.selection();
        let selection_bounds = (!selection.is_empty())
            .then(|| layout::bounds_for_range(&layout.lines, selection, line_height))
            .flatten();
        let vertical_reveal =
            (state.mode() == TextInputMode::Multiline).then_some(TextInputVerticalReveal {
                scroll_y: layout.revealed_scroll_y,
                max_scroll_y: scroll_limits.max_y,
            });

        Self {
            bounds,
            content_height: layout.content_height,
            visual_line_count,
            visible_range: layout.visible_range.clone(),
            scroll_offset: point(layout.scroll_x, layout.scroll_y),
            scroll_limits,
            caret_bounds,
            active_selection_endpoint_bounds,
            selection_bounds,
            vertical_reveal,
            lines: layout.lines.clone(),
            line_height,
        }
    }

    /// Returns bounds for a byte range in this measured layout.
    pub fn bounds_for_range(&self, range: Range<usize>) -> Option<Bounds<Pixels>> {
        if range.start > range.end {
            return None;
        }

        layout::bounds_for_range(&self.lines, range, self.line_height)
    }
}

impl TextInput {
    /// Measures geometry for proposed bounds before the widget has painted.
    pub fn measure_geometry(
        &self,
        bounds: Bounds<Pixels>,
        window: &mut Window,
    ) -> TextInputGeometry {
        let layout = layout::build_input_layout(
            self.state(),
            &self.placeholder,
            &self.theme,
            bounds,
            self.scroll_x,
            self.scroll_y,
            self.should_reveal_cursor_for_bounds(bounds),
            false,
            window,
        );

        TextInputGeometry::from_layout(self.state(), bounds, &layout, window.line_height())
    }

    /// Returns geometry from the last painted layout, when available.
    pub fn geometry(&self) -> Option<TextInputGeometry> {
        self.last_geometry.clone()
    }

    /// Returns bounds for a byte range from the last painted layout.
    pub fn bounds_for_range(&self, range: Range<usize>) -> Option<Bounds<Pixels>> {
        self.last_geometry
            .as_ref()
            .and_then(|geometry| geometry.bounds_for_range(range))
    }

    /// Returns caret bounds from the last painted layout.
    pub fn caret_bounds(&self) -> Option<Bounds<Pixels>> {
        self.last_geometry
            .as_ref()
            .and_then(|geometry| geometry.caret_bounds)
    }

    /// Returns active selection endpoint bounds from the last painted layout.
    pub fn active_selection_endpoint_bounds(&self) -> Option<Bounds<Pixels>> {
        self.last_geometry
            .as_ref()
            .and_then(|geometry| geometry.active_selection_endpoint_bounds)
    }

    /// Returns scroll limits from the last painted layout.
    pub fn scroll_limits(&self) -> Option<TextInputScrollLimits> {
        self.last_geometry
            .as_ref()
            .map(|geometry| geometry.scroll_limits)
    }

    pub(super) fn geometry_from_layout(
        &self,
        bounds: Bounds<Pixels>,
        layout: &BuiltInputLayout,
        line_height: Pixels,
    ) -> TextInputGeometry {
        TextInputGeometry::from_layout(self.state(), bounds, layout, line_height)
    }
}

fn max_scroll_x(mode: TextInputMode, lines: &[InputLineLayout], bounds: Bounds<Pixels>) -> Pixels {
    if mode != TextInputMode::SingleLine {
        return px(0.0);
    }

    lines
        .first()
        .map(|line| (line.line.width() - bounds.size.width).max(px(0.0)))
        .unwrap_or(px(0.0))
}
