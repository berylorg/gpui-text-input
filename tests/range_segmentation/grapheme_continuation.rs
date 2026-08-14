use super::*;

#[derive(Default)]
struct TraversalStats {
    pages: usize,
    max_page_bytes: usize,
    next_chunks: usize,
    prev_chunks: usize,
    pre_contexts: usize,
    replays: usize,
}

pub(super) fn scalar_partitions(text: &str, widths: &[usize]) -> Vec<ByteRange> {
    let offsets: Vec<_> = text
        .char_indices()
        .map(|(offset, _)| offset)
        .chain(std::iter::once(text.len()))
        .collect();
    let mut partitions = Vec::new();
    let mut scalar = 0;
    let mut width = 0;
    while scalar + 1 < offsets.len() {
        let count = widths[width % widths.len()];
        let end = (scalar + count).min(offsets.len() - 1);
        partitions.push(ByteRange::from_u64(offsets[scalar] as u64, offsets[end] as u64).unwrap());
        scalar = end;
        width += 1;
    }
    partitions
}

pub(super) fn exact_partition(
    partitions: &[ByteRange],
    direction: SegmentationDirection,
    origin: usize,
) -> ByteRange {
    let origin = origin as u64;
    partitions
        .iter()
        .copied()
        .find(|range| match direction {
            SegmentationDirection::Forward => {
                range.start().get() <= origin && origin < range.end().get()
            }
            SegmentationDirection::Reverse => {
                range.start().get() < origin && origin <= range.end().get()
            }
        })
        .unwrap()
}

pub(super) fn demanded_partition(
    partitions: &[ByteRange],
    edge: AdjacentPageEdge,
    direction: SegmentationDirection,
) -> ByteRange {
    match edge {
        AdjacentPageEdge::NextChunk(offset) => {
            let range = partitions
                .iter()
                .copied()
                .find(|range| range.start() == offset)
                .or_else(|| {
                    partitions
                        .iter()
                        .copied()
                        .find(|range| range.start() < offset && offset < range.end())
                })
                .unwrap();
            ByteRange::new(offset, range.end()).unwrap()
        }
        AdjacentPageEdge::PrevChunk(offset) | AdjacentPageEdge::PreContext(offset) => {
            let range = partitions
                .iter()
                .copied()
                .find(|range| range.end() == offset)
                .or_else(|| {
                    partitions
                        .iter()
                        .copied()
                        .find(|range| range.start() < offset && offset < range.end())
                })
                .unwrap();
            ByteRange::new(range.start(), offset).unwrap()
        }
        AdjacentPageEdge::Replay(offset) => match direction {
            SegmentationDirection::Forward => {
                let range = partitions
                    .iter()
                    .copied()
                    .find(|range| range.start() == offset)
                    .or_else(|| {
                        partitions
                            .iter()
                            .copied()
                            .find(|range| range.start() < offset && offset < range.end())
                    })
                    .unwrap();
                ByteRange::new(offset, range.end()).unwrap()
            }
            SegmentationDirection::Reverse => {
                let range = partitions
                    .iter()
                    .copied()
                    .find(|range| range.end() == offset)
                    .or_else(|| {
                        partitions
                            .iter()
                            .copied()
                            .find(|range| range.start() < offset && offset < range.end())
                    })
                    .unwrap();
                ByteRange::new(range.start(), offset).unwrap()
            }
        },
    }
}

fn resolve_partitioned(
    text: &str,
    partitions: &[ByteRange],
    direction: SegmentationDirection,
    origin: usize,
    max_steps: usize,
) -> (u64, TraversalStats) {
    let max_page_bytes = partitions.iter().map(|range| range.len()).max().unwrap();
    let containing = exact_partition(partitions, direction, origin);
    let first_range = match direction {
        SegmentationDirection::Forward => {
            ByteRange::new(ByteOffset::new(origin as u64), containing.end()).unwrap()
        }
        SegmentationDirection::Reverse => {
            ByteRange::new(containing.start(), ByteOffset::new(origin as u64)).unwrap()
        }
    };
    let mut progress = SegmentationContinuation::start(
        BINDING,
        REVISION,
        LogicalExtent::new(text.len() as u64, 1),
        SegmentationKind::Grapheme,
        direction,
        ByteOffset::new(origin as u64),
        limits(max_page_bytes.max(4), max_steps),
        directed_request(1, first_range, direction),
    )
    .unwrap();
    let mut next_id = 2;
    let mut stats = TraversalStats::default();
    let mut current_range = first_range;

    loop {
        let SegmentationProgress::NeedPage(mut continuation) = progress else {
            let SegmentationProgress::Complete(boundary) = progress else {
                unreachable!()
            };
            return (boundary.offset().get(), stats);
        };
        assert_eq!(
            continuation.counts(),
            gpui_text_input::SegmentationCounts {
                continuations: 1,
                pending_pages: 1,
                resident_pages: 0,
                resident_page_bytes: 0,
            }
        );
        let current = page_at(text, *continuation.pending_request(), current_range);
        stats.pages += 1;
        stats.max_page_bytes = stats.max_page_bytes.max(current.text().len());
        assert!(stats.pages < 10_000, "continuation did not converge");
        let mut successor_range = None;
        let resumed = continuation
            .resume(&current, |demand| {
                match demand.edge() {
                    AdjacentPageEdge::NextChunk(_) => stats.next_chunks += 1,
                    AdjacentPageEdge::PrevChunk(_) => stats.prev_chunks += 1,
                    AdjacentPageEdge::PreContext(_) => stats.pre_contexts += 1,
                    AdjacentPageEdge::Replay(_) => stats.replays += 1,
                }
                let range = demanded_partition(partitions, demand.edge(), direction);
                successor_range = Some(range);
                let direction = match demand.edge() {
                    AdjacentPageEdge::NextChunk(_) => SegmentationDirection::Forward,
                    AdjacentPageEdge::PrevChunk(_) | AdjacentPageEdge::PreContext(_) => {
                        SegmentationDirection::Reverse
                    }
                    AdjacentPageEdge::Replay(_) => direction,
                };
                let next = directed_request(next_id, range, direction);
                next_id += 1;
                next
            })
            .unwrap();
        progress = match resumed {
            SegmentationResume::Complete(boundary) => SegmentationProgress::Complete(boundary),
            SegmentationResume::NeedPage => {
                current_range = successor_range.expect("successor range");
                SegmentationProgress::NeedPage(continuation)
            }
        };
    }
}

fn assert_differential(text: &str, widths: &[usize]) -> TraversalStats {
    let partitions = scalar_partitions(text, widths);
    let boundaries: Vec<_> = std::iter::once(0)
        .chain(
            text.grapheme_indices(true)
                .map(|(offset, value)| offset + value.len()),
        )
        .collect();
    let origins: Vec<_> = text
        .char_indices()
        .map(|(offset, _)| offset)
        .filter(|offset| *offset > 0)
        .collect();
    let mut aggregate = TraversalStats::default();
    for origin in origins {
        let expected_forward = boundaries
            .iter()
            .copied()
            .find(|boundary| *boundary > origin)
            .unwrap_or(text.len());
        let expected_reverse = boundaries
            .iter()
            .copied()
            .rev()
            .find(|boundary| *boundary < origin)
            .unwrap_or(0);
        let (forward, forward_stats) =
            resolve_partitioned(text, &partitions, SegmentationDirection::Forward, origin, 1);
        let (reverse, reverse_stats) =
            resolve_partitioned(text, &partitions, SegmentationDirection::Reverse, origin, 1);
        assert_eq!(forward, expected_forward as u64, "forward at {origin}");
        assert_eq!(reverse, expected_reverse as u64, "reverse at {origin}");
        for stats in [forward_stats, reverse_stats] {
            aggregate.pages += stats.pages;
            aggregate.max_page_bytes = aggregate.max_page_bytes.max(stats.max_page_bytes);
            aggregate.next_chunks += stats.next_chunks;
            aggregate.prev_chunks += stats.prev_chunks;
            aggregate.pre_contexts += stats.pre_contexts;
            aggregate.replays += stats.replays;
        }
    }
    aggregate
}

#[test]
fn grapheme_matches_contiguous_results_across_valid_utf8_partitions() {
    let text = concat!(
        "x",
        "a\u{301}\u{308}",
        "\u{1f469}\u{1f3fd}\u{200d}\u{1f4bb}",
        "\u{1f1e8}\u{1f1ff}\u{1f1fa}\u{1f1f8}",
        "\u{0600}q",
        "\u{0915}\u{094d}\u{0915}",
        "\r\ny",
    );
    let mut observed_pre_context = false;
    for widths in [&[1][..], &[2], &[1, 2, 3], &[3, 1, 2]] {
        let stats = assert_differential(text, widths);
        observed_pre_context |= stats.pre_contexts > 0;
        assert!(stats.max_page_bytes <= 12);
    }
    assert!(
        observed_pre_context,
        "arbitrary-offset traversal must exercise pre-context"
    );
}

#[test]
fn growing_adversarial_families_keep_fixed_residency() {
    for length in [1, 8, 64, 128] {
        let families = [
            format!("x{}y", format!("a{}", "\u{301}".repeat(length))),
            format!("x\u{1f469}{}\u{200d}\u{1f4bb}y", "\u{301}".repeat(length)),
            format!("x{}y", "\u{1f1e8}".repeat(length + 1)),
            format!("x{}a\ny", "\u{0600}".repeat(length)),
            format!("x\u{0915}{}\u{094d}\u{0915}y", "\u{093c}".repeat(length)),
        ];
        for text in families {
            let stats = assert_differential(&text, &[1]);
            assert!(stats.max_page_bytes <= 4);
        }
    }
}

#[test]
fn exact_page_and_work_caps_admit_at_limit_and_reject_one_under() {
    let range = ByteRange::from_u64(0, 4).unwrap();
    let at_cap = SegmentationContinuation::start(
        BINDING,
        REVISION,
        LogicalExtent::new(8, 1),
        SegmentationKind::Grapheme,
        SegmentationDirection::Forward,
        ByteOffset::new(0),
        limits(4, 1),
        directed_request(1, range, SegmentationDirection::Forward),
    );
    assert!(matches!(at_cap, Ok(SegmentationProgress::NeedPage(_))));

    assert_eq!(
        SegmentationContinuation::start(
            BINDING,
            REVISION,
            LogicalExtent::new(8, 1),
            SegmentationKind::Grapheme,
            SegmentationDirection::Forward,
            ByteOffset::new(0),
            limits(4, 1),
            PageRequestKey::adjacent(
                PageRequestId::new(1),
                BINDING,
                REVISION,
                PagePurpose::Segmentation,
                ByteOffset::new(0),
                PageDirection::Forward,
                5,
            )
            .unwrap(),
        )
        .unwrap_err(),
        SegmentationError::PageRangeLimitExceeded,
    );
    assert_eq!(
        SegmentationLimits::new(3, 1).unwrap_err(),
        SegmentationError::InvalidLimits
    );
    assert_eq!(
        SegmentationLimits::new(4, 0).unwrap_err(),
        SegmentationError::InvalidLimits,
    );

    let text = "abcdef";
    let first = request(20, ByteRange::from_u64(0, 2).unwrap());
    let SegmentationProgress::NeedPage(mut continuation) = SegmentationContinuation::start(
        BINDING,
        REVISION,
        LogicalExtent::new(6, 1),
        SegmentationKind::LogicalLine,
        SegmentationDirection::Forward,
        ByteOffset::new(0),
        limits(4, 1),
        first,
    )
    .unwrap() else {
        unreachable!()
    };
    assert_eq!(
        continuation
            .resume(&page(text, first), |demand| {
                let (anchor, direction) = match demand.edge() {
                    AdjacentPageEdge::NextChunk(anchor) | AdjacentPageEdge::Replay(anchor) => {
                        (anchor, PageDirection::Forward)
                    }
                    AdjacentPageEdge::PrevChunk(anchor) | AdjacentPageEdge::PreContext(anchor) => {
                        (anchor, PageDirection::Backward)
                    }
                };
                keyed_request(
                    PageRequestId::new(21),
                    BINDING,
                    REVISION,
                    PagePurpose::Segmentation,
                    anchor,
                    direction,
                    5,
                )
            })
            .unwrap_err(),
        SegmentationError::PageRangeLimitExceeded,
    );
    assert_eq!(
        continuation
            .resume(&page(text, first), |demand| {
                let anchor = match demand.edge() {
                    AdjacentPageEdge::NextChunk(a)
                    | AdjacentPageEdge::Replay(a)
                    | AdjacentPageEdge::PrevChunk(a)
                    | AdjacentPageEdge::PreContext(a) => a,
                };
                keyed_request(
                    PageRequestId::new(20),
                    BINDING,
                    REVISION,
                    PagePurpose::Segmentation,
                    anchor,
                    PageDirection::Forward,
                    4,
                )
            })
            .unwrap_err(),
        SegmentationError::InvalidRequest,
    );
    assert_eq!(
        continuation
            .resume(&page(text, first), |demand| {
                let anchor = match demand.edge() {
                    AdjacentPageEdge::NextChunk(a)
                    | AdjacentPageEdge::Replay(a)
                    | AdjacentPageEdge::PrevChunk(a)
                    | AdjacentPageEdge::PreContext(a) => a,
                };
                keyed_request(
                    PageRequestId::new(21),
                    BINDING,
                    REVISION,
                    PagePurpose::Segmentation,
                    anchor,
                    PageDirection::Forward,
                    4,
                )
            })
            .unwrap(),
        SegmentationResume::NeedPage,
    );
    assert_eq!(continuation.pending_request().max_payload_bytes(), 4);

    let text = format!("a{}b", "\u{301}".repeat(64));
    let partitions = scalar_partitions(&text, &[1]);
    let (boundary, stats) =
        resolve_partitioned(&text, &partitions, SegmentationDirection::Forward, 0, 1);
    assert_eq!(boundary, (text.len() - 1) as u64);
    assert!(stats.pages > 32);
    assert!(stats.max_page_bytes <= 2);
}

#[test]
fn obsolete_revision_and_rebind_pages_preserve_the_pending_continuation() {
    let text = "ab";
    let first = request(1, ByteRange::from_u64(0, 1).unwrap());
    let SegmentationProgress::NeedPage(mut continuation) = SegmentationContinuation::start(
        BINDING,
        REVISION,
        LogicalExtent::new(2, 1),
        SegmentationKind::Grapheme,
        SegmentationDirection::Forward,
        ByteOffset::new(0),
        limits(4, 1),
        first,
    )
    .unwrap() else {
        unreachable!()
    };
    for stale_key in [
        keyed_request(
            PageRequestId::new(1),
            BINDING,
            SourceRevision::new(12),
            PagePurpose::Segmentation,
            ByteOffset::new(0),
            PageDirection::Forward,
            4,
        ),
        keyed_request(
            PageRequestId::new(1),
            BindingId::new(8),
            REVISION,
            PagePurpose::Segmentation,
            ByteOffset::new(0),
            PageDirection::Forward,
            4,
        ),
    ] {
        assert_eq!(
            continuation
                .resume(&page(text, stale_key), |_| unreachable!())
                .unwrap_err(),
            SegmentationError::ObsoletePage,
        );
        assert_eq!(*continuation.pending_request(), first);
    }
    assert_eq!(continuation.cancel().pending_request(), first);
}

#[test]
fn resolved_terminal_boundaries_are_marked_as_document_edges() {
    let text = "ab";
    let partitions = scalar_partitions(text, &[1]);
    let first = exact_partition(&partitions, SegmentationDirection::Forward, 1);
    let first_key = request(1, first);
    let SegmentationProgress::NeedPage(mut continuation) = SegmentationContinuation::start(
        BINDING,
        REVISION,
        LogicalExtent::new(2, 1),
        SegmentationKind::Grapheme,
        SegmentationDirection::Forward,
        ByteOffset::new(1),
        limits(4, 1),
        first_key,
    )
    .unwrap() else {
        unreachable!()
    };
    let SegmentationResume::Complete(boundary) = continuation
        .resume(&page(text, first_key), |_| unreachable!())
        .unwrap()
    else {
        unreachable!()
    };
    assert_eq!(boundary.offset(), ByteOffset::new(2));
    assert!(boundary.is_document_edge());
}
