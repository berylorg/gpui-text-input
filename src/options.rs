/// Editing layout policy for a text input.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TextInputMode {
    /// A field with one logical line. Inserted newlines become spaces.
    SingleLine,
    /// A field with newline-delimited logical lines.
    Multiline,
}

/// Construction options for [`crate::TextInputState`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TextInputOptions {
    mode: TextInputMode,
    read_only: bool,
    undo_limit: usize,
}

impl TextInputOptions {
    /// Default maximum undo snapshots retained by a field.
    pub const DEFAULT_UNDO_LIMIT: usize = 128;

    /// Creates editable single-line input options.
    pub const fn single_line() -> Self {
        Self {
            mode: TextInputMode::SingleLine,
            read_only: false,
            undo_limit: Self::DEFAULT_UNDO_LIMIT,
        }
    }

    /// Creates read-only single-line input options.
    pub const fn single_line_read_only() -> Self {
        Self {
            read_only: true,
            ..Self::single_line()
        }
    }

    /// Creates editable multiline input options.
    pub const fn multiline() -> Self {
        Self {
            mode: TextInputMode::Multiline,
            read_only: false,
            undo_limit: Self::DEFAULT_UNDO_LIMIT,
        }
    }

    /// Returns the input layout mode.
    pub const fn mode(self) -> TextInputMode {
        self.mode
    }

    /// Returns whether destructive edits are rejected.
    pub const fn is_read_only(self) -> bool {
        self.read_only
    }

    /// Returns the configured undo snapshot limit.
    pub const fn undo_limit(self) -> usize {
        self.undo_limit
    }

    /// Sets read-only behavior.
    pub const fn with_read_only(mut self, read_only: bool) -> Self {
        self.read_only = read_only;
        self
    }

    /// Sets the undo snapshot limit. A zero limit disables undo recording.
    pub const fn with_undo_limit(mut self, undo_limit: usize) -> Self {
        self.undo_limit = undo_limit;
        self
    }
}

impl Default for TextInputOptions {
    fn default() -> Self {
        Self::single_line()
    }
}
