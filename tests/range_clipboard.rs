use gpui_text_input::{
    AtomFact, AtomId, BindingId, ByteRange, ClipboardCompletion, ClipboardError, ClipboardId,
    ClipboardKind, ClipboardLimits, ClipboardProgress, ClipboardState, ClipboardWriteOutcome,
    LogicalExtent, MutationFragment, MutationFragmentPayload, MutationLimits, MutationOutcome,
    PageDemandEnvelope, PageDirection, PageEdgeFact, PageFailure, PageId, PageRequest,
    PageRequestId, RangeBinding, RangeClipboardCoordinator, RangeEditCoordinator, RangePage,
    SourceRevision,
};

fn binding(revision: u64, bytes: u64) -> RangeBinding {
    RangeBinding::new(
        BindingId::new(4),
        SourceRevision::new(revision),
        LogicalExtent::new(bytes, u64::from(bytes != 0)),
    )
}

fn clipboard(bytes: u64, cap: usize, page: u64) -> RangeClipboardCoordinator {
    RangeClipboardCoordinator::new(binding(1, bytes), ClipboardLimits::new(cap, page).unwrap())
}

fn need_page(progress: ClipboardProgress) -> gpui_text_input::ClipboardKey {
    let ClipboardProgress::NeedPage { key, .. } = progress else {
        panic!("expected page demand")
    };
    key
}

fn request(
    collector: &mut RangeClipboardCoordinator,
    key: gpui_text_input::ClipboardKey,
    id: u64,
    _start: u64,
    _end: u64,
) -> PageRequest {
    collector.request_page(key, PageRequestId::new(id)).unwrap()
}

fn page(
    request: PageRequest,
    id: u64,
    text: &str,
    atoms: Vec<AtomFact>,
    document_len: u64,
) -> RangePage {
    let PageDemandEnvelope::Adjacent {
        anchor, direction, ..
    } = request.key().demand()
    else {
        panic!("clipboard requires adjacent demand")
    };
    let text_len = u64::try_from(text.len()).unwrap();
    let range = match direction {
        PageDirection::Forward => ByteRange::from_u64(anchor.get(), anchor.get() + text_len),
        PageDirection::Backward => ByteRange::from_u64(anchor.get() - text_len, anchor.get()),
    }
    .unwrap();
    RangePage::new(
        PageId::new(id),
        request.key(),
        range,
        text.into(),
        atoms,
        if range.start().get() == 0 {
            PageEdgeFact::DocumentBoundary
        } else {
            PageEdgeFact::Continues
        },
        if range.end().get() == document_len {
            PageEdgeFact::DocumentBoundary
        } else {
            PageEdgeFact::Continues
        },
        range.end().get() == document_len,
    )
    .unwrap()
}

#[test]
fn copy_collects_complete_selection_at_the_exact_cap() {
    let mut collector = clipboard(6, 6, 4);
    let key = need_page(
        collector
            .begin(
                ClipboardId::new(1),
                ClipboardKind::Copy,
                ByteRange::from_u64(0, 6).unwrap(),
            )
            .unwrap(),
    );
    let first = request(&mut collector, key, 1, 0, 3);
    assert!(
        matches!(collector.admit_page(page(first, 1, "abc", vec![], 6)).unwrap(), ClipboardProgress::NeedPage { next_offset, .. } if next_offset.get() == 3)
    );
    let second = request(&mut collector, key, 2, 3, 6);
    let ClipboardProgress::Write(write) = collector
        .admit_page(page(second, 2, "def", vec![], 6))
        .unwrap()
    else {
        panic!("expected write")
    };
    assert_eq!(write.text(), "abcdef");
    assert_eq!(collector.counts().staged_bytes, 0);
    assert_eq!(
        collector
            .acknowledge_write(write.key(), ClipboardWriteOutcome::Written)
            .unwrap(),
        ClipboardCompletion::Copied
    );
}

#[test]
fn representation_over_cap_is_terminal_and_publishes_no_value() {
    let mut collector = clipboard(4, 3, 4);
    let key = need_page(
        collector
            .begin(
                ClipboardId::new(1),
                ClipboardKind::Copy,
                ByteRange::from_u64(0, 4).unwrap(),
            )
            .unwrap(),
    );
    let request = request(&mut collector, key, 1, 0, 4);
    assert_eq!(
        collector
            .admit_page(page(request, 1, "four", vec![], 4))
            .unwrap(),
        ClipboardProgress::Terminal(ClipboardCompletion::TooLarge)
    );
    assert_eq!(collector.state(), ClipboardState::Idle);
    assert_eq!(collector.counts().staged_bytes, 0);
}

#[test]
fn atom_fallback_replaces_cross_page_visible_bytes_once_in_logical_order() {
    let mut collector = clipboard(6, 16, 9);
    let key = need_page(
        collector
            .begin(
                ClipboardId::new(7),
                ClipboardKind::Copy,
                ByteRange::from_u64(0, 6).unwrap(),
            )
            .unwrap(),
    );
    let global = ByteRange::from_u64(1, 5).unwrap();
    let first_request = request(&mut collector, key, 1, 0, 3);
    let first_atom = AtomFact::new(
        AtomId::new(9),
        global,
        ByteRange::from_u64(1, 3).unwrap(),
        "[atom]",
    );
    assert!(matches!(
        collector
            .admit_page(page(first_request, 1, "abc", vec![first_atom], 6))
            .unwrap(),
        ClipboardProgress::NeedPage { .. }
    ));
    let second_request = request(&mut collector, key, 2, 3, 6);
    let second_atom = AtomFact::new(
        AtomId::new(9),
        global,
        ByteRange::from_u64(3, 5).unwrap(),
        "[atom]",
    );
    let ClipboardProgress::Write(write) = collector
        .admit_page(page(second_request, 2, "def", vec![second_atom], 6))
        .unwrap()
    else {
        panic!("expected write")
    };
    assert_eq!(write.text(), "a[atom]f");
}

#[test]
fn malformed_cross_page_atom_facts_publish_no_result() {
    let mut collector = clipboard(6, 16, 6);
    let key = need_page(
        collector
            .begin(
                ClipboardId::new(8),
                ClipboardKind::Copy,
                ByteRange::from_u64(0, 6).unwrap(),
            )
            .unwrap(),
    );
    let global = ByteRange::from_u64(1, 5).unwrap();
    let first_request = request(&mut collector, key, 1, 0, 3);
    let first_atom = AtomFact::new(
        AtomId::new(9),
        global,
        ByteRange::from_u64(1, 3).unwrap(),
        "one",
    );
    collector
        .admit_page(page(first_request, 1, "abc", vec![first_atom], 6))
        .unwrap();
    let second_request = request(&mut collector, key, 2, 3, 6);
    let second_atom = AtomFact::new(
        AtomId::new(9),
        global,
        ByteRange::from_u64(3, 5).unwrap(),
        "two",
    );
    assert_eq!(
        collector
            .admit_page(page(second_request, 2, "def", vec![second_atom], 6))
            .unwrap(),
        ClipboardProgress::Terminal(ClipboardCompletion::Malformed)
    );
}

#[test]
fn page_failure_and_page_cancellation_are_distinct_terminal_outcomes() {
    for (id, failure, expected) in [
        (
            1,
            PageFailure::Unavailable,
            ClipboardCompletion::PageFailed(PageFailure::Unavailable),
        ),
        (
            2,
            PageFailure::Malformed,
            ClipboardCompletion::PageFailed(PageFailure::Malformed),
        ),
        (3, PageFailure::Cancelled, ClipboardCompletion::Cancelled),
    ] {
        let mut collector = clipboard(2, 8, 4);
        let key = need_page(
            collector
                .begin(
                    ClipboardId::new(id),
                    ClipboardKind::Copy,
                    ByteRange::from_u64(0, 2).unwrap(),
                )
                .unwrap(),
        );
        let request = request(&mut collector, key, id, 0, 2);
        assert_eq!(
            collector.settle_page(request.key(), failure).unwrap(),
            ClipboardProgress::Terminal(expected)
        );
        assert_eq!(collector.counts(), Default::default());
    }
}

#[test]
fn cut_requires_successful_write_before_it_can_open_exact_deletion() {
    let mut failed = clipboard(0, 8, 4);
    let ClipboardProgress::Write(write) = failed
        .begin(
            ClipboardId::new(1),
            ClipboardKind::Cut,
            ByteRange::from_u64(0, 0).unwrap(),
        )
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(
        failed
            .acknowledge_write(write.key(), ClipboardWriteOutcome::Failed)
            .unwrap(),
        ClipboardCompletion::WriteFailed
    );

    let mut successful = clipboard(3, 8, 4);
    let key = need_page(
        successful
            .begin(
                ClipboardId::new(2),
                ClipboardKind::Cut,
                ByteRange::from_u64(0, 3).unwrap(),
            )
            .unwrap(),
    );
    let request = request(&mut successful, key, 1, 0, 3);
    let ClipboardProgress::Write(write) = successful
        .admit_page(page(request, 1, "abc", vec![], 3))
        .unwrap()
    else {
        panic!()
    };
    let ClipboardCompletion::Delete(deletion) = successful
        .acknowledge_write(write.key(), ClipboardWriteOutcome::Written)
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(deletion.binding(), binding(1, 3));
    assert_eq!(deletion.selection(), ByteRange::from_u64(0, 3).unwrap());

    let proposal = deletion.proposal(gpui_text_input::OperationId::new(5));
    let mut editor = RangeEditCoordinator::new(binding(1, 3), MutationLimits::new(2, 0).unwrap());
    editor.begin(proposal).unwrap();
    editor.accept_preflight(proposal.key()).unwrap();
    editor
        .stage(MutationFragment::new(
            proposal.key(),
            0,
            MutationFragmentPayload::Terminal,
        ))
        .unwrap();
    editor.admit_commit(proposal.key()).unwrap();
    assert_eq!(
        editor
            .settle(proposal.key(), MutationOutcome::Conflict)
            .unwrap(),
        gpui_text_input::MutationSettlement::Current(MutationOutcome::Conflict)
    );
}

#[test]
fn exact_page_keys_are_enforced_without_losing_active_work() {
    let mut collector = clipboard(6, 8, 4);
    let key = need_page(
        collector
            .begin(
                ClipboardId::new(1),
                ClipboardKind::Copy,
                ByteRange::from_u64(0, 6).unwrap(),
            )
            .unwrap(),
    );
    let expected = request(&mut collector, key, 1, 0, 3);
    let wrong_key = gpui_text_input::PageRequestKey::adjacent(
        PageRequestId::new(2),
        key.binding(),
        key.revision(),
        gpui_text_input::PagePurpose::Clipboard,
        gpui_text_input::ByteOffset::new(0),
        PageDirection::Forward,
        4,
    )
    .unwrap();
    let wrong = RangePage::new(
        PageId::new(2),
        wrong_key,
        ByteRange::from_u64(0, 3).unwrap(),
        "abc".into(),
        vec![],
        PageEdgeFact::DocumentBoundary,
        PageEdgeFact::Continues,
        false,
    )
    .unwrap();
    assert!(matches!(
        collector.admit_page(wrong),
        Err(ClipboardError::WrongPageKey { .. })
    ));
    assert!(matches!(
        collector
            .admit_page(page(expected, 1, "abc", vec![], 6))
            .unwrap(),
        ClipboardProgress::NeedPage { .. }
    ));
}

#[test]
fn cancel_rebind_and_dispose_release_all_owned_capacity() {
    let mut cancelled = clipboard(3, 8, 4);
    let key = need_page(
        cancelled
            .begin(
                ClipboardId::new(1),
                ClipboardKind::Copy,
                ByteRange::from_u64(0, 3).unwrap(),
            )
            .unwrap(),
    );
    let pending_request = request(&mut cancelled, key, 1, 0, 3);
    assert_eq!(cancelled.counts().pending_pages, 1);
    assert_eq!(
        cancelled.cancel(key).unwrap(),
        ClipboardCompletion::Cancelled
    );
    assert_eq!(cancelled.counts(), Default::default());
    assert_eq!(
        cancelled.settle_page(pending_request.key(), PageFailure::Cancelled),
        Err(ClipboardError::ObsoletePage(pending_request.key()))
    );

    let mut rebound = clipboard(3, 8, 4);
    let rebound_key = need_page(
        rebound
            .begin(
                ClipboardId::new(2),
                ClipboardKind::Copy,
                ByteRange::from_u64(0, 3).unwrap(),
            )
            .unwrap(),
    );
    request(&mut rebound, rebound_key, 2, 0, 3);
    let rebound_cancellation = rebound.rebind(binding(2, 4)).unwrap();
    assert_eq!(rebound_cancellation.key(), rebound_key);
    assert!(rebound_cancellation.pending_page().is_some());
    assert_eq!(rebound.binding(), binding(2, 4));
    assert_eq!(rebound.counts(), Default::default());

    let mut disposed = clipboard(3, 8, 4);
    let disposed_key = need_page(
        disposed
            .begin(
                ClipboardId::new(3),
                ClipboardKind::Copy,
                ByteRange::from_u64(0, 3).unwrap(),
            )
            .unwrap(),
    );
    assert_eq!(disposed.dispose().unwrap().key(), disposed_key);
    assert_eq!(disposed.state(), ClipboardState::Idle);
}

#[test]
fn empty_selection_produces_an_exact_empty_write() {
    let mut collector = clipboard(0, 0, 4);
    let ClipboardProgress::Write(write) = collector
        .begin(
            ClipboardId::new(1),
            ClipboardKind::Copy,
            ByteRange::from_u64(0, 0).unwrap(),
        )
        .unwrap()
    else {
        panic!()
    };
    assert!(write.text().is_empty());
    assert_eq!(
        collector
            .acknowledge_write(write.key(), ClipboardWriteOutcome::Written)
            .unwrap(),
        ClipboardCompletion::Copied
    );
}

#[test]
fn malformed_source_extent_and_edge_facts_are_terminal_without_write_or_delete() {
    for (range, text, preceding, following, end_of_source) in [
        (
            (0, 5),
            "abcde",
            PageEdgeFact::DocumentBoundary,
            PageEdgeFact::Continues,
            false,
        ),
        (
            (0, 4),
            "abcd",
            PageEdgeFact::Continues,
            PageEdgeFact::DocumentBoundary,
            true,
        ),
        (
            (0, 4),
            "abcd",
            PageEdgeFact::DocumentBoundary,
            PageEdgeFact::Continues,
            false,
        ),
    ] {
        let mut collector = clipboard(4, 8, 8);
        let key = need_page(
            collector
                .begin(
                    ClipboardId::new(20),
                    ClipboardKind::Cut,
                    ByteRange::from_u64(0, 4).unwrap(),
                )
                .unwrap(),
        );
        let request = collector.request_page(key, PageRequestId::new(1)).unwrap();
        let malformed = RangePage::new(
            PageId::new(1),
            request.key(),
            ByteRange::from_u64(range.0, range.1).unwrap(),
            text.into(),
            vec![],
            preceding,
            following,
            end_of_source,
        );
        match malformed {
            Ok(page) => assert_eq!(
                collector.admit_page(page).unwrap(),
                ClipboardProgress::Terminal(ClipboardCompletion::Malformed)
            ),
            Err(_) => {
                // Constructor-level malformed facts release when the host settles malformed.
                assert_eq!(
                    collector
                        .settle_page(request.key(), PageFailure::Malformed)
                        .unwrap(),
                    ClipboardProgress::Terminal(ClipboardCompletion::PageFailed(
                        PageFailure::Malformed
                    ))
                );
            }
        }
        assert_eq!(collector.counts(), Default::default());
        assert_eq!(collector.state(), ClipboardState::Idle);
    }
}

#[test]
fn clipboard_page_request_ids_are_monotonic_across_terminal_operations() {
    let mut collector = clipboard(4, 8, 4);
    let key = need_page(
        collector
            .begin(
                ClipboardId::new(30),
                ClipboardKind::Copy,
                ByteRange::from_u64(0, 4).unwrap(),
            )
            .unwrap(),
    );
    let request = collector.request_page(key, PageRequestId::new(2)).unwrap();
    collector
        .settle_page(request.key(), PageFailure::Cancelled)
        .unwrap();
    let key = need_page(
        collector
            .begin(
                ClipboardId::new(31),
                ClipboardKind::Copy,
                ByteRange::from_u64(0, 4).unwrap(),
            )
            .unwrap(),
    );
    assert_eq!(
        collector.request_page(key, PageRequestId::new(2)),
        Err(ClipboardError::RequestIdInUse(PageRequestId::new(2)))
    );
    assert!(collector.request_page(key, PageRequestId::new(3)).is_ok());
}
