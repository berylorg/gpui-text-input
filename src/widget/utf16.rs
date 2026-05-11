use std::ops::Range;

pub(super) fn range_from_utf16(text: &str, range: &Range<usize>) -> Range<usize> {
    normalize(
        offset_from_utf16(text, range.start),
        offset_from_utf16(text, range.end),
    )
}

pub(super) fn range_to_utf16(text: &str, range: &Range<usize>) -> Range<usize> {
    normalize(
        offset_to_utf16(text, range.start),
        offset_to_utf16(text, range.end),
    )
}

pub(super) fn offset_from_utf16(text: &str, offset: usize) -> usize {
    let mut utf8_offset = 0;
    let mut utf16_count = 0;

    for character in text.chars() {
        if utf16_count >= offset {
            break;
        }

        utf16_count += character.len_utf16();
        utf8_offset += character.len_utf8();
    }

    utf8_offset
}

pub(super) fn offset_to_utf16(text: &str, offset: usize) -> usize {
    let mut utf16_offset = 0;
    let mut utf8_count = 0;

    for character in text.chars() {
        if utf8_count >= offset {
            break;
        }

        utf8_count += character.len_utf8();
        utf16_offset += character.len_utf16();
    }

    utf16_offset
}

fn normalize(start: usize, end: usize) -> Range<usize> {
    if start <= end { start..end } else { end..start }
}
