use std::ops::Range;

use gpui::{Bounds, Hsla, PaintQuad, Pixels, Point, WrappedLine, fill, point, px, size};

use crate::{TextInputMode, TextInputState};

use super::{InputLineLayout, line_height_for, shape::ShapedLogicalLine};
use crate::widget::keyboard::VerticalDirection;

struct VisualLineRef {
    line_index: usize,
    visual_index: usize,
    local_range: Range<usize>,
}

pub(super) fn desired_horizontal_scroll(
    mode: TextInputMode,
    shaped: &[ShapedLogicalLine],
    state: &TextInputState,
    current: Pixels,
    bounds: Bounds<Pixels>,
    line_height: Pixels,
) -> Pixels {
    if mode != TextInputMode::SingleLine || state.text().is_empty() {
        return px(0.0);
    }
    let Some(line) = shaped.first() else {
        return px(0.0);
    };
    if line.line.width() <= bounds.size.width {
        return px(0.0);
    }
    if state.selection() == (0..state.text().len()) {
        return px(0.0);
    }

    let caret_padding = px(12.0);
    let visible_width = bounds.size.width.max(px(1.0));
    let max_scroll = (line.line.width() - visible_width).max(px(0.0));
    let caret_x = local_position_for_index(
        &line.line,
        state.cursor_offset().min(line.line.len()),
        line_height,
    )
    .x;
    let mut scroll = current;

    if caret_x - scroll < caret_padding {
        scroll = (caret_x - caret_padding).max(px(0.0));
    } else if caret_x - scroll > visible_width - caret_padding {
        scroll = (caret_x - (visible_width - caret_padding)).min(max_scroll);
    }

    scroll.clamp(px(0.0), max_scroll)
}

pub(super) fn desired_vertical_scroll(
    mode: TextInputMode,
    shaped: &[ShapedLogicalLine],
    state: &TextInputState,
    current: Pixels,
    bounds: Bounds<Pixels>,
    line_height: Pixels,
    reveal_cursor: bool,
) -> Pixels {
    if mode != TextInputMode::Multiline {
        return px(0.0);
    }

    let content_height = shaped
        .iter()
        .fold(px(0.0), |height, line| height + line.block_height)
        .max(line_height);
    let max_scroll = max_scroll_y(content_height, bounds);
    let mut scroll = current.clamp(px(0.0), max_scroll);
    if !reveal_cursor {
        return scroll;
    }

    let Some((line_top, _, visual_index)) = cursor_visual_line(shaped, state.cursor_offset())
    else {
        return scroll;
    };
    let caret_y = line_top + line_height * visual_index as f32;
    let caret_bottom = caret_y + line_height;
    let padding = line_height / 2.0;

    if caret_y - scroll < padding {
        scroll = (caret_y - padding).max(px(0.0));
    } else if caret_bottom - scroll > bounds.size.height - padding {
        scroll = (caret_bottom - (bounds.size.height - padding)).min(max_scroll);
    }

    scroll.clamp(px(0.0), max_scroll)
}

pub(super) fn selection_quads(
    lines: &[InputLineLayout],
    selected_range: &Range<usize>,
    line_height: Pixels,
    selection_color: Hsla,
) -> Vec<PaintQuad> {
    let mut quads = Vec::new();
    for line in lines {
        let start = selected_range.start.max(line.range.start);
        let end = selected_range.end.min(line.range.end);
        if start >= end {
            continue;
        }

        let local_selection = start - line.range.start..end - line.range.start;
        for (visual_index, visual_range) in
            wrapped_visual_ranges(&line.line).into_iter().enumerate()
        {
            let visual_start = local_selection.start.max(visual_range.start);
            let visual_end = local_selection.end.min(visual_range.end);
            if visual_start >= visual_end {
                continue;
            }

            let start_position =
                visual_position_for_index(&line.line, visual_start, visual_index, line_height);
            let end_position = line
                .line
                .position_for_index(visual_end, line_height)
                .unwrap_or_else(|| {
                    point(line.bounds.size.width, line_height * visual_index as f32)
                });
            quads.push(fill(
                Bounds::from_corners(
                    point(
                        line.origin.x + start_position.x,
                        line.origin.y + start_position.y,
                    ),
                    point(
                        line.origin.x + end_position.x,
                        line.origin.y + end_position.y + line_height,
                    ),
                ),
                selection_color,
            ));
        }
    }

    quads
}

pub(super) fn cursor_quad(
    lines: &[InputLineLayout],
    cursor_offset: usize,
    line_height: Pixels,
    caret_color: Hsla,
) -> Option<PaintQuad> {
    Some(fill(
        cursor_bounds(lines, cursor_offset, line_height)?,
        caret_color,
    ))
}

pub(in crate::widget) fn cursor_bounds(
    lines: &[InputLineLayout],
    cursor_offset: usize,
    line_height: Pixels,
) -> Option<Bounds<Pixels>> {
    let line = line_for_cursor(lines, cursor_offset)?;
    let local_cursor = cursor_offset
        .saturating_sub(line.range.start)
        .min(line.line.len());
    let cursor_position = local_position_for_index(&line.line, local_cursor, line_height_for(line));

    Some(Bounds::new(
        point(
            line.origin.x + cursor_position.x,
            line.origin.y + cursor_position.y,
        ),
        size(px(2.0), line_height),
    ))
}

pub(in crate::widget) fn bounds_for_range(
    lines: &[InputLineLayout],
    range: Range<usize>,
    line_height: Pixels,
) -> Option<Bounds<Pixels>> {
    let start_line = line_for_cursor(lines, range.start)?;
    let end_line = line_for_cursor(lines, range.end)?;
    let start_position = local_position_for_index(
        &start_line.line,
        range
            .start
            .saturating_sub(start_line.range.start)
            .min(start_line.line.len()),
        line_height_for(start_line),
    );
    let end_position = local_position_for_index(
        &end_line.line,
        range
            .end
            .saturating_sub(end_line.range.start)
            .min(end_line.line.len()),
        line_height_for(end_line),
    );

    Some(Bounds::from_corners(
        point(
            start_line.origin.x + start_position.x,
            start_line.origin.y + start_position.y,
        ),
        point(
            end_line.origin.x + end_position.x,
            end_line.origin.y + end_position.y + line_height,
        ),
    ))
}

pub(in crate::widget) fn index_for_position(
    lines: &[InputLineLayout],
    bounds: Bounds<Pixels>,
    position: Point<Pixels>,
) -> usize {
    if lines.is_empty() {
        return 0;
    }

    if position.y < bounds.top() {
        return lines.first().map_or(0, |line| line.range.start);
    }
    if position.y > bounds.bottom() {
        return lines.last().map_or(0, |line| line.range.end);
    }

    let Some(line) = lines
        .iter()
        .find(|line| line.bounds.contains(&position))
        .or_else(|| {
            if position.y < lines.first()?.bounds.top() {
                lines.first()
            } else {
                lines.last()
            }
        })
    else {
        return 0;
    };
    let local_position = point(position.x - line.origin.x, position.y - line.origin.y);
    let local_index = line
        .line
        .closest_index_for_position(local_position, line_height_for(line))
        .unwrap_or_else(|index| index)
        .min(line.line.len());

    line.range.start + local_index
}

pub(super) fn visible_range(
    lines: &[InputLineLayout],
    bounds: Bounds<Pixels>,
) -> Option<Range<usize>> {
    let mut visible = lines.iter().filter(|line| {
        line.bounds.bottom() >= bounds.top() && line.bounds.top() <= bounds.bottom()
    });
    let first = visible.next()?;
    let mut range = first.range.clone();
    for line in visible {
        range.end = line.range.end;
    }
    Some(range)
}

pub(in crate::widget) fn vertical_target_offset(
    lines: &[InputLineLayout],
    cursor_offset: usize,
    direction: VerticalDirection,
) -> Option<usize> {
    let visual_lines = visual_line_refs(lines);
    let current_index = visual_line_index_for_offset(lines, &visual_lines, cursor_offset)?;
    let target_index = match direction {
        VerticalDirection::Up => current_index.checked_sub(1)?,
        VerticalDirection::Down => {
            (current_index + 1 < visual_lines.len()).then_some(current_index + 1)?
        }
    };
    let current = &visual_lines[current_index];
    let current_line = lines.get(current.line_index)?;
    let current_line_height = line_height_for(current_line);
    let local_cursor = cursor_offset
        .saturating_sub(current_line.range.start)
        .min(current_line.line.len());
    let cursor_position = visual_position_for_index(
        &current_line.line,
        local_cursor,
        current.visual_index,
        current_line_height,
    );

    let target = &visual_lines[target_index];
    let target_line = lines.get(target.line_index)?;
    let target_line_height = line_height_for(target_line);
    let local_position = point(
        cursor_position.x,
        target_line_height * target.visual_index as f32 + target_line_height / 2.0,
    );
    let local_offset = target_line
        .line
        .closest_index_for_position(local_position, target_line_height)
        .unwrap_or_else(|index| index)
        .min(target_line.line.len())
        .clamp(target.local_range.start, target.local_range.end);

    Some(target_line.range.start + local_offset)
}

pub(in crate::widget) fn max_scroll_y(content_height: Pixels, bounds: Bounds<Pixels>) -> Pixels {
    (content_height - bounds.size.height).max(px(0.0))
}

fn cursor_visual_line(
    shaped: &[ShapedLogicalLine],
    cursor_offset: usize,
) -> Option<(Pixels, &WrappedLine, usize)> {
    let mut top = px(0.0);
    for shaped_line in shaped {
        if cursor_offset >= shaped_line.range.start && cursor_offset <= shaped_line.range.end {
            let local_cursor = cursor_offset
                .saturating_sub(shaped_line.range.start)
                .min(shaped_line.line.len());
            let visual_index =
                visual_line_index_for_local_offset(&shaped_line.line, local_cursor).unwrap_or(0);
            return Some((top, &shaped_line.line, visual_index));
        }
        top += shaped_line.block_height;
    }

    shaped.last().map(|line| {
        (
            (top - line.block_height).max(px(0.0)),
            &line.line,
            line.line.wrap_boundaries().len(),
        )
    })
}

fn line_for_cursor(lines: &[InputLineLayout], cursor_offset: usize) -> Option<&InputLineLayout> {
    lines
        .iter()
        .find(|line| cursor_offset >= line.range.start && cursor_offset <= line.range.end)
        .or_else(|| lines.last())
}

fn local_position_for_index(
    line: &WrappedLine,
    index: usize,
    line_height: Pixels,
) -> Point<Pixels> {
    line.position_for_index(index, line_height)
        .unwrap_or_else(|| point(px(0.0), px(0.0)))
}

fn visual_position_for_index(
    line: &WrappedLine,
    index: usize,
    visual_index: usize,
    line_height: Pixels,
) -> Point<Pixels> {
    if visual_index > 0
        && wrapped_visual_ranges(line)
            .get(visual_index)
            .is_some_and(|range| range.start == index)
    {
        return point(px(0.0), line_height * visual_index as f32);
    }

    line.position_for_index(index, line_height)
        .unwrap_or_else(|| point(px(0.0), line_height * visual_index as f32))
}

fn wrapped_visual_ranges(line: &WrappedLine) -> Vec<Range<usize>> {
    let mut ranges = Vec::with_capacity(line.wrap_boundaries().len() + 1);
    let mut start = 0usize;
    for boundary in line.wrap_boundaries() {
        let Some(run) = line.runs().get(boundary.run_ix) else {
            continue;
        };
        let Some(glyph) = run.glyphs.get(boundary.glyph_ix) else {
            continue;
        };
        let end = glyph.index.min(line.len()).max(start);
        ranges.push(start..end);
        start = end;
    }
    ranges.push(start..line.len());
    ranges
}

fn visual_line_refs(lines: &[InputLineLayout]) -> Vec<VisualLineRef> {
    lines
        .iter()
        .enumerate()
        .flat_map(|(line_index, line)| {
            wrapped_visual_ranges(&line.line)
                .into_iter()
                .enumerate()
                .map(move |(visual_index, local_range)| VisualLineRef {
                    line_index,
                    visual_index,
                    local_range,
                })
        })
        .collect()
}

fn visual_line_index_for_local_offset(line: &WrappedLine, offset: usize) -> Option<usize> {
    wrapped_visual_ranges(line)
        .iter()
        .enumerate()
        .find_map(|(index, range)| {
            let contains = if range.start == range.end {
                offset == range.start
            } else if offset == range.start || offset > range.start && offset < range.end {
                true
            } else {
                offset == range.end && index + 1 == line.wrap_boundaries().len() + 1
            };
            contains.then_some(index)
        })
}

fn visual_line_index_for_offset(
    lines: &[InputLineLayout],
    visual_lines: &[VisualLineRef],
    cursor_offset: usize,
) -> Option<usize> {
    visual_lines
        .iter()
        .enumerate()
        .find_map(|(index, visual_line)| {
            let line = lines.get(visual_line.line_index)?;
            let start = line.range.start + visual_line.local_range.start;
            let end = line.range.start + visual_line.local_range.end;
            let is_last_for_logical_line = visual_lines
                .get(index + 1)
                .is_none_or(|next| next.line_index != visual_line.line_index);
            let contains = if start == end {
                cursor_offset == start
            } else if cursor_offset == start || cursor_offset > start && cursor_offset < end {
                true
            } else {
                is_last_for_logical_line && cursor_offset == end
            };
            contains.then_some(index)
        })
        .or_else(|| (!visual_lines.is_empty()).then_some(visual_lines.len() - 1))
}
