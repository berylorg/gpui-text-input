use std::ops::Range;

use gpui::{Font, Hsla, Pixels, SharedString, TextRun, UnderlineStyle, Window, WrappedLine, px};

pub(super) struct ShapedLogicalLine {
    pub(super) range: Range<usize>,
    pub(super) line: WrappedLine,
    pub(super) block_height: Pixels,
}

pub fn wrapped_visual_line_count_for_width(
    text: &str,
    wrap_width: Pixels,
    window: &mut Window,
) -> usize {
    if text.is_empty() {
        return 1;
    }

    let style = window.text_style();
    let font_size = style.font_size.to_pixels(window.rem_size());
    let run = TextRun {
        len: text.len(),
        font: style.font(),
        color: style.color,
        background_color: None,
        underline: None,
        strikethrough: None,
    };

    window
        .text_system()
        .shape_text(
            text.to_string().into(),
            font_size,
            &[run],
            Some(wrap_width.max(px(1.0))),
            None,
        )
        .map(|lines| {
            lines
                .iter()
                .map(|line| line.wrap_boundaries().len() + 1)
                .sum::<usize>()
                .max(1)
        })
        .unwrap_or(1)
}

pub(super) fn shape_logical_lines(
    text: &str,
    placeholder: &SharedString,
    marked_range: Option<&Range<usize>>,
    atom_ranges: &[Range<usize>],
    text_color: Hsla,
    marked_underline: Hsla,
    atom_text: Hsla,
    atom_background: Option<Hsla>,
    font: Font,
    font_size: Pixels,
    line_height: Pixels,
    wrap_width: Option<Pixels>,
    showing_placeholder: bool,
    window: &mut Window,
) -> Vec<ShapedLogicalLine> {
    let line_ranges = if showing_placeholder {
        vec![0..0]
    } else {
        logical_line_ranges(text)
    };

    line_ranges
        .iter()
        .map(|range| {
            let display_text = if showing_placeholder {
                placeholder.clone()
            } else {
                text[range.clone()].to_string().into()
            };
            let runs = text_runs_for_line(
                display_text.len(),
                range,
                marked_range,
                atom_ranges,
                font.clone(),
                text_color,
                marked_underline,
                atom_text,
                atom_background,
            );
            let line = window
                .text_system()
                .shape_text(display_text, font_size, &runs, wrap_width, None)
                .ok()
                .and_then(|mut wrapped| wrapped.pop())
                .unwrap_or_else(WrappedLine::default);
            let block_height = line_height * (line.wrap_boundaries().len() + 1) as f32;

            ShapedLogicalLine {
                range: range.clone(),
                line,
                block_height,
            }
        })
        .collect()
}

fn text_runs_for_line(
    display_len: usize,
    line_range: &Range<usize>,
    marked_range: Option<&Range<usize>>,
    atom_ranges: &[Range<usize>],
    font: Font,
    color: Hsla,
    marked_underline: Hsla,
    atom_text: Hsla,
    atom_background: Option<Hsla>,
) -> Vec<TextRun> {
    let mut boundaries = vec![0usize, display_len];
    if let Some(marked_range) = marked_range {
        push_local_overlap_boundaries(&mut boundaries, line_range, marked_range);
    }
    for range in atom_ranges {
        push_local_overlap_boundaries(&mut boundaries, line_range, range);
    }
    boundaries.sort_unstable();
    boundaries.dedup();

    boundaries
        .windows(2)
        .filter_map(|boundary| {
            let start = boundary[0];
            let end = boundary[1];
            if start >= end {
                return None;
            }

            let global = line_range.start + start..line_range.start + end;
            let marked = marked_range.is_some_and(|range| ranges_intersect(&global, range));
            let atom = atom_ranges
                .iter()
                .any(|range| ranges_intersect(&global, range));
            Some(TextRun {
                len: end - start,
                font: font.clone(),
                color: if atom { atom_text } else { color }.into(),
                background_color: atom.then_some(atom_background).flatten(),
                underline: marked.then_some(UnderlineStyle {
                    color: Some(marked_underline),
                    thickness: px(1.0),
                    wavy: false,
                }),
                strikethrough: None,
            })
        })
        .collect()
}

fn push_local_overlap_boundaries(
    boundaries: &mut Vec<usize>,
    line_range: &Range<usize>,
    range: &Range<usize>,
) {
    let start = range.start.max(line_range.start);
    let end = range.end.min(line_range.end);
    if start >= end {
        return;
    }

    boundaries.push(start - line_range.start);
    boundaries.push(end - line_range.start);
}

fn logical_line_ranges(text: &str) -> Vec<Range<usize>> {
    let mut ranges = Vec::new();
    let mut start = 0;

    for (index, character) in text.char_indices() {
        if character == '\n' {
            ranges.push(start..index);
            start = index + character.len_utf8();
        }
    }

    ranges.push(start..text.len());
    ranges
}

fn ranges_intersect(left: &Range<usize>, right: &Range<usize>) -> bool {
    left.start < right.end && right.start < left.end
}
