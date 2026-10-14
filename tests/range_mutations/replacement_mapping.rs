use super::*;
use gpui_text_input::{
    InlineObjectNeighbor, MutationPositionsIntent, MutationProducerIdentity, MutationSettlement,
};

fn neighbor(id: u64, order: u64) -> InlineObjectNeighbor {
    InlineObjectNeighbor::new(
        InlineObjectId::new(id.into()),
        InlineObjectOrder::new(order.into()),
    )
}

fn at(offset: u64, gap: InlineObjectGap) -> SourcePosition {
    SourcePosition::new(ByteOffset::new(offset), gap)
}

fn fact(id: u64, anchor: u64, order: u64) -> InlineObjectFact {
    InlineObjectFact::new(
        InlineObjectId::new(id.into()),
        ByteOffset::new(anchor),
        InlineObjectOrder::new(order.into()),
        "object".to_owned(),
        InlineObjectPresentation::new(
            1,
            "",
            px(8.0),
            px(8.0),
            gpui::Pixels::ZERO,
            None,
            0,
            false,
        )
        .unwrap(),
    )
}

fn mapped_editor(inserted: &str) -> (RangeEditCoordinator, MutationProposal, MutationFinishInput) {
    let base = binding(1, "abcd");
    let replacement = SourceRange::new(
        at(1, InlineObjectGap::after(neighbor(11, 900))),
        at(3, InlineObjectGap::before(neighbor(22, 1))),
    )
    .unwrap();
    let proposal = MutationProposal::new(
        key(base, 501),
        MutationKind::Edit,
        MutationPositions::new(replacement.start(), replacement.end(), replacement.start()),
        replacement,
        0,
    );
    let mut editor = make_editor(base, 4, 4096);
    begin(&mut editor, proposal);
    if !inserted.is_empty() {
        editor
            .accept_page(text_page(
                proposal.key(),
                MutationLane::Proposal,
                0,
                0,
                MutationIdentity::ROOT,
                1,
                0,
                inserted,
            ))
            .unwrap();
    }
    let finish = MutationFinishInput::with_positions_intent(
        proposal.key(),
        editor
            .stream_finish(proposal.key(), MutationLane::Source)
            .unwrap(),
        editor
            .stream_finish(proposal.key(), MutationLane::Proposal)
            .unwrap(),
        LogicalExtent::new(2 + inserted.len() as u64, 1),
        MutationPositionsIntent::ReplacementEnd {
            replacement,
            inserted_utf8_bytes: inserted.len() as u64,
        },
    );
    (editor, proposal, finish)
}

#[test]
fn replacement_end_accepts_fresh_successor_keys_and_rejects_valid_wrong_identity_side_or_selection()
{
    for inserted in ["", "é"] {
        let (mut editor, proposal, finish) = mapped_editor(inserted);
        editor.finish_input(finish).unwrap();
        editor.admit_commit(proposal.key()).unwrap();
        let successor_text = format!("a{inserted}d");
        let successor = binding(2, &successor_text);
        let collapse = 1 + inserted.len() as u64;
        let fresh_preceding = neighbor(11, 1);
        let fresh_following = neighbor(22, 2);
        let expected = at(
            collapse,
            if inserted.is_empty() {
                InlineObjectGap::between(fresh_preceding, fresh_following).unwrap()
            } else {
                InlineObjectGap::before(fresh_following)
            },
        );
        let facts = vec![fact(11, 1, 1), fact(22, collapse, 2)];
        let (text, objects) = admitted_sources(successor, &successor_text, expected, facts);
        let wrong_sides = if inserted.is_empty() {
            at(collapse, InlineObjectGap::before(fresh_preceding))
        } else {
            at(collapse, InlineObjectGap::after(fresh_following))
        };
        for wrong in [
            MutationPositions::collapsed(wrong_sides),
            MutationPositions::new(expected, position(0), expected),
        ] {
            assert_eq!(
                editor.settle_committed(proposal.key(), successor, wrong, &text, &objects),
                Err(MutationError::WrongSuccessorPositions)
            );
            assert_eq!(editor.binding(), binding(1, "abcd"));
            assert_eq!(editor.state(), MutationState::CommitPending);
        }
        let other_following = neighbor(33, 2);
        let other_expected = at(
            collapse,
            if inserted.is_empty() {
                InlineObjectGap::between(fresh_preceding, other_following).unwrap()
            } else {
                InlineObjectGap::before(other_following)
            },
        );
        let (other_text, other_objects) = admitted_sources(
            successor,
            &successor_text,
            other_expected,
            vec![fact(11, 1, 1), fact(33, collapse, 2)],
        );
        assert_eq!(
            editor.settle_committed(
                proposal.key(),
                successor,
                MutationPositions::collapsed(other_expected),
                &other_text,
                &other_objects
            ),
            Err(MutationError::WrongSuccessorPositions)
        );
        let old_keys = at(collapse, InlineObjectGap::before(neighbor(22, 1)));
        assert!(
            editor
                .settle_committed(
                    proposal.key(),
                    successor,
                    MutationPositions::collapsed(old_keys),
                    &text,
                    &objects
                )
                .is_err()
        );
        assert!(matches!(
            editor
                .settle_committed(
                    proposal.key(),
                    successor,
                    MutationPositions::collapsed(expected),
                    &text,
                    &objects
                )
                .unwrap(),
            MutationSettlement::Current(_)
        ));
        assert_eq!(editor.binding(), successor);
        assert_eq!(editor.counts().transactions, 0);
        assert_eq!(editor.counts().retained_bytes, 0);
    }
}

#[test]
fn replacement_end_finish_rejects_changed_range_totals_or_extent_and_remains_cancellable() {
    for changed in 0..4 {
        let (mut editor, proposal, finish) = mapped_editor("é");
        let bad = MutationFinishInput::with_positions_intent(
            proposal.key(),
            finish.source(),
            finish.proposal(),
            if changed == 2 {
                LogicalExtent::new(5, 1)
            } else {
                finish.intended_extent()
            },
            MutationPositionsIntent::ReplacementEnd {
                replacement: if changed == 0 {
                    source_range(1, 3)
                } else {
                    proposal.replacement()
                },
                inserted_utf8_bytes: if changed == 1 {
                    1
                } else if changed == 3 {
                    u64::MAX
                } else {
                    2
                },
            },
        );
        assert!(editor.finish_input(bad).is_err());
        assert_eq!(editor.state(), MutationState::InputStreaming);
        assert_eq!(editor.binding(), binding(1, "abcd"));
        editor.cancel(proposal.key()).unwrap();
        assert_eq!(editor.counts().transactions, 0);
        assert_eq!(editor.counts().retained_bytes, 0);
    }
}

#[test]
fn replacement_end_refuses_object_insertion_and_atom_effects() {
    for atom in [false, true] {
        let (mut editor, proposal, _) = mapped_editor("");
        let item = if atom {
            MutationPageItem::Atom(gpui_text_input::AtomChange::Remove {
                id: gpui_text_input::AtomId::new(1),
                source_range: ByteRange::from_u64(1, 2).unwrap(),
            })
        } else {
            MutationPageItem::Object(ObjectChange::Insert {
                object: SuccessorObject::new(
                    InlineObjectId::new(33),
                    ByteOffset::new(1),
                    InlineObjectOrder::new(3),
                    0,
                    0,
                ),
            })
        };
        editor
            .accept_page(
                MutationPage::new(
                    MutationPageKey::new(
                        proposal.key(),
                        MutationLane::Proposal,
                        MutationCursor::new(0),
                        0,
                        MutationIdentity::ROOT,
                    ),
                    MutationCursor::new(1),
                    vec![item],
                )
                .unwrap(),
            )
            .unwrap();
        let finish = MutationFinishInput::with_positions_intent(
            proposal.key(),
            editor
                .stream_finish(proposal.key(), MutationLane::Source)
                .unwrap(),
            editor
                .stream_finish(proposal.key(), MutationLane::Proposal)
                .unwrap(),
            LogicalExtent::new(2, 1),
            MutationPositionsIntent::ReplacementEnd {
                replacement: proposal.replacement(),
                inserted_utf8_bytes: 0,
            },
        );
        assert_eq!(
            editor.finish_input(finish),
            Err(MutationError::WrongSuccessorPositions)
        );
        assert_eq!(editor.binding(), binding(1, "abcd"));
        editor.cancel(proposal.key()).unwrap();
    }
}

#[test]
fn replacement_end_evidence_freezes_intent_and_distinguishes_exact_commit_identity() {
    let base = binding(1, "abcd");
    for substitute_exact in [false, true] {
        let (mut original, proposal, closure) = mapped_editor("");
        original.finish_input(closure).unwrap();
        let mapped_identity = original
            .admit_commit(proposal.key())
            .unwrap()
            .finish_identity();
        let mut exact_editor = make_editor(base, 4, 4096);
        begin(&mut exact_editor, proposal);
        let exact_finish = MutationFinishInput::new(
            proposal.key(),
            closure.source(),
            closure.proposal(),
            closure.intended_extent(),
            MutationPositions::collapsed(position(1)),
        );
        exact_editor.finish_input(exact_finish).unwrap();
        assert_ne!(
            mapped_identity,
            exact_editor
                .admit_commit(proposal.key())
                .unwrap()
                .finish_identity()
        );

        let mut editor = make_editor(base, 4, 4096);
        editor
            .begin(
                MutationBeginRequest::new(proposal, MutationCursor::new(0), MutationCursor::new(0))
                    .with_replayable_producer(MutationProducerIdentity::new(1)),
            )
            .unwrap();
        let pass = editor.request_evidence(proposal.key()).unwrap();
        editor.finish_evidence(pass, closure).unwrap();
        editor.accept_preflight(proposal.key(), None).unwrap();
        let staging = editor.mutation_restart(proposal.key()).unwrap();
        editor.acknowledge_restart(staging).unwrap();
        let changed = if substitute_exact {
            exact_finish
        } else {
            MutationFinishInput::with_positions_intent(
                proposal.key(),
                closure.source(),
                closure.proposal(),
                closure.intended_extent(),
                MutationPositionsIntent::ReplacementEnd {
                    replacement: proposal.replacement(),
                    inserted_utf8_bytes: 1,
                },
            )
        };
        assert_eq!(
            editor.finish_pass_input(staging, changed),
            Err(MutationError::ReplayMismatch)
        );
        assert_eq!(editor.binding(), base);
        assert_eq!(editor.state(), MutationState::Settled);
        assert_eq!(editor.counts().transactions, 0);
        assert_eq!(editor.counts().retained_bytes, 0);
    }
}
