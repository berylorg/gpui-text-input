use std::ops::Range;

use crate::{
    TextInputAtomError, TextInputChange, TextInputMode, TextInputSelectionAtom,
    TextInputSelectionExport, TextInputState,
    boundary::{
        clamp_to_char_boundary, next_grapheme_boundary, next_word_boundary, normalize_range,
        previous_grapheme_boundary, previous_word_boundary,
    },
    newline::normalize_text,
};

impl TextInputState {
    /// Deletes the selected range, marked text, or previous grapheme.
    pub fn backspace(&mut self) -> Option<TextInputChange> {
        if self.options.is_read_only() {
            return None;
        }

        if self.has_active_selection() {
            return self.replace_text_in_range(None, "");
        }

        let cursor = self.cursor_offset();
        if cursor == 0 {
            return None;
        }

        let start = previous_grapheme_boundary(&self.text, cursor);
        self.replace_text_in_range(Some(start..cursor), "")
    }

    /// Deletes the selected range, marked text, or next grapheme.
    pub fn delete(&mut self) -> Option<TextInputChange> {
        if self.options.is_read_only() {
            return None;
        }

        if self.has_active_selection() {
            return self.replace_text_in_range(None, "");
        }

        let cursor = self.cursor_offset();
        if cursor == self.text.len() {
            return None;
        }

        let end = next_grapheme_boundary(&self.text, cursor);
        self.replace_text_in_range(Some(cursor..end), "")
    }

    /// Deletes to the previous word boundary.
    pub fn delete_word_backward(&mut self) -> Option<TextInputChange> {
        if self.options.is_read_only() {
            return None;
        }

        if self.has_active_selection() {
            return self.replace_text_in_range(None, "");
        }

        let cursor = self.cursor_offset();
        if cursor == 0 {
            return None;
        }

        let start = previous_word_boundary(&self.text, cursor);
        self.replace_text_in_range(Some(start..cursor), "")
    }

    /// Deletes to the next word boundary.
    pub fn delete_word_forward(&mut self) -> Option<TextInputChange> {
        if self.options.is_read_only() {
            return None;
        }

        if self.has_active_selection() {
            return self.replace_text_in_range(None, "");
        }

        let cursor = self.cursor_offset();
        if cursor == self.text.len() {
            return None;
        }

        let end = next_word_boundary(&self.text, cursor);
        self.replace_text_in_range(Some(cursor..end), "")
    }

    /// Cuts the selected plain text and deletes it.
    pub fn cut_selection(&mut self) -> Option<(String, TextInputChange)> {
        if self.options.is_read_only() {
            return None;
        }

        let selection = self.selected_text()?;
        let change = self.replace_text_in_range(None, "")?;
        Some((selection, change))
    }

    /// Cuts the selected text and returns atom metadata for host clipboard handling.
    pub fn cut_selection_export(&mut self) -> Option<(TextInputSelectionExport, TextInputChange)> {
        if self.options.is_read_only() {
            return None;
        }

        let selection = self.selection_export()?;
        let change = self.replace_text_in_range(None, "")?;
        Some((selection, change))
    }

    /// Pastes plain text at the cursor or replaces the active selection.
    pub fn paste(&mut self, text: &str) -> Option<TextInputChange> {
        if self.options.is_read_only() {
            return None;
        }

        self.replace_text_in_range(None, text)
    }

    /// Inserts text at `offset` without using the current selection.
    pub fn insert_text_at_offset(&mut self, offset: usize, text: &str) -> Option<TextInputChange> {
        if self.options.is_read_only() {
            return None;
        }

        let offset = clamp_to_char_boundary(&self.text, offset);
        self.replace_text_in_range(Some(offset..offset), text)
    }

    /// Inserts a newline when the input is multiline.
    pub fn insert_newline(&mut self) -> Option<TextInputChange> {
        if self.options.is_read_only() || self.options.mode() != TextInputMode::Multiline {
            return None;
        }

        self.replace_text_in_range(None, "\n")
    }

    /// Restores the previous edit snapshot.
    pub fn undo(&mut self) -> Option<TextInputChange> {
        if self.options.is_read_only() {
            return None;
        }

        let snapshot = self.undo_stack.pop_back()?;
        let old_text = self.text.clone();
        let current = self.snapshot();
        self.push_redo_snapshot(current);
        self.restore(snapshot);
        Some(TextInputChange::full_buffer(
            &old_text,
            &self.text,
            self.selected_range.clone(),
        ))
    }

    /// Re-applies the most recently undone edit snapshot.
    pub fn redo(&mut self) -> Option<TextInputChange> {
        if self.options.is_read_only() {
            return None;
        }

        let snapshot = self.redo_stack.pop_back()?;
        let old_text = self.text.clone();
        let current = self.snapshot();
        self.push_undo_snapshot(current);
        self.restore(snapshot);
        Some(TextInputChange::full_buffer(
            &old_text,
            &self.text,
            self.selected_range.clone(),
        ))
    }

    /// Clears the marked-text range without changing the buffer.
    pub fn unmark_text(&mut self) -> bool {
        if self.marked_range.is_none() {
            return false;
        }

        self.marked_range = None;
        true
    }

    /// Replaces an explicit range, marked text, selection, or cursor insertion point.
    pub fn replace_text_in_range(
        &mut self,
        replacement_range: Option<Range<usize>>,
        new_text: &str,
    ) -> Option<TextInputChange> {
        if self.options.is_read_only() {
            return None;
        }

        let replacement = normalize_text(new_text, self.options.mode());
        let range = self.active_edit_range(replacement_range);
        let cursor = range.start + replacement.len();
        self.apply_edit(range, replacement, cursor..cursor, false, None)
    }

    /// Replaces text with one opaque atom covering the inserted display text.
    pub fn replace_text_in_range_with_atom(
        &mut self,
        replacement_range: Option<Range<usize>>,
        atom_text: &str,
        atom_id: impl Into<String>,
        atom_copy_text: impl Into<String>,
    ) -> Result<Option<TextInputChange>, TextInputAtomError> {
        let display_text = normalize_text(atom_text, self.options.mode());
        let atom = TextInputSelectionAtom::new(
            atom_id,
            0..display_text.len(),
            display_text.clone(),
            atom_copy_text,
        );

        self.replace_text_in_range_with_atoms(replacement_range, &display_text, vec![atom])
    }

    /// Replaces text with host-owned opaque atom occurrences.
    pub fn replace_text_in_range_with_atoms(
        &mut self,
        replacement_range: Option<Range<usize>>,
        display_text: &str,
        atoms: impl IntoIterator<Item = TextInputSelectionAtom>,
    ) -> Result<Option<TextInputChange>, TextInputAtomError> {
        if self.options.is_read_only() {
            return Ok(None);
        }

        let normalized_text = normalize_text(display_text, self.options.mode());
        let range = self.active_edit_range(replacement_range);
        let inserted_atoms =
            Self::selection_atoms_for_display_text(range.start, &normalized_text, atoms)?;
        let cursor = range.start + normalized_text.len();

        Ok(self.apply_edit_with_inserted_atoms(
            range,
            normalized_text,
            cursor..cursor,
            false,
            None,
            inserted_atoms,
        ))
    }

    /// Removes the first atom with the host-owned id.
    pub fn remove_atom_by_id(&mut self, atom_id: &str) -> Option<TextInputChange> {
        if self.options.is_read_only() {
            return None;
        }

        let range = self
            .atoms
            .iter()
            .find(|atom| atom.id == atom_id)
            .map(|atom| atom.range.clone())?;
        self.replace_text_in_range(Some(range), "")
    }

    /// Replaces text and marks the inserted range for IME composition.
    pub fn replace_and_mark_text_in_range(
        &mut self,
        replacement_range: Option<Range<usize>>,
        new_text: &str,
        new_selected_range: Option<Range<usize>>,
    ) -> Option<TextInputChange> {
        if self.options.is_read_only() {
            return None;
        }

        let replacement = normalize_text(new_text, self.options.mode());
        let range = self.active_edit_range(replacement_range);
        let inserted_range = range.start..range.start + replacement.len();
        let selected_range =
            relative_inserted_selection(&replacement, inserted_range.start, new_selected_range);
        let marked_range = (!replacement.is_empty()).then_some(inserted_range);

        self.apply_edit(range, replacement, selected_range, false, marked_range)
    }
}

fn relative_inserted_selection(
    replacement: &str,
    inserted_start: usize,
    range: Option<Range<usize>>,
) -> Range<usize> {
    let relative = range
        .map(|range| normalize_range(replacement, range))
        .unwrap_or_else(|| replacement.len()..replacement.len());
    inserted_start + relative.start..inserted_start + relative.end
}
