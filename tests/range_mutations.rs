use gpui::{Hsla, px};
use gpui_text_input::{
    BindingId, ByteOffset, ByteRange, InlineObjectFact, InlineObjectGap, InlineObjectId,
    InlineObjectOrder, InlineObjectPresentation, LogicalExtent, MutationBeginRequest,
    MutationCursor, MutationError, MutationFinishInput, MutationIdentity, MutationKey,
    MutationKind, MutationLane, MutationLimits, MutationOutcome, MutationPage,
    MutationPageAcceptance, MutationPageItem, MutationPageKey, MutationPositions, MutationProposal,
    MutationState, ObjectChange, ObjectDemand, ObjectDemandEnvelope, ObjectDirection, ObjectPage,
    ObjectPageEdgeFact, ObjectPageId, ObjectPurpose, ObjectRequestId, ObjectResidency,
    ObjectResidencyLimits, OperationId, PageDemand, PageDemandEnvelope, PageDirection,
    PageEdgeFact, PageId, PagePurpose, PageRequestId, PresentationGeneration, RangeBinding,
    RangeEditCoordinator, RangePage, RangeResidency, ResidencyLimits, SourcePosition, SourceRange,
    SourceRevision, SuccessorObject,
};

fn binding(revision: u64, text: &str) -> RangeBinding {
    let lines = if text.is_empty() {
        0
    } else {
        text.bytes().filter(|byte| *byte == b'\n').count() as u64 + 1
    };
    RangeBinding::new(
        BindingId::new(7),
        SourceRevision::new(revision),
        LogicalExtent::new(text.len() as u64, lines),
    )
}

fn position(offset: u64) -> SourcePosition {
    SourcePosition::new(ByteOffset::new(offset), InlineObjectGap::NoObjects)
}

fn source_range(start: u64, end: u64) -> SourceRange {
    SourceRange::new(position(start), position(end)).unwrap()
}

fn object_target(
    anchor: u64,
    id: InlineObjectId,
    order: InlineObjectOrder,
) -> gpui_text_input::ObjectTarget {
    let neighbor = gpui_text_input::InlineObjectNeighbor::new(id, order);
    gpui_text_input::ObjectTarget::new(
        SourceRange::new(
            SourcePosition::new(ByteOffset::new(anchor), InlineObjectGap::Before(neighbor)),
            SourcePosition::new(ByteOffset::new(anchor), InlineObjectGap::After(neighbor)),
        )
        .unwrap(),
        id,
        order,
    )
    .unwrap()
}

fn key(binding: RangeBinding, operation: u64) -> MutationKey {
    MutationKey::new(
        binding.binding(),
        binding.revision(),
        OperationId::new(operation),
    )
}

fn make_proposal(
    binding: RangeBinding,
    operation: u64,
    range: SourceRange,
    _intended: MutationPositions,
    kind: MutationKind,
) -> MutationProposal {
    MutationProposal::new(
        key(binding, operation),
        kind,
        MutationPositions::collapsed(range.end()),
        range,
        0,
    )
}

fn make_editor(binding: RangeBinding, items: usize, bytes: usize) -> RangeEditCoordinator {
    RangeEditCoordinator::new(
        binding,
        MutationLimits::new(items, bytes)
            .unwrap()
            .with_object_limits(items, bytes, bytes)
            .unwrap(),
    )
}

fn begin(editor: &mut RangeEditCoordinator, proposal: MutationProposal) {
    editor
        .begin(MutationBeginRequest::new(
            proposal,
            MutationCursor::new(0),
            MutationCursor::new(0),
        ))
        .unwrap();
    assert_eq!(editor.state(), MutationState::PreflightPending);
    editor.accept_preflight(proposal.key(), None).unwrap();
    assert_eq!(editor.state(), MutationState::InputStreaming);
}

fn text_page(
    key: MutationKey,
    lane: MutationLane,
    cursor: u64,
    ordinal: u64,
    prior: MutationIdentity,
    next: u64,
    inserted_offset: u64,
    text: &str,
) -> MutationPage {
    MutationPage::new(
        MutationPageKey::new(key, lane, MutationCursor::new(cursor), ordinal, prior),
        MutationCursor::new(next),
        vec![MutationPageItem::Utf8 {
            inserted_offset,
            text: text.into(),
        }],
    )
    .unwrap()
}

fn finish(
    editor: &mut RangeEditCoordinator,
    key: MutationKey,
    intended_extent: LogicalExtent,
    intended: MutationPositions,
) -> MutationFinishInput {
    MutationFinishInput::new(
        key,
        editor.stream_finish(key, MutationLane::Source).unwrap(),
        editor.stream_finish(key, MutationLane::Proposal).unwrap(),
        intended_extent,
        intended,
    )
}

fn empty_finish_identity(
    base: RangeBinding,
    kind: MutationKind,
    predecessor: MutationPositions,
    replacement: SourceRange,
    intended_extent: LogicalExtent,
    intended: MutationPositions,
) -> MutationIdentity {
    let proposal = MutationProposal::new(key(base, 91), kind, predecessor, replacement, 0);
    let mut editor = make_editor(base, 4, 4096);
    begin(&mut editor, proposal);
    let finish = finish(&mut editor, proposal.key(), intended_extent, intended);
    editor.finish_input(finish).unwrap();
    editor
        .admit_commit(proposal.key())
        .unwrap()
        .finish_identity()
}

#[test]
fn finish_identity_closes_begin_direction_kind_range_and_intended_result() {
    let base = binding(1, "ab");
    let forward = MutationPositions::new(position(1), position(0), position(1));
    let reversed = MutationPositions::new(position(0), position(1), position(0));
    let unchanged_extent = LogicalExtent::new(2, 1);
    let baseline = empty_finish_identity(
        base,
        MutationKind::Edit,
        forward,
        source_range(0, 0),
        unchanged_extent,
        MutationPositions::collapsed(position(1)),
    );
    let vectors = [
        empty_finish_identity(
            base,
            MutationKind::Edit,
            reversed,
            source_range(0, 0),
            unchanged_extent,
            MutationPositions::collapsed(position(1)),
        ),
        empty_finish_identity(
            base,
            MutationKind::Undo,
            forward,
            source_range(0, 0),
            unchanged_extent,
            MutationPositions::collapsed(position(1)),
        ),
        empty_finish_identity(
            base,
            MutationKind::Edit,
            forward,
            source_range(0, 1),
            LogicalExtent::new(1, 1),
            MutationPositions::collapsed(position(1)),
        ),
        empty_finish_identity(
            base,
            MutationKind::Edit,
            forward,
            source_range(0, 0),
            unchanged_extent,
            MutationPositions::collapsed(position(0)),
        ),
        empty_finish_identity(
            RangeBinding::new(base.binding(), base.revision(), LogicalExtent::new(3, 1)),
            MutationKind::Edit,
            forward,
            source_range(0, 0),
            LogicalExtent::new(3, 1),
            MutationPositions::collapsed(position(1)),
        ),
    ];
    assert!(vectors.into_iter().all(|identity| identity != baseline));
}

#[test]
fn more_than_257_pages_keep_fixed_residency_and_checked_chain() {
    let base = binding(1, "");
    let intended = MutationPositions::collapsed(position(300));
    let proposal = make_proposal(base, 1, source_range(0, 0), intended, MutationKind::Edit);
    let mut editor = make_editor(base, 1, 1);
    begin(&mut editor, proposal);
    let mut cursor = 0;
    let mut prior = MutationIdentity::ROOT;
    for ordinal in 0..300 {
        let page = text_page(
            proposal.key(),
            MutationLane::Proposal,
            cursor,
            ordinal,
            prior,
            cursor + 1,
            ordinal,
            "x",
        );
        prior = page.cumulative_identity();
        assert!(matches!(
            editor.accept_page(page).unwrap(),
            MutationPageAcceptance::Accepted { .. }
        ));
        cursor += 1;
        let counts = editor.counts();
        assert_eq!(counts.current_pages, 0);
        assert_eq!(counts.retained_bytes, 0);
        assert_eq!(counts.transactions, 1);
    }
    let finish = finish(
        &mut editor,
        proposal.key(),
        LogicalExtent::new(300, 1),
        intended,
    );
    assert_eq!(finish.proposal().totals.pages, 300);
    assert_eq!(finish.proposal().totals.inserted_bytes, 300);
    editor.finish_input(finish).unwrap();
    editor.admit_commit(proposal.key()).unwrap();
}

#[test]
fn page_item_and_byte_caps_accept_exact_fit_and_reject_one_over() {
    let base = binding(1, "");
    let intended = MutationPositions::collapsed(position(4));
    let proposal = make_proposal(base, 2, source_range(0, 0), intended, MutationKind::Edit);
    let mut editor = make_editor(base, 2, 4);
    begin(&mut editor, proposal);
    let exact = MutationPage::new(
        MutationPageKey::new(
            proposal.key(),
            MutationLane::Proposal,
            MutationCursor::new(0),
            0,
            MutationIdentity::ROOT,
        ),
        MutationCursor::new(1),
        vec![
            MutationPageItem::Utf8 {
                inserted_offset: 0,
                text: "ab".into(),
            },
            MutationPageItem::Utf8 {
                inserted_offset: 2,
                text: "cd".into(),
            },
        ],
    )
    .unwrap();
    editor.accept_page(exact).unwrap();

    let other = make_proposal(base, 3, source_range(0, 0), intended, MutationKind::Edit);
    let mut item_editor = make_editor(base, 1, 4);
    begin(&mut item_editor, other);
    let two_items = MutationPage::new(
        MutationPageKey::new(
            other.key(),
            MutationLane::Proposal,
            MutationCursor::new(0),
            0,
            MutationIdentity::ROOT,
        ),
        MutationCursor::new(1),
        vec![
            MutationPageItem::Utf8 {
                inserted_offset: 0,
                text: "a".into(),
            },
            MutationPageItem::Utf8 {
                inserted_offset: 1,
                text: "b".into(),
            },
        ],
    )
    .unwrap();
    assert_eq!(
        item_editor.accept_page(two_items),
        Err(MutationError::PageItemLimitExceeded)
    );

    let third = make_proposal(base, 4, source_range(0, 0), intended, MutationKind::Edit);
    let mut byte_editor = make_editor(base, 2, 4);
    begin(&mut byte_editor, third);
    let over = text_page(
        third.key(),
        MutationLane::Proposal,
        0,
        0,
        MutationIdentity::ROOT,
        1,
        0,
        "abcde",
    );
    assert_eq!(
        byte_editor.accept_page(over),
        Err(MutationError::PageByteLimitExceeded)
    );
}

#[test]
fn cursor_ordinal_prior_replay_collision_and_retirement_are_exact() {
    let base = binding(1, "");
    let intended = MutationPositions::collapsed(position(2));
    let proposal = make_proposal(base, 5, source_range(0, 0), intended, MutationKind::Edit);
    let mut editor = make_editor(base, 1, 8);
    begin(&mut editor, proposal);
    let page = text_page(
        proposal.key(),
        MutationLane::Proposal,
        0,
        0,
        MutationIdentity::ROOT,
        1,
        0,
        "a",
    );
    let replay = page.clone();
    editor.accept_page(page).unwrap();
    assert_eq!(
        editor.accept_page(replay).unwrap(),
        MutationPageAcceptance::Replay
    );
    let collision = text_page(
        proposal.key(),
        MutationLane::Proposal,
        0,
        0,
        MutationIdentity::ROOT,
        1,
        0,
        "b",
    );
    assert_eq!(
        editor.accept_page(collision),
        Err(MutationError::PageCollision)
    );
    assert_eq!(editor.state(), MutationState::Settled);
    let late = text_page(
        proposal.key(),
        MutationLane::Proposal,
        0,
        0,
        MutationIdentity::ROOT,
        1,
        0,
        "a",
    );
    assert_eq!(
        editor.accept_page(late),
        Err(MutationError::ObsoleteOperation(proposal.key()))
    );

    let proposal = make_proposal(base, 6, source_range(0, 0), intended, MutationKind::Edit);
    let mut editor = make_editor(base, 1, 8);
    begin(&mut editor, proposal);
    let wrong_cursor = text_page(
        proposal.key(),
        MutationLane::Proposal,
        9,
        0,
        MutationIdentity::ROOT,
        10,
        0,
        "a",
    );
    assert_eq!(
        editor.accept_page(wrong_cursor),
        Err(MutationError::CursorMismatch)
    );
    let wrong_ordinal = text_page(
        proposal.key(),
        MutationLane::Proposal,
        0,
        2,
        MutationIdentity::ROOT,
        1,
        0,
        "a",
    );
    assert_eq!(
        editor.accept_page(wrong_ordinal),
        Err(MutationError::OrdinalMismatch {
            expected: 0,
            actual: 2
        })
    );
    let wrong_prior = text_page(
        proposal.key(),
        MutationLane::Proposal,
        0,
        0,
        MutationIdentity::new([1; 4]),
        1,
        0,
        "a",
    );
    assert_eq!(
        editor.accept_page(wrong_prior),
        Err(MutationError::PriorIdentityMismatch)
    );
}

#[test]
fn explicit_empty_finish_is_distinct_from_absent_finish_and_closes_input() {
    let base = binding(1, "abc");
    let intended = MutationPositions::collapsed(position(0));
    let proposal = make_proposal(base, 7, source_range(0, 3), intended, MutationKind::Edit);
    let mut editor = make_editor(base, 1, 8);
    begin(&mut editor, proposal);
    assert!(matches!(
        editor.admit_commit(proposal.key()),
        Err(MutationError::WrongState { .. })
    ));
    let finish = finish(
        &mut editor,
        proposal.key(),
        LogicalExtent::new(0, 0),
        intended,
    );
    assert_eq!(finish.proposal().totals.pages, 0);
    let mut unauthenticated_proposal = finish.proposal();
    unauthenticated_proposal.cumulative_identity = MutationIdentity::new([1; 4]);
    let unauthenticated = MutationFinishInput::new(
        proposal.key(),
        finish.source(),
        unauthenticated_proposal,
        finish.intended_extent(),
        intended,
    );
    assert_eq!(
        editor.finish_input(unauthenticated),
        Err(MutationError::FinishMismatch)
    );
    assert_eq!(editor.state(), MutationState::InputStreaming);
    editor.finish_input(finish).unwrap();
    let late = text_page(
        proposal.key(),
        MutationLane::Proposal,
        0,
        0,
        MutationIdentity::ROOT,
        1,
        0,
        "x",
    );
    assert_eq!(
        editor.accept_page(late),
        Err(MutationError::PostFinishInput)
    );
    editor.admit_commit(proposal.key()).unwrap();
}

#[test]
fn accepted_page_payload_releases_immediately_and_cancel_releases_once() {
    let base = binding(1, "");
    let intended = MutationPositions::collapsed(position(1));
    let proposal = make_proposal(base, 8, source_range(0, 0), intended, MutationKind::Edit);
    let mut editor = make_editor(base, 1, 8);
    begin(&mut editor, proposal);
    let page = text_page(
        proposal.key(),
        MutationLane::Proposal,
        0,
        0,
        MutationIdentity::ROOT,
        1,
        0,
        "x",
    );
    let witness = page.clone();
    assert_eq!(witness.payload_owner_count(), 2);
    editor.accept_page(page).unwrap();
    assert_eq!(witness.payload_owner_count(), 1);
    assert_eq!(
        editor.cancel(proposal.key()).unwrap(),
        gpui_text_input::MutationCancellation::Cancelled
    );
    assert_eq!(editor.released_counts().transactions, 1);
    assert_eq!(
        editor.cancel(proposal.key()),
        Err(MutationError::ObsoleteOperation(proposal.key()))
    );
    assert_eq!(editor.released_counts().transactions, 1);
}

#[test]
fn precommit_cancel_and_post_admission_detach_have_one_terminal_result() {
    let base = binding(1, "");
    let intended = MutationPositions::collapsed(position(0));
    let first = make_proposal(base, 9, source_range(0, 0), intended, MutationKind::Edit);
    let mut editor = make_editor(base, 1, 8);
    begin(&mut editor, first);
    assert_eq!(
        editor.cancel(first.key()).unwrap(),
        gpui_text_input::MutationCancellation::Cancelled
    );

    let second = make_proposal(base, 10, source_range(0, 0), intended, MutationKind::Edit);
    let mut editor = make_editor(base, 1, 8);
    begin(&mut editor, second);
    let second_finish = finish(
        &mut editor,
        second.key(),
        LogicalExtent::new(0, 0),
        intended,
    );
    editor.finish_input(second_finish).unwrap();
    editor.admit_commit(second.key()).unwrap();
    assert_eq!(
        editor.cancel(second.key()).unwrap(),
        gpui_text_input::MutationCancellation::AwaitingHostSettlement
    );
    let replacement = binding(77, "other");
    assert_eq!(
        editor.rebind(replacement),
        Some(gpui_text_input::MutationDisposal::Detached(second.key()))
    );
    assert_eq!(editor.state(), MutationState::CommitPending);
    assert_eq!(
        editor
            .settle(second.key(), MutationOutcome::Rejected)
            .unwrap(),
        gpui_text_input::MutationSettlement::Obsolete(MutationOutcome::Rejected)
    );
    assert_eq!(editor.released_counts().transactions, 1);
    assert_eq!(
        editor.settle(second.key(), MutationOutcome::Rejected),
        Err(MutationError::ObsoleteOperation(second.key()))
    );

    let third = make_proposal(base, 11, source_range(0, 0), intended, MutationKind::Edit);
    let mut disposed = make_editor(base, 1, 8);
    begin(&mut disposed, third);
    let finish = finish(
        &mut disposed,
        third.key(),
        LogicalExtent::new(0, 0),
        intended,
    );
    disposed.finish_input(finish).unwrap();
    disposed.admit_commit(third.key()).unwrap();
    assert_eq!(
        disposed.dispose(),
        Some(gpui_text_input::MutationDisposal::Detached(third.key()))
    );
    assert_eq!(
        disposed
            .settle(third.key(), MutationOutcome::Conflict)
            .unwrap(),
        gpui_text_input::MutationSettlement::Obsolete(MutationOutcome::Conflict)
    );
    assert_eq!(disposed.dispose(), None);
}

#[test]
fn directed_predecessor_and_successor_object_positions_survive_streaming() {
    let base = binding(1, "abc");
    let predecessor = MutationPositions::new(position(1), position(3), position(1));
    let intended = MutationPositions::new(position(2), position(3), position(2));
    let proposal = MutationProposal::new(
        key(base, 11),
        MutationKind::Undo,
        predecessor,
        source_range(1, 3),
        0,
    );
    assert_eq!(proposal.predecessor(), predecessor);
    let mut editor = make_editor(base, 3, 16);
    begin(&mut editor, proposal);
    let page = MutationPage::new(
        MutationPageKey::new(
            proposal.key(),
            MutationLane::Proposal,
            MutationCursor::new(0),
            0,
            MutationIdentity::ROOT,
        ),
        MutationCursor::new(1),
        vec![
            MutationPageItem::Utf8 {
                inserted_offset: 0,
                text: "éx".into(),
            },
            MutationPageItem::Object(ObjectChange::Insert {
                object: SuccessorObject::new(
                    InlineObjectId::new(1),
                    ByteOffset::new(2),
                    InlineObjectOrder::new(10),
                    1,
                    1,
                ),
            }),
            MutationPageItem::Object(ObjectChange::Insert {
                object: SuccessorObject::new(
                    InlineObjectId::new(2),
                    ByteOffset::new(2),
                    InlineObjectOrder::new(20),
                    1,
                    1,
                ),
            }),
        ],
    )
    .unwrap();
    editor.accept_page(page).unwrap();
    let finish = finish(
        &mut editor,
        proposal.key(),
        LogicalExtent::new(4, 1),
        intended,
    );
    editor.finish_input(finish).unwrap();
    editor.admit_commit(proposal.key()).unwrap();
}

#[test]
fn host_and_history_pages_derive_compact_active_object_effects() {
    let base = binding(1, "ab");
    let tracked = (InlineObjectId::new(1), InlineObjectOrder::new(10));
    let cases = [
        (
            MutationKind::Edit,
            ObjectChange::Remove {
                target: object_target(1, tracked.0, tracked.1),
            },
            Some(gpui_text_input::ActiveObjectEffect::Removed {
                id: tracked.0,
                order: tracked.1,
            }),
        ),
        (
            MutationKind::Undo,
            ObjectChange::Replace {
                target: object_target(1, tracked.0, tracked.1),
                object: SuccessorObject::new(
                    InlineObjectId::new(2),
                    ByteOffset::new(0),
                    InlineObjectOrder::new(20),
                    1,
                    1,
                ),
            },
            Some(gpui_text_input::ActiveObjectEffect::Replaced {
                id: tracked.0,
                order: tracked.1,
            }),
        ),
        (
            MutationKind::Redo,
            ObjectChange::Move {
                target: object_target(1, tracked.0, tracked.1),
                object: SuccessorObject::new(
                    tracked.0,
                    ByteOffset::new(0),
                    InlineObjectOrder::new(20),
                    1,
                    1,
                ),
            },
            None,
        ),
        (
            MutationKind::Edit,
            ObjectChange::Remove {
                target: object_target(1, InlineObjectId::new(9), InlineObjectOrder::new(90)),
            },
            None,
        ),
    ];
    for (index, (kind, change, expected)) in cases.into_iter().enumerate() {
        let proposal = MutationProposal::new(
            key(base, 120 + index as u64),
            kind,
            MutationPositions::collapsed(position(0)),
            source_range(0, 2),
            0,
        );
        let mut editor = make_editor(base, 4, 4096);
        let begin =
            MutationBeginRequest::new(proposal, MutationCursor::new(0), MutationCursor::new(0));
        editor.begin(begin).unwrap();
        editor
            .accept_preflight(proposal.key(), Some(tracked))
            .unwrap();
        let page = MutationPage::new(
            MutationPageKey::new(
                proposal.key(),
                MutationLane::Proposal,
                MutationCursor::new(0),
                0,
                MutationIdentity::ROOT,
            ),
            MutationCursor::new(1),
            vec![MutationPageItem::Object(change)],
        )
        .unwrap();
        editor.accept_page(page).unwrap();
        assert_eq!(editor.active_object_effect(), expected);
    }

    let unchanged = MutationProposal::new(
        key(base, 130),
        MutationKind::Undo,
        MutationPositions::collapsed(position(0)),
        source_range(0, 0),
        0,
    );
    let mut editor = make_editor(base, 4, 4096);
    editor
        .begin(MutationBeginRequest::new(
            unchanged,
            MutationCursor::new(0),
            MutationCursor::new(0),
        ))
        .unwrap();
    editor
        .accept_preflight(unchanged.key(), Some(tracked))
        .unwrap();
    assert_eq!(editor.active_object_effect(), None);
}

fn presentation() -> InlineObjectPresentation {
    InlineObjectPresentation::new(1, "obj", px(10.0), px(10.0), px(8.0), None::<Hsla>, 0, true)
        .unwrap()
}

fn admitted_sources(
    binding: RangeBinding,
    text: &str,
    _at: SourcePosition,
    facts: Vec<InlineObjectFact>,
) -> (RangeResidency, ObjectResidency) {
    let mut text_residency =
        RangeResidency::new(binding, ResidencyLimits::new(4, 4096, 4, 4096).unwrap());
    let PageDemand::Requested(request) = text_residency
        .demand(
            PageRequestId::new(1),
            PagePurpose::Caret,
            PageDemandEnvelope::Adjacent {
                anchor: ByteOffset::new(0),
                direction: PageDirection::Forward,
                max_payload_bytes: (text.len() as u64).max(4),
            },
        )
        .unwrap()
    else {
        panic!()
    };
    text_residency
        .admit(
            RangePage::new(
                PageId::new(1),
                request.key(),
                ByteRange::from_u64(0, text.len() as u64).unwrap(),
                text.to_owned(),
                vec![],
                PageEdgeFact::DocumentBoundary,
                PageEdgeFact::DocumentBoundary,
                true,
            )
            .unwrap(),
        )
        .unwrap();
    let mut object_residency = ObjectResidency::new(
        binding,
        PresentationGeneration::new(1),
        ObjectResidencyLimits::new(8, 16, 4096, 4096, 8, 16, 4096).unwrap(),
    );
    let demand = ObjectDemandEnvelope::range(
        ByteRange::from_u64(0, text.len() as u64).unwrap(),
        None,
        ObjectDirection::Forward,
        facts.len().max(1),
        4096,
    )
    .unwrap();
    let ObjectDemand::Requested(request) = object_residency
        .demand(
            ObjectRequestId::new(2),
            ObjectPurpose::MutationSuccessor,
            demand,
        )
        .unwrap()
    else {
        panic!()
    };
    let page = ObjectPage::new(
        ObjectPageId::new(2),
        request.key(),
        facts,
        ObjectPageEdgeFact::EnvelopeBoundary,
        ObjectPageEdgeFact::EnvelopeBoundary,
        true,
        None,
    )
    .unwrap();
    let proofs = text_residency
        .prove_object_page_anchors(binding, &page)
        .unwrap();
    object_residency.admit(page, proofs).unwrap();
    (text_residency, object_residency)
}

#[test]
fn committed_result_rejects_wrong_binding_revision_extent_positions_scalar_and_gap() {
    let base = binding(1, "");
    let intended = MutationPositions::collapsed(position(2));
    let proposal = make_proposal(base, 12, source_range(0, 0), intended, MutationKind::Edit);
    let mut editor = make_editor(base, 1, 8);
    begin(&mut editor, proposal);
    let page = text_page(
        proposal.key(),
        MutationLane::Proposal,
        0,
        0,
        MutationIdentity::ROOT,
        1,
        0,
        "é",
    );
    editor.accept_page(page).unwrap();
    let finish = finish(
        &mut editor,
        proposal.key(),
        LogicalExtent::new(2, 1),
        intended,
    );
    editor.finish_input(finish).unwrap();
    editor.admit_commit(proposal.key()).unwrap();

    let wrong_binding = RangeBinding::new(
        BindingId::new(8),
        SourceRevision::new(2),
        LogicalExtent::new(2, 1),
    );
    let (text, objects) = admitted_sources(wrong_binding, "é", position(2), vec![]);
    assert_eq!(
        editor.settle_committed(proposal.key(), wrong_binding, intended, &text, &objects,),
        Err(MutationError::IncoherentSuccessor)
    );

    let wrong = binding(1, "é");
    let (text, objects) = admitted_sources(wrong, "é", position(2), vec![]);
    assert_eq!(
        editor.settle_committed(proposal.key(), wrong, intended, &text, &objects),
        Err(MutationError::IncoherentSuccessor)
    );

    let wrong_extent = RangeBinding::new(
        base.binding(),
        SourceRevision::new(2),
        LogicalExtent::new(3, 1),
    );
    let (text, objects) = admitted_sources(wrong_extent, "éx", position(2), vec![]);
    assert_eq!(
        editor.settle_committed(proposal.key(), wrong_extent, intended, &text, &objects),
        Err(MutationError::IncoherentSuccessor)
    );

    let successor = binding(2, "é");
    let wrong_caret = MutationPositions::new(position(0), position(2), position(2));
    let (text, objects) = admitted_sources(successor, "é", position(2), vec![]);
    assert_eq!(
        editor.settle_committed(proposal.key(), successor, wrong_caret, &text, &objects),
        Err(MutationError::WrongSuccessorPositions)
    );

    let wrong_selection = MutationPositions::new(position(2), position(0), position(2));
    assert_eq!(
        editor.settle_committed(proposal.key(), successor, wrong_selection, &text, &objects,),
        Err(MutationError::WrongSuccessorPositions)
    );

    let wrong_scalar = MutationPositions::collapsed(position(1));
    let (text, objects) = admitted_sources(successor, "é", position(1), vec![]);
    assert_eq!(
        editor.settle_committed(proposal.key(), successor, wrong_scalar, &text, &objects),
        Err(MutationError::InvalidTextBoundaryProof)
    );

    let object = InlineObjectFact::new(
        InlineObjectId::new(9),
        ByteOffset::new(2),
        InlineObjectOrder::new(1),
        "obj",
        presentation(),
    );
    let (text, objects) = admitted_sources(successor, "é", position(2), vec![object]);
    assert_eq!(
        editor.settle_committed(proposal.key(), successor, intended, &text, &objects),
        Err(MutationError::InvalidObjectGapProof)
    );

    let (text, objects) = admitted_sources(successor, "é", position(2), vec![]);
    assert!(matches!(
        editor
            .settle_committed(proposal.key(), successor, intended, &text, &objects)
            .unwrap(),
        gpui_text_input::MutationSettlement::Current(MutationOutcome::Committed(_))
    ));
    assert_eq!(editor.state(), MutationState::Settled);
    let released = editor.released_counts();
    assert_eq!(
        editor.settle_committed(proposal.key(), successor, intended, &text, &objects),
        Err(MutationError::ObsoleteOperation(proposal.key()))
    );
    assert_eq!(editor.released_counts(), released);
}

#[test]
fn overallocated_text_is_canonicalized_before_page_retention() {
    let base = binding(1, "");
    let mutation_key = key(base, 120);
    let mut overallocated = String::with_capacity(16 * 1024);
    overallocated.push('x');
    assert!(overallocated.capacity() >= 16 * 1024);
    let page = MutationPage::new(
        MutationPageKey::new(
            mutation_key,
            MutationLane::Proposal,
            MutationCursor::new(0),
            0,
            MutationIdentity::ROOT,
        ),
        MutationCursor::new(1),
        vec![MutationPageItem::Utf8 {
            inserted_offset: 0,
            text: overallocated.into_boxed_str(),
        }],
    )
    .unwrap();
    assert_eq!(page.totals().retained_bytes, 1);
    assert_eq!(
        page.items()[0],
        MutationPageItem::Utf8 {
            inserted_offset: 0,
            text: "x".into(),
        }
    );

    let intended = MutationPositions::collapsed(position(1));
    let proposal = make_proposal(base, 120, source_range(0, 0), intended, MutationKind::Edit);
    let mut editor = make_editor(base, 1, 1);
    begin(&mut editor, proposal);
    assert!(matches!(
        editor.accept_page(page),
        Ok(MutationPageAcceptance::Accepted { .. })
    ));
}

#[test]
fn operation_high_water_rejects_cancelled_and_rejected_aba_reuse() {
    let base = binding(1, "");
    let intended = MutationPositions::collapsed(position(0));
    let a = make_proposal(base, 200, source_range(0, 0), intended, MutationKind::Edit);
    let b = make_proposal(base, 201, source_range(0, 0), intended, MutationKind::Edit);
    let mut editor = make_editor(base, 1, 8);

    begin(&mut editor, a);
    assert_eq!(
        editor.cancel(a.key()).unwrap(),
        gpui_text_input::MutationCancellation::Cancelled
    );
    begin(&mut editor, b);
    editor.reject_input(b.key()).unwrap();

    let a_reuse = MutationBeginRequest::new(a, MutationCursor::new(0), MutationCursor::new(0));
    assert_eq!(
        editor.begin(a_reuse),
        Err(MutationError::ObsoleteOperation(a.key()))
    );
    let exact_b = MutationBeginRequest::new(b, MutationCursor::new(0), MutationCursor::new(0));
    assert_eq!(
        editor.begin(exact_b),
        Err(MutationError::ObsoleteOperation(b.key()))
    );
    let colliding_b = MutationProposal::new(
        b.key(),
        MutationKind::Undo,
        b.predecessor(),
        b.replacement(),
        b.replacement_line_breaks(),
    );
    assert_eq!(
        editor.begin(MutationBeginRequest::new(
            colliding_b,
            MutationCursor::new(0),
            MutationCursor::new(0),
        )),
        Err(MutationError::OperationCollision)
    );
}

#[test]
fn committed_revision_starts_fresh_operation_epoch_and_retires_predecessor_key() {
    for successor_operation in [200, 1] {
        let base = binding(1, "");
        let intended = MutationPositions::collapsed(position(0));
        let predecessor =
            make_proposal(base, 200, source_range(0, 0), intended, MutationKind::Edit);
        let predecessor_key = predecessor.key();
        let mut editor = make_editor(base, 1, 8);
        begin(&mut editor, predecessor);
        let finish = finish(&mut editor, predecessor_key, base.extent(), intended);
        editor.finish_input(finish).unwrap();
        editor.admit_commit(predecessor_key).unwrap();
        let successor = binding(2, "");
        let (text, objects) = admitted_sources(successor, "", position(0), vec![]);
        assert!(matches!(
            editor
                .settle_committed(predecessor_key, successor, intended, &text, &objects)
                .unwrap(),
            gpui_text_input::MutationSettlement::Current(MutationOutcome::Committed(_))
        ));
        assert_eq!(editor.binding(), successor);
        assert_eq!(
            editor.settle(predecessor_key, MutationOutcome::Rejected),
            Err(MutationError::ObsoleteOperation(predecessor_key))
        );

        let fresh = make_proposal(
            successor,
            successor_operation,
            source_range(0, 0),
            intended,
            MutationKind::Edit,
        );
        editor
            .begin(MutationBeginRequest::new(
                fresh,
                MutationCursor::new(0),
                MutationCursor::new(0),
            ))
            .unwrap();
        assert_eq!(editor.active_key(), Some(fresh.key()));
        assert!(matches!(
            editor.settle(predecessor_key, MutationOutcome::Rejected),
            Err(MutationError::WrongKey { expected, actual })
                if expected == fresh.key() && actual == predecessor_key
        ));
        assert_eq!(editor.active_key(), Some(fresh.key()));
        assert_eq!(
            editor.cancel(fresh.key()).unwrap(),
            gpui_text_input::MutationCancellation::Cancelled
        );
    }
}

#[test]
fn lane_cursor_cannot_return_after_advancing() {
    let base = binding(1, "");
    let mutation_key = key(base, 210);
    for lane in [MutationLane::Source, MutationLane::Proposal] {
        let first = text_page(
            mutation_key,
            lane,
            10,
            0,
            MutationIdentity::ROOT,
            11,
            0,
            "x",
        );
        for text in ["x", "different"] {
            assert_eq!(
                MutationPage::new(
                    MutationPageKey::new(
                        mutation_key,
                        lane,
                        first.next_cursor(),
                        1,
                        first.cumulative_identity(),
                    ),
                    MutationCursor::new(10),
                    vec![MutationPageItem::Utf8 {
                        inserted_offset: 1,
                        text: text.into(),
                    }],
                ),
                Err(MutationError::MalformedPage)
            );
        }
    }
}
