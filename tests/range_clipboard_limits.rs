use gpui_text_input::{
    AtomFact, AtomId, BindingId, ByteRange, ClipboardCompletion, ClipboardId, ClipboardKind,
    ClipboardLimits, ClipboardProgress, ClipboardWriteOutcome, LogicalExtent, MutationLimits,
    OperationId, PageEdgeFact, PageId, PageRequestId, RangeBinding, RangeClipboardCoordinator,
    RangeEditCoordinator, RangePage, SourceRevision,
};

fn collect_with_page_cap(cap: u64) -> (RangeClipboardCoordinator, gpui_text_input::ClipboardKey) {
    let binding = RangeBinding::new(
        BindingId::new(90),
        SourceRevision::new(1),
        LogicalExtent::new(1, 1),
    );
    let mut collector =
        RangeClipboardCoordinator::new(binding, ClipboardLimits::new(16, cap).unwrap());
    let ClipboardProgress::NeedPage { key, .. } = collector
        .begin(
            ClipboardId::new(cap),
            ClipboardKind::Copy,
            ByteRange::from_u64(0, 1).unwrap(),
        )
        .unwrap()
    else {
        panic!("nonempty selection requires a page")
    };
    (collector, key)
}

fn atom_page(
    collector: &mut RangeClipboardCoordinator,
    key: gpui_text_input::ClipboardKey,
    fallback: &str,
) -> RangePage {
    let request = collector.request_page(key, PageRequestId::new(1)).unwrap();
    let range = ByteRange::from_u64(0, 1).unwrap();
    RangePage::new(
        PageId::new(1),
        request.key(),
        range,
        "x".into(),
        vec![AtomFact::new(AtomId::new(1), range, range, fallback)],
        PageEdgeFact::DocumentBoundary,
        PageEdgeFact::DocumentBoundary,
        true,
    )
    .unwrap()
}

#[test]
fn atom_fallback_can_push_a_tiny_page_over_the_retained_payload_cap() {
    let (mut collector, key) = collect_with_page_cap(4);
    let page = atom_page(&mut collector, key, "abcd");
    assert_eq!(page.retained_bytes(), 5);
    assert_eq!(
        collector.admit_page(page).unwrap(),
        ClipboardProgress::Terminal(ClipboardCompletion::PageTooLarge)
    );
    assert_eq!(collector.counts(), Default::default());
}

#[test]
fn retained_payload_exactly_at_the_page_cap_is_accepted() {
    let (mut collector, key) = collect_with_page_cap(4);
    let page = atom_page(&mut collector, key, "abc");
    assert_eq!(page.retained_bytes(), 4);
    let ClipboardProgress::Write(write) = collector.admit_page(page).unwrap() else {
        panic!("exact-cap page should produce the complete value")
    };
    assert_eq!(write.text(), "abc");
}

#[test]
fn cut_carries_the_exact_selected_line_break_count_into_deletion_preflight() {
    let binding = RangeBinding::new(
        BindingId::new(91),
        SourceRevision::new(1),
        LogicalExtent::new(3, 2),
    );
    let mut collector =
        RangeClipboardCoordinator::new(binding, ClipboardLimits::new(8, 8).unwrap());
    let ClipboardProgress::NeedPage { key, .. } = collector
        .begin(
            ClipboardId::new(3),
            ClipboardKind::Cut,
            ByteRange::from_u64(0, 3).unwrap(),
        )
        .unwrap()
    else {
        panic!()
    };
    let request = collector.request_page(key, PageRequestId::new(3)).unwrap();
    let page = RangePage::new(
        PageId::new(3),
        request.key(),
        key.selection(),
        "a\nb".into(),
        vec![],
        PageEdgeFact::DocumentBoundary,
        PageEdgeFact::DocumentBoundary,
        true,
    )
    .unwrap();
    let ClipboardProgress::Write(write) = collector.admit_page(page).unwrap() else {
        panic!()
    };
    let ClipboardCompletion::Delete(deletion) = collector
        .acknowledge_write(write.key(), ClipboardWriteOutcome::Written)
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(deletion.selection_line_breaks(), 1);
    let mut edits = RangeEditCoordinator::new(binding, MutationLimits::new(2, 0).unwrap());
    edits.begin(deletion.proposal(OperationId::new(4))).unwrap();
}
