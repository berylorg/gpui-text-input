use gpui_text_input::{TextInputAtom, TextInputOptions, TextInputState, TextInputValueLimitError};

fn facts(
    input: &TextInputState,
) -> (
    String,
    std::ops::Range<usize>,
    bool,
    Option<std::ops::Range<usize>>,
    Vec<TextInputAtom>,
    gpui_text_input::TextInputRetainedCounts,
) {
    (
        input.text().to_owned(),
        input.selection(),
        input.selection_reversed(),
        input.marked_range(),
        input.atoms().to_vec(),
        input.retained_counts(),
    )
}

#[test]
fn configuration_is_optional_fallible_and_zero_allows_only_empty_values() {
    let mut input = TextInputState::new("é🌳", TextInputOptions::multiline());
    assert_eq!(input.value_byte_limit(), None);
    input.set_value_byte_limit(Some(6)).unwrap();
    let before = facts(&input);
    assert_eq!(
        input.set_value_byte_limit(Some(5)),
        Err(TextInputValueLimitError {
            limit: 5,
            attempted_bytes: 6
        })
    );
    assert_eq!(input.value_byte_limit(), Some(6));
    assert_eq!(facts(&input), before);
    assert!(input.reset_text(""));
    input.set_value_byte_limit(Some(0)).unwrap();
    assert!(input.paste("\r\n").is_none());
    assert_eq!(
        input.take_value_limit_error(),
        Some(TextInputValueLimitError {
            limit: 0,
            attempted_bytes: 1
        })
    );
    assert_eq!(input.text(), "");
    input.set_value_byte_limit(None).unwrap();
    assert!(input.paste(&"é".repeat(40_000)).is_some());
}

#[test]
fn complete_normalized_utf8_value_determines_the_boundary() {
    for (options, expected) in [
        (TextInputOptions::single_line(), "é 🌳"),
        (TextInputOptions::multiline(), "é\n\n🌳"),
    ] {
        let mut input = TextInputState::new("", options);
        input.set_value_byte_limit(Some(expected.len())).unwrap();
        assert!(input.paste("é\r\n\n🌳").is_some());
        assert_eq!(input.text(), expected);
        let before = facts(&input);
        assert!(input.paste("x").is_none());
        assert_eq!(facts(&input), before);
        assert_eq!(
            input.take_value_limit_error().unwrap().attempted_bytes,
            expected.len() + 1
        );
        input.set_selection(0..2, true);
        assert!(input.paste("ab").is_some());
        assert_eq!(input.text().len(), expected.len());
    }
    for (text, range) in [("ab🌳cd", 2..6), ("abTREEcd", 3..4)] {
        let mut input = TextInputState::new(text, TextInputOptions::single_line());
        input
            .set_atoms([TextInputAtom::new("atom", 2..6, "tree")])
            .unwrap();
        input.set_value_byte_limit(Some(8)).unwrap();
        let before = facts(&input);
        assert!(
            input
                .replace_text_in_range(Some(range.clone()), "12345")
                .is_none()
        );
        assert_eq!(facts(&input), before);
        assert_eq!(input.take_value_limit_error().unwrap().attempted_bytes, 9);
        assert!(input.replace_text_in_range(Some(range), "1234").is_some());
        assert_eq!(input.text(), "ab1234cd");
    }
}

#[test]
fn rejected_replacements_preserve_composition_directed_selection_atoms_and_history() {
    let mut input = TextInputState::new("a🌳bc", TextInputOptions::multiline());
    input
        .set_atoms([TextInputAtom::new("atom", 1..5, "tree")])
        .unwrap();
    input
        .replace_and_mark_text_in_range(Some(5..6), "候", Some(0..0))
        .unwrap();
    input.set_selection(0..1, true);
    let limit = input.text().len();
    input.set_value_byte_limit(Some(limit)).unwrap();
    let before = facts(&input);
    let too_large = "🌳".repeat(20_000);
    for _ in 0..8 {
        assert!(input.paste(&too_large).is_none());
        assert_eq!(facts(&input), before);
        assert!(input.insert_text_at_offset(0, &too_large).is_none());
        assert_eq!(facts(&input), before);
        assert!(
            input
                .replace_text_in_range(Some(1..5), &too_large)
                .is_none()
        );
        assert_eq!(facts(&input), before);
        assert!(
            input
                .replace_and_mark_text_in_range(None, &too_large, Some(1..2))
                .is_none()
        );
        assert_eq!(facts(&input), before);
        assert!(
            input
                .replace_text_in_range_with_atom(None, &too_large, "new", "copy")
                .unwrap()
                .is_none()
        );
        assert_eq!(facts(&input), before);
        let unconsumed_atoms =
            std::iter::from_fn(|| -> Option<gpui_text_input::TextInputSelectionAtom> {
                panic!("rejected insertion must not consume atom metadata")
            });
        assert!(
            input
                .replace_text_in_range_with_atoms(None, &too_large, unconsumed_atoms)
                .unwrap()
                .is_none()
        );
        assert_eq!(facts(&input), before);
        assert!(!input.reset_text(too_large.clone()));
        assert_eq!(facts(&input), before);
        let refusal = input.take_value_limit_error().unwrap();
        assert_eq!(refusal.limit, limit);
        assert_eq!(refusal.attempted_bytes, too_large.len());
        assert!(input.take_value_limit_error().is_none());
    }
    assert!(input.paste("x").is_some());
    assert_eq!(input.marked_range(), None);
}

#[test]
fn tightened_limits_refuse_history_without_popping_or_reordering_it() {
    let mut input = TextInputState::new("large", TextInputOptions::multiline());
    input.set_selection(0..5, true);
    input.paste("x").unwrap();
    input.set_value_byte_limit(Some(1)).unwrap();
    let before = facts(&input);
    for _ in 0..3 {
        assert!(input.undo().is_none());
        assert_eq!(facts(&input), before);
        assert_eq!(input.take_value_limit_error().unwrap().attempted_bytes, 5);
    }
    input.set_value_byte_limit(None).unwrap();
    input.undo().unwrap();
    assert_eq!(input.text(), "large");
    assert_eq!(input.selection(), 0..5);
    assert!(input.selection_reversed());
    input.redo().unwrap();
    assert_eq!(input.text(), "x");

    let mut input = TextInputState::new("", TextInputOptions::single_line());
    input.paste("large").unwrap();
    input.undo().unwrap();
    input.set_value_byte_limit(Some(0)).unwrap();
    let before = facts(&input);
    assert!(input.redo().is_none());
    assert_eq!(facts(&input), before);
    input.set_value_byte_limit(Some(5)).unwrap();
    input.redo().unwrap();
    assert_eq!(input.text(), "large");
}

#[test]
fn dynamic_read_only_preserves_composition_and_resumes_the_same_history() {
    let mut input = TextInputState::new("abc", TextInputOptions::multiline());
    input
        .replace_and_mark_text_in_range(Some(1..2), "候", Some(0..0))
        .unwrap();
    let before = facts(&input);
    input.set_read_only(true);
    assert!(input.paste("x").is_none());
    assert!(input.undo().is_none());
    assert!(
        input
            .replace_and_mark_text_in_range(None, "x", None)
            .is_none()
    );
    assert!(!input.unmark_text());
    assert_eq!(facts(&input), before);
    assert!(input.take_value_limit_error().is_none());
    input.set_read_only(false);
    input.undo().unwrap();
    assert_eq!(input.text(), "abc");
    input.set_read_only(true);
    input.set_value_byte_limit(Some(3)).unwrap();
    assert!(!input.reset_text("oversize"));
    assert!(input.reset_text("ok"));
    assert_eq!(input.text(), "ok");
}
