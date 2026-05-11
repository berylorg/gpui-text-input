use std::{collections::VecDeque, ops::Range};

use crate::atom::{
    OffsetAffinity, normalize_edit_range_with_atoms, normalize_selection_range_with_atoms,
    range_intersects_atoms, ranges_intersect, transform_atoms_after_edit, validate_atoms,
    validate_selection_atoms,
};
use crate::{
    TextInputAtom, TextInputAtomError, TextInputChange, TextInputMode, TextInputOptions,
    TextInputSelectionAtom, TextInputSelectionExport,
    boundary::{line_range_at, word_range_at},
    newline::normalize_text,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct EditSnapshot {
    pub(crate) text: String,
    pub(crate) selected_range: Range<usize>,
    pub(crate) selection_reversed: bool,
    pub(crate) marked_range: Option<Range<usize>>,
    pub(crate) atoms: Vec<TextInputAtom>,
}

/// Plain-text editing state shared by single-line and multiline text inputs.
#[derive(Clone, Debug)]
pub struct TextInputState {
    pub(crate) text: String,
    pub(crate) selected_range: Range<usize>,
    pub(crate) selection_reversed: bool,
    pub(crate) marked_range: Option<Range<usize>>,
    pub(crate) atoms: Vec<TextInputAtom>,
    pub(crate) options: TextInputOptions,
    pub(crate) undo_stack: VecDeque<EditSnapshot>,
    pub(crate) redo_stack: VecDeque<EditSnapshot>,
}

impl TextInputState {
    /// Creates a text input state with normalized initial text.
    pub fn new(initial_value: impl Into<String>, options: TextInputOptions) -> Self {
        let text = normalize_text(&initial_value.into(), options.mode());
        let cursor = text.len();

        Self {
            text,
            selected_range: cursor..cursor,
            selection_reversed: false,
            marked_range: None,
            atoms: Vec::new(),
            options,
            undo_stack: VecDeque::new(),
            redo_stack: VecDeque::new(),
        }
    }

    /// Returns the current plain text buffer.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Returns construction options for this state.
    pub fn options(&self) -> TextInputOptions {
        self.options
    }

    /// Returns the field layout mode.
    pub fn mode(&self) -> TextInputMode {
        self.options.mode()
    }

    /// Returns whether destructive edits are rejected.
    pub fn is_read_only(&self) -> bool {
        self.options.is_read_only()
    }

    /// Replaces the buffer from host state and clears transient edit history.
    pub fn reset_text(&mut self, text: impl Into<String>) -> bool {
        let text = normalize_text(&text.into(), self.options.mode());
        let cursor = text.len();
        self.undo_stack.clear();
        self.redo_stack.clear();

        if self.text == text
            && self.selected_range == (cursor..cursor)
            && !self.selection_reversed
            && self.marked_range.is_none()
            && self.atoms.is_empty()
        {
            return false;
        }

        self.text = text;
        self.selected_range = cursor..cursor;
        self.selection_reversed = false;
        self.marked_range = None;
        self.atoms.clear();
        true
    }

    /// Returns the current normalized selection range.
    pub fn selection(&self) -> Range<usize> {
        self.selected_range.clone()
    }

    /// Returns whether the active selection anchor is at the range end.
    pub fn selection_reversed(&self) -> bool {
        self.selection_reversed
    }

    /// Returns the current IME marked-text range, if any.
    pub fn marked_range(&self) -> Option<Range<usize>> {
        self.marked_range.clone()
    }

    /// Returns opaque inline atom ranges in display-text order.
    pub fn atoms(&self) -> &[TextInputAtom] {
        &self.atoms
    }

    /// Returns the atom containing `offset`, if any.
    pub fn atom_at_offset(&self, offset: usize) -> Option<&TextInputAtom> {
        let offset = crate::boundary::clamp_to_char_boundary(&self.text, offset);
        self.atoms
            .iter()
            .find(|atom| offset >= atom.range.start && offset <= atom.range.end)
    }

    /// Replaces the host-owned atom set without changing display text.
    pub fn set_atoms(
        &mut self,
        atoms: impl IntoIterator<Item = TextInputAtom>,
    ) -> Result<bool, TextInputAtomError> {
        let atoms = validate_atoms(self.text(), atoms)?;
        let selected_range = normalize_selection_range_with_atoms(
            self.selected_range.clone(),
            &self.text,
            &atoms,
            OffsetAffinity::Nearest,
        );
        let marked_range = self
            .marked_range
            .clone()
            .filter(|range| !range_intersects_atoms(range, &atoms));
        let changed = self.atoms != atoms
            || self.selected_range != selected_range
            || self.marked_range != marked_range;

        if changed {
            self.atoms = atoms;
            self.selected_range = selected_range;
            self.marked_range = marked_range;
        }

        Ok(changed)
    }

    /// Returns the active cursor offset.
    pub fn cursor_offset(&self) -> usize {
        if self.selection_reversed {
            self.selected_range.start
        } else {
            self.selected_range.end
        }
    }

    /// Returns selected fallback copy text when the selection is non-empty.
    pub fn selected_text(&self) -> Option<String> {
        self.selection_export()
            .map(|selection| selection.copy_text().to_string())
    }

    /// Returns display text, fallback copy text, and selected atom metadata.
    pub fn selection_export(&self) -> Option<TextInputSelectionExport> {
        let range = normalize_selection_range_with_atoms(
            self.selected_range.clone(),
            &self.text,
            &self.atoms,
            OffsetAffinity::Nearest,
        );
        (!range.is_empty()).then(|| self.export_range(&range))
    }

    /// Returns the word-boundary range at a byte offset.
    pub fn word_range_at_offset(&self, offset: usize) -> Option<Range<usize>> {
        word_range_at(&self.text, offset)
    }

    /// Returns the logical line range at a byte offset.
    pub fn line_range_at_offset(&self, offset: usize) -> Range<usize> {
        match self.options.mode() {
            TextInputMode::SingleLine => 0..self.text.len(),
            TextInputMode::Multiline => line_range_at(&self.text, offset),
        }
    }

    pub(crate) fn has_active_selection(&self) -> bool {
        !self.selected_range.is_empty() || self.marked_range.is_some()
    }

    pub(crate) fn active_edit_range(
        &self,
        replacement_range: Option<Range<usize>>,
    ) -> Range<usize> {
        let range = replacement_range
            .or_else(|| self.marked_range.clone())
            .unwrap_or_else(|| self.selected_range.clone());

        normalize_edit_range_with_atoms(range, &self.text, &self.atoms)
    }

    pub(crate) fn apply_edit(
        &mut self,
        range: Range<usize>,
        replacement: String,
        selected_range: Range<usize>,
        selection_reversed: bool,
        marked_range: Option<Range<usize>>,
    ) -> Option<TextInputChange> {
        self.apply_edit_with_inserted_atoms(
            range,
            replacement,
            selected_range,
            selection_reversed,
            marked_range,
            Vec::new(),
        )
    }

    pub(crate) fn apply_edit_with_inserted_atoms(
        &mut self,
        range: Range<usize>,
        replacement: String,
        selected_range: Range<usize>,
        selection_reversed: bool,
        marked_range: Option<Range<usize>>,
        inserted_atoms: Vec<TextInputAtom>,
    ) -> Option<TextInputChange> {
        let range = normalize_edit_range_with_atoms(range, &self.text, &self.atoms);
        let next_atoms =
            transform_atoms_after_edit(&self.atoms, &range, replacement.len(), inserted_atoms);
        let mut next_text =
            String::with_capacity(self.text.len() - range.len() + replacement.len());
        next_text.push_str(&self.text[..range.start]);
        next_text.push_str(&replacement);
        next_text.push_str(&self.text[range.end..]);

        let selected_range = normalize_selection_range_with_atoms(
            selected_range,
            &next_text,
            &next_atoms,
            OffsetAffinity::Nearest,
        );
        let marked_range = marked_range
            .map(|range| {
                normalize_selection_range_with_atoms(
                    range,
                    &next_text,
                    &next_atoms,
                    OffsetAffinity::Nearest,
                )
            })
            .filter(|range| !range_intersects_atoms(range, &next_atoms));
        let text_changed = self.text != next_text;

        if !text_changed
            && self.selected_range == selected_range
            && self.selection_reversed == selection_reversed
            && self.marked_range == marked_range
            && self.atoms == next_atoms
        {
            return None;
        }

        let change = TextInputChange::new(range, replacement, selected_range.clone(), text_changed);
        let current = self.snapshot();
        self.push_undo_snapshot(current);
        self.redo_stack.clear();
        self.text = next_text;
        self.selected_range = selected_range;
        self.selection_reversed = selection_reversed;
        self.marked_range = marked_range;
        self.atoms = next_atoms;
        Some(change)
    }

    pub(crate) fn snapshot(&self) -> EditSnapshot {
        EditSnapshot {
            text: self.text.clone(),
            selected_range: self.selected_range.clone(),
            selection_reversed: self.selection_reversed,
            marked_range: self.marked_range.clone(),
            atoms: self.atoms.clone(),
        }
    }

    pub(crate) fn restore(&mut self, snapshot: EditSnapshot) {
        self.text = snapshot.text;
        self.selected_range = snapshot.selected_range;
        self.selection_reversed = snapshot.selection_reversed;
        self.marked_range = snapshot.marked_range;
        self.atoms = snapshot.atoms;
    }

    pub(crate) fn selection_atoms_for_display_text(
        range_start: usize,
        normalized_text: &str,
        atoms: impl IntoIterator<Item = TextInputSelectionAtom>,
    ) -> Result<Vec<TextInputAtom>, TextInputAtomError> {
        Ok(validate_selection_atoms(normalized_text, atoms)?
            .into_iter()
            .map(|atom| {
                TextInputAtom::new(
                    atom.id,
                    range_start + atom.range.start..range_start + atom.range.end,
                    atom.copy_text,
                )
            })
            .collect())
    }

    fn export_range(&self, range: &Range<usize>) -> TextInputSelectionExport {
        let range = normalize_selection_range_with_atoms(
            range.clone(),
            &self.text,
            &self.atoms,
            OffsetAffinity::Nearest,
        );
        let display_text = self.text[range.clone()].to_string();
        let mut copied = String::new();
        let mut selected_atoms = Vec::new();
        let mut cursor = range.start;

        for atom in &self.atoms {
            if !ranges_intersect(&range, &atom.range) {
                continue;
            }
            if cursor < atom.range.start {
                copied.push_str(&self.text[cursor..atom.range.start]);
            }
            copied.push_str(&atom.copy_text);
            selected_atoms.push(TextInputSelectionAtom::new(
                atom.id.clone(),
                atom.range.start - range.start..atom.range.end - range.start,
                self.text[atom.range.clone()].to_string(),
                atom.copy_text.clone(),
            ));
            cursor = atom.range.end;
        }

        if cursor < range.end {
            copied.push_str(&self.text[cursor..range.end]);
        }

        TextInputSelectionExport::new(display_text, copied, selected_atoms)
    }

    pub(crate) fn push_undo_snapshot(&mut self, snapshot: EditSnapshot) {
        Self::push_snapshot(&mut self.undo_stack, snapshot, self.options.undo_limit());
    }

    pub(crate) fn push_redo_snapshot(&mut self, snapshot: EditSnapshot) {
        Self::push_snapshot(&mut self.redo_stack, snapshot, self.options.undo_limit());
    }

    fn push_snapshot(stack: &mut VecDeque<EditSnapshot>, snapshot: EditSnapshot, limit: usize) {
        if limit == 0 || stack.back() == Some(&snapshot) {
            return;
        }

        stack.push_back(snapshot);
        while stack.len() > limit {
            stack.pop_front();
        }
    }
}
