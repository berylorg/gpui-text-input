use super::*;
use gpui_text_input::{
    AtomChange, AtomId, MutationCancellation, MutationCounts, MutationDisposal, MutationError,
    MutationSettlement, MutationState,
};

fn staged(base: RangeBinding, operation: u64, text: &str) -> (RangeEditCoordinator, MutationKey) {
    let proposal = proposal(
        base,
        operation,
        source_range(0, base.extent().byte_len()),
        0,
    );
    let mut editor = editor(base);
    editor.begin(proposal).unwrap();
    editor.accept_preflight(proposal.key()).unwrap();
    let base_text = "x".repeat(base.extent().byte_len() as usize);
    let proof_positions = [proposal.replacement().start(), proposal.replacement().end()];
    let (proof_text, proof_objects) = admitted_sources_for_positions(
        base,
        &base_text,
        &proof_positions,
        vec![],
        1_000 + operation,
    );
    editor
        .reserve_source_positions(
            proposal.key(),
            &proof_positions,
            &proof_text,
            &proof_objects,
        )
        .unwrap();
    if !text.is_empty() {
        editor
            .stage(MutationFragment::new(
                proposal.key(),
                0,
                MutationFragmentPayload::Utf8 {
                    inserted_offset: 0,
                    text: text.into(),
                },
            ))
            .unwrap();
    }
    let ordinal = usize::from(!text.is_empty());
    editor
        .stage(terminal(
            proposal.key(),
            ordinal,
            position(text.len() as u64),
        ))
        .unwrap();
    (editor, proposal.key())
}

#[test]
fn invalid_staging_is_terminal_and_releases_exactly_once() {
    let base = binding(1, "base");
    let proposal = proposal(base, 1, source_range(0, 4), 0);
    let mut editor = RangeEditCoordinator::new(base, MutationLimits::new(2, 2).unwrap());
    editor.begin(proposal).unwrap();
    editor.accept_preflight(proposal.key()).unwrap();
    let proof_positions = [proposal.replacement().start(), proposal.replacement().end()];
    let (text, objects) =
        admitted_sources_for_positions(base, "base", &proof_positions, vec![], 900);
    editor
        .reserve_source_positions(proposal.key(), &proof_positions, &text, &objects)
        .unwrap();
    editor
        .stage(MutationFragment::new(
            proposal.key(),
            0,
            MutationFragmentPayload::Utf8 {
                inserted_offset: 0,
                text: "x".into(),
            },
        ))
        .unwrap();
    assert_eq!(
        editor.stage(MutationFragment::new(
            proposal.key(),
            2,
            MutationFragmentPayload::Utf8 {
                inserted_offset: 1,
                text: "x".into(),
            },
        )),
        Err(MutationError::FragmentOutOfOrder {
            expected: 1,
            actual: 2,
        })
    );
    assert_eq!(editor.state(), MutationState::Idle);
    assert_eq!(editor.counts(), MutationCounts::default());
    assert_eq!(
        editor.stage(terminal(proposal.key(), 1, position(1))),
        Err(MutationError::ObsoleteOperation(proposal.key()))
    );
    assert_eq!(
        editor.admit_commit(proposal.key()),
        Err(MutationError::ObsoleteOperation(proposal.key()))
    );
    let released = MutationCounts {
        fragments: 1,
        staged_bytes: 1,
        proofs: 2,
        source_pages: 2,
        transactions: 1,
        ..MutationCounts::default()
    };
    assert_eq!(editor.released_counts(), released);
    assert_eq!(
        editor.cancel(proposal.key()),
        Err(MutationError::ObsoleteOperation(proposal.key()))
    );
    assert_eq!(
        editor.reject_staging(proposal.key()),
        Err(MutationError::ObsoleteOperation(proposal.key()))
    );
    assert_eq!(editor.released_counts(), released);

    let proposal = super::proposal(base, 2, source_range(0, 4), 0);
    let mut editor = RangeEditCoordinator::new(base, MutationLimits::new(2, 2).unwrap());
    editor.begin(proposal).unwrap();
    editor.accept_preflight(proposal.key()).unwrap();
    editor
        .stage(terminal(proposal.key(), 0, position(0)))
        .unwrap();
    assert_eq!(
        editor.stage(terminal(proposal.key(), 1, position(0))),
        Err(MutationError::PostTerminalFragment)
    );
    assert_eq!(editor.state(), MutationState::Idle);
    assert_eq!(editor.counts(), MutationCounts::default());
    assert_eq!(editor.released_counts().fragments, 1);
    assert_eq!(editor.released_counts().transactions, 1);
    assert_eq!(
        editor.admit_commit(proposal.key()),
        Err(MutationError::ObsoleteOperation(proposal.key()))
    );
}

#[test]
fn cap_and_atom_validation_errors_cannot_be_recovered_by_fragment_substitution() {
    let base = binding(1, "base");

    let capped = proposal(base, 3, source_range(0, 4), 0);
    let mut capped_editor = RangeEditCoordinator::new(base, MutationLimits::new(1, 1).unwrap());
    capped_editor.begin(capped).unwrap();
    capped_editor.accept_preflight(capped.key()).unwrap();
    capped_editor
        .stage(MutationFragment::new(
            capped.key(),
            0,
            MutationFragmentPayload::Utf8 {
                inserted_offset: 0,
                text: "x".into(),
            },
        ))
        .unwrap();
    assert_eq!(
        capped_editor.stage(terminal(capped.key(), 1, position(1))),
        Err(MutationError::FragmentLimitExceeded)
    );
    assert_eq!(capped_editor.state(), MutationState::Idle);
    assert_eq!(capped_editor.released_counts().fragments, 1);
    assert_eq!(capped_editor.released_counts().staged_bytes, 1);
    assert_eq!(capped_editor.released_counts().transactions, 1);
    assert_eq!(
        capped_editor.stage(terminal(capped.key(), 1, position(1))),
        Err(MutationError::ObsoleteOperation(capped.key()))
    );
    assert_eq!(
        capped_editor.admit_commit(capped.key()),
        Err(MutationError::ObsoleteOperation(capped.key()))
    );

    let atom = proposal(base, 4, source_range(0, 4), 0);
    let mut atom_editor = editor(base);
    atom_editor.begin(atom).unwrap();
    atom_editor.accept_preflight(atom.key()).unwrap();
    assert_eq!(
        atom_editor.stage(MutationFragment::new(
            atom.key(),
            0,
            MutationFragmentPayload::Atom(AtomChange::Insert {
                id: AtomId::new(1),
                inserted_range: ByteRange::from_u64(0, 1).unwrap(),
                fallback_copy: "x".into(),
            }),
        )),
        Err(MutationError::MalformedAtomChange)
    );
    assert_eq!(atom_editor.state(), MutationState::Idle);
    assert_eq!(atom_editor.released_counts().transactions, 1);
    assert_eq!(
        atom_editor.stage(terminal(atom.key(), 0, position(0))),
        Err(MutationError::ObsoleteOperation(atom.key()))
    );
    assert_eq!(
        atom_editor.admit_commit(atom.key()),
        Err(MutationError::ObsoleteOperation(atom.key()))
    );
}

#[test]
fn cancellation_before_and_after_admission_has_exact_once_release() {
    let base = binding(1, "base");
    let (mut before, before_key) = staged(base, 1, "x");
    assert_eq!(
        before.cancel(before_key).unwrap(),
        MutationCancellation::Cancelled
    );
    let released = before.released_counts();
    assert_eq!(released.fragments, 2);
    assert_eq!(released.transactions, 1);
    assert_eq!(
        before.cancel(before_key),
        Err(MutationError::ObsoleteOperation(before_key))
    );
    assert_eq!(before.released_counts(), released);

    let (mut after, after_key) = staged(base, 2, "x");
    after.admit_commit(after_key).unwrap();
    assert_eq!(
        after.cancel(after_key).unwrap(),
        MutationCancellation::AwaitingHostSettlement
    );
    assert_eq!(after.state(), MutationState::CommitPending);
    let successor = binding(2, "x");
    let positions = MutationPositions::collapsed(position(1));
    let (text, objects) = admitted_sources(successor, "x", position(1), vec![], 20);
    after
        .settle_committed(after_key, successor, positions, &text, &objects)
        .unwrap();
    assert_eq!(after.binding(), successor);
    assert_eq!(after.released_counts().proofs, 5);
    assert_eq!(after.released_counts().transactions, 1);
}

#[test]
fn stale_and_wrong_successor_proofs_never_adopt() {
    let base = binding(1, "base");
    let (mut editor, key) = staged(base, 1, "next");
    editor.admit_commit(key).unwrap();
    let successor = binding(2, "next");
    let wrong_positions = MutationPositions::collapsed(position(0));
    let (text, objects) = admitted_sources(successor, "next", position(0), vec![], 30);
    assert_eq!(
        editor.settle_committed(key, successor, wrong_positions, &text, &objects),
        Err(MutationError::WrongSuccessorPositions)
    );
    assert_eq!(editor.binding(), base);
    assert_eq!(editor.state(), MutationState::CommitPending);

    let stale_binding = binding(3, "next");
    let (stale_text, stale_objects) =
        admitted_sources(stale_binding, "next", position(4), vec![], 31);
    assert_eq!(
        editor.settle_committed(
            key,
            successor,
            MutationPositions::collapsed(position(4)),
            &stale_text,
            &stale_objects,
        ),
        Err(MutationError::StalePositionProof)
    );
    let (valid_text, valid_objects) = admitted_sources(successor, "next", position(4), vec![], 32);
    editor
        .settle_committed(
            key,
            successor,
            MutationPositions::collapsed(position(4)),
            &valid_text,
            &valid_objects,
        )
        .unwrap();
}

#[test]
fn rebind_unmount_and_late_settlement_are_excluded_and_released() {
    let base = binding(1, "base");
    let (mut precommit, precommit_key) = staged(base, 1, "x");
    let replacement = binding(8, "replacement");
    assert_eq!(
        precommit.rebind(replacement),
        Some(MutationDisposal::Cancelled(precommit_key))
    );
    assert_eq!(precommit.binding(), replacement);
    assert_eq!(precommit.released_counts().transactions, 1);

    let (mut admitted, admitted_key) = staged(base, 2, "x");
    admitted.admit_commit(admitted_key).unwrap();
    assert_eq!(
        admitted.dispose(),
        Some(MutationDisposal::Detached(admitted_key))
    );
    assert_eq!(admitted.state(), MutationState::DetachedCommit);
    assert_eq!(admitted.counts().fragments, 0);
    assert_eq!(admitted.counts().transactions, 1);
    let successor = binding(2, "x");
    let positions = MutationPositions::collapsed(position(1));
    let (text, objects) = admitted_sources(successor, "x", position(1), vec![], 50);
    assert!(matches!(
        admitted
            .settle_committed(admitted_key, successor, positions, &text, &objects)
            .unwrap(),
        MutationSettlement::Obsolete(MutationOutcome::Committed(_))
    ));
    assert_eq!(admitted.binding(), base);
    assert_eq!(admitted.released_counts().transactions, 1);
}

#[test]
fn rejected_conflict_cancelled_and_error_settle_without_base_change() {
    for (operation, outcome) in [
        (1, MutationOutcome::Rejected),
        (2, MutationOutcome::Conflict),
        (3, MutationOutcome::Cancelled),
        (4, MutationOutcome::Error),
    ] {
        let base = binding(1, "base");
        let (mut editor, key) = staged(base, operation, "x");
        editor.admit_commit(key).unwrap();
        editor.settle(key, outcome).unwrap();
        assert_eq!(editor.binding(), base);
        assert_eq!(editor.counts(), Default::default());
        assert_eq!(editor.released_counts().transactions, 1);
    }
}

#[test]
fn base_source_proof_reservations_release_once_with_the_transaction() {
    let base = binding(1, "base");
    let at = position(0);
    let proposal = proposal(base, 70, SourceRange::new(at, at).unwrap(), 0);
    let (text, objects) = admitted_sources(base, "base", at, vec![], 70);
    let mut editor = editor(base);
    editor.begin(proposal).unwrap();
    editor.accept_preflight(proposal.key()).unwrap();
    editor
        .reserve_source_positions(proposal.key(), &[at], &text, &objects)
        .unwrap();
    assert_eq!(editor.counts().proofs, 1);
    assert_eq!(editor.counts().source_pages, 1);
    editor.reject_staging(proposal.key()).unwrap();
    assert_eq!(editor.released_counts().proofs, 1);
    assert_eq!(editor.released_counts().source_pages, 1);
    assert_eq!(
        editor.reject_staging(proposal.key()),
        Err(MutationError::ObsoleteOperation(proposal.key()))
    );
    assert_eq!(editor.released_counts().proofs, 1);
}

#[test]
fn unadmitted_sources_cannot_mint_mutation_position_proofs() {
    let base = binding(1, "base");
    let at = position(2);
    let proposal = proposal(base, 71, SourceRange::new(at, at).unwrap(), 0);
    let mut editor = editor(base);
    editor.begin(proposal).unwrap();
    editor.accept_preflight(proposal.key()).unwrap();

    let empty_text = RangeResidency::new(
        base,
        ResidencyLimits::new(4, 64 * 1024, 4, 64 * 1024).unwrap(),
    );
    let empty_objects = ObjectResidency::new(
        base,
        PresentationGeneration::new(1),
        ObjectResidencyLimits::new(4, 16, 64 * 1024, 32 * 1024, 4, 16, 64 * 1024).unwrap(),
    );
    assert_eq!(
        editor.reserve_source_positions(proposal.key(), &[at], &empty_text, &empty_objects),
        Err(MutationError::MissingTextBoundaryProof)
    );
    assert_eq!(editor.counts().proofs, 0);

    let (admitted_text, _) = admitted_sources(base, "base", at, vec![], 71);
    assert_eq!(
        editor.reserve_source_positions(proposal.key(), &[at], &admitted_text, &empty_objects),
        Err(MutationError::InvalidObjectGapProof)
    );
    assert_eq!(editor.counts().proofs, 0);
    assert_eq!(editor.counts().source_pages, 0);
    editor.stage(terminal(proposal.key(), 0, at)).unwrap();
    assert_eq!(
        editor.admit_commit(proposal.key()),
        Err(MutationError::MissingPositionProof(at))
    );
    assert_eq!(editor.state(), MutationState::Staging);
    assert_eq!(editor.counts().proofs, 0);
    editor.cancel(proposal.key()).unwrap();
    assert_eq!(editor.released_counts().fragments, 1);
    assert_eq!(editor.released_counts().proofs, 0);
    assert_eq!(editor.released_counts().transactions, 1);
}

#[test]
fn exact_base_proof_set_rejects_extra_duplicate_and_stale_coverage() {
    let base = binding(1, "base");
    let at = position(0);
    let extra = position(1);
    let positions = [at, extra];
    let (text, objects) = admitted_sources_for_positions(base, "base", &positions, vec![], 80);

    let extra_proposal = proposal(base, 80, SourceRange::new(at, at).unwrap(), 0);
    let mut extra_editor = editor(base);
    extra_editor.begin(extra_proposal).unwrap();
    extra_editor.accept_preflight(extra_proposal.key()).unwrap();
    extra_editor
        .reserve_source_positions(extra_proposal.key(), &positions, &text, &objects)
        .unwrap();
    extra_editor
        .stage(terminal(extra_proposal.key(), 0, at))
        .unwrap();
    assert_eq!(
        extra_editor.admit_commit(extra_proposal.key()),
        Err(MutationError::UnexpectedPositionProof(extra))
    );
    assert_eq!(extra_editor.state(), MutationState::Staging);
    extra_editor.cancel(extra_proposal.key()).unwrap();
    assert_eq!(extra_editor.released_counts().proofs, 2);

    let duplicate = proposal(base, 81, SourceRange::new(at, at).unwrap(), 0);
    let mut duplicate_editor = editor(base);
    duplicate_editor.begin(duplicate).unwrap();
    duplicate_editor.accept_preflight(duplicate.key()).unwrap();
    assert_eq!(
        duplicate_editor.reserve_source_positions(duplicate.key(), &[at, at], &text, &objects),
        Err(MutationError::DuplicatePositionProof(at))
    );
    assert_eq!(duplicate_editor.counts().proofs, 0);

    let stale = binding(2, "base");
    let (stale_text, stale_objects) = admitted_sources(stale, "base", at, vec![], 82);
    assert_eq!(
        duplicate_editor.reserve_source_positions(
            duplicate.key(),
            &[at],
            &stale_text,
            &stale_objects,
        ),
        Err(MutationError::StalePositionProof)
    );
    assert_eq!(duplicate_editor.counts().proofs, 0);
    duplicate_editor.cancel(duplicate.key()).unwrap();
    assert_eq!(duplicate_editor.released_counts().proofs, 0);
    assert_eq!(duplicate_editor.released_counts().transactions, 1);
}
