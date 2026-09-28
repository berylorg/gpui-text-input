use super::*;

const GENEROUS: (usize, usize) = (512 * 1024, 32 * 1024);

#[gpui::test]
fn request_capacity_refusal_preserves_job_and_same_id_retry(cx: &mut TestAppContext) {
    with_text_system(cx, |text_system| {
        for object in [false, true] {
            let source = "abcdefghijklmnopqrs";
            let mut required = GENEROUS;
            for attempt in 0..6 {
                let mut owner = owner_with_retained_items(
                    source,
                    16,
                    10000.,
                    2,
                    GENEROUS.0,
                    GENEROUS.1,
                    style(),
                )
                .unwrap();
                let job = start_index(&mut owner, 1);
                if object {
                    let text = page(&mut owner, job, source, 0, source.len(), 1);
                    assert_eq!(
                        owner
                            .admit_page(job, &text, text_system)
                            .unwrap()
                            .progress(),
                        ExactGeometryProgress::NeedObjects
                    );
                }
                let before = owner.counts();
                let capacity = match attempt {
                    0 => (usize::MAX, usize::MAX),
                    1 => required,
                    2 => (required.0 - 1, required.1),
                    3 => (required.0, required.1 - 1),
                    4 => (0, required.1),
                    _ => (required.0, 0),
                };
                let request = |owner: &mut ExactGeometryOwner, cap: (usize, usize)| {
                    if object {
                        owner
                            .request_object_page_with_capacity(
                                job,
                                ObjectRequestId::new(2),
                                1,
                                4096,
                                cap.0,
                                cap.1,
                            )
                            .map(|_| ())
                    } else {
                        owner
                            .request_page_with_capacity(job, PageRequestId::new(2), cap.0, cap.1)
                            .map(|_| ())
                    }
                };
                let result = request(&mut owner, capacity);
                if attempt >= 2 {
                    assert_eq!(result, Err(ExactGeometryError::CapacityExceeded));
                    assert_eq!(owner.counts(), before);
                    request(&mut owner, required).unwrap();
                } else {
                    result.unwrap();
                }
                let after = owner.counts();
                if attempt == 0 {
                    required = (after.total_bytes(), after.total_items());
                }
                assert_eq!((after.total_bytes(), after.total_items()), required);
                assert_eq!(after.total_items(), before.total_items() + 1);
                assert_eq!(
                    request(&mut owner, (0, 0)),
                    Err(ExactGeometryError::PageAlreadyPending)
                );
                assert_eq!(owner.counts(), after);
                let obsolete = GeometryJobKey::new(job.geometry(), GeometryJobId::new(9));
                let result = if object {
                    owner
                        .request_object_page_with_capacity(
                            obsolete,
                            ObjectRequestId::new(3),
                            1,
                            4096,
                            0,
                            0,
                        )
                        .map(|_| ())
                } else {
                    owner
                        .request_page_with_capacity(obsolete, PageRequestId::new(3), 0, 0)
                        .map(|_| ())
                };
                assert_eq!(result, Err(ExactGeometryError::ObsoleteJob(obsolete)));
                assert_eq!(owner.counts(), after);
            }
        }
    });
}

#[gpui::test]
fn oversized_request_ceiling_matches_configured_request_admission(cx: &mut TestAppContext) {
    with_text_system(cx, |text_system| {
        for object in [false, true] {
            let source = "abcdefghijklmnopqrs";
            let mut required = GENEROUS;
            for attempt in 0..2 {
                let configured = match attempt {
                    0 => GENEROUS,
                    _ => required,
                };
                let mut owner = owner_with_retained_items(
                    source,
                    16,
                    10000.,
                    2,
                    configured.0,
                    configured.1,
                    style(),
                )
                .unwrap();
                let start = owner.start_index(GeometryJobId::new(1)).unwrap();
                let job = start.key();
                let mut peak = (
                    start.admission_required_bytes(),
                    start.admission_required_items(),
                );
                if object {
                    let text = page(&mut owner, job, source, 0, source.len(), 1);
                    let admission = owner.admit_page(job, &text, text_system).unwrap();
                    peak.0 = peak.0.max(admission.admission_required_bytes());
                    peak.1 = peak.1.max(admission.admission_required_items());
                }
                let result = if object {
                    owner
                        .request_object_page_with_capacity(
                            job,
                            ObjectRequestId::new(2),
                            1,
                            4096,
                            usize::MAX,
                            usize::MAX,
                        )
                        .map(|_| ())
                } else {
                    owner
                        .request_page_with_capacity(
                            job,
                            PageRequestId::new(2),
                            usize::MAX,
                            usize::MAX,
                        )
                        .map(|_| ())
                };
                result.unwrap();
                required = (
                    peak.0.max(owner.counts().total_bytes()),
                    peak.1.max(owner.counts().total_items()),
                );
                assert!(required.0 <= configured.0);
                assert!(required.1 <= configured.1);
            }
        }
    });
}
