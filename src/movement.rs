use std::ops::Range;

use crate::{
    TextInputMode, TextInputState,
    atom::{OffsetAffinity, normalize_selection_range_with_atoms, snap_offset_to_atoms},
    boundary::{
        line_end_at, line_start_at, next_grapheme_boundary, next_word_boundary,
        previous_grapheme_boundary, previous_word_boundary,
    },
};

impl TextInputState {
    /// Places the cursor at `offset`.
    pub fn move_to_offset(&mut self, offset: usize) -> bool {
        let offset = snap_offset_to_atoms(&self.text, &self.atoms, offset, OffsetAffinity::Nearest);
        self.set_selection(offset..offset, false)
    }

    /// Extends or shrinks the current selection to `offset`.
    pub fn select_to_offset(&mut self, offset: usize) -> bool {
        let offset = snap_offset_to_atoms(&self.text, &self.atoms, offset, OffsetAffinity::Nearest);
        let mut next_range = self.selected_range.clone();
        let mut next_reversed = self.selection_reversed;

        if next_reversed {
            next_range.start = offset;
        } else {
            next_range.end = offset;
        }

        if next_range.end < next_range.start {
            next_reversed = !next_reversed;
            next_range = next_range.end..next_range.start;
        }

        self.set_selection(next_range, next_reversed)
    }

    /// Sets the selection directly, normalizing to valid text boundaries.
    pub fn set_selection(&mut self, range: Range<usize>, reversed: bool) -> bool {
        let range = normalize_selection_range_with_atoms(
            range,
            &self.text,
            &self.atoms,
            OffsetAffinity::Nearest,
        );
        if self.selected_range == range
            && self.selection_reversed == reversed
            && self.marked_range.is_none()
        {
            return false;
        }

        self.selected_range = range;
        self.selection_reversed = reversed;
        self.marked_range = None;
        true
    }

    /// Moves left by one grapheme, or collapses a selection to its start.
    pub fn move_left(&mut self) -> bool {
        if !self.selected_range.is_empty() {
            return self.move_to_offset(self.selected_range.start);
        }

        let offset = previous_grapheme_boundary(&self.text, self.cursor_offset());
        self.move_to_offset(snap_offset_to_atoms(
            &self.text,
            &self.atoms,
            offset,
            OffsetAffinity::Backward,
        ))
    }

    /// Moves right by one grapheme, or collapses a selection to its end.
    pub fn move_right(&mut self) -> bool {
        if !self.selected_range.is_empty() {
            return self.move_to_offset(self.selected_range.end);
        }

        let offset = next_grapheme_boundary(&self.text, self.cursor_offset());
        self.move_to_offset(snap_offset_to_atoms(
            &self.text,
            &self.atoms,
            offset,
            OffsetAffinity::Forward,
        ))
    }

    /// Extends selection left by one grapheme.
    pub fn select_left(&mut self) -> bool {
        let offset = previous_grapheme_boundary(&self.text, self.cursor_offset());
        self.select_to_offset(snap_offset_to_atoms(
            &self.text,
            &self.atoms,
            offset,
            OffsetAffinity::Backward,
        ))
    }

    /// Extends selection right by one grapheme.
    pub fn select_right(&mut self) -> bool {
        let offset = next_grapheme_boundary(&self.text, self.cursor_offset());
        self.select_to_offset(snap_offset_to_atoms(
            &self.text,
            &self.atoms,
            offset,
            OffsetAffinity::Forward,
        ))
    }

    /// Moves left to the previous word boundary.
    pub fn move_word_left(&mut self) -> bool {
        if !self.selected_range.is_empty() {
            return self.move_to_offset(self.selected_range.start);
        }

        let offset = previous_word_boundary(&self.text, self.cursor_offset());
        self.move_to_offset(snap_offset_to_atoms(
            &self.text,
            &self.atoms,
            offset,
            OffsetAffinity::Backward,
        ))
    }

    /// Moves right to the next word boundary.
    pub fn move_word_right(&mut self) -> bool {
        if !self.selected_range.is_empty() {
            return self.move_to_offset(self.selected_range.end);
        }

        let offset = next_word_boundary(&self.text, self.cursor_offset());
        self.move_to_offset(snap_offset_to_atoms(
            &self.text,
            &self.atoms,
            offset,
            OffsetAffinity::Forward,
        ))
    }

    /// Extends selection left to the previous word boundary.
    pub fn select_word_left(&mut self) -> bool {
        let offset = previous_word_boundary(&self.text, self.cursor_offset());
        self.select_to_offset(snap_offset_to_atoms(
            &self.text,
            &self.atoms,
            offset,
            OffsetAffinity::Backward,
        ))
    }

    /// Extends selection right to the next word boundary.
    pub fn select_word_right(&mut self) -> bool {
        let offset = next_word_boundary(&self.text, self.cursor_offset());
        self.select_to_offset(snap_offset_to_atoms(
            &self.text,
            &self.atoms,
            offset,
            OffsetAffinity::Forward,
        ))
    }

    /// Moves to the start of the current logical line.
    pub fn move_home(&mut self) -> bool {
        self.move_to_offset(self.line_start())
    }

    /// Moves to the end of the current logical line.
    pub fn move_end(&mut self) -> bool {
        self.move_to_offset(self.line_end())
    }

    /// Extends selection to the start of the current logical line.
    pub fn select_home(&mut self) -> bool {
        self.select_to_offset(self.line_start())
    }

    /// Extends selection to the end of the current logical line.
    pub fn select_end(&mut self) -> bool {
        self.select_to_offset(self.line_end())
    }

    /// Moves to the start of the buffer.
    pub fn move_to_start(&mut self) -> bool {
        self.move_to_offset(0)
    }

    /// Moves to the end of the buffer.
    pub fn move_to_end(&mut self) -> bool {
        self.move_to_offset(self.text.len())
    }

    /// Extends selection to the start of the buffer.
    pub fn select_to_start(&mut self) -> bool {
        self.select_to_offset(0)
    }

    /// Extends selection to the end of the buffer.
    pub fn select_to_end(&mut self) -> bool {
        self.select_to_offset(self.text.len())
    }

    /// Selects the entire buffer.
    pub fn select_all(&mut self) -> bool {
        self.set_selection(0..self.text.len(), false)
    }

    /// Selects the word-like segment at `offset`.
    pub fn select_word_at_offset(&mut self, offset: usize) -> bool {
        let Some(range) = self.word_range_at_offset(offset) else {
            return self.move_to_offset(0);
        };

        self.set_selection(range, false)
    }

    /// Selects the logical line at `offset`.
    pub fn select_line_at_offset(&mut self, offset: usize) -> bool {
        let range = self.line_range_at_offset(offset);
        self.set_selection(range, false)
    }

    fn line_start(&self) -> usize {
        match self.options.mode() {
            TextInputMode::SingleLine => 0,
            TextInputMode::Multiline => line_start_at(&self.text, self.cursor_offset()),
        }
    }

    fn line_end(&self) -> usize {
        match self.options.mode() {
            TextInputMode::SingleLine => self.text.len(),
            TextInputMode::Multiline => line_end_at(&self.text, self.cursor_offset()),
        }
    }
}
