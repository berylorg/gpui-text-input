use super::*;
use gpui::{Bounds, Pixels, StreamingLayoutMap, point, size};

const MAXIMUM: RangeSurfaceCharge = RangeSurfaceCharge {
    bytes: usize::MAX,
    items: usize::MAX,
};

fn map(offset: u64, x: f32, y: f32) -> StreamingLayoutMap {
    StreamingLayoutMap {
        logical_position: StreamingLayoutPosition::at(offset),
        position: point(px(x), px(y)),
    }
}

fn rect(x: f32, y: f32, width: f32) -> Bounds<Pixels> {
    Bounds::new(point(px(x), px(y)), size(px(width), px(10.)))
}

#[test]
fn candidate_highlights_preserve_wrapping_direction_and_admit_combined_storage() {
    let current = RangeSurfaceCharge {
        bytes: 1000,
        items: 7,
    };
    let maps = [
        map(0, 0., 0.),
        map(1, 10., 0.),
        map(2, 5., 10.),
        map(3, 15., 10.),
    ];
    let expected_selection = vec![rect(0., 0., 10.), rect(10., 0., 90.), rect(0., 10., 5.)];
    let expected_composition = vec![rect(10., 0., 90.), rect(0., 10., 5.), rect(5., 10., 10.)];
    let required = RangeSurfaceCharge {
        bytes: current.bytes + 6 * std::mem::size_of::<Bounds<Pixels>>(),
        items: current.items + 6,
    };
    for (anchor, head) in [(0, 2), (2, 0)] {
        let selection = RangeSourceSelection {
            anchor: position(anchor),
            head: position(head),
        };
        let composition = Some(ByteRange::new(ByteOffset::new(1), ByteOffset::new(3)).unwrap());
        let mut peak = current;
        let result = RangePrepublicationSession::test_prepare_highlight_geometry(
            &maps,
            &[],
            selection,
            composition,
            current,
            required,
            required,
            &mut peak,
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            result,
            (expected_selection.clone(), expected_composition.clone())
        );
        assert_eq!(peak, required);
        for limit in [
            RangeSurfaceCharge {
                bytes: required.bytes - 1,
                ..required
            },
            RangeSurfaceCharge {
                items: required.items - 1,
                ..required
            },
        ] {
            for _ in 0..3 {
                assert!(
                    RangePrepublicationSession::test_prepare_highlight_geometry(
                        &maps,
                        &[],
                        selection,
                        composition,
                        current,
                        MAXIMUM,
                        limit,
                        &mut peak,
                    )
                    .unwrap()
                    .is_none()
                );
                assert_eq!(peak, required);
                assert_eq!(
                    RangePrepublicationSession::test_prepare_highlight_geometry(
                        &maps,
                        &[],
                        selection,
                        composition,
                        current,
                        limit,
                        limit,
                        &mut peak,
                    )
                    .unwrap_err(),
                    RangePrepublicationFailure::TerminalCapacity
                );
            }
            assert_eq!(
                RangePrepublicationSession::test_prepare_highlight_geometry(
                    &maps,
                    &[],
                    selection,
                    composition,
                    current,
                    required,
                    required,
                    &mut peak,
                )
                .unwrap()
                .unwrap(),
                result
            );
        }
    }
}

#[test]
fn candidate_highlights_keep_object_gap_selection_and_empty_text_ranges() {
    let before = SourcePosition::new(
        ByteOffset::new(1),
        InlineObjectGap::before(InlineObjectNeighbor::new(
            InlineObjectId::new(1),
            InlineObjectOrder::new(1),
        )),
    );
    let after = SourcePosition::new(
        ByteOffset::new(1),
        InlineObjectGap::after(InlineObjectNeighbor::new(
            InlineObjectId::new(1),
            InlineObjectOrder::new(1),
        )),
    );
    let mut gap = map(1, 99., 99.);
    gap.logical_position.gap =
        gpui::StreamingObjectGap::before(gpui::StreamingObjectId(1), gpui::StreamingObjectOrder(1));
    let maps = [map(0, 0., 0.), gap, map(2, 20., 0.)];
    let objects = [(before, after, rect(8., 0., 4.))];
    let mut peak = RangeSurfaceCharge::default();
    let run = |selection, composition, peak: &mut RangeSurfaceCharge| {
        RangePrepublicationSession::test_prepare_highlight_geometry(
            &maps,
            &objects,
            selection,
            composition,
            RangeSurfaceCharge::default(),
            MAXIMUM,
            MAXIMUM,
            peak,
        )
        .unwrap()
        .unwrap()
    };
    assert_eq!(
        run(
            RangeSourceSelection {
                anchor: before,
                head: after
            },
            None,
            &mut peak
        ),
        (vec![rect(8., 0., 4.)], vec![])
    );
    assert_eq!(
        run(
            RangeSourceSelection {
                anchor: position(0),
                head: position(2)
            },
            None,
            &mut peak
        ),
        (vec![rect(0., 0., 20.), rect(8., 0., 4.)], vec![])
    );
    assert_eq!(
        run(
            RangeSourceSelection::caret(before),
            Some(ByteRange::new(ByteOffset::new(1), ByteOffset::new(1)).unwrap()),
            &mut peak
        ),
        (vec![], vec![])
    );
    assert_eq!(
        RangePrepublicationSession::test_prepare_highlight_geometry(
            &[],
            &[],
            RangeSourceSelection::caret(position(0)),
            None,
            RangeSurfaceCharge::default(),
            RangeSurfaceCharge::default(),
            RangeSurfaceCharge::default(),
            &mut RangeSurfaceCharge::default(),
        )
        .unwrap()
        .unwrap(),
        (vec![], vec![])
    );
}

#[test]
fn candidate_highlights_reject_enclosing_arithmetic_before_growth() {
    for current in [
        RangeSurfaceCharge {
            bytes: usize::MAX,
            items: 0,
        },
        RangeSurfaceCharge {
            bytes: 0,
            items: usize::MAX,
        },
    ] {
        let mut peak = current;
        assert_eq!(
            RangePrepublicationSession::test_prepare_highlight_geometry(
                &[map(0, 0., 0.), map(1, 5., 0.)],
                &[],
                RangeSourceSelection {
                    anchor: position(0),
                    head: position(1)
                },
                None,
                current,
                MAXIMUM,
                MAXIMUM,
                &mut peak,
            )
            .unwrap_err(),
            RangePrepublicationFailure::Arithmetic
        );
        assert_eq!(peak, current);
    }
}
