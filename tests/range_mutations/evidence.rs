use super::*;
use gpui_text_input::{MutationPass, MutationPassKind, MutationProducerIdentity};

fn evidence_editor(producer: u64) -> (RangeEditCoordinator, MutationKey, MutationPass) {
    let base = binding(1, "");
    let mut editor = make_editor(base, 4, 4096);
    let proposal = make_proposal(
        base,
        1,
        source_range(0, 0),
        MutationPositions::collapsed(position(0)),
        MutationKind::Edit,
    );
    editor
        .begin(
            MutationBeginRequest::new(proposal, MutationCursor::new(0), MutationCursor::new(0))
                .with_replayable_producer(MutationProducerIdentity::new(producer)),
        )
        .unwrap();
    let pass = editor.request_evidence(proposal.key()).unwrap();
    (editor, proposal.key(), pass)
}

fn generated_page(
    editor: &RangeEditCoordinator,
    key: MutationKey,
    ordinal: u64,
    changed: bool,
) -> MutationPage {
    let lane = if ordinal < 2 {
        MutationLane::Source
    } else {
        MutationLane::Proposal
    };
    let frontier = editor.stream_finish(key, lane).unwrap();
    let item = if ordinal == 302 {
        MutationPageItem::Object(ObjectChange::Insert {
            object: SuccessorObject::new(
                InlineObjectId::new(20),
                ByteOffset::new(150),
                InlineObjectOrder::new(1),
                1,
                1,
            ),
        })
    } else {
        MutationPageItem::Utf8 {
            inserted_offset: if lane == MutationLane::Proposal {
                ordinal - 2
            } else {
                ordinal
            },
            text: if changed { "z" } else { "x" }.into(),
        }
    };
    MutationPage::new(
        MutationPageKey::new(
            key,
            lane,
            frontier.next_cursor,
            frontier.next_ordinal,
            frontier.cumulative_identity,
        ),
        MutationCursor::new(frontier.next_cursor.get() + 1),
        vec![item],
    )
    .unwrap()
}

fn collect_evidence(
    editor: &mut RangeEditCoordinator,
    key: MutationKey,
    pass: MutationPass,
) -> MutationFinishInput {
    for ordinal in 0..303 {
        let page = generated_page(editor, key, ordinal, false);
        let payload = page.clone();
        let acknowledgement = editor.submit_evidence_page(pass, page).unwrap();
        assert_eq!(acknowledgement.request(), ordinal);
        assert_eq!(payload.payload_owner_count(), 1);
        assert_eq!(editor.counts().retained_bytes, 0);
        editor.acknowledge_evidence_page(acknowledgement).unwrap();
    }
    let finish = finish(
        editor,
        key,
        LogicalExtent::new(300, 1),
        MutationPositions::collapsed(position(300)),
    );
    editor.finish_evidence(pass, finish).unwrap();
    finish
}

fn admit_replay(editor: &mut RangeEditCoordinator, key: MutationKey) -> MutationPass {
    editor.accept_preflight(key, None).unwrap();
    let restart = editor.mutation_restart(key).unwrap();
    assert_eq!(restart.kind(), MutationPassKind::Staging);
    editor.acknowledge_restart(restart).unwrap();
    restart
}

#[test]
fn evidence_replays_generated_mixed_lanes_beyond_arbitrary_fragment_ceiling() {
    let (mut editor, key, pass) = evidence_editor(31);
    let closure = collect_evidence(&mut editor, key, pass);
    assert_eq!(closure.proposal().totals.objects, 1);
    assert_eq!(editor.binding(), binding(1, ""));
    let staging = admit_replay(&mut editor, key);
    assert_ne!(pass, staging);
    for ordinal in 0..303 {
        let page = generated_page(&editor, key, ordinal, false);
        editor.accept_pass_page(staging, page).unwrap();
    }
    editor.finish_pass_input(staging, closure).unwrap();
    editor.admit_commit(key).unwrap();
    assert_eq!(editor.state(), MutationState::CommitPending);
    assert_eq!(editor.counts().current_pages, 0);
    assert_eq!(
        editor.dispose(),
        Some(gpui_text_input::MutationDisposal::Detached(key))
    );
}

#[test]
fn evidence_waits_for_exact_acknowledgement_and_explicit_eof() {
    let (mut editor, key, pass) = evidence_editor(31);
    let page = generated_page(&editor, key, 0, false);
    let ack = editor.submit_evidence_page(pass, page.clone()).unwrap();
    assert_eq!(
        editor.submit_evidence_page(pass, page.clone()),
        Err(MutationError::EvidenceAcknowledgementPending)
    );
    let closure = finish(
        &mut editor,
        key,
        LogicalExtent::new(0, 0),
        MutationPositions::collapsed(position(0)),
    );
    assert_eq!(
        editor.finish_evidence(pass, closure),
        Err(MutationError::EvidenceAcknowledgementPending)
    );
    assert_eq!(
        editor.accept_preflight(key, None),
        Err(MutationError::MissingFinishInput)
    );
    editor.acknowledge_evidence_page(ack).unwrap();
    let replay_ack = editor.submit_evidence_page(pass, page).unwrap();
    assert!(replay_ack.request() > ack.request());
    assert_eq!(
        editor.acknowledge_evidence_page(ack),
        Err(MutationError::EvidenceAcknowledgementMismatch)
    );
    editor.acknowledge_evidence_page(replay_ack).unwrap();
    assert_eq!(
        editor.accept_preflight(key, None),
        Err(MutationError::MissingFinishInput)
    );
    editor.finish_evidence(pass, closure).unwrap();
    editor.accept_preflight(key, None).unwrap();
    let staging = editor.mutation_restart(key).unwrap();
    assert_eq!(
        editor.acknowledge_restart(pass),
        Err(MutationError::WrongMutationPass)
    );
    let page = generated_page(&editor, key, 0, false);
    assert_eq!(
        editor.accept_pass_page(staging, page),
        Err(MutationError::WrongMutationPass)
    );
    editor.acknowledge_restart(staging).unwrap();
    assert_eq!(
        editor.acknowledge_evidence_page(replay_ack),
        Err(MutationError::WrongMutationPass)
    );
    assert_eq!(
        editor.finish_evidence(pass, closure),
        Err(MutationError::WrongMutationPass)
    );
    assert!(editor.admit_commit(key).is_err());
}

#[test]
fn evidence_zero_page_edit_still_requires_finish_and_restart() {
    let (mut editor, key, pass) = evidence_editor(31);
    let closure = finish(
        &mut editor,
        key,
        LogicalExtent::new(0, 0),
        MutationPositions::collapsed(position(0)),
    );
    assert_eq!(
        editor.finish_input(closure),
        Err(MutationError::EvidenceRequired)
    );
    assert!(editor.admit_commit(key).is_err());
    editor.finish_evidence(pass, closure).unwrap();
    let staging = admit_replay(&mut editor, key);
    assert_eq!(
        editor.finish_input(closure),
        Err(MutationError::EvidenceRequired)
    );
    editor.finish_pass_input(staging, closure).unwrap();
    editor.admit_commit(key).unwrap();
}

#[test]
fn evidence_rejects_substituted_producer_and_bypass_without_changing_frontiers() {
    let (mut editor, key, pass) = evidence_editor(31);
    let (_, _, substituted) = evidence_editor(32);
    let page = generated_page(&editor, key, 0, false);
    assert_eq!(
        editor.submit_evidence_page(substituted, page.clone()),
        Err(MutationError::WrongMutationPass)
    );
    assert_eq!(
        editor.accept_page(page.clone()),
        Err(MutationError::EvidenceRequired)
    );
    assert_eq!(
        editor
            .stream_finish(key, MutationLane::Source)
            .unwrap()
            .next_ordinal,
        0
    );
    let ack = editor.submit_evidence_page(pass, page).unwrap();
    editor.acknowledge_evidence_page(ack).unwrap();
}

#[test]
fn evidence_changed_replay_or_truncated_lane_cannot_commit() {
    for changed in [false, true] {
        let (mut editor, key, pass) = evidence_editor(31);
        let closure = collect_evidence(&mut editor, key, pass);
        let staging = admit_replay(&mut editor, key);
        for ordinal in 0..if changed { 303 } else { 302 } {
            let page = generated_page(&editor, key, ordinal, changed && ordinal == 299);
            editor.accept_pass_page(staging, page).unwrap();
        }
        assert_eq!(
            editor.finish_pass_input(staging, closure),
            Err(MutationError::FinishMismatch)
        );
        assert_eq!(editor.state(), MutationState::Settled);
        assert!(editor.admit_commit(key).is_err());
        assert_eq!(editor.binding(), binding(1, ""));
    }
}

#[test]
fn evidence_rejects_changed_finish_extent_and_directed_endpoints() {
    for change_extent in [false, true] {
        let (mut editor, key, pass) = evidence_editor(31);
        let closure = collect_evidence(&mut editor, key, pass);
        let staging = admit_replay(&mut editor, key);
        let changed = MutationFinishInput::new(
            key,
            closure.source(),
            closure.proposal(),
            if change_extent {
                LogicalExtent::new(301, 1)
            } else {
                closure.intended_extent()
            },
            if change_extent {
                closure.intended()
            } else {
                MutationPositions::new(position(300), position(10), position(300))
            },
        );
        assert_eq!(
            editor.finish_pass_input(staging, changed),
            Err(MutationError::ReplayMismatch)
        );
        assert_eq!(editor.state(), MutationState::Settled);
    }
}

#[test]
fn evidence_cancellation_disposal_and_rebind_release_outstanding_receipts() {
    for action in 0..3 {
        let (mut editor, key, pass) = evidence_editor(31);
        let page = generated_page(&editor, key, 0, false);
        let ack = editor.submit_evidence_page(pass, page).unwrap();
        match action {
            0 => {
                editor.cancel(key).unwrap();
            }
            1 => {
                assert_eq!(
                    editor.dispose(),
                    Some(gpui_text_input::MutationDisposal::Cancelled(key))
                );
            }
            _ => {
                assert_eq!(
                    editor.rebind(binding(2, "x")),
                    Some(gpui_text_input::MutationDisposal::Cancelled(key))
                );
            }
        }
        assert_eq!(editor.counts().transactions, 0);
        assert!(editor.acknowledge_evidence_page(ack).is_err());
        assert!(editor.accept_preflight(key, None).is_err());
    }
}

#[test]
fn evidence_unavailable_producer_cannot_fall_through_to_admission() {
    let base = binding(1, "");
    let mut editor = make_editor(base, 4, 4096);
    let proposal = make_proposal(
        base,
        1,
        source_range(0, 0),
        MutationPositions::collapsed(position(0)),
        MutationKind::Edit,
    );
    editor
        .begin(MutationBeginRequest::new(
            proposal,
            MutationCursor::new(0),
            MutationCursor::new(0),
        ))
        .unwrap();
    assert_eq!(
        editor.request_evidence(proposal.key()),
        Err(MutationError::EvidenceUnavailable)
    );
    assert_eq!(
        editor.accept_preflight(proposal.key(), None),
        Err(MutationError::MissingFinishInput)
    );
    editor.reject_preflight(proposal.key()).unwrap();
}
