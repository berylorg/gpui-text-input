use gpui_text_input::{
    BindingId, ByteRange, LogicalExtent, MutationError, MutationFragment, MutationFragmentPayload,
    MutationKey, MutationKind, MutationLimits, MutationOutcome, MutationProposal, OperationId,
    RangeBinding, RangeEditCoordinator, SourceRevision,
};

fn binding(revision: u64, bytes: u64, lines: u64) -> RangeBinding {
    RangeBinding::new(
        BindingId::new(70),
        SourceRevision::new(revision),
        LogicalExtent::new(bytes, lines),
    )
}

fn commit(
    base: RangeBinding,
    replacement: ByteRange,
    replacement_breaks: u64,
    inserted: &str,
    successor: RangeBinding,
) -> Result<(), MutationError> {
    let mut editor = RangeEditCoordinator::new(base, MutationLimits::new(3, 64).unwrap());
    let key = MutationKey::new(base.binding(), base.revision(), OperationId::new(1));
    editor.begin(MutationProposal::new(
        key,
        MutationKind::Edit,
        replacement,
        replacement_breaks,
    ))?;
    editor.accept_preflight(key)?;
    editor.stage(MutationFragment::new(
        key,
        0,
        MutationFragmentPayload::Utf8 {
            inserted_offset: 0,
            text: inserted.into(),
        },
    ))?;
    editor.stage(MutationFragment::new(
        key,
        1,
        MutationFragmentPayload::Terminal,
    ))?;
    editor.admit_commit(key)?;
    editor.settle(key, MutationOutcome::Committed(successor))?;
    Ok(())
}

#[test]
fn unchanged_noop_preserves_the_complete_extent() {
    commit(
        binding(1, 3, 1),
        ByteRange::from_u64(1, 1).unwrap(),
        0,
        "",
        binding(2, 3, 1),
    )
    .unwrap();
}

#[test]
fn newline_insert_delete_and_replacement_compute_exact_line_counts() {
    commit(
        binding(1, 3, 1),
        ByteRange::from_u64(1, 1).unwrap(),
        0,
        "\n",
        binding(2, 4, 2),
    )
    .unwrap();
    commit(
        binding(1, 3, 2),
        ByteRange::from_u64(1, 2).unwrap(),
        1,
        "",
        binding(2, 2, 1),
    )
    .unwrap();
    commit(
        binding(1, 3, 2),
        ByteRange::from_u64(1, 2).unwrap(),
        1,
        "\n\n",
        binding(2, 4, 3),
    )
    .unwrap();
}

#[test]
fn inserted_line_breaks_are_counted_across_ordered_utf8_fragments() {
    let base = binding(1, 1, 1);
    let mut editor = RangeEditCoordinator::new(base, MutationLimits::new(4, 8).unwrap());
    let key = MutationKey::new(base.binding(), base.revision(), OperationId::new(11));
    editor
        .begin(MutationProposal::new(
            key,
            MutationKind::Edit,
            ByteRange::from_u64(1, 1).unwrap(),
            0,
        ))
        .unwrap();
    editor.accept_preflight(key).unwrap();
    for (ordinal, offset, text) in [(0, 0, "\n"), (1, 1, "x\n")] {
        editor
            .stage(MutationFragment::new(
                key,
                ordinal,
                MutationFragmentPayload::Utf8 {
                    inserted_offset: offset,
                    text: text.into(),
                },
            ))
            .unwrap();
    }
    editor
        .stage(MutationFragment::new(
            key,
            2,
            MutationFragmentPayload::Terminal,
        ))
        .unwrap();
    editor.admit_commit(key).unwrap();
    editor
        .settle(key, MutationOutcome::Committed(binding(2, 4, 3)))
        .unwrap();
}

#[test]
fn empty_and_nonempty_transitions_use_zero_or_breaks_plus_one_lines() {
    commit(
        binding(1, 0, 0),
        ByteRange::from_u64(0, 0).unwrap(),
        0,
        "a\nb",
        binding(2, 3, 2),
    )
    .unwrap();
    commit(
        binding(1, 3, 2),
        ByteRange::from_u64(0, 3).unwrap(),
        1,
        "",
        binding(2, 0, 0),
    )
    .unwrap();
}

#[test]
fn malformed_removed_line_facts_and_base_extents_are_rejected_at_begin() {
    let limits = MutationLimits::new(2, 8).unwrap();
    let base = binding(1, 3, 2);
    for (operation, range, breaks) in [
        (1, ByteRange::from_u64(1, 1).unwrap(), 1),
        (2, ByteRange::from_u64(0, 1).unwrap(), 2),
        (3, ByteRange::from_u64(0, 3).unwrap(), 0),
    ] {
        let mut editor = RangeEditCoordinator::new(base, limits);
        let key = MutationKey::new(base.binding(), base.revision(), OperationId::new(operation));
        assert_eq!(
            editor.begin(MutationProposal::new(
                key,
                MutationKind::Edit,
                range,
                breaks,
            )),
            Err(MutationError::MalformedReplacementLineBreaks)
        );
    }

    let malformed = binding(1, 0, 1);
    let mut editor = RangeEditCoordinator::new(malformed, limits);
    let key = MutationKey::new(
        malformed.binding(),
        malformed.revision(),
        OperationId::new(4),
    );
    assert_eq!(
        editor.begin(MutationProposal::new(
            key,
            MutationKind::Edit,
            ByteRange::from_u64(0, 0).unwrap(),
            0,
        )),
        Err(MutationError::MalformedBaseExtent)
    );
}

#[test]
fn wrong_successor_line_count_is_rejected_without_settling() {
    let base = binding(1, 3, 1);
    let mut editor = RangeEditCoordinator::new(base, MutationLimits::new(3, 8).unwrap());
    let key = MutationKey::new(base.binding(), base.revision(), OperationId::new(9));
    editor
        .begin(MutationProposal::new(
            key,
            MutationKind::Edit,
            ByteRange::from_u64(1, 1).unwrap(),
            0,
        ))
        .unwrap();
    editor.accept_preflight(key).unwrap();
    editor
        .stage(MutationFragment::new(
            key,
            0,
            MutationFragmentPayload::Utf8 {
                inserted_offset: 0,
                text: "\n".into(),
            },
        ))
        .unwrap();
    editor
        .stage(MutationFragment::new(
            key,
            1,
            MutationFragmentPayload::Terminal,
        ))
        .unwrap();
    editor.admit_commit(key).unwrap();
    assert_eq!(
        editor.settle(key, MutationOutcome::Committed(binding(2, 4, 1))),
        Err(MutationError::IncoherentSuccessor)
    );
}
