use gpui_text_input::{
    TextInputAtom, TextInputAtomError, TextInputOptions, TextInputSelectionAtom, TextInputState,
};

#[test]
fn single_line_normalizes_inserted_newlines() {
    let mut input = TextInputState::new("", TextInputOptions::single_line());

    let change = input
        .paste("a\r\nb\nc\rd")
        .expect("paste should change the buffer");

    assert_eq!(input.text(), "a b c d");
    assert_eq!(change.replaced_range, 0..0);
    assert_eq!(change.replacement, "a b c d");
    assert_eq!(change.inserted_range, 0..7);
    assert_eq!(change.selection, 7..7);
    assert!(change.text_changed);
}

#[test]
fn multiline_normalizes_line_endings_and_preserves_newline_insertions() {
    let mut input = TextInputState::new("a\r\nb\rc", TextInputOptions::multiline());

    assert_eq!(input.text(), "a\nb\nc");
    input.move_to_offset(1);
    let change = input
        .insert_newline()
        .expect("multiline newline should be inserted");

    assert_eq!(input.text(), "a\n\nb\nc");
    assert_eq!(change.replaced_range, 1..1);
    assert_eq!(change.replacement, "\n");
    assert_eq!(change.selection, 2..2);
}

#[test]
fn grapheme_movement_and_deletion_do_not_split_clusters() {
    let emoji = "\u{1f468}\u{200d}\u{1f469}\u{200d}\u{1f467}\u{200d}\u{1f466}";
    let text = format!("a{emoji}b");
    let mut input = TextInputState::new(text, TextInputOptions::single_line());

    input.move_to_end();
    assert!(input.move_left());
    assert_eq!(input.cursor_offset(), 1 + emoji.len());
    assert!(input.move_left());
    assert_eq!(input.cursor_offset(), 1);

    input.move_to_offset(1 + emoji.len());
    input.backspace().expect("emoji cluster should be deleted");
    assert_eq!(input.text(), "ab");
}

#[test]
fn word_movement_and_deletion_use_shared_boundaries() {
    let mut input = TextInputState::new("one  two three", TextInputOptions::single_line());

    input.move_to_end();
    input.move_word_left();
    assert_eq!(input.cursor_offset(), "one  two ".len());
    input.move_word_left();
    assert_eq!(input.cursor_offset(), "one  ".len());

    input.move_to_end();
    input
        .delete_word_backward()
        .expect("last word should be deleted");
    assert_eq!(input.text(), "one  two ");
}

#[test]
fn selection_replacement_reports_precise_change() {
    let mut input = TextInputState::new("abc", TextInputOptions::single_line());

    input.set_selection(1..2, false);
    assert_eq!(input.selected_text().as_deref(), Some("b"));
    let change = input.paste("X").expect("selection should be replaced");

    assert_eq!(input.text(), "aXc");
    assert_eq!(change.replaced_range, 1..2);
    assert_eq!(change.replacement, "X");
    assert_eq!(change.inserted_range, 1..2);
    assert_eq!(change.selection, 2..2);
    assert!(change.text_changed);
}

#[test]
fn same_text_replacement_can_report_selection_change_without_text_change() {
    let mut input = TextInputState::new("abc", TextInputOptions::single_line());

    input.set_selection(1..2, false);
    let change = input
        .replace_text_in_range(None, "b")
        .expect("selection collapse is still state change");

    assert_eq!(input.text(), "abc");
    assert_eq!(input.selection(), 2..2);
    assert_eq!(change.replaced_range, 1..2);
    assert_eq!(change.replacement, "b");
    assert!(!change.text_changed);
}

#[test]
fn multiline_line_commands_use_logical_lines() {
    let mut input = TextInputState::new("alpha\nbeta\ngamma", TextInputOptions::multiline());

    input.move_to_offset("alpha\nbe".len());
    input.move_home();
    assert_eq!(input.cursor_offset(), "alpha\n".len());
    input.move_end();
    assert_eq!(input.cursor_offset(), "alpha\nbeta".len());

    input.select_line_at_offset("alpha\nb".len());
    assert_eq!(input.selection(), "alpha\n".len().."alpha\nbeta".len());
    assert_eq!(input.selected_text().as_deref(), Some("beta"));
}

#[test]
fn read_only_fields_keep_navigation_and_reject_destructive_edits() {
    let mut input = TextInputState::new("abc", TextInputOptions::single_line_read_only());

    assert!(input.move_left());
    assert_eq!(input.cursor_offset(), 2);
    assert!(input.select_all());
    assert_eq!(input.selected_text().as_deref(), Some("abc"));

    assert!(input.paste("x").is_none());
    assert!(input.backspace().is_none());
    assert!(input.cut_selection().is_none());
    assert_eq!(input.text(), "abc");
}

#[test]
fn undo_redo_restore_text_and_reset_clears_history() {
    let mut input = TextInputState::new("", TextInputOptions::single_line());

    input.paste("a").expect("first edit");
    input.paste("b").expect("second edit");
    assert_eq!(input.text(), "ab");

    let undo = input.undo().expect("undo should restore previous text");
    assert_eq!(input.text(), "a");
    assert_eq!(undo.replaced_range, 0..2);
    assert_eq!(undo.replacement, "a");
    assert!(undo.text_changed);

    input.redo().expect("redo should restore second edit");
    assert_eq!(input.text(), "ab");

    assert!(input.reset_text("z"));
    assert!(input.undo().is_none());
    assert_eq!(input.text(), "z");
}

#[test]
fn reset_text_clears_history_when_visible_state_is_unchanged() {
    let mut input = TextInputState::new("", TextInputOptions::single_line());

    input.paste("a").expect("edit should be recorded");
    assert_eq!(input.text(), "a");
    assert!(!input.reset_text("a"));

    assert!(input.undo().is_none());
    assert_eq!(input.text(), "a");
}

#[test]
fn marked_text_is_replaced_by_followup_edit() {
    let mut input = TextInputState::new("abc", TextInputOptions::single_line());

    input.set_selection(1..2, false);
    input
        .replace_and_mark_text_in_range(None, "x", Some(0..1))
        .expect("marked text should replace selection");

    assert_eq!(input.text(), "axc");
    assert_eq!(input.marked_range(), Some(1..2));
    assert_eq!(input.selection(), 1..2);

    input.paste("y").expect("marked text should be replaced");
    assert_eq!(input.text(), "ayc");
    assert_eq!(input.marked_range(), None);
    assert_eq!(input.selection(), 2..2);
}

#[test]
fn lib_docs_single_line_example_is_covered_by_nextest() {
    let mut input = TextInputState::new("", TextInputOptions::single_line());
    let change = input.paste("alpha\nbeta").expect("paste changes text");

    assert_eq!(input.text(), "alpha beta");
    assert_eq!(change.replacement, "alpha beta");
}

#[test]
fn lib_docs_multiline_example_is_covered_by_nextest() {
    let mut input = TextInputState::new("one\ntwo", TextInputOptions::multiline());
    input.move_to_end();
    input.insert_newline().expect("newline changes text");
    input.paste("three").expect("paste changes text");
    input.move_home();

    assert_eq!(input.text(), "one\ntwo\nthree");
    assert_eq!(input.selection(), 8..8);
}

#[test]
fn word_boundaries_handle_punctuation_and_whitespace_segments() {
    let mut input = TextInputState::new("one,  two", TextInputOptions::single_line());

    input.move_to_start();
    input.move_word_right();
    assert_eq!(input.cursor_offset(), "one".len());
    input.move_word_right();
    assert_eq!(input.cursor_offset(), "one,  ".len());
    input.select_word_at_offset("one".len());
    assert_eq!(input.selected_text().as_deref(), Some(","));
}

#[test]
fn undo_limit_truncates_old_history() {
    let mut input = TextInputState::new("", TextInputOptions::single_line().with_undo_limit(1));

    input.paste("a").expect("first edit");
    input.paste("b").expect("second edit");
    input.paste("c").expect("third edit");

    input.undo().expect("latest edit can be undone");
    assert_eq!(input.text(), "ab");
    assert!(input.undo().is_none());
}

#[test]
fn builder_read_only_option_rejects_multiline_edits() {
    let mut input = TextInputState::new("a\nb", TextInputOptions::multiline().with_read_only(true));

    input.move_to_start();
    input.select_to_end();

    assert_eq!(input.selected_text().as_deref(), Some("a\nb"));
    assert!(input.insert_newline().is_none());
    assert!(input.paste("c").is_none());
    assert_eq!(input.text(), "a\nb");
}

#[test]
fn marked_text_ranges_are_based_on_normalized_replacement_text() {
    let mut input = TextInputState::new("", TextInputOptions::single_line());

    input
        .replace_and_mark_text_in_range(None, "x\r\ny", Some(1..3))
        .expect("normalized marked text should be inserted");

    assert_eq!(input.text(), "x y");
    assert_eq!(input.marked_range(), Some(0..3));
    assert_eq!(input.selection(), 1..3);
}

#[test]
fn atoms_snap_navigation_and_delete_as_whole_ranges() {
    let mut input = TextInputState::new("x[A]y", TextInputOptions::single_line());
    input
        .set_atoms(vec![TextInputAtom::new(
            "atom-a",
            "x".len().."x[A]".len(),
            "[Attachment A]",
        )])
        .expect("atom range is valid");

    input.move_to_offset("x[A]".len());
    input.move_left();
    assert_eq!(input.cursor_offset(), "x".len());

    input.move_right();
    assert_eq!(input.cursor_offset(), "x[A]".len());

    input.backspace().expect("atom should be deleted as a unit");
    assert_eq!(input.text(), "xy");
    assert!(input.atoms().is_empty());
    assert_eq!(input.cursor_offset(), "x".len());
}

#[test]
fn atom_selection_export_uses_fallback_copy_text() {
    let mut input = TextInputState::new("See [A] now", TextInputOptions::single_line());
    let atom_range = "See ".len().."See [A]".len();
    input
        .set_atoms(vec![TextInputAtom::new(
            "atom-a",
            atom_range.clone(),
            "[Attachment A]",
        )])
        .expect("atom range is valid");
    input.select_all();

    let selection = input.selection_export().expect("selection should export");

    assert_eq!(selection.display_text(), "See [A] now");
    assert_eq!(selection.copy_text(), "See [Attachment A] now");
    assert_eq!(selection.atoms().len(), 1);
    assert_eq!(selection.atoms()[0].id(), "atom-a");
    assert_eq!(selection.atoms()[0].range(), atom_range);
    assert_eq!(selection.atoms()[0].display_text(), "[A]");
    assert_eq!(selection.atoms()[0].copy_text(), "[Attachment A]");
}

#[test]
fn atom_insertions_shift_existing_atoms_and_undo_restores_them() {
    let mut input = TextInputState::new("x[A]y", TextInputOptions::single_line());
    input
        .set_atoms(vec![TextInputAtom::new(
            "atom-a",
            "x".len().."x[A]".len(),
            "[Attachment A]",
        )])
        .expect("atom range is valid");

    input.move_to_offset(1);
    input
        .paste("!")
        .expect("plain text insert should shift atom");
    assert_eq!(input.text(), "x![A]y");
    assert_eq!(input.atoms()[0].range(), "x!".len().."x![A]".len());

    input.undo().expect("undo should restore atom range");
    assert_eq!(input.text(), "x[A]y");
    assert_eq!(input.atoms()[0].range(), "x".len().."x[A]".len());
}

#[test]
fn atom_selection_paste_validates_display_ranges() {
    let mut input = TextInputState::new("Start end", TextInputOptions::single_line());
    input.move_to_offset("Start ".len());

    input
        .replace_text_in_range_with_atoms(
            None,
            "[A]",
            vec![TextInputSelectionAtom::new(
                "atom-a",
                0..3,
                "[A]",
                "[Attachment A]",
            )],
        )
        .expect("atom paste should validate")
        .expect("atom paste should change text");

    assert_eq!(input.text(), "Start [A]end");
    assert_eq!(input.atoms()[0].range(), "Start ".len().."Start [A]".len());

    assert_eq!(
        input.replace_text_in_range_with_atoms(
            None,
            "[A]",
            vec![TextInputSelectionAtom::new(
                "atom-b",
                0..3,
                "[B]",
                "[Attachment B]",
            )],
        ),
        Err(TextInputAtomError::InvalidRange)
    );
}
