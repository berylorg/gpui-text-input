use super::*;

const MAX: RangeSurfaceCharge = RangeSurfaceCharge {
    bytes: usize::MAX,
    items: usize::MAX,
};

fn charge(items: usize) -> RangeSurfaceCharge {
    RangeSurfaceCharge {
        bytes: items * 8,
        items,
    }
}

fn values() -> [Vec<u64>; 2] {
    let mut first = Vec::with_capacity(8);
    first.extend([11, 12]);
    let mut second = Vec::with_capacity(5);
    second.extend([21, 22, 23]);
    [first, second]
}

#[test]
fn boxed_collections_admit_overlap_and_release_excess_before_next_conversion() {
    let original = values();
    let allocated = original.iter().map(Vec::capacity).sum::<usize>();
    let required = charge(10 + allocated + original[0].len());
    let mut peak = charge(10);
    let (boxed, retained) = RangePrepublicationSession::test_box_collections(
        original,
        charge(allocated),
        charge(10),
        required,
        required,
        &mut peak,
    )
    .unwrap()
    .unwrap();
    assert_eq!(&*boxed[0], &[11, 12]);
    assert_eq!(&*boxed[1], &[21, 22, 23]);
    assert_eq!(retained, charge(5));
    assert_eq!(peak, required);
    for available in [
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
            let mut peak = charge(10);
            assert!(
                RangePrepublicationSession::test_box_collections(
                    values(),
                    charge(allocated),
                    charge(10),
                    required,
                    available,
                    &mut peak
                )
                .unwrap()
                .is_none()
            );
            assert_eq!(peak, required);
        }
        assert_eq!(
            RangePrepublicationSession::test_box_collections(
                values(),
                charge(allocated),
                charge(10),
                available,
                available,
                &mut charge(10)
            )
            .unwrap_err(),
            RangePrepublicationFailure::TerminalCapacity
        );
    }
}

#[test]
fn boxed_collections_empty_exact_storage_and_enclosing_overflow() {
    for values in [
        [Vec::new(), Vec::new()],
        [vec![1, 2], vec![3]],
        [Vec::with_capacity(4), Vec::new()],
    ] {
        let allocated = values.iter().map(Vec::capacity).sum::<usize>();
        let length = values.iter().map(Vec::len).sum::<usize>();
        let expected = values.clone();
        let mut peak = charge(0);
        let (boxed, retained) = RangePrepublicationSession::test_box_collections(
            values,
            charge(allocated),
            charge(0),
            charge(allocated),
            charge(allocated),
            &mut peak,
        )
        .unwrap()
        .unwrap();
        assert_eq!(&*boxed[0], expected[0]);
        assert_eq!(&*boxed[1], expected[1]);
        assert_eq!(retained, charge(length));
        assert_eq!(peak, charge(allocated));
    }
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
            RangePrepublicationSession::test_box_collections(
                [vec![1], Vec::new()],
                charge(1),
                current,
                MAX,
                MAX,
                &mut peak
            )
            .unwrap_err(),
            RangePrepublicationFailure::Arithmetic
        );
        assert_eq!(peak, current);
    }
}

#[test]
fn boxed_collections_later_conversion_refusal_can_be_retried() {
    let values = || {
        let mut second = Vec::with_capacity(9);
        second.extend(0..8);
        [vec![42], second]
    };
    let allocated = values().iter().map(Vec::capacity).sum::<usize>();
    let required = charge(allocated + 8);
    for available in [
        RangeSurfaceCharge {
            bytes: required.bytes - 1,
            ..required
        },
        RangeSurfaceCharge {
            items: required.items - 1,
            ..required
        },
        required,
    ] {
        let mut peak = charge(0);
        let result = RangePrepublicationSession::test_box_collections(
            values(),
            charge(allocated),
            charge(0),
            required,
            available,
            &mut peak,
        )
        .unwrap();
        assert_eq!(peak, required);
        if available == required {
            let (boxed, retained) = result.unwrap();
            assert_eq!(&*boxed[0], &[42]);
            assert_eq!(&*boxed[1], &[0, 1, 2, 3, 4, 5, 6, 7]);
            assert_eq!(retained, charge(9));
        } else {
            assert!(result.is_none());
        }
    }
}
