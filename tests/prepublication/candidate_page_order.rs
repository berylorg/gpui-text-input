use super::*;

#[test]
fn candidate_page_order_preserves_equal_positions_and_admits_growth() {
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
        (vec![9], vec![0]),
        (vec![9, 3, 9, 1, 3], vec![3, 1, 4, 0, 2]),
    ] {
        let starts: Vec<_> = starts.into_iter().map(ByteOffset::new).collect();
        let mut required = current;
        assert_eq!(
            RangePrepublicationSession::test_prepare_page_order(
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
        assert_eq!(required.items, current.items + starts.len());
        assert_eq!(required.bytes == current.bytes, starts.is_empty());
        let mut peak = current;
        assert_eq!(
            RangePrepublicationSession::test_prepare_page_order(
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
                    RangePrepublicationSession::test_prepare_page_order(
                        &starts, current, maximum, limit, &mut peak,
                    )
                    .unwrap()
                    .is_none()
                );
                assert_eq!(peak, required);
                assert_eq!(
                    RangePrepublicationSession::test_prepare_page_order(
                        &starts, current, limit, limit, &mut peak,
                    )
                    .unwrap_err(),
                    RangePrepublicationFailure::TerminalCapacity
                );
            }
            assert_eq!(
                RangePrepublicationSession::test_prepare_page_order(
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
fn candidate_page_order_rejects_enclosing_arithmetic() {
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
            RangePrepublicationSession::test_prepare_page_order(
                &[ByteOffset::new(0)],
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
