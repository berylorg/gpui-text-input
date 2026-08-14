use super::grapheme_continuation::{demanded_partition, exact_partition, scalar_partitions};
use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum EdgeKind {
    Next,
    Prev,
    Context,
    Replay,
}

fn edge_kind(edge: AdjacentPageEdge) -> EdgeKind {
    match edge {
        AdjacentPageEdge::NextChunk(_) => EdgeKind::Next,
        AdjacentPageEdge::PrevChunk(_) => EdgeKind::Prev,
        AdjacentPageEdge::PreContext(_) => EdgeKind::Context,
        AdjacentPageEdge::Replay(_) => EdgeKind::Replay,
    }
}

fn assert_rejection(
    continuation: &mut SegmentationContinuation,
    current_page: &RangePage,
    expected: SegmentationError,
    mut invalid: impl FnMut(AdjacentPageEdge) -> PageRequestKey,
) -> AdjacentPageEdge {
    let pending = *continuation.pending_request();
    let mut observed = None;
    assert_eq!(
        continuation
            .resume(current_page, |demand| {
                observed = Some(demand.edge());
                invalid(demand.edge())
            })
            .unwrap_err(),
        expected,
    );
    assert_eq!(*continuation.pending_request(), pending);
    observed.expect("cursor must reproduce its exact demand")
}

fn reject_matrix_then_finish(
    text: &str,
    segmentation_kind: SegmentationKind,
    direction: SegmentationDirection,
    origin: usize,
) -> (u64, Vec<EdgeKind>) {
    let partitions = scalar_partitions(text, &[1]);
    let first_range = exact_partition(&partitions, direction, origin);
    let mut progress = SegmentationContinuation::start(
        BINDING,
        REVISION,
        LogicalExtent::new(text.len() as u64, 1),
        segmentation_kind,
        direction,
        ByteOffset::new(origin as u64),
        limits(4, 1),
        directed_request(1, first_range, direction),
    )
    .unwrap();
    let mut next_id = 2;
    let mut covered = Vec::new();

    loop {
        let SegmentationProgress::NeedPage(mut continuation) = progress else {
            let SegmentationProgress::Complete(boundary) = progress else {
                unreachable!()
            };
            return (boundary.offset().get(), covered);
        };
        let pending = *continuation.pending_request();
        let current_page = page(text, pending);
        if let Some(malformed) = over_extent_page(text, pending) {
            let counts = continuation.counts();
            assert_eq!(
                continuation
                    .resume(&malformed, |_| panic!(
                        "malformed admission must not advance"
                    ))
                    .unwrap_err(),
                SegmentationError::MalformedPage,
            );
            assert_eq!(*continuation.pending_request(), pending);
            assert_eq!(continuation.counts(), counts);
        }
        let mut observed = None;
        let probe = continuation.resume(&current_page, |demand| {
            observed = Some(demand.edge());
            request(next_id, ByteRange::from_u64(0, 5).unwrap())
        });
        let edge = match probe {
            Ok(SegmentationResume::Complete(boundary)) => {
                return (boundary.offset().get(), covered);
            }
            Ok(SegmentationResume::NeedPage) => unreachable!(),
            Err(error) => {
                assert_eq!(error, SegmentationError::PageRangeLimitExceeded);
                assert_eq!(*continuation.pending_request(), pending);
                observed.expect("incomplete cursor must request a successor")
            }
        };
        let valid_range = demanded_partition(&partitions, edge, direction);
        let kind = edge_kind(edge);
        if !covered.contains(&kind) {
            let nonadjacent = partitions
                .iter()
                .copied()
                .find(|range| *range != valid_range)
                .unwrap();
            assert_eq!(
                edge_kind(assert_rejection(
                    &mut continuation,
                    &current_page,
                    SegmentationError::NonAdjacentRequest,
                    |_| request(next_id, nonadjacent),
                )),
                kind,
            );
            for invalid in [
                keyed_request(
                    PageRequestId::new(next_id),
                    BindingId::new(99),
                    REVISION,
                    PagePurpose::Segmentation,
                    valid_range.start(),
                    PageDirection::Forward,
                    valid_range.len().max(4),
                ),
                keyed_request(
                    PageRequestId::new(next_id),
                    BINDING,
                    SourceRevision::new(99),
                    PagePurpose::Segmentation,
                    valid_range.start(),
                    PageDirection::Forward,
                    valid_range.len().max(4),
                ),
                keyed_request(
                    PageRequestId::new(next_id),
                    BINDING,
                    REVISION,
                    PagePurpose::Caret,
                    valid_range.start(),
                    PageDirection::Forward,
                    valid_range.len().max(4),
                ),
                keyed_request(
                    pending.id(),
                    BINDING,
                    REVISION,
                    PagePurpose::Segmentation,
                    valid_range.start(),
                    PageDirection::Forward,
                    valid_range.len().max(4),
                ),
            ] {
                assert_eq!(
                    edge_kind(assert_rejection(
                        &mut continuation,
                        &current_page,
                        SegmentationError::InvalidRequest,
                        |_| invalid,
                    )),
                    kind,
                );
            }
            covered.push(kind);
        }

        let resumed = continuation
            .resume(&current_page, |retry| {
                assert_eq!(retry.edge(), edge);
                let request_direction = match retry.edge() {
                    AdjacentPageEdge::NextChunk(_) => SegmentationDirection::Forward,
                    AdjacentPageEdge::PrevChunk(_) | AdjacentPageEdge::PreContext(_) => {
                        SegmentationDirection::Reverse
                    }
                    AdjacentPageEdge::Replay(_) => direction,
                };
                let valid = directed_request(next_id, valid_range, request_direction);
                next_id += 1;
                valid
            })
            .unwrap();
        progress = match resumed {
            SegmentationResume::Complete(boundary) => SegmentationProgress::Complete(boundary),
            SegmentationResume::NeedPage => SegmentationProgress::NeedPage(continuation),
        };
    }
}

#[test]
fn grapheme_successor_rejection_is_atomic_during_traversal_context_and_replay() {
    let text = concat!(
        "x",
        "\u{1f469}\u{301}\u{200d}\u{1f4bb}",
        "\u{1f1e8}\u{1f1ff}\u{1f1fa}\u{1f1f8}",
        "y",
    );
    let boundaries: Vec<_> = std::iter::once(0)
        .chain(
            text.grapheme_indices(true)
                .map(|(offset, value)| offset + value.len()),
        )
        .collect();
    let origins: Vec<_> = text
        .char_indices()
        .map(|(offset, _)| offset)
        .filter(|offset| *offset > 0 && *offset < text.len())
        .collect();
    let mut covered = Vec::new();
    for origin in origins {
        for direction in [
            SegmentationDirection::Forward,
            SegmentationDirection::Reverse,
        ] {
            let expected = match direction {
                SegmentationDirection::Forward => boundaries
                    .iter()
                    .copied()
                    .find(|boundary| *boundary > origin)
                    .unwrap_or(text.len()),
                SegmentationDirection::Reverse => boundaries
                    .iter()
                    .copied()
                    .rev()
                    .find(|boundary| *boundary < origin)
                    .unwrap_or(0),
            };
            let (actual, seen) =
                reject_matrix_then_finish(text, SegmentationKind::Grapheme, direction, origin);
            assert_eq!(actual, expected as u64);
            for kind in seen {
                if !covered.contains(&kind) {
                    covered.push(kind);
                }
            }
        }
    }
    for required in [
        EdgeKind::Next,
        EdgeKind::Prev,
        EdgeKind::Context,
        EdgeKind::Replay,
    ] {
        assert!(covered.contains(&required), "did not exercise {required:?}");
    }
}

#[test]
fn word_cursor_successor_rejection_is_atomic() {
    let text = "alpha.beta gamma";
    let boundaries: Vec<_> = std::iter::once(0)
        .chain(
            text.split_word_bound_indices()
                .map(|(offset, value)| offset + value.len()),
        )
        .collect();
    let origin = 2;
    for direction in [
        SegmentationDirection::Forward,
        SegmentationDirection::Reverse,
    ] {
        let expected = match direction {
            SegmentationDirection::Forward => boundaries
                .iter()
                .copied()
                .find(|boundary| *boundary > origin)
                .unwrap(),
            SegmentationDirection::Reverse => boundaries
                .iter()
                .copied()
                .rev()
                .find(|boundary| *boundary < origin)
                .unwrap(),
        };
        let (actual, covered) =
            reject_matrix_then_finish(text, SegmentationKind::Word, direction, origin);
        assert_eq!(actual, expected as u64);
        assert!(!covered.is_empty());
    }
}

#[test]
fn logical_line_successor_rejection_is_atomic_in_both_directions() {
    let text = "abcdef";
    for (direction, origin, expected) in [
        (SegmentationDirection::Forward, 0, 6),
        (SegmentationDirection::Reverse, 6, 0),
    ] {
        let (actual, covered) =
            reject_matrix_then_finish(text, SegmentationKind::LogicalLine, direction, origin);
        assert_eq!(actual, expected);
        assert!(!covered.is_empty());
    }
}
