use super::*;

#[test]
fn candidate_transfer_buffers_admit_combined_growth() {
    let current = RangeSurfaceCharge {
        bytes: 1000,
        items: 7,
    };
    let maximum = RangeSurfaceCharge {
        bytes: usize::MAX,
        items: usize::MAX,
    };
    for (text, objects) in [(0, 0), (3, 0), (0, 2), (3, 2)] {
        let expected = RangeSurfaceCharge {
            bytes: current.bytes
                + text * std::mem::size_of::<RangePage>()
                + objects * std::mem::size_of::<ObjectPage>(),
            items: current.items + text + objects,
        };
        let mut peak = current;
        let (pages, object_pages) = RangePrepublicationSession::test_prepare_transfer_buffers(
            text, objects, current, expected, expected, &mut peak,
        )
        .unwrap()
        .unwrap();
        assert!(pages.is_empty() && object_pages.is_empty());
        assert_eq!(pages.capacity(), text);
        assert_eq!(object_pages.capacity(), objects);
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
                    RangePrepublicationSession::test_prepare_transfer_buffers(
                        text, objects, current, maximum, limit, &mut peak
                    )
                    .unwrap()
                    .is_none()
                );
                assert_eq!(peak, expected);
                assert_eq!(
                    RangePrepublicationSession::test_prepare_transfer_buffers(
                        text, objects, current, limit, maximum, &mut peak
                    )
                    .unwrap_err(),
                    RangePrepublicationFailure::TerminalCapacity
                );
            }
        }
    }
}

#[test]
fn candidate_transfer_buffers_reject_arithmetic_before_allocation() {
    let maximum = RangeSurfaceCharge {
        bytes: usize::MAX,
        items: usize::MAX,
    };
    for (text, objects, current) in [
        (usize::MAX, 0, RangeSurfaceCharge::default()),
        (0, usize::MAX, RangeSurfaceCharge::default()),
        (1, 1, maximum),
    ] {
        let mut peak = current;
        assert_eq!(
            RangePrepublicationSession::test_prepare_transfer_buffers(
                text, objects, current, maximum, maximum, &mut peak
            )
            .unwrap_err(),
            RangePrepublicationFailure::Arithmetic
        );
        assert_eq!(peak, current);
    }
}
