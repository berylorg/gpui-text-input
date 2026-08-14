use gpui_text_input::{
    AtomChange, AtomId, BindingId, ByteRange, LogicalExtent, MutationError, MutationFragment,
    MutationFragmentPayload, MutationKey, MutationKind, MutationLimits, MutationProposal,
    OperationId, RangeBinding, RangeEditCoordinator, SourceRevision,
};

fn editor(operation: u64) -> (RangeEditCoordinator, MutationKey) {
    let binding = RangeBinding::new(
        BindingId::new(80),
        SourceRevision::new(1),
        LogicalExtent::new(20, 1),
    );
    let key = MutationKey::new(
        binding.binding(),
        binding.revision(),
        OperationId::new(operation),
    );
    let mut editor = RangeEditCoordinator::new(binding, MutationLimits::new(12, 128).unwrap());
    editor
        .begin(MutationProposal::new(
            key,
            MutationKind::Edit,
            ByteRange::from_u64(0, 20).unwrap(),
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
                text: "abcdefghijklmnopqrst".into(),
            },
        ))
        .unwrap();
    (editor, key)
}

fn insert(id: u64, start: u64, end: u64) -> MutationFragmentPayload {
    MutationFragmentPayload::Atom(AtomChange::Insert {
        id: AtomId::new(id),
        inserted_range: ByteRange::from_u64(start, end).unwrap(),
        fallback_copy: format!("atom-{id}"),
    })
}

fn remove(id: u64, start: u64, end: u64) -> MutationFragmentPayload {
    MutationFragmentPayload::Atom(AtomChange::Remove {
        id: AtomId::new(id),
        source_range: ByteRange::from_u64(start, end).unwrap(),
    })
}

#[test]
fn ordered_atom_sets_allow_one_remove_and_insert_of_the_same_id_as_a_move() {
    let (mut editor, key) = editor(1);
    for (ordinal, payload) in [
        insert(1, 0, 2),
        insert(2, 2, 4),
        remove(1, 5, 7),
        remove(3, 7, 9),
    ]
    .into_iter()
    .enumerate()
    {
        editor
            .stage(MutationFragment::new(key, ordinal + 1, payload))
            .unwrap();
    }
    assert_eq!(editor.staged_fragments().len(), 5);
}

#[test]
fn duplicate_insert_ids_are_rejected_atomically() {
    let (mut editor, key) = editor(2);
    editor
        .stage(MutationFragment::new(key, 1, insert(1, 0, 2)))
        .unwrap();
    assert_eq!(
        editor.stage(MutationFragment::new(key, 2, insert(1, 2, 4))),
        Err(MutationError::DuplicateAtomInsert(AtomId::new(1)))
    );
    assert_eq!(editor.staged_fragments().len(), 2);
    editor
        .stage(MutationFragment::new(key, 2, insert(2, 2, 4)))
        .unwrap();
}

#[test]
fn duplicate_remove_ids_and_ranges_are_rejected() {
    let (mut editor, key) = editor(3);
    editor
        .stage(MutationFragment::new(key, 1, remove(1, 0, 2)))
        .unwrap();
    assert_eq!(
        editor.stage(MutationFragment::new(key, 2, remove(1, 2, 4))),
        Err(MutationError::DuplicateAtomRemove(AtomId::new(1)))
    );
    assert_eq!(
        editor.stage(MutationFragment::new(key, 2, remove(2, 0, 2))),
        Err(MutationError::DuplicateAtomRemoveRange(
            ByteRange::from_u64(0, 2).unwrap()
        ))
    );
}

#[test]
fn inserted_atom_overlap_and_reversal_are_rejected() {
    for (operation, second) in [(4, insert(2, 4, 7)), (5, insert(2, 1, 3))] {
        let (mut editor, key) = editor(operation);
        let first = ByteRange::from_u64(2, 5).unwrap();
        editor
            .stage(MutationFragment::new(key, 1, insert(1, 2, 5)))
            .unwrap();
        assert!(matches!(
            editor.stage(MutationFragment::new(key, 2, second)),
            Err(MutationError::InsertedAtomRangeOutOfOrder { previous, .. }) if previous == first
        ));
    }
}

#[test]
fn removed_atom_overlap_and_reversal_are_rejected() {
    for (operation, second) in [(6, remove(2, 4, 7)), (7, remove(2, 1, 3))] {
        let (mut editor, key) = editor(operation);
        let first = ByteRange::from_u64(2, 5).unwrap();
        editor
            .stage(MutationFragment::new(key, 1, remove(1, 2, 5)))
            .unwrap();
        assert!(matches!(
            editor.stage(MutationFragment::new(key, 2, second)),
            Err(MutationError::RemovedAtomRangeOutOfOrder { previous, .. }) if previous == first
        ));
    }
}

#[test]
fn malformed_empty_or_outside_atom_ranges_remain_atomic() {
    let (mut editor, key) = editor(8);
    assert_eq!(
        editor.stage(MutationFragment::new(key, 1, insert(1, 2, 2))),
        Err(MutationError::MalformedAtomChange)
    );
    assert_eq!(
        editor.stage(MutationFragment::new(key, 1, remove(1, 20, 20))),
        Err(MutationError::MalformedAtomChange)
    );
    assert_eq!(editor.staged_fragments().len(), 1);
}
