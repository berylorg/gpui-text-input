use std::ops::Range;

use crate::TextInputChange;

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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TextInputSelection {
    pub range: Range<usize>,
    pub reversed: bool,
}

#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum TextInputEvent {
    Changed(TextInputChange),
    SelectionChanged(TextInputSelection),
    CommandHandled(TextInputCommand),
    InlineAtomClicked {
        atom_id: String,
        position: gpui::Point<gpui::Pixels>,
    },
}
