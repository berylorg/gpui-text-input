use std::sync::Arc;

use gpui::{SharedString, px};
use gpui_text_input::{
    AtomFact, AtomId, BindingId, ByteOffset, ByteRange, ClipboardCompletion, ClipboardError,
    ClipboardId, ClipboardKey, ClipboardKind, ClipboardLimits, ClipboardProgress,
    ClipboardProvenanceLimits, ClipboardProvenancePage, ClipboardProvenancePolicy,
    ClipboardWriteOutcome, InlineObjectFact, InlineObjectGap, InlineObjectId, InlineObjectNeighbor,
    InlineObjectOrder, InlineObjectPresentation, LogicalExtent, MutationPositions, ObjectPage,
    ObjectPageEdgeFact, ObjectPageId, ObjectRequestId, PageDirection, PageEdgeFact, PageId,
    PageRequestId, PresentationGeneration, RangeBinding, RangeClipboardCoordinator, RangePage,
    SourcePosition, SourceRange, SourceRevision, TextInputAtomClipboardPolicy,
};

trait PreparedClipboardHarness {
    fn admit_object_page(&mut self, page: ObjectPage) -> Result<ClipboardProgress, ClipboardError>;
    fn admit_text_page(&mut self, page: RangePage) -> Result<ClipboardProgress, ClipboardError>;
}

impl PreparedClipboardHarness for RangeClipboardCoordinator {
    fn admit_object_page(&mut self, page: ObjectPage) -> Result<ClipboardProgress, ClipboardError> {
        let prepared = self.prepare_object_page(&page)?;
        let commit = self.commit_object_page(page, prepared)?;
        finish_prepared(self, commit)
    }

    fn admit_text_page(&mut self, page: RangePage) -> Result<ClipboardProgress, ClipboardError> {
        let prepared = self.prepare_text_page(&page)?;
        let commit = self.commit_text_page(page, prepared)?;
        finish_prepared(self, commit)
    }
}

fn finish_prepared(
    collector: &mut RangeClipboardCoordinator,
    mut commit: gpui_text_input::ClipboardPreparedCommit,
) -> Result<ClipboardProgress, ClipboardError> {
    loop {
        if let Some(progress) = commit.into_progress() {
            return Ok(progress);
        }
        let prepared = collector.prepare_next()?;
        commit = collector.commit_prepared(prepared)?;
    }
}

#[test]
fn prepared_begin_is_exact_nonmutating_and_instance_bound_for_all_modes() {
    for (ordinal, stream) in [false, true].into_iter().enumerate() {
        for empty in [true, false] {
            let source = "a";
            let mut collector = if stream {
                provenance_coordinator(source, 64, 1, 2, 4096)
            } else {
                coordinator(source, 64, 1)
            };
            let selection = SourceRange::new(position(0), position(u64::from(!empty))).unwrap();
            let prepared = collector
                .prepare_begin(
                    ClipboardId::new(95_000 + ordinal as u64 * 2 + u64::from(!empty)),
                    ClipboardKind::Copy,
                    selection,
                    predecessor(selection),
                )
                .unwrap();
            assert_eq!(collector.state(), gpui_text_input::ClipboardState::Idle);
            assert_eq!(collector.counts(), Default::default());
            assert_eq!(collector.ownership_charge(), Default::default());
            assert_eq!(prepared.peak_ownership(), prepared.successor_ownership());
            assert_eq!(
                prepared.successor_ownership().items(),
                if stream { 2 } else { 1 }
            );
            assert!(prepared.successor_ownership().bytes() > 0);
            let exact = prepared.successor_ownership();
            let admitted = |bytes, items| exact.bytes() <= bytes && exact.items() <= items;
            assert!(admitted(exact.bytes(), exact.items()));
            assert!(!admitted(exact.bytes() - 1, exact.items()));
            assert!(!admitted(exact.bytes(), exact.items() - 1));
            assert_eq!(collector.state(), gpui_text_input::ClipboardState::Idle);
            assert_eq!(collector.ownership_charge(), Default::default());

            let progress = collector.commit_begin(prepared).unwrap();
            if empty {
                assert!(matches!(progress, ClipboardProgress::Write(_)));
            } else {
                assert!(matches!(progress, ClipboardProgress::NeedObjectPage { .. }));
                assert_eq!(
                    collector.ownership_charge().items(),
                    if stream { 2 } else { 1 }
                );
            }
        }
    }

    let selection = SourceRange::new(position(0), position(1)).unwrap();
    let first = coordinator("a", 64, 1);
    let prepared = first
        .prepare_begin(
            ClipboardId::new(95_100),
            ClipboardKind::Cut,
            selection,
            predecessor(selection),
        )
        .unwrap();
    let mut other = coordinator("a", 64, 1);
    assert_eq!(
        other.commit_begin(prepared),
        Err(ClipboardError::StalePreparation)
    );
    assert_eq!(other.state(), gpui_text_input::ClipboardState::Idle);
    assert_eq!(other.ownership_charge(), Default::default());

    let mut rebound = coordinator("a", 64, 1);
    let prepared = rebound
        .prepare_begin(
            ClipboardId::new(95_101),
            ClipboardKind::Copy,
            selection,
            predecessor(selection),
        )
        .unwrap();
    rebound.rebind(RangeBinding::new(
        BindingId::new(7),
        SourceRevision::new(10),
        LogicalExtent::new(1, 1),
    ));
    assert_eq!(
        rebound.commit_begin(prepared),
        Err(ClipboardError::StalePreparation)
    );
    assert_eq!(rebound.ownership_charge(), Default::default());
}

#[test]
fn terminal_response_preparations_consume_exact_response_and_release_dispatch() {
    let selection = SourceRange::new(position(0), position(1)).unwrap();
    let mut object_terminal = coordinator("a", 64, 1);
    let ClipboardProgress::NeedObjectPage { key, .. } = object_terminal
        .begin(
            ClipboardId::new(90_001),
            ClipboardKind::Copy,
            selection,
            predecessor(selection),
        )
        .unwrap()
    else {
        unreachable!()
    };
    let request = object_terminal
        .request_object_page(key, ObjectRequestId::new(1))
        .unwrap();
    let oversized = ObjectPage::new(
        ObjectPageId::new(1),
        request.key(),
        Vec::with_capacity(4096),
        ObjectPageEdgeFact::EnvelopeBoundary,
        ObjectPageEdgeFact::EnvelopeBoundary,
        true,
        None,
    )
    .unwrap();
    let prepared = object_terminal.prepare_object_page(&oversized).unwrap();
    assert!(prepared.transfers_response());
    let committed = object_terminal
        .commit_object_page(oversized, prepared)
        .unwrap();
    assert_eq!(committed.released_object_page(), Some(request.key()));
    assert_eq!(
        committed.into_progress(),
        Some(ClipboardProgress::Terminal(ClipboardCompletion::Malformed))
    );
    assert_eq!(object_terminal.counts(), Default::default());

    let mut text_terminal = coordinator("a", 64, 1);
    let ClipboardProgress::NeedObjectPage { key, .. } = text_terminal
        .begin(
            ClipboardId::new(90_002),
            ClipboardKind::Copy,
            selection,
            predecessor(selection),
        )
        .unwrap()
    else {
        unreachable!()
    };
    let object_request = text_terminal
        .request_object_page(key, ObjectRequestId::new(1))
        .unwrap();
    let ClipboardProgress::NeedTextPage { key, .. } = text_terminal
        .admit_object_page(object_page(object_request, &[], 2))
        .unwrap()
    else {
        unreachable!()
    };
    let request = text_terminal
        .request_text_page(key, PageRequestId::new(2))
        .unwrap();
    let range = ByteRange::from_u64(0, 1).unwrap();
    let oversized = RangePage::new(
        PageId::new(3),
        request.key(),
        range,
        "a".to_owned(),
        vec![AtomFact::new(AtomId::new(3), range, range, "overflow")],
        PageEdgeFact::DocumentBoundary,
        PageEdgeFact::DocumentBoundary,
        true,
    )
    .unwrap();
    let prepared = text_terminal.prepare_text_page(&oversized).unwrap();
    assert!(prepared.transfers_response());
    let committed = text_terminal.commit_text_page(oversized, prepared).unwrap();
    assert_eq!(committed.released_text_page(), Some(request.key()));
    assert_eq!(
        committed.into_progress(),
        Some(ClipboardProgress::Terminal(
            ClipboardCompletion::TextPageTooLarge
        ))
    );
    assert_eq!(text_terminal.counts(), Default::default());
}

#[test]
fn exact_output_layout_failure_is_terminal_and_releases_retained_response() {
    let source = "a";
    let selection = SourceRange::new(position(0), position(1)).unwrap();
    let impossible_layout = (isize::MAX as usize).checked_add(1).unwrap();
    let mut collector = coordinator(source, impossible_layout, 1);
    let ClipboardProgress::NeedObjectPage { key, .. } = collector
        .begin(
            ClipboardId::new(90_003),
            ClipboardKind::Cut,
            selection,
            predecessor(selection),
        )
        .unwrap()
    else {
        unreachable!()
    };
    let object_request = collector
        .request_object_page(key, ObjectRequestId::new(1))
        .unwrap();
    let ClipboardProgress::NeedTextPage { key, .. } = collector
        .admit_object_page(object_page(object_request, &[], 1))
        .unwrap()
    else {
        unreachable!()
    };
    let request = collector
        .request_text_page(key, PageRequestId::new(2))
        .unwrap();
    let request_key = request.key();
    let page = text_page(source, request, 2);
    let prepared = collector.prepare_text_page(&page).unwrap();
    let mut commit = collector.commit_text_page(page, prepared).unwrap();
    let terminal = loop {
        let released = commit.released_text_page();
        if let Some(progress) = commit.into_progress() {
            assert_eq!(released, Some(request_key));
            break progress;
        }
        let prepared = collector.prepare_next().unwrap();
        commit = collector.commit_prepared(prepared).unwrap();
    };
    assert_eq!(
        terminal,
        ClipboardProgress::Terminal(ClipboardCompletion::AllocationFailed)
    );
    assert_eq!(collector.state(), gpui_text_input::ClipboardState::Idle);
    assert_eq!(collector.counts(), Default::default());
    assert_eq!(collector.ownership_charge().bytes(), 0);
}

#[test]
fn prepared_tokens_reject_wrong_response_coordinator_lifecycle_and_duplicates() {
    let selection = SourceRange::new(position(0), position(1)).unwrap();
    let begin = |collector: &mut RangeClipboardCoordinator| {
        let ClipboardProgress::NeedObjectPage { key, .. } = collector
            .begin(
                ClipboardId::new(91_001),
                ClipboardKind::Copy,
                selection,
                predecessor(selection),
            )
            .unwrap()
        else {
            unreachable!()
        };
        collector
            .request_object_page(key, ObjectRequestId::new(1))
            .unwrap()
    };
    let left = object(91, 0, 1, "a");
    let right = object(92, 0, 1, "b");

    let mut exact = coordinator("a", 64, 1);
    let request = begin(&mut exact);
    let mut left_objects = Vec::with_capacity(1);
    left_objects.push(left);
    let page = ObjectPage::new(
        ObjectPageId::new(1),
        request.key(),
        left_objects,
        ObjectPageEdgeFact::EnvelopeBoundary,
        ObjectPageEdgeFact::EnvelopeBoundary,
        true,
        None,
    )
    .unwrap();
    let mut right_objects = Vec::with_capacity(1);
    right_objects.push(right);
    let different = ObjectPage::new(
        ObjectPageId::new(1),
        request.key(),
        right_objects,
        ObjectPageEdgeFact::EnvelopeBoundary,
        ObjectPageEdgeFact::EnvelopeBoundary,
        true,
        None,
    )
    .unwrap();
    assert_eq!(page.retained_charge(), different.retained_charge());
    let wrong = exact.prepare_object_page(&page).unwrap();
    assert_eq!(
        exact.commit_object_page(different, wrong),
        Err(ClipboardError::WrongPreparation)
    );
    assert_eq!(
        exact.state(),
        gpui_text_input::ClipboardState::ObjectPagePending
    );
    let mut equal_objects = Vec::with_capacity(1);
    equal_objects.push(object(91, 0, 1, "a"));
    let independent_equal = ObjectPage::new(
        ObjectPageId::new(1),
        request.key(),
        equal_objects,
        ObjectPageEdgeFact::EnvelopeBoundary,
        ObjectPageEdgeFact::EnvelopeBoundary,
        true,
        None,
    )
    .unwrap();
    assert_eq!(page.retained_charge(), independent_equal.retained_charge());
    let wrong = exact.prepare_object_page(&page).unwrap();
    assert_eq!(
        exact.commit_object_page(independent_equal, wrong),
        Err(ClipboardError::WrongPreparation)
    );
    let clone = page.clone();
    let first = exact.prepare_object_page(&page).unwrap();
    let duplicate = exact.prepare_object_page(&page).unwrap();
    exact.commit_object_page(clone, first).unwrap();
    assert_eq!(
        exact.commit_object_page(page, duplicate),
        Err(ClipboardError::StalePreparation)
    );

    let mut capacity_bound = coordinator("a", 64, 1);
    let request = begin(&mut capacity_bound);
    let mut spare = Vec::with_capacity(4);
    spare.push(object(93, 0, 1, "c"));
    let page = ObjectPage::new(
        ObjectPageId::new(4),
        request.key(),
        spare,
        ObjectPageEdgeFact::EnvelopeBoundary,
        ObjectPageEdgeFact::EnvelopeBoundary,
        true,
        None,
    )
    .unwrap();
    let compact_clone = page.clone();
    assert_ne!(page.retained_charge(), compact_clone.retained_charge());
    let prepared = capacity_bound.prepare_object_page(&page).unwrap();
    assert_eq!(
        capacity_bound.commit_object_page(compact_clone, prepared),
        Err(ClipboardError::WrongPreparation)
    );
    let prepared = capacity_bound.prepare_object_page(&page).unwrap();
    capacity_bound.commit_object_page(page, prepared).unwrap();

    let mut first = coordinator("a", 64, 1);
    let first_request = begin(&mut first);
    let first_page = object_page(first_request, &[], 2);
    let foreign = first.prepare_object_page(&first_page).unwrap();
    let mut second = coordinator("a", 64, 1);
    let second_request = begin(&mut second);
    assert_eq!(first_request.key(), second_request.key());
    let second_page = object_page(second_request, &[], 2);
    assert_eq!(
        second.commit_object_page(second_page, foreign),
        Err(ClipboardError::WrongPreparation)
    );
    assert_eq!(
        second.state(),
        gpui_text_input::ClipboardState::ObjectPagePending
    );

    let stale = first.prepare_object_page(&first_page).unwrap();
    let cancelled_key = ClipboardKey::new(
        ClipboardId::new(91_001),
        binding("a").binding(),
        binding("a").revision(),
        selection,
        predecessor(selection),
    );
    assert_eq!(
        first.cancel(cancelled_key).unwrap(),
        ClipboardCompletion::Cancelled
    );
    assert_eq!(
        first.commit_object_page(first_page, stale),
        Err(ClipboardError::StalePreparation)
    );

    let mut dropped = coordinator("a", 64, 1);
    let dropped_request = begin(&mut dropped);
    let dropped_page = object_page(dropped_request, &[], 3);
    let dropped_token = dropped.prepare_object_page(&dropped_page).unwrap();
    drop(dropped);
    let mut recreated = coordinator("a", 64, 1);
    let recreated_request = begin(&mut recreated);
    assert_eq!(dropped_request.key(), recreated_request.key());
    assert_eq!(
        recreated.commit_object_page(dropped_page, dropped_token),
        Err(ClipboardError::WrongPreparation)
    );
}

#[test]
fn response_capacity_charge_is_exact_for_sparse_pages_and_coordinator_transfer() {
    let selection = SourceRange::new(position(0), position(1)).unwrap();
    let mut collector = coordinator("a", 64, 8);
    let ClipboardProgress::NeedObjectPage { key, .. } = collector
        .begin(
            ClipboardId::new(92_001),
            ClipboardKind::Copy,
            selection,
            predecessor(selection),
        )
        .unwrap()
    else {
        unreachable!()
    };
    let request = collector
        .request_object_page(key, ObjectRequestId::new(1))
        .unwrap();
    let mut fallback = String::with_capacity(257);
    fallback.push('o');
    let mut display = String::with_capacity(513);
    display.push('p');
    let presentation =
        InlineObjectPresentation::new(92, display, px(8.0), px(8.0), px(6.0), None, 0, true)
            .unwrap();
    let mut objects = Vec::with_capacity(8);
    objects.push(InlineObjectFact::new(
        InlineObjectId::new(92),
        ByteOffset::new(0),
        InlineObjectOrder::new(1),
        fallback,
        presentation,
    ));
    let page = ObjectPage::new(
        ObjectPageId::new(92),
        request.key(),
        objects,
        ObjectPageEdgeFact::EnvelopeBoundary,
        ObjectPageEdgeFact::EnvelopeBoundary,
        true,
        None,
    )
    .unwrap();
    let charge = page.retained_charge();
    assert_eq!(charge.objects(), 1);
    let current = collector.ownership_charge();
    let prepared = collector.prepare_object_page(&page).unwrap();
    assert_eq!(
        prepared.successor_ownership().bytes() - current.bytes(),
        charge.bytes() - std::mem::size_of::<ObjectPage>()
    );
    assert_eq!(prepared.successor_ownership().items() - current.items(), 8);
    collector.commit_object_page(page, prepared).unwrap();

    let mut text_collector = coordinator("a", 64, 1);
    let ClipboardProgress::NeedObjectPage { key, .. } = text_collector
        .begin(
            ClipboardId::new(92_002),
            ClipboardKind::Copy,
            selection,
            predecessor(selection),
        )
        .unwrap()
    else {
        unreachable!()
    };
    let object_request = text_collector
        .request_object_page(key, ObjectRequestId::new(1))
        .unwrap();
    let ClipboardProgress::NeedTextPage { key, .. } = text_collector
        .admit_object_page(object_page(object_request, &[], 93))
        .unwrap()
    else {
        unreachable!()
    };
    let request = text_collector
        .request_text_page(key, PageRequestId::new(2))
        .unwrap();
    let mut text = String::with_capacity(64);
    text.push('a');
    let mut atom_fallback = String::with_capacity(129);
    atom_fallback.push('x');
    let range = ByteRange::from_u64(0, 1).unwrap();
    let mut atoms = Vec::with_capacity(8);
    atoms.push(AtomFact::new(AtomId::new(93), range, range, atom_fallback));
    let page = RangePage::new(
        PageId::new(93),
        request.key(),
        range,
        text,
        atoms,
        PageEdgeFact::DocumentBoundary,
        PageEdgeFact::DocumentBoundary,
        true,
    )
    .unwrap();
    let charge = page.retained_charge();
    let current = text_collector.ownership_charge();
    let prepared = text_collector.prepare_text_page(&page).unwrap();
    assert_eq!(
        prepared.successor_ownership().bytes() - current.bytes(),
        charge.bytes() - std::mem::size_of::<RangePage>()
    );
    assert_eq!(
        prepared.successor_ownership().items() - current.items(),
        charge.items() - 1
    );
    text_collector.commit_text_page(page, prepared).unwrap();
}

fn acknowledge_prepared(
    collector: &mut RangeClipboardCoordinator,
    page: ClipboardProvenancePage,
) -> Result<ClipboardProgress, ClipboardError> {
    let prepared = collector.acknowledge_provenance_page(page)?;
    let commit = collector.commit_prepared(prepared)?;
    finish_prepared(collector, commit)
}

fn binding(source: &str) -> RangeBinding {
    RangeBinding::new(
        BindingId::new(7),
        SourceRevision::new(3),
        LogicalExtent::new(
            source.len() as u64,
            source.bytes().filter(|b| *b == b'\n').count() as u64,
        ),
    )
}

fn neighbor(id: u128, order: u128) -> InlineObjectNeighbor {
    InlineObjectNeighbor::new(InlineObjectId::new(id), InlineObjectOrder::new(order))
}

fn position(offset: u64) -> SourcePosition {
    SourcePosition::new(ByteOffset::new(offset), InlineObjectGap::NoObjects)
}

fn predecessor(selection: SourceRange) -> MutationPositions {
    MutationPositions::new(selection.end(), selection.start(), selection.end())
}

fn object(id: u128, anchor: u64, order: u128, fallback: &str) -> InlineObjectFact {
    InlineObjectFact::new(
        InlineObjectId::new(id),
        ByteOffset::new(anchor),
        InlineObjectOrder::new(order),
        fallback,
        InlineObjectPresentation::new(
            id as u64,
            SharedString::new(Arc::<str>::from(fallback)),
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

fn coordinator(source: &str, cap: usize, object_count: usize) -> RangeClipboardCoordinator {
    coordinator_with_policy(
        source,
        cap,
        object_count,
        TextInputAtomClipboardPolicy::PlainText,
    )
}

fn coordinator_with_policy(
    source: &str,
    cap: usize,
    object_count: usize,
    atom_policy: TextInputAtomClipboardPolicy,
) -> RangeClipboardCoordinator {
    RangeClipboardCoordinator::new_composite(
        binding(source),
        PresentationGeneration::new(9),
        atom_policy,
        ClipboardLimits::new_composite(cap, 4, object_count, 64 * 1024).unwrap(),
    )
    .unwrap()
}

fn provenance_coordinator(
    source: &str,
    cap: usize,
    object_count: usize,
    page_items: usize,
    page_bytes: usize,
) -> RangeClipboardCoordinator {
    let provenance = ClipboardProvenanceLimits::new(page_items, page_bytes).unwrap();
    RangeClipboardCoordinator::new_composite(
        binding(source),
        PresentationGeneration::new(9),
        TextInputAtomClipboardPolicy::PlainText,
        ClipboardLimits::new_composite(cap, 4, object_count, 64 * 1024)
            .unwrap()
            .with_provenance(ClipboardProvenancePolicy::Stream(provenance)),
    )
    .unwrap()
}

fn object_page(
    request: gpui_text_input::ObjectRequest,
    all: &[InlineObjectFact],
    id: u64,
) -> ObjectPage {
    let demand = request.key().demand();
    let cursor = demand.cursor();
    let mut eligible = all
        .iter()
        .filter(|object| demand.contains_anchor(object.anchor()))
        .filter(|object| cursor.is_none_or(|cursor| object.cursor() > cursor))
        .take(demand.max_objects() + 1)
        .cloned()
        .collect::<Vec<_>>();
    let complete = eligible.len() <= demand.max_objects();
    if !complete {
        eligible.pop();
    }
    let continuation = (!complete).then(|| eligible.last().expect("progressing page").cursor());
    ObjectPage::new(
        ObjectPageId::new(id),
        request.key(),
        eligible,
        cursor.map_or(
            ObjectPageEdgeFact::EnvelopeBoundary,
            ObjectPageEdgeFact::Continues,
        ),
        continuation.map_or(
            ObjectPageEdgeFact::EnvelopeBoundary,
            ObjectPageEdgeFact::Continues,
        ),
        complete,
        continuation,
    )
    .unwrap()
}

fn text_page(source: &str, request: gpui_text_input::PageRequest, id: u64) -> RangePage {
    let key = request.key();
    let gpui_text_input::PageDemandEnvelope::Adjacent {
        anchor,
        direction: PageDirection::Forward,
        max_payload_bytes,
    } = key.demand()
    else {
        panic!("clipboard uses forward adjacent text pages")
    };
    let start = anchor.get() as usize;
    let mut end = start
        .saturating_add(max_payload_bytes as usize)
        .min(source.len());
    while end > start && !source.is_char_boundary(end) {
        end -= 1;
    }
    RangePage::new(
        PageId::new(id),
        key,
        ByteRange::from_u64(start as u64, end as u64).unwrap(),
        source[start..end].to_owned(),
        vec![],
        if start == 0 {
            PageEdgeFact::DocumentBoundary
        } else {
            PageEdgeFact::Continues
        },
        if end == source.len() {
            PageEdgeFact::DocumentBoundary
        } else {
            PageEdgeFact::Continues
        },
        end == source.len(),
    )
    .unwrap()
}

fn classification_text_page(
    source: &str,
    request: gpui_text_input::PageRequest,
    id: u64,
    atom: Option<ByteRange>,
) -> RangePage {
    let key = request.key();
    let gpui_text_input::PageDemandEnvelope::Adjacent {
        anchor,
        direction: PageDirection::Forward,
        ..
    } = key.demand()
    else {
        panic!("clipboard uses forward adjacent text pages")
    };
    let start = anchor.get() as usize;
    let end = start.saturating_add(4).min(source.len());
    let range = ByteRange::from_u64(start as u64, end as u64).unwrap();
    let atoms = atom
        .filter(|atom| range.contains(*atom))
        .map(|atom| vec![AtomFact::new(AtomId::new(91), atom, atom, "x")])
        .unwrap_or_default();
    RangePage::new(
        PageId::new(id),
        key,
        range,
        source[start..end].to_owned(),
        atoms,
        if start == 0 {
            PageEdgeFact::DocumentBoundary
        } else {
            PageEdgeFact::Continues
        },
        if end == source.len() {
            PageEdgeFact::DocumentBoundary
        } else {
            PageEdgeFact::Continues
        },
        end == source.len(),
    )
    .unwrap()
}

fn propagating_coordinator(source: &str, max_bytes: usize) -> RangeClipboardCoordinator {
    RangeClipboardCoordinator::new_composite(
        binding(source),
        PresentationGeneration::new(9),
        TextInputAtomClipboardPolicy::Propagate,
        ClipboardLimits::new_composite(max_bytes, 8, 1, 64 * 1024).unwrap(),
    )
    .unwrap()
}

fn collect(
    collector: &mut RangeClipboardCoordinator,
    source: &str,
    objects: &[InlineObjectFact],
    progress: ClipboardProgress,
) -> gpui_text_input::ClipboardWriteRequest {
    let mut progress = progress;
    let mut request_id = 1u64;
    loop {
        progress = match progress {
            ClipboardProgress::NeedObjectPage { key, .. } => {
                let request = collector
                    .request_object_page(key, ObjectRequestId::new(request_id))
                    .unwrap();
                request_id += 1;
                collector
                    .admit_object_page(object_page(request, objects, request_id))
                    .unwrap()
            }
            ClipboardProgress::NeedTextPage { key, .. } => {
                let request = collector
                    .request_text_page(key, PageRequestId::new(request_id))
                    .unwrap();
                request_id += 1;
                collector
                    .admit_text_page(text_page(source, request, request_id))
                    .unwrap()
            }
            ClipboardProgress::Write(write) => return write,
            ClipboardProgress::ProvenancePage(_) => {
                panic!("omit policy emitted provenance")
            }
            ClipboardProgress::Terminal(outcome) => panic!("unexpected terminal: {outcome:?}"),
        };
    }
}

fn collect_provenance(
    collector: &mut RangeClipboardCoordinator,
    source: &str,
    objects: &[InlineObjectFact],
    mut progress: ClipboardProgress,
) -> (
    Vec<ClipboardProvenancePage>,
    gpui_text_input::ClipboardWriteRequest,
) {
    let mut request_id = 1u64;
    let mut pages = Vec::new();
    loop {
        progress = match progress {
            ClipboardProgress::NeedObjectPage { key, .. } => {
                let request = collector
                    .request_object_page(key, ObjectRequestId::new(request_id))
                    .unwrap();
                request_id += 1;
                collector
                    .admit_object_page(object_page(request, objects, request_id))
                    .unwrap()
            }
            ClipboardProgress::NeedTextPage { key, .. } => {
                let request = collector
                    .request_text_page(key, PageRequestId::new(request_id))
                    .unwrap();
                request_id += 1;
                collector
                    .admit_text_page(text_page(source, request, request_id))
                    .unwrap()
            }
            ClipboardProgress::ProvenancePage(page) => {
                pages.push(page.clone());
                acknowledge_prepared(collector, page).unwrap()
            }
            ClipboardProgress::Write(write) => return (pages, write),
            ClipboardProgress::Terminal(outcome) => panic!("unexpected terminal: {outcome:?}"),
        };
    }
}

#[test]
fn provenance_stream_pages_same_anchor_and_empty_fallbacks_exactly() {
    let source = "ab";
    let objects = vec![
        object(11, 1, 1, ""),
        object(12, 1, 2, "XY"),
        object(13, 1, 3, "z"),
    ];
    let selection = SourceRange::new(position(0), position(2)).unwrap();
    let mut collector = provenance_coordinator(source, 16, 1, 2, 4096);
    let progress = collector
        .begin(
            ClipboardId::new(41),
            ClipboardKind::Copy,
            selection,
            predecessor(selection),
        )
        .unwrap();
    let (pages, write) = collect_provenance(&mut collector, source, &objects, progress);

    assert_eq!(write.text(), "aXYzb");
    assert_eq!(pages.len(), 2);
    assert_eq!(pages[0].key().page_ordinal(), 0);
    assert_eq!(pages[1].key().page_ordinal(), 1);
    assert_eq!(pages[1].prior_identity(), pages[0].cumulative_identity());
    assert_eq!(pages[0].items().len(), 2);
    assert_eq!(pages[1].items().len(), 1);
    assert_eq!(pages[0].items()[0].object_id(), InlineObjectId::new(11));
    assert_eq!(
        pages[0].items()[0].output_range(),
        ByteRange::from_u64(1, 1).unwrap()
    );
    assert_eq!(
        pages[0].items()[1].output_range(),
        ByteRange::from_u64(1, 3).unwrap()
    );
    assert_eq!(
        pages[1].items()[0].output_range(),
        ByteRange::from_u64(3, 4).unwrap()
    );
    assert_eq!(pages[0].next_cursor().item_ordinal(), 2);
    assert_eq!(pages[1].next_cursor().item_ordinal(), 3);
    assert_eq!(
        pages[0].next_cursor().preceding_object(),
        Some(objects[1].cursor())
    );
    assert_eq!(
        pages[1].next_cursor().preceding_object(),
        Some(objects[2].cursor())
    );
    assert!(pages.iter().all(|page| page.retained_bytes() <= 4096));

    let closure = write.provenance().expect("stream closes on write");
    assert_eq!(closure.page_count(), 2);
    assert_eq!(closure.item_count(), 3);
    assert_eq!(closure.fallback_bytes(), 3);
    assert_eq!(closure.output_bytes(), 5);
    assert_eq!(closure.prior_identity(), pages[1].cumulative_identity());
    assert_ne!(closure.final_identity(), closure.prior_identity());
    assert_eq!(collector.counts().retained_provenance_items, 0);
    assert_eq!(collector.counts().retained_provenance_bytes, 0);
    assert_eq!(
        collector
            .acknowledge_write(write.key(), ClipboardWriteOutcome::Written)
            .unwrap(),
        ClipboardCompletion::Copied
    );
    assert_eq!(collector.counts(), Default::default());
}

#[test]
fn provenance_acknowledgement_collides_same_key_and_preserves_custody_for_wrong_keys() {
    let source = "ab";
    let selection = SourceRange::new(position(0), position(2)).unwrap();
    let first_objects = vec![object(21, 1, 1, "x"), object(22, 1, 2, "y")];
    let collision_objects = vec![object(31, 1, 1, "x"), object(32, 1, 2, "y")];

    let first_page = |objects: &[InlineObjectFact]| {
        let mut collector = provenance_coordinator(source, 16, 2, 1, 4096);
        let mut progress = collector
            .begin(
                ClipboardId::new(51),
                ClipboardKind::Copy,
                selection,
                predecessor(selection),
            )
            .unwrap();
        let mut request_id = 1;
        loop {
            progress = match progress {
                ClipboardProgress::NeedObjectPage { key, .. } => {
                    let request = collector
                        .request_object_page(key, ObjectRequestId::new(request_id))
                        .unwrap();
                    request_id += 1;
                    collector
                        .admit_object_page(object_page(request, objects, request_id))
                        .unwrap()
                }
                ClipboardProgress::NeedTextPage { key, .. } => {
                    let request = collector
                        .request_text_page(key, PageRequestId::new(request_id))
                        .unwrap();
                    request_id += 1;
                    collector
                        .admit_text_page(text_page(source, request, request_id))
                        .unwrap()
                }
                ClipboardProgress::ProvenancePage(page) => return (collector, page),
                ClipboardProgress::Write(_) => panic!("missing provenance page"),
                ClipboardProgress::Terminal(outcome) => panic!("unexpected terminal: {outcome:?}"),
            };
        }
    };

    let (mut collided, page) = first_page(&first_objects);
    let (_, collision) = first_page(&collision_objects);
    assert_eq!(page.key(), collision.key());
    assert_ne!(page, collision);
    assert_eq!(
        collided.acknowledge_provenance_page(collision),
        Err(ClipboardError::ProvenancePageCollision(page.key()))
    );
    assert_eq!(collided.counts(), Default::default());

    let (mut collector, page) = first_page(&first_objects);
    let next = acknowledge_prepared(&mut collector, page.clone()).unwrap();
    let next_page = match next {
        ClipboardProgress::ProvenancePage(next_page) => next_page,
        other => panic!("expected successive provenance page, got {other:?}"),
    };
    let next_page_key = next_page.key();
    let (mut reordered, expected_first) = first_page(&first_objects);
    assert_eq!(
        reordered.acknowledge_provenance_page(next_page),
        Err(ClipboardError::WrongProvenancePage {
            expected: expected_first.key(),
            actual: next_page_key,
        })
    );
    assert_eq!(
        collector.acknowledge_provenance_page(page.clone()),
        Err(ClipboardError::WrongProvenancePage {
            expected: next_page_key,
            actual: page.key(),
        })
    );
    assert_eq!(
        reordered.cancel(expected_first.key().clipboard()).unwrap(),
        ClipboardCompletion::Cancelled
    );
    assert_eq!(reordered.counts(), Default::default());
    let expected_first_key = expected_first.key();
    assert_eq!(
        reordered.acknowledge_provenance_page(expected_first),
        Err(ClipboardError::ObsoleteProvenancePage(expected_first_key))
    );
}

#[test]
fn provenance_limits_accept_exact_retained_charge_and_reject_one_under() {
    assert_eq!(
        ClipboardProvenanceLimits::new(0, usize::MAX),
        Err(ClipboardError::InvalidLimits)
    );
    assert_eq!(
        ClipboardProvenanceLimits::new(1, 1),
        Err(ClipboardError::InvalidLimits)
    );

    let source = "ab";
    let objects = vec![object(61, 1, 1, "")];
    let selection = SourceRange::new(position(0), position(2)).unwrap();
    let mut roomy = provenance_coordinator(source, 16, 1, 1, 4096);
    let progress = roomy
        .begin(
            ClipboardId::new(61),
            ClipboardKind::Copy,
            selection,
            predecessor(selection),
        )
        .unwrap();
    let (pages, _) = collect_provenance(&mut roomy, source, &objects, progress);
    let exact = pages[0].retained_bytes();
    assert!(ClipboardProvenanceLimits::new(1, exact).is_ok());
    assert_eq!(
        ClipboardProvenanceLimits::new(1, exact - 1),
        Err(ClipboardError::InvalidLimits)
    );
}

#[test]
fn clipboard_ownership_charge_counts_queued_and_current_object_payloads_exactly() {
    let source = "ab";
    let selection = SourceRange::new(position(0), position(2)).unwrap();
    let charge_after_objects = |objects: &[InlineObjectFact]| {
        let mut collector = provenance_coordinator(source, 64 * 1024, 2, 2, 4096);
        let progress = collector
            .begin(
                ClipboardId::new(71),
                ClipboardKind::Copy,
                selection,
                predecessor(selection),
            )
            .unwrap();
        let key = match progress {
            ClipboardProgress::NeedObjectPage { key, .. } => key,
            other => panic!("expected object demand, got {other:?}"),
        };
        let request = collector
            .request_object_page(key, ObjectRequestId::new(1))
            .unwrap();
        let progress = collector
            .admit_object_page(object_page(request, objects, 1))
            .unwrap();
        assert!(matches!(progress, ClipboardProgress::NeedTextPage { .. }));
        let counts = collector.counts();
        assert_eq!(counts.retained_object_facts, 2);
        assert_eq!(counts.owned_bytes, collector.ownership_charge().bytes());
        assert_eq!(counts.owned_items, collector.ownership_charge().items());
        let charge = collector.ownership_charge();
        assert_eq!(
            collector.cancel(key).unwrap(),
            ClipboardCompletion::Cancelled
        );
        assert_eq!(collector.ownership_charge(), Default::default());
        charge
    };

    let empty = [object(711, 1, 1, ""), object(712, 1, 2, "")];
    let payload = "x".repeat(257);
    let populated = [object(711, 1, 1, ""), object(712, 1, 2, &payload)];
    let empty_charge = charge_after_objects(&empty);
    let populated_charge = charge_after_objects(&populated);
    assert_eq!(
        populated_charge.bytes() - empty_charge.bytes(),
        payload.len() * 2
    );
    assert_eq!(populated_charge.items(), empty_charge.items());
}

#[test]
fn provenance_ack_releases_page_before_lazy_next_builder_allocation() {
    let source = "abcdefghij";
    let objects = [object(721, 1, 1, ""), object(722, 9, 1, "")];
    let selection = SourceRange::new(position(0), position(10)).unwrap();
    let mut collector = provenance_coordinator(source, 64, 1, 1, 4096);
    let mut progress = collector
        .begin(
            ClipboardId::new(72),
            ClipboardKind::Copy,
            selection,
            predecessor(selection),
        )
        .unwrap();
    let fixed_collection = collector.counts().retained_provenance_bytes;
    assert!(fixed_collection > 0);
    let mut request_id = 1;
    let first_page = loop {
        progress = match progress {
            ClipboardProgress::NeedObjectPage { key, .. } => {
                let request = collector
                    .request_object_page(key, ObjectRequestId::new(request_id))
                    .unwrap();
                request_id += 1;
                collector
                    .admit_object_page(object_page(request, &objects, request_id))
                    .unwrap()
            }
            ClipboardProgress::NeedTextPage { key, .. } => {
                let request = collector
                    .request_text_page(key, PageRequestId::new(request_id))
                    .unwrap();
                request_id += 1;
                collector
                    .admit_text_page(text_page(source, request, request_id))
                    .unwrap()
            }
            ClipboardProgress::ProvenancePage(page) => break page,
            other => panic!("expected first provenance page, got {other:?}"),
        };
    };
    let page_owned = collector.counts().retained_provenance_bytes;
    assert!(page_owned > fixed_collection);

    progress = acknowledge_prepared(&mut collector, first_page).unwrap();
    let fixed = collector.counts().retained_provenance_bytes;
    assert_eq!(fixed, fixed_collection, "progress={progress:?}");
    assert!(fixed < page_owned);
    assert_eq!(collector.counts().retained_provenance_items, 0);
    assert!(matches!(
        progress,
        ClipboardProgress::NeedObjectPage { .. } | ClipboardProgress::NeedTextPage { .. }
    ));

    let key = loop {
        if let ClipboardProgress::ProvenancePage(page) = &progress {
            break page.key().clipboard();
        }
        assert_eq!(collector.counts().retained_provenance_bytes, fixed);
        progress = match progress {
            ClipboardProgress::NeedObjectPage { key, .. } => {
                let request = collector
                    .request_object_page(key, ObjectRequestId::new(request_id))
                    .unwrap();
                request_id += 1;
                collector
                    .admit_object_page(object_page(request, &objects, request_id))
                    .unwrap()
            }
            ClipboardProgress::NeedTextPage { key, .. } => {
                let request = collector
                    .request_text_page(key, PageRequestId::new(request_id))
                    .unwrap();
                request_id += 1;
                collector
                    .admit_text_page(text_page(source, request, request_id))
                    .unwrap()
            }
            ClipboardProgress::ProvenancePage(_) => unreachable!(),
            other => panic!("expected bounded progress to next provenance page, got {other:?}"),
        };
    };
    assert!(collector.counts().retained_provenance_bytes > fixed);
    assert_eq!(
        collector.cancel(key).unwrap(),
        ClipboardCompletion::Cancelled
    );
    assert_eq!(collector.ownership_charge(), Default::default());
}

#[test]
fn empty_text_only_and_reversed_selections_are_exact() {
    let source = "aéz";
    let mut empty = coordinator(source, 16, 2);
    let progress = empty
        .begin_selection(
            ClipboardId::new(1),
            ClipboardKind::Copy,
            position(1),
            position(1),
        )
        .unwrap();
    let write = collect(&mut empty, source, &[], progress);
    assert_eq!(write.text(), "");

    let mut reversed = coordinator(source, source.len(), 2);
    let progress = reversed
        .begin_selection(
            ClipboardId::new(2),
            ClipboardKind::Cut,
            position(source.len() as u64),
            position(0),
        )
        .unwrap();
    let write = collect(&mut reversed, source, &[], progress);
    assert_eq!(write.text(), source);
    let ClipboardCompletion::Delete(deletion) = reversed
        .acknowledge_write(write.key(), ClipboardWriteOutcome::Written)
        .unwrap()
    else {
        panic!("successful reversed cut must authorize deletion")
    };
    assert_eq!(
        deletion.predecessor(),
        MutationPositions::new(position(0), position(source.len() as u64), position(0))
    );
}

#[test]
fn propagation_policy_classifies_both_object_kinds_without_write_or_cut_deletion() {
    let source = "ab";
    let selection = SourceRange::new(position(0), position(2)).unwrap();
    let mut object_collector =
        coordinator_with_policy(source, 16, 1, TextInputAtomClipboardPolicy::Propagate);
    let ClipboardProgress::NeedObjectPage { key, .. } = object_collector
        .begin(
            ClipboardId::new(100),
            ClipboardKind::Cut,
            selection,
            predecessor(selection),
        )
        .unwrap()
    else {
        panic!("nonempty selection begins bounded object classification")
    };
    let request = object_collector
        .request_object_page(key, ObjectRequestId::new(1))
        .unwrap();
    let ClipboardProgress::NeedTextPage { key, .. } = object_collector
        .admit_object_page(object_page(request, &[object(1, 1, 1, "[object]")], 2))
        .unwrap()
    else {
        panic!("classification streams text preceding the selected object")
    };
    let text_request = object_collector
        .request_text_page(key, PageRequestId::new(2))
        .unwrap();
    assert_eq!(
        object_collector
            .admit_text_page(text_page(source, text_request, 3))
            .unwrap(),
        ClipboardProgress::Terminal(ClipboardCompletion::Propagate(ClipboardKind::Cut))
    );
    assert_eq!(object_collector.counts(), Default::default());

    let mut atom_collector =
        coordinator_with_policy(source, 16, 1, TextInputAtomClipboardPolicy::Propagate);
    let ClipboardProgress::NeedObjectPage { key, .. } = atom_collector
        .begin(
            ClipboardId::new(101),
            ClipboardKind::Copy,
            selection,
            predecessor(selection),
        )
        .unwrap()
    else {
        panic!("nonempty selection begins bounded object classification")
    };
    let object_request = atom_collector
        .request_object_page(key, ObjectRequestId::new(1))
        .unwrap();
    let ClipboardProgress::NeedTextPage { key, .. } = atom_collector
        .admit_object_page(object_page(object_request, &[], 2))
        .unwrap()
    else {
        panic!("object-free selection continues through bounded text pages")
    };
    let text_request = atom_collector
        .request_text_page(key, PageRequestId::new(2))
        .unwrap();
    let base = text_page(source, text_request, 3);
    let atom_range = ByteRange::from_u64(0, 2).unwrap();
    let atom_page = RangePage::new(
        base.id(),
        base.key(),
        base.range(),
        base.text().to_owned(),
        vec![AtomFact::new(AtomId::new(1), atom_range, atom_range, "x")],
        base.preceding(),
        base.following(),
        base.end_of_source(),
    )
    .unwrap();
    assert_eq!(
        atom_collector.admit_text_page(atom_page).unwrap(),
        ClipboardProgress::Terminal(ClipboardCompletion::Propagate(ClipboardKind::Copy))
    );
    assert_eq!(atom_collector.counts(), Default::default());
}

#[test]
fn propagation_classification_releases_custody_on_every_terminal_path() {
    let source = "abcd";
    let selection = SourceRange::new(position(0), position(4)).unwrap();
    let make = || coordinator_with_policy(source, 8, 1, TextInputAtomClipboardPolicy::Propagate);
    let begin = |collector: &mut RangeClipboardCoordinator, id| {
        let ClipboardProgress::NeedObjectPage { key, .. } = collector
            .begin(
                ClipboardId::new(id),
                ClipboardKind::Cut,
                selection,
                predecessor(selection),
            )
            .unwrap()
        else {
            panic!("classification starts with one bounded object page")
        };
        let request = collector
            .request_object_page(key, ObjectRequestId::new(1))
            .unwrap();
        (key, request.key())
    };

    let mut cancelled = make();
    let (key, request) = begin(&mut cancelled, 110);
    assert_eq!(
        cancelled.cancel(key).unwrap(),
        ClipboardCompletion::Cancelled
    );
    assert!(matches!(
        cancelled.settle_object_page(request, gpui_text_input::ObjectPageFailure::Cancelled),
        Err(gpui_text_input::ClipboardError::ObsoleteObjectPage(obsolete)) if obsolete == request
    ));
    assert_eq!(cancelled.counts(), Default::default());

    let mut rebound = make();
    let _ = begin(&mut rebound, 111);
    assert!(rebound.rebind(binding("replacement")).is_some());
    assert_eq!(rebound.counts(), Default::default());

    let mut disposed = make();
    let _ = begin(&mut disposed, 112);
    assert!(disposed.dispose().is_some());
    assert_eq!(disposed.counts(), Default::default());

    let mut failed = make();
    let (_, request) = begin(&mut failed, 113);
    assert_eq!(
        failed
            .settle_object_page(request, gpui_text_input::ObjectPageFailure::Unavailable)
            .unwrap(),
        ClipboardProgress::Terminal(ClipboardCompletion::ObjectPageFailed(
            gpui_text_input::ObjectPageFailure::Unavailable,
        ))
    );
    assert_eq!(failed.counts(), Default::default());

    let mut capped = coordinator_with_policy(source, 1, 1, TextInputAtomClipboardPolicy::Propagate);
    let ClipboardProgress::NeedObjectPage { key, .. } = capped
        .begin(
            ClipboardId::new(114),
            ClipboardKind::Copy,
            selection,
            predecessor(selection),
        )
        .unwrap()
    else {
        panic!("classification starts with one bounded object page")
    };
    let object_request = capped
        .request_object_page(key, ObjectRequestId::new(1))
        .unwrap();
    let ClipboardProgress::NeedTextPage { key, .. } = capped
        .admit_object_page(object_page(object_request, &[], 2))
        .unwrap()
    else {
        panic!("object-free selection continues to bounded text classification")
    };
    let text_request = capped
        .request_text_page(key, PageRequestId::new(2))
        .unwrap();
    let ClipboardProgress::NeedTextPage {
        key, next_offset, ..
    } = capped
        .admit_text_page(text_page(source, text_request, 3))
        .unwrap()
    else {
        panic!("atom-free classification restarts capped collection")
    };
    assert_eq!(next_offset, ByteOffset::new(0));
    assert_eq!(capped.counts().staged_bytes, 0);
    let collection_request = capped
        .request_text_page(key, PageRequestId::new(3))
        .unwrap();
    assert_eq!(
        capped
            .admit_text_page(text_page(source, collection_request, 4))
            .unwrap(),
        ClipboardProgress::Terminal(ClipboardCompletion::TooLarge)
    );
    assert_eq!(capped.counts(), Default::default());
}

#[test]
fn propagation_classifies_late_source_atoms_beyond_output_cap_for_copy_and_cut() {
    let source = "0123456789abcdefghijklmn";
    let selection = SourceRange::new(position(0), position(source.len() as u64)).unwrap();
    let atom = ByteRange::from_u64(16, 20).unwrap();
    for (ordinal, kind) in [ClipboardKind::Copy, ClipboardKind::Cut]
        .into_iter()
        .enumerate()
    {
        let mut collector = propagating_coordinator(source, 2);
        let mut progress = collector
            .begin(
                ClipboardId::new(120 + ordinal as u64),
                kind,
                selection,
                predecessor(selection),
            )
            .unwrap();
        let mut request_id = 1;
        let mut text_pages = 0;
        loop {
            assert_eq!(
                collector.counts().staged_bytes,
                0,
                "classification never retains fallback output"
            );
            progress = match progress {
                ClipboardProgress::NeedObjectPage { key, .. } => {
                    let request = collector
                        .request_object_page(key, ObjectRequestId::new(request_id))
                        .unwrap();
                    request_id += 1;
                    collector
                        .admit_object_page(object_page(request, &[], request_id))
                        .unwrap()
                }
                ClipboardProgress::NeedTextPage { key, .. } => {
                    let request = collector
                        .request_text_page(key, PageRequestId::new(request_id))
                        .unwrap();
                    request_id += 1;
                    text_pages += 1;
                    collector
                        .admit_text_page(classification_text_page(
                            source,
                            request,
                            request_id,
                            Some(atom),
                        ))
                        .unwrap()
                }
                ClipboardProgress::Terminal(outcome) => {
                    assert_eq!(outcome, ClipboardCompletion::Propagate(kind));
                    break;
                }
                ClipboardProgress::Write(_) => panic!("atom-bearing selection reached write"),
                ClipboardProgress::ProvenancePage(_) => {
                    panic!("omit policy emitted provenance")
                }
            };
        }
        assert!(
            text_pages > 4,
            "atom is classified after several bounded pages"
        );
        assert_eq!(collector.counts(), Default::default());
    }
}

#[test]
fn atom_free_classification_restarts_exact_text_collection_before_write_or_cap() {
    let source = "abcdefghijkl";
    let selection = SourceRange::new(position(0), position(source.len() as u64)).unwrap();
    for (max_bytes, expected_text) in [(source.len(), Some(source)), (5, None)] {
        let mut collector = propagating_coordinator(source, max_bytes);
        let mut progress = collector
            .begin(
                ClipboardId::new(130 + max_bytes as u64),
                ClipboardKind::Copy,
                selection,
                predecessor(selection),
            )
            .unwrap();
        let mut request_id = 1;
        let mut text_pass = 0;
        let mut prior_offset = None;
        let terminal = loop {
            progress = match progress {
                ClipboardProgress::NeedObjectPage { key, .. } => {
                    let request = collector
                        .request_object_page(key, ObjectRequestId::new(request_id))
                        .unwrap();
                    request_id += 1;
                    collector
                        .admit_object_page(object_page(request, &[], request_id))
                        .unwrap()
                }
                ClipboardProgress::NeedTextPage {
                    key, next_offset, ..
                } => {
                    if prior_offset.is_some_and(|prior| next_offset < prior) {
                        text_pass += 1;
                        assert_eq!(next_offset, ByteOffset::new(0));
                        assert_eq!(collector.counts().staged_bytes, 0);
                    }
                    if text_pass == 0 {
                        assert_eq!(collector.counts().staged_bytes, 0);
                    }
                    prior_offset = Some(next_offset);
                    let request = collector
                        .request_text_page(key, PageRequestId::new(request_id))
                        .unwrap();
                    request_id += 1;
                    collector
                        .admit_text_page(classification_text_page(
                            source, request, request_id, None,
                        ))
                        .unwrap()
                }
                ClipboardProgress::Write(write) => break Ok(write),
                ClipboardProgress::ProvenancePage(_) => {
                    panic!("omit policy emitted provenance")
                }
                ClipboardProgress::Terminal(outcome) => break Err(outcome),
            };
        };
        assert_eq!(
            text_pass, 1,
            "atom-free classification restarts exactly once"
        );
        match expected_text {
            Some(expected) => {
                let write = terminal.expect("within-cap text reaches write");
                assert_eq!(write.text(), expected);
            }
            None => {
                assert_eq!(terminal, Err(ClipboardCompletion::TooLarge));
                assert_eq!(collector.counts(), Default::default());
            }
        }
    }
}

#[test]
fn one_object_object_only_and_same_anchor_selection_follow_gap_order() {
    let source = "ab";
    let first = neighbor(1, 10);
    let second = neighbor(2, 20);
    let objects = [object(1, 1, 10, "[one]"), object(2, 1, 20, "[two]")];

    let only_second = SourceRange::new(
        SourcePosition::new(
            ByteOffset::new(1),
            InlineObjectGap::between(first, second).unwrap(),
        ),
        SourcePosition::new(ByteOffset::new(1), InlineObjectGap::after(second)),
    )
    .unwrap();
    let mut collector = coordinator(source, 16, 1);
    let progress = collector
        .begin(
            ClipboardId::new(3),
            ClipboardKind::Copy,
            only_second,
            predecessor(only_second),
        )
        .unwrap();
    let write = collect(&mut collector, source, &objects, progress);
    assert_eq!(write.text(), "[two]");
    assert_eq!(collector.counts().retained_object_facts, 0);

    let one = [object(8, 0, 1, "object")];
    let range = SourceRange::new(
        SourcePosition::new(ByteOffset::new(0), InlineObjectGap::before(neighbor(8, 1))),
        SourcePosition::new(ByteOffset::new(0), InlineObjectGap::after(neighbor(8, 1))),
    )
    .unwrap();
    let mut collector = coordinator("", 6, 1);
    let progress = collector
        .begin(
            ClipboardId::new(4),
            ClipboardKind::Copy,
            range,
            predecessor(range),
        )
        .unwrap();
    assert_eq!(collect(&mut collector, "", &one, progress).text(), "object");
}

#[test]
fn mixed_and_exact_boundary_selection_emit_only_selected_objects() {
    let source = "abc";
    let at_start = neighbor(10, 1);
    let middle = neighbor(11, 1);
    let at_end = neighbor(12, 1);
    let objects = [
        object(10, 0, 1, "S"),
        object(11, 1, 1, "M"),
        object(12, 3, 1, "E"),
    ];
    let range = SourceRange::new(
        SourcePosition::new(ByteOffset::new(0), InlineObjectGap::after(at_start)),
        SourcePosition::new(ByteOffset::new(3), InlineObjectGap::before(at_end)),
    )
    .unwrap();
    let mut collector = coordinator(source, 8, 2);
    let progress = collector
        .begin(
            ClipboardId::new(5),
            ClipboardKind::Copy,
            range,
            predecessor(range),
        )
        .unwrap();
    assert_eq!(
        collect(&mut collector, source, &objects, progress).text(),
        "aMbc"
    );
    assert_eq!(source, "abc", "fallback never becomes source bytes");
    let _ = middle;
}

#[test]
fn exact_cap_is_accepted_and_one_byte_over_is_terminal_before_write() {
    let source = "ab";
    let object = object(1, 1, 1, "XY");
    let selection = SourceRange::new(position(0), position(2)).unwrap();
    let mut exact = coordinator(source, 4, 1);
    let progress = exact
        .begin(
            ClipboardId::new(6),
            ClipboardKind::Copy,
            selection,
            predecessor(selection),
        )
        .unwrap();
    assert_eq!(
        collect(&mut exact, source, &[object.clone()], progress).text(),
        "aXYb"
    );

    let mut over = coordinator(source, 3, 1);
    let mut progress = over
        .begin(
            ClipboardId::new(7),
            ClipboardKind::Copy,
            selection,
            predecessor(selection),
        )
        .unwrap();
    let mut id = 1;
    loop {
        progress = match progress {
            ClipboardProgress::NeedObjectPage { key, .. } => {
                let request = over
                    .request_object_page(key, ObjectRequestId::new(id))
                    .unwrap();
                id += 1;
                over.admit_object_page(object_page(request, &[object.clone()], id))
                    .unwrap()
            }
            ClipboardProgress::NeedTextPage { key, .. } => {
                let request = over.request_text_page(key, PageRequestId::new(id)).unwrap();
                id += 1;
                over.admit_text_page(text_page(source, request, id))
                    .unwrap()
            }
            ClipboardProgress::Terminal(outcome) => {
                assert_eq!(outcome, ClipboardCompletion::TooLarge);
                break;
            }
            ClipboardProgress::Write(_) => panic!("over-cap value reached platform write"),
            ClipboardProgress::ProvenancePage(_) => {
                panic!("omit policy emitted provenance")
            }
        };
    }
    assert_eq!(over.counts(), Default::default());
}

#[test]
fn object_failure_cancellation_rebind_and_dispose_release_once() {
    let selection = SourceRange::new(position(0), position(1)).unwrap();
    for (failure, expected) in [
        (
            gpui_text_input::ObjectPageFailure::Unavailable,
            ClipboardCompletion::ObjectPageFailed(gpui_text_input::ObjectPageFailure::Unavailable),
        ),
        (
            gpui_text_input::ObjectPageFailure::Cancelled,
            ClipboardCompletion::Cancelled,
        ),
    ] {
        let mut collector = coordinator("a", 8, 1);
        let ClipboardProgress::NeedObjectPage { key, .. } = collector
            .begin(
                ClipboardId::new(8),
                ClipboardKind::Copy,
                selection,
                predecessor(selection),
            )
            .unwrap()
        else {
            unreachable!()
        };
        let request = collector
            .request_object_page(key, ObjectRequestId::new(1))
            .unwrap();
        assert_eq!(
            collector
                .settle_object_page(request.key(), failure)
                .unwrap(),
            ClipboardProgress::Terminal(expected)
        );
        assert_eq!(collector.counts(), Default::default());
    }

    let mut rebound = coordinator("a", 8, 1);
    let ClipboardProgress::NeedObjectPage { key, .. } = rebound
        .begin(
            ClipboardId::new(9),
            ClipboardKind::Copy,
            selection,
            predecessor(selection),
        )
        .unwrap()
    else {
        unreachable!()
    };
    let request = rebound
        .request_object_page(key, ObjectRequestId::new(1))
        .unwrap();
    let cancellation = rebound.rebind(binding("other")).unwrap();
    assert_eq!(cancellation.pending_object_page(), Some(request.key()));
    assert_eq!(rebound.counts(), Default::default());

    let mut disposed = coordinator("a", 8, 1);
    let ClipboardProgress::NeedObjectPage { key, .. } = disposed
        .begin(
            ClipboardId::new(10),
            ClipboardKind::Copy,
            selection,
            predecessor(selection),
        )
        .unwrap()
    else {
        unreachable!()
    };
    let _ = disposed
        .request_object_page(key, ObjectRequestId::new(1))
        .unwrap();
    assert!(disposed.dispose().is_some());
    assert!(disposed.dispose().is_none());
}

#[test]
fn text_failure_and_explicit_cancellation_publish_no_write_or_delete() {
    let selection = SourceRange::new(position(0), position(1)).unwrap();
    let mut failed = coordinator("a", 8, 1);
    let ClipboardProgress::NeedObjectPage { key, .. } = failed
        .begin(
            ClipboardId::new(20),
            ClipboardKind::Cut,
            selection,
            predecessor(selection),
        )
        .unwrap()
    else {
        unreachable!()
    };
    let object_request = failed
        .request_object_page(key, ObjectRequestId::new(1))
        .unwrap();
    let progress = failed
        .admit_object_page(object_page(object_request, &[], 1))
        .unwrap();
    let ClipboardProgress::NeedTextPage { key, .. } = progress else {
        unreachable!()
    };
    let text_request = failed
        .request_text_page(key, PageRequestId::new(2))
        .unwrap();
    assert_eq!(
        failed
            .settle_text_page(
                text_request.key(),
                gpui_text_input::PageFailure::Unavailable
            )
            .unwrap(),
        ClipboardProgress::Terminal(ClipboardCompletion::TextPageFailed(
            gpui_text_input::PageFailure::Unavailable,
        ))
    );
    assert_eq!(failed.counts(), Default::default());

    let mut cancelled = coordinator("a", 8, 1);
    let ClipboardProgress::NeedObjectPage { key, .. } = cancelled
        .begin(
            ClipboardId::new(21),
            ClipboardKind::Copy,
            selection,
            predecessor(selection),
        )
        .unwrap()
    else {
        unreachable!()
    };
    let _ = cancelled
        .request_object_page(key, ObjectRequestId::new(1))
        .unwrap();
    assert_eq!(
        cancelled.cancel(key).unwrap(),
        ClipboardCompletion::Cancelled
    );
    assert_eq!(cancelled.counts(), Default::default());
}

#[test]
fn cut_authorizes_exact_deletion_only_after_successful_write() {
    let source = "abc";
    let selection = SourceRange::new(position(0), position(3)).unwrap();
    let mut failed = coordinator(source, 3, 1);
    let progress = failed
        .begin(
            ClipboardId::new(11),
            ClipboardKind::Cut,
            selection,
            predecessor(selection),
        )
        .unwrap();
    let write = collect(&mut failed, source, &[], progress);
    assert_eq!(
        failed
            .acknowledge_write(write.key(), ClipboardWriteOutcome::Failed)
            .unwrap(),
        ClipboardCompletion::WriteFailed
    );

    let mut written = coordinator(source, 3, 1);
    let progress = written
        .begin(
            ClipboardId::new(12),
            ClipboardKind::Cut,
            selection,
            predecessor(selection),
        )
        .unwrap();
    let write = collect(&mut written, source, &[], progress);
    let ClipboardCompletion::Delete(deletion) = written
        .acknowledge_write(write.key(), ClipboardWriteOutcome::Written)
        .unwrap()
    else {
        panic!("successful cut write must authorize deletion")
    };
    assert_eq!(deletion.binding(), binding(source));
    assert_eq!(deletion.selection(), selection);
    assert_eq!(deletion.predecessor(), predecessor(selection));
    let proposal = deletion
        .proposal(gpui_text_input::OperationId::new(99), selection)
        .unwrap();
    assert_eq!(proposal.replacement(), selection);
    assert_eq!(proposal.predecessor(), predecessor(selection));
}
