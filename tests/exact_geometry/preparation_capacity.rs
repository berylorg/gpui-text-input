use super::*;

#[path = "../../src/range_geometry/exact/transition/capacity.rs"]
mod capacity;
use capacity::PreparationCapacity;
#[path = "../../src/range_geometry/exact/transition/release.rs"]
mod release;

fn budget(bytes: usize, items: usize) -> PreparationCapacity {
    PreparationCapacity {
        refused_capacity: None,
        bytes: 100,
        items: 10,
        max_bytes: bytes,
        max_items: items,
        peak_bytes: 0,
        peak_items: 0,
    }
}

#[test]
fn refusal_retains_both_attempted_peaks_and_does_not_change_the_base() {
    for (bytes, items, allowed) in [
        (150, 15, true),
        (149, 15, false),
        (150, 14, false),
        (0, 0, false),
    ] {
        let mut capacity = budget(bytes, items);
        assert_eq!(capacity.admit(50, 5).is_ok(), allowed);
        assert_eq!(capacity.refused_capacity, (!allowed).then_some((150, 15)));
        assert_eq!((capacity.peak_bytes, capacity.peak_items), (150, 15));
        assert_eq!((capacity.bytes, capacity.items), (100, 10));
        capacity.max_bytes = 150;
        capacity.max_items = 15;
        assert_eq!(capacity.admit(50, 5).unwrap(), (150, 15));
        assert_eq!(capacity.refused_capacity, None);
        assert_eq!(capacity.admit(0, 0).unwrap(), (100, 10));
        assert_eq!((capacity.peak_bytes, capacity.peak_items), (150, 15));
    }
}

#[test]
fn overflow_records_saturated_evidence_but_never_admits() {
    for (bytes, items, expected) in [
        (usize::MAX, 1, (usize::MAX, 11)),
        (1, usize::MAX, (101, usize::MAX)),
        (usize::MAX, usize::MAX, (usize::MAX, usize::MAX)),
    ] {
        let mut capacity = budget(usize::MAX, usize::MAX);
        assert_eq!(
            capacity.admit(bytes, items),
            Err(ExactGeometryError::CapacityExceeded)
        );
        assert_eq!((capacity.peak_bytes, capacity.peak_items), expected);
        assert_eq!(capacity.refused_capacity, None);
    }
}

#[test]
fn cleanup_preparation_extends_the_same_peak_before_reservation() {
    let mut owner =
        owner_with_retained_items("abc", 16, 10000., 2, 512 * 1024, 32768, style()).unwrap();
    let job = start_index(&mut owner, 1);
    let text = page(&mut owner, job, "abc", 0, 3, 1);
    let before = owner.counts();
    let storage = std::mem::size_of::<GeometryJobKey>() + std::mem::size_of::<PageRequestKey>();
    let required_bytes = 150 + storage;
    for (bytes, items, allowed) in [
        (required_bytes, 17, true),
        (required_bytes - 1, 17, false),
        (required_bytes, 16, false),
    ] {
        let mut capacity = budget(bytes, items);
        let (bytes, items) = capacity.admit(50, 5).unwrap();
        let release = release::PreparedRelease {
            jobs: [Some(job), None, None, None],
            page: Some(text.key()),
            ..Default::default()
        };
        let result = release.prepare(bytes, items, &mut capacity);
        assert_eq!(result.is_ok(), allowed);
        assert_eq!(
            (capacity.peak_bytes, capacity.peak_items),
            (required_bytes, 17)
        );
        if let Ok((released, bytes, items)) = result {
            assert_eq!((bytes, items), (required_bytes, 17));
            assert_eq!(released.jobs, [job]);
            assert_eq!(released.pages, [text.key()]);
        }
    }
    assert!(owner.request_page(job, PageRequestId::new(2)).is_err());
    assert_eq!(owner.counts(), before);
}
