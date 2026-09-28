use super::*;

#[test]
fn candidate_fragment_maps_preserve_adjacent_deduplication_and_admit_growth() {
    let current = RangeSurfaceCharge {
        bytes: 1000,
        items: 7,
    };
    let maximum = RangeSurfaceCharge {
        bytes: usize::MAX,
        items: usize::MAX,
    };
    for (starts, expected) in [
        (vec![], vec![]),
        (vec![9], vec![9]),
        (vec![9, 9, 3, 3, 9], vec![9, 3, 9]),
    ] {
        let starts: Vec<_> = starts.into_iter().map(map).collect();
        let expected: Vec<_> = expected.into_iter().map(map).collect();
        let mut required = current;
        assert_eq!(
            RangePrepublicationSession::test_prepare_fragment_maps(
                &starts,
                current,
                maximum,
                maximum,
                &mut required,
            )
            .unwrap()
            .unwrap(),
            expected,
        );
        assert_eq!(required.items, current.items + expected.len());
        assert_eq!(required.bytes == current.bytes, starts.is_empty());
        let mut peak = current;
        assert_eq!(
            RangePrepublicationSession::test_prepare_fragment_maps(
                &starts, current, required, required, &mut peak,
            )
            .unwrap()
            .unwrap(),
            expected,
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
                let mut peak = current;
                assert!(
                    RangePrepublicationSession::test_prepare_fragment_maps(
                        &starts, current, maximum, limit, &mut peak,
                    )
                    .unwrap()
                    .is_none()
                );
                assert_eq!(peak, required);
                assert_eq!(
                    RangePrepublicationSession::test_prepare_fragment_maps(
                        &starts, current, limit, limit, &mut peak,
                    )
                    .unwrap_err(),
                    RangePrepublicationFailure::TerminalCapacity
                );
            }
            assert_eq!(
                RangePrepublicationSession::test_prepare_fragment_maps(
                    &starts, current, maximum, required, &mut peak,
                )
                .unwrap()
                .unwrap(),
                expected
            );
        }
    }
}

#[test]
fn candidate_fragment_maps_reject_enclosing_arithmetic() {
    let maximum = RangeSurfaceCharge {
        bytes: usize::MAX,
        items: usize::MAX,
    };
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
            RangePrepublicationSession::test_prepare_fragment_maps(
                &[map(0)],
                current,
                maximum,
                maximum,
                &mut peak,
            )
            .unwrap_err(),
            RangePrepublicationFailure::Arithmetic
        );
        assert_eq!(peak, current);
    }
}

fn map(offset: u64) -> gpui::StreamingLayoutMap {
    gpui::StreamingLayoutMap {
        logical_position: StreamingLayoutPosition::at(offset),
        position: gpui::point(px(offset as f32), px(0.0)),
    }
}

#[test]
fn candidate_fragment_maps_keep_first_placement_and_distinct_object_gaps() {
    let mut first = map(9);
    first.logical_position.gap =
        gpui::StreamingObjectGap::before(gpui::StreamingObjectId(1), gpui::StreamingObjectOrder(1));
    let mut duplicate = first;
    duplicate.position.x = px(100.0);
    let mut after = first;
    after.logical_position.gap =
        gpui::StreamingObjectGap::after(gpui::StreamingObjectId(1), gpui::StreamingObjectOrder(1));
    let maximum = RangeSurfaceCharge {
        bytes: usize::MAX,
        items: usize::MAX,
    };
    let mut peak = RangeSurfaceCharge::default();
    let result = RangePrepublicationSession::test_prepare_fragment_maps(
        &[first, duplicate, after, first],
        RangeSurfaceCharge::default(),
        maximum,
        maximum,
        &mut peak,
    )
    .unwrap()
    .unwrap();
    assert_eq!(result, [first, after, first]);
    assert_eq!(peak.items, 3);
}
