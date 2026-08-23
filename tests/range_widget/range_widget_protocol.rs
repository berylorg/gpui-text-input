use gpui::px;
use gpui_text_input::{
    BindingId, ByteOffset, InlineObjectGap, LogicalExtent, MutationBeginRequest,
    MutationCancelRequest, MutationCursor, MutationFinishInput, MutationIdentity, MutationKey,
    MutationKind, MutationLane, MutationPage, MutationPageItem, MutationPageKey,
    MutationPageRequest, MutationPositions, MutationProposal, MutationStreamFinish, MutationTotals,
    OperationId, RangeBinding, RangeHistoryCommit, RangeHistoryFrontier, RangeHistoryIntent,
    RangeHistoryOutcome, RangeHistorySession, RangeRestorationScrollAnchor, RangeRestorationSeed,
    RangeSourceSelection, RangeTextInputRequest, SourcePosition, SourceRange, SourceRevision,
};

fn binding(revision: u64, bytes: u64) -> RangeBinding {
    RangeBinding::new(
        BindingId::new(17),
        SourceRevision::new(revision),
        LogicalExtent::new(bytes, u64::from(bytes != 0)),
    )
}

fn position(offset: u64) -> SourcePosition {
    SourcePosition::new(ByteOffset::new(offset), InlineObjectGap::NoObjects)
}

fn begin(kind: MutationKind) -> MutationBeginRequest {
    let predecessor_binding = binding(1, 3);
    let key = MutationKey::new(
        predecessor_binding.binding(),
        predecessor_binding.revision(),
        OperationId::new(8),
    );
    let predecessor = MutationPositions::new(position(1), position(3), position(1));
    let replacement = SourceRange::new(position(1), position(3)).unwrap();
    MutationBeginRequest::new(
        MutationProposal::new(key, kind, predecessor, replacement, 0),
        MutationCursor::new(10),
        MutationCursor::new(20),
    )
}

#[test]
fn restoration_seed_remains_compact_and_payload_free() {
    let at = position(1);
    let seed = RangeRestorationSeed {
        binding: binding(1, 3),
        caret: at,
        selection: RangeSourceSelection::caret(at),
        scroll: RangeRestorationScrollAnchor {
            position: at,
            intra_anchor: px(0.0),
        },
        history: None,
    };
    assert!(!std::mem::needs_drop::<RangeRestorationSeed>());
    assert!(std::mem::size_of_val(&seed) <= 512);
}

#[test]
fn widget_protocol_has_distinct_bounded_request_variants() {
    let begin = begin(MutationKind::Edit);
    let key = begin.proposal().key();
    let page = MutationPage::new(
        MutationPageKey::new(
            key,
            MutationLane::Proposal,
            begin.proposal_cursor(),
            0,
            MutationIdentity::ROOT,
        ),
        MutationCursor::new(21),
        vec![MutationPageItem::Utf8 {
            inserted_offset: 0,
            text: "x".into(),
        }],
    )
    .unwrap();
    let empty = MutationStreamFinish {
        next_cursor: begin.source_cursor(),
        next_ordinal: 0,
        cumulative_identity: MutationIdentity::ROOT,
        totals: MutationTotals::default(),
    };
    let proposal = MutationStreamFinish {
        next_cursor: page.next_cursor(),
        next_ordinal: 1,
        cumulative_identity: page.cumulative_identity(),
        totals: page.totals(),
    };
    let finish = MutationFinishInput::new(
        key,
        empty,
        proposal,
        LogicalExtent::new(2, 1),
        MutationPositions::collapsed(position(2)),
    );
    let requests = [
        RangeTextInputRequest::MutationBegin(begin),
        RangeTextInputRequest::MutationProposalPage(MutationPageRequest::new(page)),
        RangeTextInputRequest::MutationFinishInput(finish),
        RangeTextInputRequest::CancelMutation(MutationCancelRequest::new(key)),
    ];
    assert!(matches!(
        requests[0],
        RangeTextInputRequest::MutationBegin(_)
    ));
    assert!(matches!(
        requests[1],
        RangeTextInputRequest::MutationProposalPage(_)
    ));
    assert!(matches!(
        requests[2],
        RangeTextInputRequest::MutationFinishInput(_)
    ));
    assert!(matches!(
        requests[3],
        RangeTextInputRequest::CancelMutation(_)
    ));
}

#[test]
fn history_session_and_commit_are_compact_and_preserve_exact_availability() {
    let predecessor_binding = binding(1, 3);
    let key = MutationKey::new(
        predecessor_binding.binding(),
        predecessor_binding.revision(),
        OperationId::new(8),
    );
    let frontier = RangeHistoryFrontier {
        binding: predecessor_binding,
        id: 12,
        undo_available: true,
        redo_available: false,
    };
    let predecessor = position(3);
    let predecessor_selection = RangeSourceSelection {
        anchor: position(1),
        head: predecessor,
    };
    let intent = RangeHistoryIntent::new(
        key,
        predecessor_binding,
        MutationKind::Undo,
        frontier,
        predecessor,
        predecessor_selection,
    );
    let session = RangeHistorySession::new(intent);
    assert_eq!(session.intent(), intent);
    assert_eq!(intent.binding(), predecessor_binding);
    assert_eq!(intent.frontier(), frontier);
    assert_eq!(intent.kind(), MutationKind::Undo);
    assert_eq!(intent.caret(), predecessor);
    assert_eq!(intent.selection(), predecessor_selection);
    assert!(!std::mem::needs_drop::<RangeHistoryIntent>());
    assert!(std::mem::size_of::<RangeHistoryIntent>() <= 512);
    let successor_binding = binding(2, 4);
    let successor_frontier = RangeHistoryFrontier {
        binding: successor_binding,
        id: 13,
        undo_available: false,
        redo_available: true,
    };
    let commit = RangeHistoryCommit::new(
        successor_binding,
        position(4),
        RangeSourceSelection::caret(position(4)),
        successor_frontier,
    );
    assert_eq!(commit.binding(), successor_binding);
    assert_eq!(commit.frontier(), successor_frontier);
    assert!(!std::mem::needs_drop::<RangeHistorySession>());
    assert!(!std::mem::needs_drop::<RangeHistoryCommit>());
    assert!(std::mem::size_of::<RangeHistorySession>() <= 512);
    assert!(std::mem::size_of::<RangeHistoryCommit>() <= 512);
    assert!(matches!(
        RangeHistoryOutcome::Committed(commit),
        RangeHistoryOutcome::Committed(committed) if committed == commit
    ));
}

#[test]
fn source_and_proposal_lanes_cannot_alias() {
    let begin = begin(MutationKind::Redo);
    let key = begin.proposal().key();
    let source = MutationPageKey::new(
        key,
        MutationLane::Source,
        MutationCursor::new(1),
        0,
        MutationIdentity::ROOT,
    );
    let proposal = MutationPageKey::new(
        key,
        MutationLane::Proposal,
        MutationCursor::new(1),
        0,
        MutationIdentity::ROOT,
    );
    assert_ne!(source, proposal);
}
