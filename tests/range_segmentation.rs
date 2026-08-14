use gpui_text_input::{
    AdjacentPageEdge, BindingId, ByteOffset, ByteRange, LogicalExtent, PageDirection, PageEdgeFact,
    PageId, PagePurpose, PageRequestId, PageRequestKey, RangePage, SegmentationContinuation,
    SegmentationDirection, SegmentationError, SegmentationKind, SegmentationLimits,
    SegmentationProgress, SegmentationResume, SourceRevision,
};
use unicode_segmentation::UnicodeSegmentation;

#[path = "range_segmentation/atomic_resume.rs"]
mod atomic_resume;
#[path = "range_segmentation/grapheme_continuation.rs"]
mod grapheme_continuation;

const BINDING: BindingId = BindingId::new(7);
const REVISION: SourceRevision = SourceRevision::new(11);

fn limits(page_bytes: u64, steps: usize) -> SegmentationLimits {
    SegmentationLimits::new(page_bytes, steps).unwrap()
}

fn directed_request(id: u64, range: ByteRange, direction: SegmentationDirection) -> PageRequestKey {
    let (anchor, page_direction) = match direction {
        SegmentationDirection::Forward => (range.start(), PageDirection::Forward),
        SegmentationDirection::Reverse => (range.end(), PageDirection::Backward),
    };
    PageRequestKey::adjacent(
        PageRequestId::new(id),
        BINDING,
        REVISION,
        PagePurpose::Segmentation,
        anchor,
        page_direction,
        range.len().max(4),
    )
    .unwrap()
}

fn request(id: u64, range: ByteRange) -> PageRequestKey {
    directed_request(id, range, SegmentationDirection::Forward)
}

fn keyed_request(
    id: PageRequestId,
    binding: BindingId,
    revision: SourceRevision,
    purpose: PagePurpose,
    anchor: ByteOffset,
    direction: PageDirection,
    cap: u64,
) -> PageRequestKey {
    PageRequestKey::adjacent(id, binding, revision, purpose, anchor, direction, cap).unwrap()
}

fn page(text: &str, key: PageRequestKey) -> RangePage {
    let gpui_text_input::PageDemandEnvelope::Adjacent {
        anchor,
        direction,
        max_payload_bytes,
    } = key.demand()
    else {
        unreachable!()
    };
    let anchor = usize::try_from(anchor.get()).unwrap();
    let cap = usize::try_from(max_payload_bytes).unwrap();
    let range = match direction {
        PageDirection::Forward => {
            let end = text
                .char_indices()
                .map(|(i, _)| i)
                .chain(std::iter::once(text.len()))
                .filter(|end| *end >= anchor && *end - anchor <= cap)
                .max()
                .unwrap();
            ByteRange::from_u64(anchor as u64, end as u64).unwrap()
        }
        PageDirection::Backward => {
            let start = text
                .char_indices()
                .map(|(i, _)| i)
                .filter(|start| *start <= anchor && anchor - *start <= cap)
                .min()
                .unwrap_or(anchor);
            ByteRange::from_u64(start as u64, anchor as u64).unwrap()
        }
    };
    let start = usize::try_from(range.start().get()).unwrap();
    let end = usize::try_from(range.end().get()).unwrap();
    RangePage::new(
        PageId::new(key.id().get()),
        key,
        range,
        text[start..end].to_owned(),
        Vec::new(),
        if start == 0 {
            PageEdgeFact::DocumentBoundary
        } else {
            PageEdgeFact::Continues
        },
        if end == text.len() {
            PageEdgeFact::DocumentBoundary
        } else {
            PageEdgeFact::Continues
        },
        end == text.len(),
    )
    .unwrap()
}

fn page_at(text: &str, key: PageRequestKey, range: ByteRange) -> RangePage {
    let start = usize::try_from(range.start().get()).unwrap();
    let end = usize::try_from(range.end().get()).unwrap();
    RangePage::new(
        PageId::new(key.id().get()),
        key,
        range,
        text[start..end].to_owned(),
        vec![],
        if start == 0 {
            PageEdgeFact::DocumentBoundary
        } else {
            PageEdgeFact::Continues
        },
        if end == text.len() {
            PageEdgeFact::DocumentBoundary
        } else {
            PageEdgeFact::Continues
        },
        end == text.len(),
    )
    .unwrap()
}

fn over_extent_page(text: &str, key: PageRequestKey) -> Option<RangePage> {
    let gpui_text_input::PageDemandEnvelope::Adjacent {
        anchor,
        direction,
        max_payload_bytes,
    } = key.demand()
    else {
        unreachable!()
    };
    if direction == PageDirection::Backward
        || text.len() as u64 + 1 - anchor.get() > max_payload_bytes
    {
        return None;
    }
    let extended = format!("{text}x");
    let range = ByteRange::from_u64(anchor.get(), extended.len() as u64).unwrap();
    let start = usize::try_from(anchor.get()).unwrap();
    Some(
        RangePage::new(
            PageId::new(key.id().get()),
            key,
            range,
            extended[start..].to_owned(),
            vec![],
            if start == 0 {
                PageEdgeFact::DocumentBoundary
            } else {
                PageEdgeFact::Continues
            },
            PageEdgeFact::Continues,
            false,
        )
        .unwrap(),
    )
}

fn scalar_page(
    text: &str,
    id: u64,
    edge: AdjacentPageEdge,
    traversal: SegmentationDirection,
) -> PageRequestKey {
    let (range, direction) = match edge {
        AdjacentPageEdge::NextChunk(start) => {
            let start = usize::try_from(start.get()).unwrap();
            let end = text[start..]
                .char_indices()
                .nth(2)
                .map_or(text.len(), |(relative, _)| start + relative);
            (
                ByteRange::from_u64(start as u64, end as u64).unwrap(),
                SegmentationDirection::Forward,
            )
        }
        AdjacentPageEdge::PrevChunk(end) | AdjacentPageEdge::PreContext(end) => {
            let end = usize::try_from(end.get()).unwrap();
            let start = text[..end]
                .char_indices()
                .rev()
                .nth(1)
                .map_or(0, |(offset, _)| offset);
            (
                ByteRange::from_u64(start as u64, end as u64).unwrap(),
                SegmentationDirection::Reverse,
            )
        }
        AdjacentPageEdge::Replay(anchor) => match traversal {
            SegmentationDirection::Forward => {
                let start = usize::try_from(anchor.get()).unwrap();
                let end = text[start..]
                    .char_indices()
                    .nth(2)
                    .map_or(text.len(), |(relative, _)| start + relative);
                (
                    ByteRange::from_u64(start as u64, end as u64).unwrap(),
                    traversal,
                )
            }
            SegmentationDirection::Reverse => {
                let end = usize::try_from(anchor.get()).unwrap();
                let start = text[..end]
                    .char_indices()
                    .rev()
                    .nth(1)
                    .map_or(0, |(offset, _)| offset);
                (
                    ByteRange::from_u64(start as u64, end as u64).unwrap(),
                    traversal,
                )
            }
        },
    };
    directed_request(id, range, direction)
}

fn resolve(
    text: &str,
    kind: SegmentationKind,
    direction: SegmentationDirection,
    origin: usize,
) -> (u64, usize) {
    let initial_range = match direction {
        SegmentationDirection::Forward => {
            let end = origin + text[origin..].chars().next().unwrap().len_utf8();
            ByteRange::from_u64(origin as u64, end as u64).unwrap()
        }
        SegmentationDirection::Reverse => {
            let start = text[..origin].char_indices().next_back().unwrap().0;
            ByteRange::from_u64(start as u64, origin as u64).unwrap()
        }
    };
    let mut retained_high_water = 0;
    let mut steps = 0;
    let mut progress = SegmentationContinuation::start(
        BINDING,
        REVISION,
        LogicalExtent::new(text.len() as u64, 1),
        kind,
        direction,
        ByteOffset::new(origin as u64),
        limits(8, 1),
        directed_request(1, initial_range, direction),
    )
    .unwrap();
    let mut next_id = 2;
    loop {
        steps += 1;
        assert!(
            steps < 512,
            "segmentation continuation did not converge: {kind:?} {direction:?} at {origin} in {text:?}"
        );
        let SegmentationProgress::NeedPage(mut continuation) = progress else {
            let SegmentationProgress::Complete(boundary) = progress else {
                unreachable!()
            };
            return (boundary.offset().get(), retained_high_water);
        };
        let current_page = page(text, *continuation.pending_request());
        retained_high_water = retained_high_water.max(current_page.text().len());
        let resume = continuation
            .resume(&current_page, |demand| {
                let next = scalar_page(text, next_id, demand.edge(), direction);
                next_id += 1;
                next
            })
            .unwrap_or_else(|error| {
                panic!(
                    "{error:?} for {kind:?} {direction:?} at {origin} with page {:?}",
                    current_page.range()
                )
            });
        progress = match resume {
            SegmentationResume::Complete(boundary) => SegmentationProgress::Complete(boundary),
            SegmentationResume::NeedPage => SegmentationProgress::NeedPage(continuation),
        };
    }
}

#[test]
fn grapheme_traversal_crosses_fragmented_utf8_edges_both_ways() {
    let family = "👨‍👩‍👧‍👦";
    let text = format!("a{family}b");
    assert_eq!(
        resolve(
            &text,
            SegmentationKind::Grapheme,
            SegmentationDirection::Forward,
            1
        )
        .0,
        (1 + family.len()) as u64,
    );
    assert_eq!(
        resolve(
            &text,
            SegmentationKind::Grapheme,
            SegmentationDirection::Reverse,
            1 + family.len(),
        )
        .0,
        1,
    );
}

#[test]
fn word_traversal_uses_streaming_cursor_from_arbitrary_offsets() {
    let text = "a.b";
    assert_eq!(
        resolve(
            text,
            SegmentationKind::Word,
            SegmentationDirection::Forward,
            1
        )
        .0,
        3,
    );
    assert_eq!(
        resolve(
            text,
            SegmentationKind::Word,
            SegmentationDirection::Reverse,
            2
        )
        .0,
        0,
    );
}

#[test]
fn logical_lines_do_not_treat_fragment_edges_as_boundaries() {
    let text = "alpha\nbeta\ngamma";
    assert_eq!(
        resolve(
            text,
            SegmentationKind::LogicalLine,
            SegmentationDirection::Forward,
            7,
        )
        .0,
        11,
    );
    assert_eq!(
        resolve(
            text,
            SegmentationKind::LogicalLine,
            SegmentationDirection::Reverse,
            9,
        )
        .0,
        6,
    );
}

#[test]
fn exact_page_key_is_required_and_continuation_retains_no_chunk() {
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
        panic!("non-edge traversal must request a page");
    };
    assert!(std::mem::size_of_val(&continuation) < 1024);

    let stale_key = PageRequestKey::adjacent(
        PageRequestId::new(99),
        BINDING,
        REVISION,
        PagePurpose::Segmentation,
        ByteOffset::new(0),
        PageDirection::Forward,
        4,
    )
    .unwrap();
    let stale_page = page(text, stale_key);
    assert_eq!(
        continuation
            .resume(&stale_page, |_| panic!("obsolete pages must not advance"))
            .unwrap_err(),
        SegmentationError::ObsoletePage,
    );
    assert_eq!(*continuation.pending_request(), first);
    assert_eq!(continuation.cancel().pending_request(), first);
}

#[test]
fn over_extent_pages_are_rejected_atomically_for_every_segmentation_kind_and_direction() {
    for kind in [
        SegmentationKind::Grapheme,
        SegmentationKind::Word,
        SegmentationKind::LogicalLine,
    ] {
        let forward = request(1, ByteRange::from_u64(0, 2).unwrap());
        let SegmentationProgress::NeedPage(mut continuation) = SegmentationContinuation::start(
            BINDING,
            REVISION,
            LogicalExtent::new(1, 1),
            kind,
            SegmentationDirection::Forward,
            ByteOffset::new(0),
            limits(4, 4),
            forward,
        )
        .unwrap() else {
            unreachable!()
        };
        let malformed = page_at("a\n", forward, ByteRange::from_u64(0, 2).unwrap());
        let pending = *continuation.pending_request();
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
        assert!(matches!(
            continuation
                .resume(
                    &page_at("a", forward, ByteRange::from_u64(0, 1).unwrap()),
                    |_| { panic!("one-byte source must complete") }
                )
                .unwrap(),
            SegmentationResume::Complete(_)
        ));

        let reverse = directed_request(
            2,
            ByteRange::from_u64(0, 1).unwrap(),
            SegmentationDirection::Reverse,
        );
        let SegmentationProgress::NeedPage(mut continuation) = SegmentationContinuation::start(
            BINDING,
            REVISION,
            LogicalExtent::new(1, 1),
            kind,
            SegmentationDirection::Reverse,
            ByteOffset::new(1),
            limits(4, 4),
            reverse,
        )
        .unwrap() else {
            unreachable!()
        };
        // A backward envelope fixes the returned end at its anchor. Because start validation
        // keeps that anchor within the extent, no envelope-valid reverse over-extent page exists.
        assert!(
            RangePage::new(
                PageId::new(2),
                reverse,
                ByteRange::from_u64(0, 2).unwrap(),
                "a\n".to_owned(),
                vec![],
                PageEdgeFact::DocumentBoundary,
                PageEdgeFact::Continues,
                false,
            )
            .is_err()
        );
        assert!(matches!(
            continuation
                .resume(
                    &page_at("a", reverse, ByteRange::from_u64(0, 1).unwrap()),
                    |_| { panic!("one-byte source must complete") }
                )
                .unwrap(),
            SegmentationResume::Complete(_)
        ));
    }
}

#[test]
fn continuation_rejects_a_nonadjacent_followup_request() {
    let text = "abcdef";
    let first = request(1, ByteRange::from_u64(0, 1).unwrap());
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
            .resume(&page(text, first), |_| {
                request(2, ByteRange::from_u64(0, 1).unwrap())
            })
            .unwrap_err(),
        SegmentationError::NonAdjacentRequest,
    );
}

#[test]
fn empty_and_document_edge_traversal_complete_without_page_payloads() {
    for (kind, direction) in [
        (SegmentationKind::Grapheme, SegmentationDirection::Forward),
        (SegmentationKind::Word, SegmentationDirection::Reverse),
        (
            SegmentationKind::LogicalLine,
            SegmentationDirection::Forward,
        ),
    ] {
        let edge = directed_request(1, ByteRange::from_u64(0, 0).unwrap(), direction);
        let SegmentationProgress::Complete(boundary) = SegmentationContinuation::start(
            BINDING,
            REVISION,
            LogicalExtent::new(0, 0),
            kind,
            direction,
            ByteOffset::new(0),
            limits(4, 1),
            edge,
        )
        .unwrap() else {
            panic!("empty source edge must be immediately known")
        };
        assert_eq!(boundary.offset(), ByteOffset::new(0));
        assert!(boundary.is_document_edge());
    }
}

#[test]
fn immediate_document_edge_rejects_wrong_direction_and_over_cap() {
    for direction in [
        SegmentationDirection::Forward,
        SegmentationDirection::Reverse,
    ] {
        let wrong = directed_request(
            1,
            ByteRange::from_u64(0, 0).unwrap(),
            match direction {
                SegmentationDirection::Forward => SegmentationDirection::Reverse,
                SegmentationDirection::Reverse => SegmentationDirection::Forward,
            },
        );
        assert_eq!(
            SegmentationContinuation::start(
                BINDING,
                REVISION,
                LogicalExtent::new(0, 0),
                SegmentationKind::Grapheme,
                direction,
                ByteOffset::new(0),
                limits(4, 1),
                wrong,
            )
            .unwrap_err(),
            SegmentationError::InvalidRequest
        );
        let over = PageRequestKey::adjacent(
            PageRequestId::new(2),
            BINDING,
            REVISION,
            PagePurpose::Segmentation,
            ByteOffset::new(0),
            match direction {
                SegmentationDirection::Forward => PageDirection::Forward,
                SegmentationDirection::Reverse => PageDirection::Backward,
            },
            5,
        )
        .unwrap();
        assert_eq!(
            SegmentationContinuation::start(
                BINDING,
                REVISION,
                LogicalExtent::new(0, 0),
                SegmentationKind::Grapheme,
                direction,
                ByteOffset::new(0),
                limits(4, 1),
                over,
            )
            .unwrap_err(),
            SegmentationError::PageRangeLimitExceeded
        );
        let valid = directed_request(3, ByteRange::from_u64(0, 0).unwrap(), direction);
        assert!(matches!(
            SegmentationContinuation::start(
                BINDING,
                REVISION,
                LogicalExtent::new(0, 0),
                SegmentationKind::Grapheme,
                direction,
                ByteOffset::new(0),
                limits(4, 1),
                valid,
            ),
            Ok(SegmentationProgress::Complete(_))
        ));
    }
}

#[test]
fn invalid_scalar_origin_is_a_typed_error_instead_of_a_panic() {
    let text = "é";
    let key = request(1, ByteRange::from_u64(0, 2).unwrap());
    assert_eq!(
        SegmentationContinuation::start(
            BINDING,
            REVISION,
            LogicalExtent::new(2, 1),
            SegmentationKind::Word,
            SegmentationDirection::Forward,
            ByteOffset::new(1),
            limits(4, 1),
            key,
        )
        .unwrap_err(),
        SegmentationError::InvalidRequest
    );
    let _ = text;
}

#[test]
fn fragmented_traversal_matches_contiguous_boundary_corpora() {
    let corpus = [
        "plain words, punctuation!",
        "a\u{301}b café हिंदी",
        "👨\u{200d}👩\u{200d}👧\u{200d}👦 flags 🇨🇿🇺🇸",
        "first\n\nthird\r\nfifth",
    ];
    for text in corpus {
        let scalar_offsets: Vec<_> = text
            .char_indices()
            .map(|(offset, _)| offset)
            .chain(std::iter::once(text.len()))
            .collect();
        let grapheme: Vec<_> = std::iter::once(0)
            .chain(
                text.grapheme_indices(true)
                    .map(|(offset, value)| offset + value.len()),
            )
            .collect();
        let word: Vec<_> = std::iter::once(0)
            .chain(
                text.split_word_bound_indices()
                    .map(|(offset, value)| offset + value.len()),
            )
            .collect();
        let line: Vec<_> = std::iter::once(0)
            .chain(
                text.bytes()
                    .enumerate()
                    .filter_map(|(offset, byte)| (byte == b'\n').then_some(offset + 1)),
            )
            .chain(std::iter::once(text.len()))
            .collect();

        for (kind, boundaries) in [
            (SegmentationKind::Grapheme, grapheme),
            (SegmentationKind::Word, word),
            (SegmentationKind::LogicalLine, line),
        ] {
            for origin in scalar_offsets
                .iter()
                .copied()
                .filter(|offset| *offset > 0 && *offset < text.len())
            {
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
                let forward = resolve(text, kind, SegmentationDirection::Forward, origin);
                let reverse = resolve(text, kind, SegmentationDirection::Reverse, origin);
                assert_eq!(
                    forward.0, expected_forward as u64,
                    "{kind:?} forward at {origin} in {text:?}"
                );
                assert_eq!(
                    reverse.0, expected_reverse as u64,
                    "{kind:?} reverse at {origin} in {text:?}"
                );
                assert!(forward.1 <= 64 && reverse.1 <= 64);
            }
        }
    }
}
