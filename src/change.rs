use std::ops::Range;

/// Description of an accepted edit operation.
///
/// Byte ranges always describe valid UTF-8 boundaries. `replaced_range` is the
/// range in the old buffer, `replacement` is the text inserted at that range,
/// and `inserted_range` is the replacement's range in the new buffer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TextInputChange {
    /// The range replaced in the buffer before the edit was applied.
    pub replaced_range: Range<usize>,
    /// The normalized replacement text inserted by the edit.
    pub replacement: String,
    /// The range occupied by `replacement` after the edit was applied.
    pub inserted_range: Range<usize>,
    /// The normalized selection after the edit was applied.
    pub selection: Range<usize>,
    /// Whether the accepted edit changed the text buffer.
    pub text_changed: bool,
}

impl TextInputChange {
    pub(crate) fn new(
        replaced_range: Range<usize>,
        replacement: String,
        selection: Range<usize>,
        text_changed: bool,
    ) -> Self {
        let inserted_range = replaced_range.start..replaced_range.start + replacement.len();
        Self {
            replaced_range,
            replacement,
            inserted_range,
            selection,
            text_changed,
        }
    }

    pub(crate) fn full_buffer(old_text: &str, new_text: &str, selection: Range<usize>) -> Self {
        Self {
            replaced_range: 0..old_text.len(),
            replacement: new_text.to_string(),
            inserted_range: 0..new_text.len(),
            selection,
            text_changed: old_text != new_text,
        }
    }
}
