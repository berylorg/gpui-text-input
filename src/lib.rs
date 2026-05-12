//! App-neutral text-input primitives for GPUI applications.
//!
//! The crate separates the plain text editing model from any host
//! application meaning. Hosts decide what a field represents, how changes are
//! validated, and how accepted text is persisted or submitted.
//!
//! # Examples
//!
//! Single-line fields normalize inserted newlines into spaces:
//!
//! ```
//! use gpui_text_input::{TextInputOptions, TextInputState};
//!
//! let mut input = TextInputState::new("", TextInputOptions::single_line());
//! let change = input.paste("alpha\nbeta").expect("paste changes text");
//!
//! assert_eq!(input.text(), "alpha beta");
//! assert_eq!(change.replacement, "alpha beta");
//! ```
//!
//! Multiline fields preserve logical newlines and expose line movement:
//!
//! ```
//! use gpui_text_input::{TextInputOptions, TextInputState};
//!
//! let mut input = TextInputState::new("one\ntwo", TextInputOptions::multiline());
//! input.move_to_end();
//! input.insert_newline().expect("newline changes text");
//! input.paste("three").expect("paste changes text");
//! input.move_home();
//!
//! assert_eq!(input.text(), "one\ntwo\nthree");
//! assert_eq!(input.selection(), 8..8);
//! ```
//!
//! GPUI widgets are entity based. Install the app-neutral key bindings once,
//! then create a [`TextInput`] entity from a view:
//!
//! ```
//! use gpui::{App, Context};
//! use gpui_text_input::{
//!     TextInput, TextInputEnterKey, TextInputSingleLineVerticalKey, ensure_text_input_bindings,
//! };
//!
//! fn install_bindings(cx: &mut App) {
//!     ensure_text_input_bindings(cx);
//! }
//!
//! fn build_input(cx: &mut Context<TextInput>) -> TextInput {
//!     let mut input = TextInput::new("", "Value", cx);
//!     input.set_enter_key(TextInputEnterKey::Propagate);
//!     input.set_single_line_vertical_key(TextInputSingleLineVerticalKey::Propagate);
//!     input
//! }
//! ```
//!
//! Opaque atom ranges can be used by hosts that render domain content as text
//! markers while keeping the reusable editor unaware of the domain payload:
//!
//! ```
//! use gpui_text_input::{TextInputAtom, TextInputOptions, TextInputState};
//!
//! let mut input = TextInputState::new("See [A]", TextInputOptions::single_line());
//! input
//!     .set_atoms(vec![TextInputAtom::new("asset-a", 4..7, "[Attachment A]")])
//!     .expect("atom range should match display text");
//! input.select_all();
//!
//! let selection = input.selection_export().expect("selection should export");
//! assert_eq!(selection.display_text(), "See [A]");
//! assert_eq!(selection.copy_text(), "See [Attachment A]");
//! ```
//!
//! Retained-count diagnostics expose app-neutral lower-bound byte counts:
//!
//! ```
//! use gpui_text_input::{TextInputOptions, TextInputState};
//!
//! let input = TextInputState::new("draft", TextInputOptions::single_line());
//! let counts = input.retained_counts();
//!
//! assert_eq!(counts.current_text_bytes, "draft".len());
//! ```

mod actions;
mod atom;
mod boundary;
mod change;
mod editing;
mod movement;
mod newline;
mod options;
mod state;
mod widget;

pub use actions::{
    Backspace, Copy, Cut, Delete, DeleteWordBackward, DeleteWordForward, Enter, InsertNewline,
    MoveDown, MoveEnd, MoveHome, MoveLeft, MoveRight, MoveToEnd, MoveToStart, MoveUp, MoveWordLeft,
    MoveWordRight, Paste, Redo, SelectAll, SelectDown, SelectEnd, SelectHome, SelectLeft,
    SelectRight, SelectToEnd, SelectToStart, SelectUp, SelectWordLeft, SelectWordRight,
    TEXT_INPUT_KEY_CONTEXT, Undo, ensure_text_input_bindings,
};
pub use atom::{
    TextInputAtom, TextInputAtomError, TextInputSelectionAtom, TextInputSelectionExport,
};
pub use change::TextInputChange;
pub use options::{TextInputMode, TextInputOptions};
pub use state::{TextInputRetainedCounts, TextInputState};
pub use widget::{
    TextInput, TextInputAtomClipboardPolicy, TextInputCommand, TextInputEnterKey, TextInputEvent,
    TextInputRichPastePolicy, TextInputSelection, TextInputSingleLineVerticalKey, TextInputTheme,
    wrapped_visual_line_count_for_width,
};
