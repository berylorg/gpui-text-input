use super::*;
use gpui::{SharedString, px};
use gpui_text_input::{
    AtomChange, AtomId, InlineObjectFact, InlineObjectGap, InlineObjectId, InlineObjectNeighbor,
    InlineObjectOrder, InlineObjectPresentation, MutationError, MutationSettlement, ObjectChange,
    ObjectTarget, SuccessorObject,
};

fn neighbor(id: u128, order: u128) -> InlineObjectNeighbor {
    InlineObjectNeighbor::new(InlineObjectId::new(id), InlineObjectOrder::new(order))
}

fn isolated(anchor: u64, id: u128, order: u128) -> ObjectTarget {
    let object = neighbor(id, order);
    ObjectTarget::new(
        SourceRange::new(
            SourcePosition::new(ByteOffset::new(anchor), InlineObjectGap::before(object)),
            SourcePosition::new(ByteOffset::new(anchor), InlineObjectGap::after(object)),
        )
        .unwrap(),
        object.id(),
        object.order(),
    )
    .unwrap()
}

fn target_with_neighbors(
    anchor: u64,
    id: u128,
    order: u128,
    preceding: Option<InlineObjectNeighbor>,
    following: Option<InlineObjectNeighbor>,
) -> ObjectTarget {
    let object = neighbor(id, order);
    let start = preceding.map_or_else(
        || SourcePosition::new(ByteOffset::new(anchor), InlineObjectGap::before(object)),
        |preceding| {
            SourcePosition::new(
                ByteOffset::new(anchor),
                InlineObjectGap::between(preceding, object).unwrap(),
            )
        },
    );
    let end = following.map_or_else(
        || SourcePosition::new(ByteOffset::new(anchor), InlineObjectGap::after(object)),
        |following| {
            SourcePosition::new(
                ByteOffset::new(anchor),
                InlineObjectGap::between(object, following).unwrap(),
            )
        },
    );
    ObjectTarget::new(
        SourceRange::new(start, end).unwrap(),
        object.id(),
        object.order(),
    )
    .unwrap()
}

fn fact(id: u128, anchor: u64, order: u128) -> InlineObjectFact {
    InlineObjectFact::new(
        InlineObjectId::new(id),
        ByteOffset::new(anchor),
        InlineObjectOrder::new(order),
        format!("[{id}]"),
        InlineObjectPresentation::new(
            id as u64,
            SharedString::new_static("object"),
            px(10.),
            px(10.),
            px(8.),
            None,
            0,
            true,
        )
        .unwrap(),
    )
}

#[test]
fn insert_remove_replace_and_move_share_one_ordered_slot() {
    let base = binding(1, "abcd");
    let first = neighbor(1, 10);
    let second = neighbor(2, 20);
    let replacement = SourceRange::new(
        SourcePosition::new(ByteOffset::new(2), InlineObjectGap::before(first)),
        SourcePosition::new(ByteOffset::new(2), InlineObjectGap::after(second)),
    )
    .unwrap();
    let initial = proposal(base, 1, replacement, 0);
    let mut editor = editor(base);
    editor.begin(initial).unwrap();
    editor.accept_preflight(initial.key()).unwrap();

    let remove = ObjectTarget::new(
        SourceRange::new(
            replacement.start(),
            SourcePosition::new(
                ByteOffset::new(2),
                InlineObjectGap::between(first, second).unwrap(),
            ),
        )
        .unwrap(),
        first.id(),
        first.order(),
    )
    .unwrap();
    editor
        .stage(MutationFragment::new(
            initial.key(),
            0,
            MutationFragmentPayload::Object(ObjectChange::Remove { target: remove }),
        ))
        .unwrap();
    editor
        .stage(MutationFragment::new(
            initial.key(),
            1,
            MutationFragmentPayload::Object(ObjectChange::Replace {
                target: ObjectTarget::new(
                    SourceRange::new(remove.range().end(), replacement.end()).unwrap(),
                    second.id(),
                    second.order(),
                )
                .unwrap(),
                object: SuccessorObject::new(
                    InlineObjectId::new(3),
                    ByteOffset::new(2),
                    InlineObjectOrder::new(20),
                    12,
                    6,
                ),
            }),
        ))
        .unwrap();
    assert_eq!(editor.counts().objects, 2);
    assert_eq!(editor.counts().object_bytes, 12);
    assert_eq!(editor.counts().presentation_bytes, 6);
    assert_eq!(
        editor.begin(proposal(base, 2, source_range(0, 0), 0)),
        Err(MutationError::Busy(initial.key()))
    );
    assert!(matches!(
        editor.reject_staging(initial.key()).unwrap(),
        MutationSettlement::Current(MutationOutcome::Rejected)
    ));

    for (operation, change) in [
        (
            3,
            ObjectChange::Insert {
                at: position(1),
                object: SuccessorObject::new(
                    InlineObjectId::new(7),
                    ByteOffset::new(1),
                    InlineObjectOrder::new(7),
                    1,
                    1,
                ),
            },
        ),
        (
            4,
            ObjectChange::Move {
                target: isolated(1, 8, 8),
                to: position(2),
                object: SuccessorObject::new(
                    InlineObjectId::new(8),
                    ByteOffset::new(2),
                    InlineObjectOrder::new(80),
                    0,
                    0,
                ),
            },
        ),
    ] {
        let range = match change {
            ObjectChange::Insert { at, .. } => SourceRange::new(at, at).unwrap(),
            ObjectChange::Move { target, to, .. } => {
                SourceRange::new(target.range().start(), to).unwrap()
            }
            _ => unreachable!(),
        };
        let proposal = proposal(base, operation, range, 0);
        editor.begin(proposal).unwrap();
        editor.accept_preflight(proposal.key()).unwrap();
        editor
            .stage(MutationFragment::new(
                proposal.key(),
                0,
                MutationFragmentPayload::Object(change),
            ))
            .unwrap();
        editor.reject_staging(proposal.key()).unwrap();
    }
}

#[test]
fn successor_objects_must_match_destination_anchor_and_gap_order() {
    let base = binding(1, "abcd");
    let replacement = source_range(0, 4);
    let rejects = |operation, change| {
        let proposal = proposal(base, operation, replacement, 0);
        let mut editor = editor(base);
        editor.begin(proposal).unwrap();
        editor.accept_preflight(proposal.key()).unwrap();
        assert_eq!(
            editor.stage(MutationFragment::new(
                proposal.key(),
                0,
                MutationFragmentPayload::Object(change),
            )),
            Err(MutationError::MalformedObjectChange)
        );
        assert_eq!(editor.state(), gpui_text_input::MutationState::Idle);
        assert_eq!(editor.counts().fragments, 0);
        assert_eq!(editor.counts().objects, 0);
        assert_eq!(editor.released_counts().transactions, 1);
    };

    rejects(
        100,
        ObjectChange::Insert {
            at: position(1),
            object: SuccessorObject::new(
                InlineObjectId::new(30),
                ByteOffset::new(2),
                InlineObjectOrder::new(5),
                0,
                0,
            ),
        },
    );
    let following = neighbor(31, 10);
    rejects(
        101,
        ObjectChange::Insert {
            at: SourcePosition::new(ByteOffset::new(1), InlineObjectGap::before(following)),
            object: SuccessorObject::new(
                InlineObjectId::new(32),
                ByteOffset::new(1),
                InlineObjectOrder::new(20),
                0,
                0,
            ),
        },
    );
    rejects(
        102,
        ObjectChange::Move {
            target: isolated(1, 33, 8),
            to: position(2),
            object: SuccessorObject::new(
                InlineObjectId::new(33),
                ByteOffset::new(3),
                InlineObjectOrder::new(30),
                0,
                0,
            ),
        },
    );
    rejects(
        103,
        ObjectChange::Move {
            target: isolated(1, 34, 8),
            to: SourcePosition::new(ByteOffset::new(2), InlineObjectGap::before(following)),
            object: SuccessorObject::new(
                InlineObjectId::new(34),
                ByteOffset::new(2),
                InlineObjectOrder::new(20),
                0,
                0,
            ),
        },
    );
    rejects(
        104,
        ObjectChange::Replace {
            target: isolated(1, 35, 8),
            object: SuccessorObject::new(
                InlineObjectId::new(36),
                ByteOffset::new(2),
                InlineObjectOrder::new(8),
                0,
                0,
            ),
        },
    );
    rejects(
        105,
        ObjectChange::Replace {
            target: isolated(1, 37, 8),
            object: SuccessorObject::new(
                InlineObjectId::new(38),
                ByteOffset::new(1),
                InlineObjectOrder::new(9),
                0,
                0,
            ),
        },
    );
}

#[test]
fn replacement_identities_cannot_reuse_other_targets_or_successors() {
    let base = binding(1, "abcd");
    let replacement = source_range(0, 4);

    let reintroduced = proposal(base, 110, replacement, 0);
    let mut reintroduced_editor = editor(base);
    reintroduced_editor.begin(reintroduced).unwrap();
    reintroduced_editor
        .accept_preflight(reintroduced.key())
        .unwrap();
    reintroduced_editor
        .stage(MutationFragment::new(
            reintroduced.key(),
            0,
            MutationFragmentPayload::Object(ObjectChange::Remove {
                target: isolated(1, 40, 10),
            }),
        ))
        .unwrap();
    assert_eq!(
        reintroduced_editor.stage(MutationFragment::new(
            reintroduced.key(),
            1,
            MutationFragmentPayload::Object(ObjectChange::Replace {
                target: isolated(2, 41, 20),
                object: SuccessorObject::new(
                    InlineObjectId::new(40),
                    ByteOffset::new(2),
                    InlineObjectOrder::new(20),
                    0,
                    0,
                ),
            }),
        )),
        Err(MutationError::DuplicateObjectChange(InlineObjectId::new(
            40
        )))
    );
    assert_eq!(
        reintroduced_editor.state(),
        gpui_text_input::MutationState::Idle
    );
    assert_eq!(reintroduced_editor.counts().objects, 0);
    assert_eq!(reintroduced_editor.released_counts().objects, 1);
    assert_eq!(reintroduced_editor.released_counts().transactions, 1);

    let duplicate = proposal(base, 111, replacement, 0);
    let mut duplicate_editor = editor(base);
    duplicate_editor.begin(duplicate).unwrap();
    duplicate_editor.accept_preflight(duplicate.key()).unwrap();
    duplicate_editor
        .stage(MutationFragment::new(
            duplicate.key(),
            0,
            MutationFragmentPayload::Object(ObjectChange::Replace {
                target: isolated(1, 42, 10),
                object: SuccessorObject::new(
                    InlineObjectId::new(50),
                    ByteOffset::new(1),
                    InlineObjectOrder::new(10),
                    2,
                    3,
                ),
            }),
        ))
        .unwrap();
    assert_eq!(
        duplicate_editor.stage(MutationFragment::new(
            duplicate.key(),
            1,
            MutationFragmentPayload::Object(ObjectChange::Replace {
                target: isolated(2, 43, 20),
                object: SuccessorObject::new(
                    InlineObjectId::new(50),
                    ByteOffset::new(2),
                    InlineObjectOrder::new(20),
                    0,
                    0,
                ),
            }),
        )),
        Err(MutationError::DuplicateObjectChange(InlineObjectId::new(
            50
        )))
    );
    assert_eq!(
        duplicate_editor.state(),
        gpui_text_input::MutationState::Idle
    );
    assert_eq!(duplicate_editor.counts().objects, 0);
    assert_eq!(duplicate_editor.released_counts().objects, 1);
    assert_eq!(duplicate_editor.released_counts().object_bytes, 2);
    assert_eq!(duplicate_editor.released_counts().presentation_bytes, 3);
    assert_eq!(duplicate_editor.released_counts().transactions, 1);

    let allowed = proposal(base, 112, replacement, 0);
    let mut allowed_editor = editor(base);
    allowed_editor.begin(allowed).unwrap();
    allowed_editor.accept_preflight(allowed.key()).unwrap();
    for (ordinal, change) in [
        ObjectChange::Replace {
            target: isolated(1, 60, 10),
            object: SuccessorObject::new(
                InlineObjectId::new(60),
                ByteOffset::new(1),
                InlineObjectOrder::new(10),
                0,
                0,
            ),
        },
        ObjectChange::Replace {
            target: isolated(2, 61, 20),
            object: SuccessorObject::new(
                InlineObjectId::new(62),
                ByteOffset::new(2),
                InlineObjectOrder::new(20),
                0,
                0,
            ),
        },
    ]
    .into_iter()
    .enumerate()
    {
        allowed_editor
            .stage(MutationFragment::new(
                allowed.key(),
                ordinal,
                MutationFragmentPayload::Object(change),
            ))
            .unwrap();
    }
    assert_eq!(allowed_editor.counts().objects, 2);
    allowed_editor.reject_staging(allowed.key()).unwrap();
}

#[test]
fn replacement_identity_cannot_collide_with_proven_unchanged_neighbors() {
    let base = binding(1, "abcd");
    let rejects = |operation, target: ObjectTarget, successor_id, facts, nonce| {
        let proposal = proposal(base, operation, target.range(), 0);
        let proof_positions = [target.range().start(), target.range().end()];
        let (text, objects) =
            admitted_sources_for_positions(base, "abcd", &proof_positions, facts, nonce);
        let mut editor = editor(base);
        editor.begin(proposal).unwrap();
        editor.accept_preflight(proposal.key()).unwrap();
        editor
            .reserve_source_positions(proposal.key(), &proof_positions, &text, &objects)
            .unwrap();
        assert_eq!(
            editor.stage(MutationFragment::new(
                proposal.key(),
                0,
                MutationFragmentPayload::Object(ObjectChange::Replace {
                    target,
                    object: SuccessorObject::new(
                        successor_id,
                        target.range().start().byte_offset,
                        target.order(),
                        0,
                        0,
                    ),
                }),
            )),
            Err(MutationError::DuplicateObjectChange(successor_id))
        );
        assert_eq!(editor.state(), gpui_text_input::MutationState::Idle);
        assert_eq!(editor.counts().objects, 0);
        assert_eq!(
            editor.stage(terminal(proposal.key(), 0, position(1))),
            Err(MutationError::ObsoleteOperation(proposal.key()))
        );
        assert_eq!(
            editor.admit_commit(proposal.key()),
            Err(MutationError::ObsoleteOperation(proposal.key()))
        );
        assert_eq!(editor.released_counts().proofs, 2);
        assert_eq!(editor.released_counts().objects, 0);
        assert_eq!(editor.released_counts().transactions, 1);
    };

    let preceding = neighbor(80, 10);
    let target = target_with_neighbors(1, 81, 20, Some(preceding), None);
    rejects(
        120,
        target,
        preceding.id(),
        vec![fact(80, 1, 10), fact(81, 1, 20)],
        180,
    );

    let following = neighbor(83, 30);
    let target = target_with_neighbors(1, 82, 20, None, Some(following));
    rejects(
        121,
        target,
        following.id(),
        vec![fact(82, 1, 20), fact(83, 1, 30)],
        190,
    );
}

#[test]
fn replacement_identity_accepts_target_new_and_unproven_distant_ids() {
    let base = binding(1, "abcd");
    let preceding = neighbor(90, 10);
    let following = neighbor(92, 30);
    let target = target_with_neighbors(1, 91, 20, Some(preceding), Some(following));
    let accepts = |operation, successor_id| {
        let proposal = proposal(base, operation, target.range(), 0);
        let mut editor = editor(base);
        editor.begin(proposal).unwrap();
        editor.accept_preflight(proposal.key()).unwrap();
        editor
            .stage(MutationFragment::new(
                proposal.key(),
                0,
                MutationFragmentPayload::Object(ObjectChange::Replace {
                    target,
                    object: SuccessorObject::new(
                        successor_id,
                        ByteOffset::new(1),
                        InlineObjectOrder::new(20),
                        0,
                        0,
                    ),
                }),
            ))
            .unwrap();
        assert_eq!(editor.counts().objects, 1);
        editor.reject_staging(proposal.key()).unwrap();
    };

    accepts(122, target.id());
    accepts(123, InlineObjectId::new(93));
    // Identity knowledge outside the exact adjacent witnesses remains host-owned and unscanned.
    accepts(124, InlineObjectId::new(9_999));
}

#[test]
fn commit_admission_requires_every_staged_object_position_proof() {
    let base = binding(1, "abcd");
    let missing = proposal(base, 113, source_range(0, 4), 0);
    let endpoints = [missing.replacement().start(), missing.replacement().end()];
    let (text, objects) = admitted_sources_for_positions(base, "abcd", &endpoints, vec![], 160);
    let mut missing_editor = editor(base);
    missing_editor.begin(missing).unwrap();
    missing_editor.accept_preflight(missing.key()).unwrap();
    missing_editor
        .reserve_source_positions(missing.key(), &endpoints, &text, &objects)
        .unwrap();
    let object_position = position(2);
    missing_editor
        .stage(MutationFragment::new(
            missing.key(),
            0,
            MutationFragmentPayload::Utf8 {
                inserted_offset: 0,
                text: "abcd".into(),
            },
        ))
        .unwrap();
    missing_editor
        .stage(MutationFragment::new(
            missing.key(),
            1,
            MutationFragmentPayload::Object(ObjectChange::Insert {
                at: object_position,
                object: SuccessorObject::new(
                    InlineObjectId::new(70),
                    ByteOffset::new(2),
                    InlineObjectOrder::new(10),
                    0,
                    0,
                ),
            }),
        ))
        .unwrap();
    missing_editor
        .stage(terminal(missing.key(), 2, position(4)))
        .unwrap();
    assert_eq!(
        missing_editor.admit_commit(missing.key()),
        Err(MutationError::MissingPositionProof(object_position))
    );
    assert_eq!(
        missing_editor.state(),
        gpui_text_input::MutationState::Staging
    );
    assert_eq!(missing_editor.counts().proofs, 2);
    missing_editor.cancel(missing.key()).unwrap();
    assert_eq!(missing_editor.released_counts().proofs, 2);
    assert_eq!(missing_editor.released_counts().transactions, 1);

    let complete = proposal(base, 114, source_range(0, 4), 0);
    let complete_positions = [
        complete.replacement().start(),
        complete.replacement().end(),
        object_position,
    ];
    let (text, objects) =
        admitted_sources_for_positions(base, "abcd", &complete_positions, vec![], 170);
    let mut complete_editor = editor(base);
    complete_editor.begin(complete).unwrap();
    complete_editor.accept_preflight(complete.key()).unwrap();
    complete_editor
        .reserve_source_positions(complete.key(), &complete_positions, &text, &objects)
        .unwrap();
    complete_editor
        .stage(MutationFragment::new(
            complete.key(),
            0,
            MutationFragmentPayload::Utf8 {
                inserted_offset: 0,
                text: "abcd".into(),
            },
        ))
        .unwrap();
    complete_editor
        .stage(MutationFragment::new(
            complete.key(),
            1,
            MutationFragmentPayload::Object(ObjectChange::Insert {
                at: object_position,
                object: SuccessorObject::new(
                    InlineObjectId::new(71),
                    ByteOffset::new(2),
                    InlineObjectOrder::new(10),
                    0,
                    0,
                ),
            }),
        ))
        .unwrap();
    complete_editor
        .stage(terminal(complete.key(), 2, position(4)))
        .unwrap();
    complete_editor.admit_commit(complete.key()).unwrap();
    complete_editor
        .settle(complete.key(), MutationOutcome::Rejected)
        .unwrap();
    assert_eq!(complete_editor.released_counts().proofs, 3);
    assert_eq!(complete_editor.released_counts().transactions, 1);
}

#[test]
fn utf8_and_source_covering_atoms_remain_atomic_with_object_fragments() {
    let base = binding(1, "aXb");
    let proposal = proposal(base, 9, source_range(1, 2), 0);
    let mut editor = editor(base);
    editor.begin(proposal).unwrap();
    editor.accept_preflight(proposal.key()).unwrap();
    let base_positions = [proposal.replacement().start(), proposal.replacement().end()];
    let (base_text, base_objects) =
        admitted_sources_for_positions(base, "aXb", &base_positions, vec![], 80);
    editor
        .reserve_source_positions(proposal.key(), &base_positions, &base_text, &base_objects)
        .unwrap();
    editor
        .stage(MutationFragment::new(
            proposal.key(),
            0,
            MutationFragmentPayload::Utf8 {
                inserted_offset: 0,
                text: "é".into(),
            },
        ))
        .unwrap();
    editor
        .stage(MutationFragment::new(
            proposal.key(),
            1,
            MutationFragmentPayload::Atom(AtomChange::Remove {
                id: AtomId::new(1),
                source_range: ByteRange::from_u64(1, 2).unwrap(),
            }),
        ))
        .unwrap();
    editor
        .stage(MutationFragment::new(
            proposal.key(),
            2,
            MutationFragmentPayload::Atom(AtomChange::Insert {
                id: AtomId::new(1),
                inserted_range: ByteRange::from_u64(0, 2).unwrap(),
                fallback_copy: "é".into(),
            }),
        ))
        .unwrap();
    editor
        .stage(terminal(proposal.key(), 3, position(3)))
        .unwrap();
    editor.admit_commit(proposal.key()).unwrap();
    let successor = binding(2, "aéb");
    let positions = MutationPositions::collapsed(position(3));
    let (text, objects) = admitted_sources(successor, "aéb", position(3), vec![], 90);
    assert!(matches!(
        editor
            .settle_committed(proposal.key(), successor, positions, &text, &objects)
            .unwrap(),
        MutationSettlement::Current(MutationOutcome::Committed(_))
    ));
    assert_eq!(editor.binding(), successor);
    assert_eq!(editor.counts(), Default::default());
}

#[test]
fn text_inserted_between_same_anchor_objects_keeps_the_composite_cut() {
    let base = binding(1, "ab");
    let first = neighbor(1, 10);
    let second = neighbor(2, 20);
    let between = SourcePosition::new(
        ByteOffset::new(1),
        InlineObjectGap::between(first, second).unwrap(),
    );
    let replacement = SourceRange::new(between, between).unwrap();
    let proposal = proposal(base, 20, replacement, 0);
    let mut editor = editor(base);
    editor.begin(proposal).unwrap();
    editor.accept_preflight(proposal.key()).unwrap();
    let (base_text, base_objects) = admitted_sources(
        base,
        "ab",
        between,
        vec![fact(1, 1, 10), fact(2, 1, 20)],
        110,
    );
    editor
        .reserve_source_positions(proposal.key(), &[between], &base_text, &base_objects)
        .unwrap();
    editor
        .stage(MutationFragment::new(
            proposal.key(),
            0,
            MutationFragmentPayload::Utf8 {
                inserted_offset: 0,
                text: "X".into(),
            },
        ))
        .unwrap();
    editor
        .stage(terminal(proposal.key(), 1, position(2)))
        .unwrap();
    editor.admit_commit(proposal.key()).unwrap();
    let successor = binding(2, "aXb");
    let positions = MutationPositions::collapsed(position(2));
    let (text, objects) = admitted_sources(successor, "aXb", position(2), vec![], 120);
    editor
        .settle_committed(proposal.key(), successor, positions, &text, &objects)
        .unwrap();
    assert_eq!(proposal.replacement().start(), between);
    assert_eq!(proposal.replacement().end(), between);
    assert_eq!(editor.binding(), successor);
}

#[test]
fn coherent_object_pages_prove_before_between_and_after_but_not_false_adjacency() {
    let successor = binding(2, "ab");
    let anchor = ByteOffset::new(1);
    let first = neighbor(1, 10);
    let second = neighbor(2, 20);
    let proves = |position, nonce| {
        let (text, objects) = admitted_sources(
            successor,
            "ab",
            position,
            vec![fact(1, 1, 10), fact(2, 1, 20)],
            nonce,
        );
        let proposal = proposal(
            successor,
            nonce,
            SourceRange::new(position, position).unwrap(),
            0,
        );
        let mut editor = editor(successor);
        editor.begin(proposal).unwrap();
        editor.accept_preflight(proposal.key()).unwrap();
        editor.reserve_source_positions(proposal.key(), &[position], &text, &objects)
    };
    for (position, nonce) in [
        SourcePosition::new(anchor, InlineObjectGap::before(first)),
        SourcePosition::new(anchor, InlineObjectGap::between(first, second).unwrap()),
        SourcePosition::new(anchor, InlineObjectGap::after(second)),
    ]
    .into_iter()
    .zip(130..)
    {
        assert!(proves(position, nonce).is_ok());
    }
    let unknown = neighbor(3, 30);
    assert_eq!(
        proves(
            SourcePosition::new(anchor, InlineObjectGap::between(first, unknown).unwrap(),),
            140,
        ),
        Err(MutationError::InvalidObjectGapProof)
    );
}

#[test]
fn independent_object_and_presentation_caps_reject_one_over_without_admission() {
    let base = binding(1, "a");
    let at = position(0);
    let proposal = proposal(base, 40, SourceRange::new(at, at).unwrap(), 0);
    let limits = MutationLimits::new(4, 8)
        .unwrap()
        .with_object_limits(1, 2, 1)
        .unwrap();
    let mut editor = RangeEditCoordinator::new(base, limits);
    editor.begin(proposal).unwrap();
    editor.accept_preflight(proposal.key()).unwrap();
    assert_eq!(
        editor.stage(MutationFragment::new(
            proposal.key(),
            0,
            MutationFragmentPayload::Object(ObjectChange::Insert {
                at,
                object: SuccessorObject::new(
                    InlineObjectId::new(1),
                    ByteOffset::new(0),
                    InlineObjectOrder::new(1),
                    2,
                    2,
                ),
            }),
        )),
        Err(MutationError::PresentationByteLimitExceeded)
    );
    assert_eq!(editor.state(), gpui_text_input::MutationState::Idle);
    assert_eq!(editor.counts().objects, 0);
    assert_eq!(editor.released_counts().transactions, 1);
    assert_eq!(
        editor.stage(terminal(proposal.key(), 0, position(0))),
        Err(MutationError::ObsoleteOperation(proposal.key()))
    );
}
