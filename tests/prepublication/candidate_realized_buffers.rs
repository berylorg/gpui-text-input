use super::*;

#[test]
fn candidate_realized_buffers_admit_combined_growth() {
    let current = RangeSurfaceCharge {
        bytes: 1000,
        items: 7,
    };
    let maximum = RangeSurfaceCharge {
        bytes: usize::MAX,
        items: usize::MAX,
    };
    for (objects, gaps) in [(0, 0), (3, 0), (0, 2), (3, 2)] {
        let expected = RangeSurfaceCharge {
            bytes: current.bytes
                + objects * std::mem::size_of::<gpui_text_input::RealizedInlineObjectGeometry>()
                + gaps * std::mem::size_of::<gpui_text_input::RealizedObjectGapGeometry>(),
            items: current.items + objects + gaps,
        };
        let mut peak = current;
        let (object_buffer, gap_buffer) =
            RangePrepublicationSession::test_prepare_realized_buffers(
                objects, gaps, current, expected, expected, &mut peak,
            )
            .unwrap()
            .unwrap();
        assert!(object_buffer.is_empty() && gap_buffer.is_empty());
        assert_eq!(object_buffer.capacity(), objects);
        assert_eq!(gap_buffer.capacity(), gaps);
        assert_eq!(peak, expected);
        for limit in [
            RangeSurfaceCharge {
                bytes: expected.bytes - 1,
                ..expected
            },
            RangeSurfaceCharge {
                items: expected.items - 1,
                ..expected
            },
        ] {
            for _ in 0..3 {
                let mut peak = current;
                assert!(
                    RangePrepublicationSession::test_prepare_realized_buffers(
                        objects, gaps, current, maximum, limit, &mut peak
                    )
                    .unwrap()
                    .is_none()
                );
                assert_eq!(peak, expected);
                assert_eq!(
                    RangePrepublicationSession::test_prepare_realized_buffers(
                        objects, gaps, current, limit, maximum, &mut peak
                    )
                    .unwrap_err(),
                    RangePrepublicationFailure::TerminalCapacity
                );
            }
            assert!(
                RangePrepublicationSession::test_prepare_realized_buffers(
                    objects, gaps, current, maximum, expected, &mut peak
                )
                .unwrap()
                .is_some()
            );
        }
    }
}

#[test]
fn candidate_realized_buffers_reject_arithmetic_before_allocation() {
    let maximum = RangeSurfaceCharge {
        bytes: usize::MAX,
        items: usize::MAX,
    };
    for (objects, gaps, current) in [
        (usize::MAX, 0, RangeSurfaceCharge::default()),
        (0, usize::MAX, RangeSurfaceCharge::default()),
        (1, 1, maximum),
        (
            1,
            0,
            RangeSurfaceCharge {
                bytes: 0,
                items: usize::MAX,
            },
        ),
        (
            1,
            0,
            RangeSurfaceCharge {
                bytes: usize::MAX,
                items: 0,
            },
        ),
    ] {
        let mut peak = current;
        assert_eq!(
            RangePrepublicationSession::test_prepare_realized_buffers(
                objects, gaps, current, maximum, maximum, &mut peak
            )
            .unwrap_err(),
            RangePrepublicationFailure::Arithmetic
        );
        assert_eq!(peak, current);
    }
}
