use super::*;

#[gpui::test]
fn checkpoint_backing_is_admitted_before_repeated_growth(cx: &mut TestAppContext) {
    with_text_system(cx, |text_system| {
        for capacity in [2, 4, 8, 16] {
            let source = "a".repeat(capacity / 2 * 8 + 3);
            let mut required = (0, 0);
            let mut accepted_checkpoints = 0;
            for attempt in 0..4 {
                let (bytes, items) = match attempt {
                    0 => (512 * 1024, 32 * 1024),
                    1 => required,
                    2 => (required.0 - 1, required.1),
                    _ => (required.0, required.1 - 1),
                };
                let mut owner =
                    owner_with_retained_items(&source, 8, 10000., capacity, bytes, items, style())
                        .unwrap();
                let job = start_index(&mut owner, 1);
                let page = page(&mut owner, job, &source, 0, source.len() - 1, 1);
                let result = admit_page_with_empty_objects(&mut owner, job, &page, text_system);
                if attempt < 2 {
                    let admission = result.unwrap();
                    assert_eq!(admission.progress(), ExactGeometryProgress::Scanning);
                    required = (
                        admission.admission_required_bytes(),
                        admission.admission_required_items(),
                    );
                    accepted_checkpoints = owner.counts().checkpoints;
                    assert_eq!(accepted_checkpoints, capacity);
                } else {
                    let failure = result.unwrap_err();
                    assert_eq!(failure.error(), &ExactGeometryError::CapacityExceeded);
                    assert_eq!(
                        failure.stage(),
                        gpui_text_input::ExactGeometryFailureStage::Checkpoint
                    );
                    assert!(
                        failure.release().counts.checkpoints < accepted_checkpoints,
                        "capacity={capacity} attempt={attempt}: {:?}",
                        failure.release().counts
                    );
                    assert_eq!(failure.release().jobs, vec![job]);
                    assert_eq!(owner.counts().active_job_items, 0);
                    assert!(owner.index().is_none());
                }
            }
        }
    });
}

#[gpui::test]
fn terminal_checkpoints_preserve_origin_and_bounded_eviction(cx: &mut TestAppContext) {
    with_text_system(cx, |text_system| {
        for capacity in [2, 3, 5] {
            for source in [String::new(), "abcdefgh".repeat(10)] {
                let mut required = (0, 0);
                for attempt in 0..4 {
                    let (bytes, items) = match attempt {
                        0 => (512 * 1024, 32 * 1024),
                        1 => required,
                        2 => (required.0 - 1, required.1),
                        _ => (required.0, required.1 - 1),
                    };
                    let mut owner = owner_with_retained_items(
                        &source,
                        8,
                        10000.,
                        capacity,
                        bytes,
                        items,
                        style(),
                    )
                    .unwrap();
                    let job = start_index(&mut owner, 1);
                    let page = page(&mut owner, job, &source, 0, source.len(), 1);
                    let result = admit_page_with_empty_objects(&mut owner, job, &page, text_system);
                    if attempt < 2 {
                        let admission = result.unwrap();
                        assert_eq!(admission.progress(), ExactGeometryProgress::IndexComplete);
                        required = (
                            admission.admission_required_bytes(),
                            admission.admission_required_items(),
                        );
                        let checkpoints = owner.index().unwrap().checkpoints();
                        assert_eq!(
                            checkpoints.len(),
                            if source.is_empty() { 2 } else { capacity }
                        );
                        assert_eq!(checkpoints[0].source().byte_offset, ByteOffset::new(0));
                        assert!(!checkpoints[0].is_terminal());
                        assert_eq!(
                            checkpoints.last().unwrap().source().byte_offset,
                            ByteOffset::new(source.len() as u64)
                        );
                        assert!(checkpoints.last().unwrap().is_terminal());
                    } else {
                        let failure = result.unwrap_err();
                        assert!(matches!(
                            failure.error(),
                            ExactGeometryError::CapacityExceeded
                                | ExactGeometryError::Layout(
                                    gpui::StreamingLayoutError::CapacityExceeded(
                                        gpui::StreamingLayoutComponent::Total
                                    )
                                )
                        ));
                        assert_eq!(failure.release().jobs, vec![job]);
                        assert_eq!(owner.counts().active_job_items, 0);
                        assert!(owner.index().is_none());
                    }
                }
            }
        }
    });
}
