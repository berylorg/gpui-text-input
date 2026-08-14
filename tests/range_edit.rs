use gpui_text_input::{
    AtomChange, AtomId, BindingId, ByteRange, LogicalExtent, MutationCancellation,
    MutationDisposal, MutationError, MutationFragment, MutationFragmentPayload, MutationKey,
    MutationKind, MutationLimits, MutationOutcome, MutationProposal, MutationSettlement,
    MutationState, OperationId, RangeBinding, RangeEditCoordinator, SourceRevision,
};

fn binding(revision: u64, bytes: u64) -> RangeBinding {
    RangeBinding::new(
        BindingId::new(7),
        SourceRevision::new(revision),
        LogicalExtent::new(bytes, u64::from(bytes != 0)),
    )
}

fn key(revision: u64, operation: u64) -> MutationKey {
    MutationKey::new(
        BindingId::new(7),
        SourceRevision::new(revision),
        OperationId::new(operation),
    )
}

fn coordinator(revision: u64, bytes: u64) -> RangeEditCoordinator {
    RangeEditCoordinator::new(
        binding(revision, bytes),
        MutationLimits::new(8, 64).unwrap(),
    )
}

fn begin_staging(
    editor: &mut RangeEditCoordinator,
    key: MutationKey,
    kind: MutationKind,
    range: ByteRange,
) {
    editor
        .begin(MutationProposal::new(key, kind, range, 0))
        .unwrap();
    assert_eq!(editor.state(), MutationState::Preflight);
    editor.accept_preflight(key).unwrap();
}

fn terminal(editor: &mut RangeEditCoordinator, key: MutationKey, ordinal: usize) {
    editor
        .stage(MutationFragment::new(
            key,
            ordinal,
            MutationFragmentPayload::Terminal,
        ))
        .unwrap();
}

#[test]
fn edit_stages_in_order_and_adopts_only_a_coherent_successor() {
    let mut editor = coordinator(1, 5);
    let key = key(1, 10);
    begin_staging(
        &mut editor,
        key,
        MutationKind::Edit,
        ByteRange::from_u64(1, 4).unwrap(),
    );
    editor
        .stage(MutationFragment::new(
            key,
            0,
            MutationFragmentPayload::Utf8 {
                inserted_offset: 0,
                text: "hello".into(),
            },
        ))
        .unwrap();
    editor
        .stage(MutationFragment::new(
            key,
            1,
            MutationFragmentPayload::Atom(AtomChange::Insert {
                id: AtomId::new(2),
                inserted_range: ByteRange::from_u64(0, 5).unwrap(),
                fallback_copy: "fallback".into(),
            }),
        ))
        .unwrap();
    terminal(&mut editor, key, 2);
    assert_eq!(editor.staged_fragments().len(), 3);
    assert_eq!(editor.counts().staged_bytes, 13);
    editor.admit_commit(key).unwrap();
    let successor = binding(2, 7);
    assert_eq!(
        editor
            .settle(key, MutationOutcome::Committed(successor))
            .unwrap(),
        MutationSettlement::Current(MutationOutcome::Committed(successor))
    );
    assert_eq!(editor.binding(), successor);
    assert_eq!(editor.counts().staged_bytes, 0);
    assert_eq!(
        editor.settle(key, MutationOutcome::Rejected),
        Err(MutationError::ObsoleteOperation(key))
    );
}

#[test]
fn undo_and_redo_use_the_same_exclusive_transaction_boundary() {
    let mut editor = coordinator(1, 3);
    let undo = key(1, 1);
    begin_staging(
        &mut editor,
        undo,
        MutationKind::Undo,
        ByteRange::from_u64(0, 3).unwrap(),
    );
    assert_eq!(
        editor.begin(MutationProposal::new(
            key(1, 2),
            MutationKind::Redo,
            ByteRange::from_u64(0, 0).unwrap(),
            0,
        )),
        Err(MutationError::Busy(undo))
    );
    terminal(&mut editor, undo, 0);
    editor.admit_commit(undo).unwrap();
    let after_undo = binding(2, 0);
    editor
        .settle(undo, MutationOutcome::Committed(after_undo))
        .unwrap();

    let redo = key(2, 2);
    begin_staging(
        &mut editor,
        redo,
        MutationKind::Redo,
        ByteRange::from_u64(0, 0).unwrap(),
    );
    editor
        .stage(MutationFragment::new(
            redo,
            0,
            MutationFragmentPayload::Utf8 {
                inserted_offset: 0,
                text: "abc".into(),
            },
        ))
        .unwrap();
    terminal(&mut editor, redo, 1);
    editor.admit_commit(redo).unwrap();
    editor
        .settle(redo, MutationOutcome::Committed(binding(3, 3)))
        .unwrap();
}

#[test]
fn empty_insert_and_empty_replacement_are_checked_exactly() {
    let mut deletion = coordinator(1, 4);
    let delete = key(1, 1);
    begin_staging(
        &mut deletion,
        delete,
        MutationKind::Edit,
        ByteRange::from_u64(1, 4).unwrap(),
    );
    terminal(&mut deletion, delete, 0);
    deletion.admit_commit(delete).unwrap();
    deletion
        .settle(delete, MutationOutcome::Committed(binding(2, 1)))
        .unwrap();

    let mut insertion = coordinator(1, 4);
    let insert = key(1, 2);
    begin_staging(
        &mut insertion,
        insert,
        MutationKind::Edit,
        ByteRange::from_u64(2, 2).unwrap(),
    );
    insertion
        .stage(MutationFragment::new(
            insert,
            0,
            MutationFragmentPayload::Utf8 {
                inserted_offset: 0,
                text: String::new(),
            },
        ))
        .unwrap();
    terminal(&mut insertion, insert, 1);
    insertion.admit_commit(insert).unwrap();
    insertion
        .settle(insert, MutationOutcome::Committed(binding(2, 4)))
        .unwrap();
}

#[test]
fn fragment_order_terminal_and_capacity_are_enforced() {
    let mut editor = RangeEditCoordinator::new(binding(1, 4), MutationLimits::new(2, 3).unwrap());
    let admitted_key = key(1, 1);
    begin_staging(
        &mut editor,
        admitted_key,
        MutationKind::Edit,
        ByteRange::from_u64(0, 0).unwrap(),
    );
    assert_eq!(
        editor.stage(MutationFragment::new(
            admitted_key,
            1,
            MutationFragmentPayload::Terminal
        )),
        Err(MutationError::FragmentOutOfOrder {
            expected: 0,
            actual: 1
        })
    );
    assert_eq!(
        editor.stage(MutationFragment::new(
            admitted_key,
            0,
            MutationFragmentPayload::Utf8 {
                inserted_offset: 1,
                text: "a".into()
            }
        )),
        Err(MutationError::InsertOffsetMismatch {
            expected: 0,
            actual: 1
        })
    );
    assert_eq!(
        editor.stage(MutationFragment::new(
            admitted_key,
            0,
            MutationFragmentPayload::Utf8 {
                inserted_offset: 0,
                text: "four".into()
            }
        )),
        Err(MutationError::StagedByteLimitExceeded)
    );
    editor
        .stage(MutationFragment::new(
            admitted_key,
            0,
            MutationFragmentPayload::Utf8 {
                inserted_offset: 0,
                text: "abc".into(),
            },
        ))
        .unwrap();
    assert_eq!(
        editor.admit_commit(admitted_key),
        Err(MutationError::MissingTerminalFragment)
    );
    terminal(&mut editor, admitted_key, 1);
    assert_eq!(
        editor.stage(MutationFragment::new(
            admitted_key,
            2,
            MutationFragmentPayload::Terminal
        )),
        Err(MutationError::PostTerminalFragment)
    );
}

#[test]
fn fragment_count_limit_and_staged_host_rejection_release_the_transaction() {
    let mut limited = RangeEditCoordinator::new(binding(1, 1), MutationLimits::new(1, 8).unwrap());
    let limited_key = key(1, 40);
    begin_staging(
        &mut limited,
        limited_key,
        MutationKind::Edit,
        ByteRange::from_u64(0, 0).unwrap(),
    );
    limited
        .stage(MutationFragment::new(
            limited_key,
            0,
            MutationFragmentPayload::Utf8 {
                inserted_offset: 0,
                text: "x".into(),
            },
        ))
        .unwrap();
    assert_eq!(
        limited.stage(MutationFragment::new(
            limited_key,
            1,
            MutationFragmentPayload::Terminal,
        )),
        Err(MutationError::FragmentLimitExceeded)
    );
    assert_eq!(
        limited.reject_staging(limited_key).unwrap(),
        MutationSettlement::Current(MutationOutcome::Rejected)
    );
    assert_eq!(limited.counts(), Default::default());
}

#[test]
fn cancellation_before_admission_is_terminal_but_after_admission_waits_for_host() {
    let mut before = coordinator(1, 2);
    let before_key = key(1, 1);
    before
        .begin(MutationProposal::new(
            before_key,
            MutationKind::Edit,
            ByteRange::from_u64(0, 0).unwrap(),
            0,
        ))
        .unwrap();
    assert_eq!(
        before.cancel(before_key).unwrap(),
        MutationCancellation::Cancelled
    );
    assert_eq!(before.state(), MutationState::Idle);

    let mut after = coordinator(1, 2);
    let after_key = key(1, 2);
    begin_staging(
        &mut after,
        after_key,
        MutationKind::Edit,
        ByteRange::from_u64(0, 0).unwrap(),
    );
    terminal(&mut after, after_key, 0);
    after.admit_commit(after_key).unwrap();
    assert_eq!(
        after.cancel(after_key).unwrap(),
        MutationCancellation::AwaitingHostSettlement
    );
    assert_eq!(
        after.settle(after_key, MutationOutcome::Rejected).unwrap(),
        MutationSettlement::Current(MutationOutcome::Rejected)
    );
}

#[test]
fn every_noncommitted_terminal_releases_capacity_without_revision_adoption() {
    for (index, outcome) in [
        MutationOutcome::Rejected,
        MutationOutcome::Conflict,
        MutationOutcome::Cancelled,
        MutationOutcome::Error,
    ]
    .into_iter()
    .enumerate()
    {
        let mut editor = coordinator(1, 2);
        let key = key(1, index as u64 + 1);
        begin_staging(
            &mut editor,
            key,
            MutationKind::Edit,
            ByteRange::from_u64(0, 0).unwrap(),
        );
        editor
            .stage(MutationFragment::new(
                key,
                0,
                MutationFragmentPayload::Utf8 {
                    inserted_offset: 0,
                    text: "x".into(),
                },
            ))
            .unwrap();
        terminal(&mut editor, key, 1);
        editor.admit_commit(key).unwrap();
        assert_eq!(
            editor.settle(key, outcome).unwrap(),
            MutationSettlement::Current(outcome)
        );
        assert_eq!(editor.binding(), binding(1, 2));
        assert_eq!(editor.counts().staged_bytes, 0);
    }
}

#[test]
fn wrong_stale_and_incoherent_results_are_rejected() {
    let mut editor = coordinator(4, 5);
    let current = key(4, 1);
    assert!(matches!(
        editor.begin(MutationProposal::new(
            key(3, 1),
            MutationKind::Edit,
            ByteRange::from_u64(0, 0).unwrap(),
            0,
        )),
        Err(MutationError::WrongKey { .. })
    ));
    begin_staging(
        &mut editor,
        current,
        MutationKind::Edit,
        ByteRange::from_u64(0, 1).unwrap(),
    );
    terminal(&mut editor, current, 0);
    editor.admit_commit(current).unwrap();
    assert!(matches!(
        editor.settle(key(4, 2), MutationOutcome::Rejected),
        Err(MutationError::WrongKey { .. })
    ));
    assert_eq!(
        editor.settle(current, MutationOutcome::Committed(binding(5, 99))),
        Err(MutationError::IncoherentSuccessor)
    );
    assert_eq!(editor.state(), MutationState::CommitPending);
}

#[test]
fn rebind_and_disposal_detach_admitted_results_and_release_staging() {
    let mut editor = coordinator(1, 3);
    let admitted_key = key(1, 1);
    begin_staging(
        &mut editor,
        admitted_key,
        MutationKind::Edit,
        ByteRange::from_u64(0, 0).unwrap(),
    );
    editor
        .stage(MutationFragment::new(
            admitted_key,
            0,
            MutationFragmentPayload::Utf8 {
                inserted_offset: 0,
                text: "x".into(),
            },
        ))
        .unwrap();
    terminal(&mut editor, admitted_key, 1);
    editor.admit_commit(admitted_key).unwrap();
    assert_eq!(
        editor.rebind(binding(9, 8)),
        Some(MutationDisposal::Detached(admitted_key))
    );
    assert_eq!(editor.state(), MutationState::DetachedCommit);
    assert_eq!(editor.counts().staged_bytes, 0);
    assert_eq!(
        editor
            .settle(admitted_key, MutationOutcome::Committed(binding(2, 4)))
            .unwrap(),
        MutationSettlement::Obsolete(MutationOutcome::Committed(binding(2, 4)))
    );
    assert_eq!(editor.binding(), binding(9, 8));

    let mut staged = coordinator(1, 3);
    let staged_key = key(1, 3);
    begin_staging(
        &mut staged,
        staged_key,
        MutationKind::Edit,
        ByteRange::from_u64(0, 0).unwrap(),
    );
    staged
        .stage(MutationFragment::new(
            staged_key,
            0,
            MutationFragmentPayload::Utf8 {
                inserted_offset: 0,
                text: "x".into(),
            },
        ))
        .unwrap();
    assert_eq!(
        staged.dispose(),
        Some(MutationDisposal::Cancelled(staged_key))
    );
    assert_eq!(staged.counts().staged_bytes, 0);
    assert_eq!(staged.state(), MutationState::Idle);
}
