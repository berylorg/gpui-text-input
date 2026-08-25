use std::sync::Arc;

use gpui::{SharedString, px};
use gpui_text_input::{
    AtomFact, AtomId, BindingId, ByteOffset, ByteRange, ClipboardCompletion, ClipboardId,
    ClipboardKind, ClipboardLimits, ClipboardProgress, ClipboardWriteOutcome, InlineObjectFact,
    InlineObjectGap, InlineObjectId, InlineObjectNeighbor, InlineObjectOrder,
    InlineObjectPresentation, LogicalExtent, MutationPositions, ObjectPage, ObjectPageEdgeFact,
    ObjectPageId, ObjectRequestId, PageDirection, PageEdgeFact, PageId, PageRequestId,
    PresentationGeneration, RangeBinding, RangeClipboardCoordinator, RangePage, SourcePosition,
    SourceRange, SourceRevision, TextInputAtomClipboardPolicy,
};

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
            ClipboardProgress::Terminal(outcome) => panic!("unexpected terminal: {outcome:?}"),
        };
    }
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
