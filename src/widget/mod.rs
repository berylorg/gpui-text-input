use std::ops::Range;

use gpui::{
    App, Bounds, Context, EntityInputHandler, EventEmitter, FocusHandle, Focusable, Pixels, Point,
    SharedString, UTF16Selection, Window, point, px,
};

use crate::{
    TextInputAtom, TextInputAtomError, TextInputChange, TextInputMode, TextInputOptions,
    TextInputSelectionAtom, TextInputSelectionExport, TextInputState,
};

mod events;
mod ime;
mod keyboard;
mod layout;
mod render;
mod theme;
mod utf16;

pub use events::{TextInputCommand, TextInputEvent, TextInputSelection};
pub use layout::wrapped_visual_line_count_for_width;
pub use theme::TextInputTheme;

use layout::InputLineLayout;

/// Policy for the unmodified Enter key.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TextInputEnterKey {
    /// Insert a newline when the field is multiline.
    InsertNewline,
    /// Let the host handle Enter through an ancestor action listener.
    Propagate,
}

/// Policy for Up and Down when the field is single-line.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TextInputSingleLineVerticalKey {
    /// Treat Up and Down as handled text-input commands.
    Handle,
    /// Let the host handle Up and Down through ancestor action listeners.
    Propagate,
}

/// Policy for copy and cut commands when the selection contains atoms.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TextInputAtomClipboardPolicy {
    /// Write atom fallback text as ordinary plain text.
    PlainText,
    /// Let the host handle atom clipboard data through an ancestor listener.
    Propagate,
}

/// Policy for paste commands carrying non-plain or metadata-rich clipboard data.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TextInputRichPastePolicy {
    /// Insert the clipboard's plain text projection when one exists.
    PlainText,
    /// Let the host inspect rich clipboard entries before falling back to text.
    Propagate,
}

/// Reusable app-neutral GPUI text input.
pub struct TextInput {
    focus_handle: FocusHandle,
    state: TextInputState,
    placeholder: SharedString,
    theme: TextInputTheme,
    enabled: bool,
    enter_key: TextInputEnterKey,
    single_line_vertical_key: TextInputSingleLineVerticalKey,
    atom_clipboard_policy: TextInputAtomClipboardPolicy,
    rich_paste_policy: TextInputRichPastePolicy,
    last_layout: Vec<InputLineLayout>,
    last_bounds: Option<Bounds<Pixels>>,
    scroll_x: Pixels,
    scroll_y: Pixels,
    content_height: Pixels,
    visible_range: Range<usize>,
    reveal_cursor: bool,
    is_selecting: bool,
}

impl TextInput {
    /// Creates a single-line text input.
    pub fn new(
        initial_value: impl Into<String>,
        placeholder: impl Into<SharedString>,
        cx: &mut Context<Self>,
    ) -> Self {
        Self::new_with_options(
            initial_value,
            placeholder,
            TextInputOptions::single_line(),
            cx,
        )
    }

    /// Creates a multiline text input.
    pub fn multiline(
        initial_value: impl Into<String>,
        placeholder: impl Into<SharedString>,
        cx: &mut Context<Self>,
    ) -> Self {
        Self::new_with_options(
            initial_value,
            placeholder,
            TextInputOptions::multiline(),
            cx,
        )
    }

    /// Creates a text input with explicit model options.
    pub fn new_with_options(
        initial_value: impl Into<String>,
        placeholder: impl Into<SharedString>,
        options: TextInputOptions,
        cx: &mut Context<Self>,
    ) -> Self {
        let state = TextInputState::new(initial_value, options);
        let cursor = state.cursor_offset();
        Self {
            focus_handle: cx.focus_handle(),
            state,
            placeholder: placeholder.into(),
            theme: TextInputTheme::default(),
            enabled: true,
            enter_key: TextInputEnterKey::InsertNewline,
            single_line_vertical_key: TextInputSingleLineVerticalKey::Handle,
            atom_clipboard_policy: TextInputAtomClipboardPolicy::PlainText,
            rich_paste_policy: TextInputRichPastePolicy::PlainText,
            last_layout: Vec::new(),
            last_bounds: None,
            scroll_x: px(0.0),
            scroll_y: px(0.0),
            content_height: px(0.0),
            visible_range: cursor..cursor,
            reveal_cursor: true,
            is_selecting: false,
        }
    }

    /// Returns the current plain text.
    pub fn text(&self) -> &str {
        self.state.text()
    }

    /// Returns the current cursor offset.
    pub fn cursor_offset(&self) -> usize {
        self.state.cursor_offset()
    }

    /// Returns the current selection range.
    pub fn selection(&self) -> Range<usize> {
        self.state.selection()
    }

    /// Returns opaque inline atom ranges.
    pub fn atoms(&self) -> &[TextInputAtom] {
        self.state.atoms()
    }

    /// Returns whether IME marked text is active.
    pub fn has_marked_text(&self) -> bool {
        self.state.marked_range().is_some()
    }

    /// Returns selection export with atom fallback text.
    pub fn selection_export(&self) -> Option<TextInputSelectionExport> {
        self.state.selection_export()
    }

    /// Returns the current model state.
    pub fn state(&self) -> &TextInputState {
        &self.state
    }

    /// Returns the current visible byte range from the last rendered layout.
    pub fn visible_range(&self) -> Range<usize> {
        self.visible_range.clone()
    }

    /// Returns the current scroll offset from the last rendered layout.
    pub fn scroll_offset(&self) -> Point<Pixels> {
        point(self.scroll_x, self.scroll_y)
    }

    /// Returns whether the widget accepts focus and text input.
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Enables or disables the widget.
    pub fn set_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if self.enabled == enabled {
            return;
        }

        self.enabled = enabled;
        cx.notify();
    }

    /// Sets visual colors used by the text renderer.
    pub fn set_theme(&mut self, theme: TextInputTheme, cx: &mut Context<Self>) {
        if self.theme == theme {
            return;
        }

        self.theme = theme;
        cx.notify();
    }

    /// Sets the unmodified Enter-key policy.
    pub fn set_enter_key(&mut self, enter_key: TextInputEnterKey) {
        self.enter_key = enter_key;
    }

    /// Sets the single-line Up/Down key policy.
    pub fn set_single_line_vertical_key(&mut self, key: TextInputSingleLineVerticalKey) {
        self.single_line_vertical_key = key;
    }

    /// Sets atom clipboard propagation policy.
    pub fn set_atom_clipboard_policy(&mut self, policy: TextInputAtomClipboardPolicy) {
        self.atom_clipboard_policy = policy;
    }

    /// Sets rich clipboard paste propagation policy.
    pub fn set_rich_paste_policy(&mut self, policy: TextInputRichPastePolicy) {
        self.rich_paste_policy = policy;
    }

    /// Returns a clone of the GPUI focus handle.
    pub fn tab_focus_handle(&self) -> FocusHandle {
        self.focus_handle.clone()
    }

    /// Focuses the input.
    pub fn focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.enabled {
            window.focus(&self.focus_handle);
            cx.notify();
        }
    }

    /// Replaces all text from host state and clears transient edit history.
    pub fn set_text(&mut self, text: impl Into<String>, cx: &mut Context<Self>) -> bool {
        let changed = self.state.reset_text(text);
        self.scroll_x = px(0.0);
        self.scroll_y = px(0.0);
        self.reveal_cursor = true;
        self.finish_selection_change(changed, cx);
        changed
    }

    /// Replaces all text and selects it.
    pub fn set_text_and_select(&mut self, text: impl Into<String>, cx: &mut Context<Self>) -> bool {
        let changed = self.set_text(text, cx) | self.state.select_all();
        self.finish_selection_change(changed, cx);
        changed
    }

    /// Replaces the atom set without changing display text.
    pub fn set_atoms(
        &mut self,
        atoms: impl IntoIterator<Item = TextInputAtom>,
        cx: &mut Context<Self>,
    ) -> Result<bool, TextInputAtomError> {
        let changed = self.state.set_atoms(atoms)?;
        self.finish_selection_change(changed, cx);
        Ok(changed)
    }

    /// Selects all text.
    pub fn select_all_text(&mut self, cx: &mut Context<Self>) -> bool {
        let changed = self.state.select_all();
        self.finish_selection_change(changed, cx);
        changed
    }

    /// Sets selection to an explicit byte range.
    pub fn set_selection(
        &mut self,
        range: Range<usize>,
        reversed: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        let changed = self.state.set_selection(range, reversed);
        self.finish_selection_change(changed, cx);
        changed
    }

    /// Inserts text at the current selection.
    pub fn replace_selected_text(&mut self, text: &str, cx: &mut Context<Self>) -> bool {
        let changed = self.state.paste(text);
        self.finish_change(changed, cx)
    }

    /// Inserts text at a byte offset without using the current selection.
    pub fn insert_text_at_offset(
        &mut self,
        offset: usize,
        text: &str,
        cx: &mut Context<Self>,
    ) -> bool {
        let changed = self.state.insert_text_at_offset(offset, text);
        self.finish_change(changed, cx)
    }

    /// Inserts a newline if this is a multiline field.
    pub fn insert_newline(&mut self, cx: &mut Context<Self>) -> bool {
        let changed = self.state.insert_newline();
        self.finish_change(changed, cx)
    }

    /// Replaces an explicit range with one opaque atom.
    pub fn replace_text_range_with_atom(
        &mut self,
        range: Range<usize>,
        atom_text: &str,
        atom_id: impl Into<String>,
        atom_copy_text: impl Into<String>,
        cx: &mut Context<Self>,
    ) -> Result<bool, TextInputAtomError> {
        let changed = self.state.replace_text_in_range_with_atom(
            Some(range),
            atom_text,
            atom_id,
            atom_copy_text,
        )?;
        Ok(self.finish_change(changed, cx))
    }

    /// Replaces the active selection with text containing opaque atoms.
    pub fn replace_selected_text_with_atoms(
        &mut self,
        display_text: &str,
        atoms: impl IntoIterator<Item = TextInputSelectionAtom>,
        cx: &mut Context<Self>,
    ) -> Result<bool, TextInputAtomError> {
        let changed = self
            .state
            .replace_text_in_range_with_atoms(None, display_text, atoms)?;
        Ok(self.finish_change(changed, cx))
    }

    /// Cuts selected text and returns atom metadata for host clipboard handling.
    pub fn cut_selection_export(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Option<TextInputSelectionExport> {
        let (selection, change) = self.state.cut_selection_export()?;
        let _ = self.finish_change(Some(change), cx);
        Some(selection)
    }

    /// Removes the first atom with the host-owned id.
    pub fn remove_atom_by_id(&mut self, atom_id: &str, cx: &mut Context<Self>) -> bool {
        let changed = self.state.remove_atom_by_id(atom_id);
        self.finish_change(changed, cx)
    }

    fn finish_change(&mut self, change: Option<TextInputChange>, cx: &mut Context<Self>) -> bool {
        let Some(change) = change else {
            return false;
        };

        self.reveal_cursor = true;
        self.emit_selection_changed(cx);
        cx.emit(TextInputEvent::Changed(change));
        cx.notify();
        true
    }

    fn finish_selection_change(&mut self, changed: bool, cx: &mut Context<Self>) {
        if changed {
            self.reveal_cursor = true;
            self.emit_selection_changed(cx);
            cx.notify();
        }
    }

    fn emit_selection_changed(&self, cx: &mut Context<Self>) {
        cx.emit(TextInputEvent::SelectionChanged(TextInputSelection {
            range: self.state.selection(),
            reversed: self.state.selection_reversed(),
        }));
    }
}

impl EventEmitter<TextInputEvent> for TextInput {}

impl Focusable for TextInput {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}
