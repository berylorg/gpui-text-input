use super::*;

#[test]
fn index_startup_admits_checkpoint_and_job_before_allocation() {
    for source in ["", "bounded index startup"] {
        let mut required = (0, 0);
        for attempt in 0..4 {
            let (bytes, items) = match attempt {
                0 => (512 * 1024, 32 * 1024),
                1 => required,
                2 => (required.0 - 1, required.1),
                _ => (required.0, required.1 - 1),
            };
            let mut owner =
                owner_with_retained_items(source, 8, 100., 4, bytes, items, style()).unwrap();
            let before = owner.counts();
            let key = owner.key();
            let id = GeometryJobId::new(1);
            let result = owner.start_index(id);
            if attempt < 2 {
                let start = result.unwrap();
                required = (
                    start.admission_required_bytes(),
                    start.admission_required_items(),
                );
                assert_eq!(start.progress(), ExactGeometryProgress::Scanning);
                assert_eq!(owner.counts().checkpoints, 1);
                assert_eq!(owner.counts().active_job_items, 2);
                assert!(required.0 > owner.counts().total_bytes());
                assert_eq!(required.1, owner.counts().total_items() + 1);
                assert_eq!(owner.retained_high_water_bytes(), required.0);
                assert_eq!(owner.retained_high_water_items(), required.1);
            } else {
                assert_eq!(result.unwrap_err(), ExactGeometryError::CapacityExceeded);
                assert_eq!(owner.counts(), before);
                assert_eq!(owner.key(), key);
                assert!(owner.index().is_none());
                assert!(owner.estimate().is_none());
                assert_eq!(
                    owner.start_index(id).unwrap_err(),
                    ExactGeometryError::CapacityExceeded
                );
                assert_eq!(owner.counts(), before);
            }
        }
    }
}
