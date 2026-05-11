use std::ops::Range;

use unicode_segmentation::UnicodeSegmentation;

#[derive(Clone, Copy)]
struct WordSegment {
    start: usize,
    end: usize,
    whitespace: bool,
}

pub(crate) fn clamp_to_char_boundary(text: &str, offset: usize) -> usize {
    let mut offset = offset.min(text.len());
    while !text.is_char_boundary(offset) {
        offset -= 1;
    }
    offset
}

pub(crate) fn normalize_range(text: &str, range: Range<usize>) -> Range<usize> {
    let start = clamp_to_char_boundary(text, range.start);
    let end = clamp_to_char_boundary(text, range.end);
    if start <= end { start..end } else { end..start }
}

pub(crate) fn previous_grapheme_boundary(text: &str, offset: usize) -> usize {
    let offset = clamp_to_char_boundary(text, offset);
    text[..offset]
        .grapheme_indices(true)
        .last()
        .map(|(index, _)| index)
        .unwrap_or(0)
}

pub(crate) fn next_grapheme_boundary(text: &str, offset: usize) -> usize {
    let offset = clamp_to_char_boundary(text, offset);
    if offset >= text.len() {
        return text.len();
    }

    text[offset..]
        .grapheme_indices(true)
        .nth(1)
        .map(|(index, _)| offset + index)
        .unwrap_or(text.len())
}

pub(crate) fn previous_word_boundary(text: &str, offset: usize) -> usize {
    let offset = clamp_to_char_boundary(text, offset);
    if offset == 0 || text.is_empty() {
        return 0;
    }

    let segments = word_segments(text);
    let lookup = previous_grapheme_boundary(text, offset);
    let Some(mut index) = segments.iter().position(|segment| lookup < segment.end) else {
        return 0;
    };

    if segments[index].whitespace {
        while index > 0 && segments[index].whitespace {
            index -= 1;
        }

        if segments[index].whitespace {
            return 0;
        }
    }

    segments[index].start
}

pub(crate) fn next_word_boundary(text: &str, offset: usize) -> usize {
    let offset = clamp_to_char_boundary(text, offset);
    if offset >= text.len() || text.is_empty() {
        return text.len();
    }

    let segments = word_segments(text);
    let Some(mut index) = segments.iter().position(|segment| offset < segment.end) else {
        return text.len();
    };

    if segments[index].whitespace {
        while index < segments.len() && segments[index].whitespace {
            index += 1;
        }
    } else {
        index += 1;
        while index < segments.len() && segments[index].whitespace {
            index += 1;
        }
    }

    segments
        .get(index)
        .map_or(text.len(), |segment| segment.start)
}

pub(crate) fn word_range_at(text: &str, offset: usize) -> Option<Range<usize>> {
    if text.is_empty() {
        return None;
    }

    let offset = clamp_to_char_boundary(text, offset);
    let lookup = if offset >= text.len() {
        previous_grapheme_boundary(text, text.len())
    } else {
        offset
    };

    word_segments(text)
        .into_iter()
        .find(|segment| lookup < segment.end)
        .map(|segment| segment.start..segment.end)
}

pub(crate) fn line_start_at(text: &str, offset: usize) -> usize {
    let offset = clamp_to_char_boundary(text, offset);
    text[..offset]
        .rfind('\n')
        .map(|index| index + '\n'.len_utf8())
        .unwrap_or(0)
}

pub(crate) fn line_end_at(text: &str, offset: usize) -> usize {
    let offset = clamp_to_char_boundary(text, offset);
    text[offset..]
        .find('\n')
        .map(|index| offset + index)
        .unwrap_or(text.len())
}

pub(crate) fn line_range_at(text: &str, offset: usize) -> Range<usize> {
    line_start_at(text, offset)..line_end_at(text, offset)
}

fn word_segments(text: &str) -> Vec<WordSegment> {
    text.split_word_bound_indices()
        .filter_map(|(start, segment)| {
            (!segment.is_empty()).then_some(WordSegment {
                start,
                end: start + segment.len(),
                whitespace: segment.chars().all(char::is_whitespace),
            })
        })
        .collect()
}
