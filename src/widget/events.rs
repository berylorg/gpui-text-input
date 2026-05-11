use std::ops::Range;

use crate::TextInputChange;

/// User-visible baseline command handled by a text input.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum TextInputCommand {
    Backspace,
    Delete,
    DeleteWordBackward,
    DeleteWordForward,
    MoveLeft,
    MoveRight,
    MoveUp,
    MoveDown,
    MoveWordLeft,
    MoveWordRight,
    SelectLeft,
    SelectRight,
    SelectUp,
    SelectDown,
    SelectWordLeft,
    SelectWordRight,
    MoveHome,
    MoveEnd,
    SelectHome,
    SelectEnd,
    MoveToStart,
    MoveToEnd,
    SelectToStart,
    SelectToEnd,
    SelectAll,
    InsertNewline,
    Copy,
    Cut,
    Paste,
    Undo,
    Redo,
}

/// Selection state reported by app-neutral text-input events.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TextInputSelection {
    pub range: Range<usize>,
    pub reversed: bool,
}

/// Events emitted by the reusable text-input widget.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum TextInputEvent {
    /// The text buffer or edit state changed through an accepted edit.
    Changed(TextInputChange),
    /// The caret or selection changed.
    SelectionChanged(TextInputSelection),
    /// A baseline text-input command was handled.
    CommandHandled(TextInputCommand),
    /// An opaque inline atom was clicked.
    InlineAtomClicked {
        atom_id: String,
        position: gpui::Point<gpui::Pixels>,
    },
}
