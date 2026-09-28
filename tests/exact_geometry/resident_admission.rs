use super::*;
use gpui_text_input::{
    ObjectPurpose, ObjectRequestKey, PageDirection, PagePurpose, PageRequestKey,
};

fn resident_text(
    source: &str,
    original: PageRequestKey,
    revision: SourceRevision,
    start: usize,
    id: u64,
) -> RangePage {
    let key = PageRequestKey::adjacent(
        PageRequestId::new(99),
        original.binding(),
        revision,
        PagePurpose::Caret,
        ByteOffset::new(start as u64),
        PageDirection::Forward,
        original.max_payload_bytes(),
    )
    .unwrap();
    RangePage::new(
        PageId::new(id),
        key,
        ByteRange::from_u64(start as u64, source.len() as u64).unwrap(),
        source[start..].to_owned(),
        vec![],
        if start == 0 {
            PageEdgeFact::DocumentBoundary
        } else {
            PageEdgeFact::Continues
        },
        PageEdgeFact::DocumentBoundary,
        true,
    )
    .unwrap()
}

#[gpui::test]
fn resident_responses_accept_original_keys_and_keep_external_validation_strict(
    cx: &mut TestAppContext,
) {
    with_text_system(cx, |text_system| {
        for object in [false, true] {
            let mut required = (usize::MAX, usize::MAX);
            for attempt in 0..6 {
                let source = "abcd";
                let mut owner = owner_with_retained_items(
                    source,
                    16,
                    10000.,
                    2,
                    512 * 1024,
                    32 * 1024,
                    style(),
                )
                .unwrap();
                let job = start_index(&mut owner, 1);
                let text = page(&mut owner, job, source, 0, source.len(), 1);
                let capacity = match attempt {
                    0 | 1 => required,
                    2 => (required.0 - 1, required.1),
                    3 => (required.0, required.1 - 1),
                    4 => (0, required.1),
                    _ => (required.0, 0),
                };
                let mut expected_object = None;
                let result = if object {
                    owner.admit_page(job, &text, text_system).unwrap();
                    let expected = owner
                        .request_object_page(job, ObjectRequestId::new(1), 8, 16 * 1024)
                        .unwrap()
                        .key();
                    expected_object = Some(expected);
                    let make_objects = |revision, demand| {
                        ObjectPage::new(
                            ObjectPageId::new(99),
                            ObjectRequestKey::new(
                                ObjectRequestId::new(99),
                                expected.binding(),
                                revision,
                                expected.presentation_generation(),
                                ObjectPurpose::Caret,
                                demand,
                            )
                            .unwrap(),
                            vec![],
                            ObjectPageEdgeFact::EnvelopeBoundary,
                            ObjectPageEdgeFact::EnvelopeBoundary,
                            true,
                            None,
                        )
                        .unwrap()
                    };
                    let objects = make_objects(expected.revision(), expected.demand());
                    let wrong = make_objects(SourceRevision::new(999), expected.demand());
                    let before = owner.counts();
                    assert!(matches!(
                        owner
                            .admit_object_page(job, &text, &objects, text_system)
                            .unwrap_err()
                            .error(),
                        ExactGeometryError::WrongObjectPage(_)
                    ));
                    assert!(matches!(
                        owner
                            .admit_resident_object_page_with_capacity(
                                job,
                                &text,
                                &wrong,
                                text_system,
                                0,
                                0
                            )
                            .unwrap_err()
                            .error(),
                        ExactGeometryError::WrongObjectPage(_)
                    ));
                    assert_eq!(owner.counts(), before);
                    let wrong_demand = gpui_text_input::ObjectDemandEnvelope::anchor(
                        ByteOffset::new(0),
                        None,
                        gpui_text_input::ObjectDirection::Forward,
                        8,
                        16 * 1024,
                    )
                    .unwrap();
                    let wrong_objects = make_objects(expected.revision(), wrong_demand);
                    for (active_text, resident_objects) in [
                        (&text, &wrong_objects),
                        (
                            &resident_text(source, text.key(), text.key().revision(), 0, 99),
                            &objects,
                        ),
                        (
                            &resident_text(
                                "abc",
                                text.key(),
                                text.key().revision(),
                                0,
                                text.id().get(),
                            ),
                            &objects,
                        ),
                    ] {
                        assert!(matches!(
                            owner
                                .admit_resident_object_page_with_capacity(
                                    job,
                                    active_text,
                                    resident_objects,
                                    text_system,
                                    0,
                                    0,
                                )
                                .unwrap_err()
                                .error(),
                            ExactGeometryError::WrongObjectPage(_)
                        ));
                        assert_eq!(owner.counts(), before);
                    }
                    owner.admit_resident_object_page_with_capacity(
                        job,
                        &text,
                        &objects,
                        text_system,
                        capacity.0,
                        capacity.1,
                    )
                } else {
                    let resident = resident_text(source, text.key(), text.key().revision(), 0, 99);
                    let wrong = resident_text(source, text.key(), SourceRevision::new(999), 0, 99);
                    let before = owner.counts();
                    assert!(matches!(
                        owner
                            .admit_page(job, &resident, text_system)
                            .unwrap_err()
                            .error(),
                        ExactGeometryError::WrongPage(_)
                    ));
                    assert!(matches!(
                        owner
                            .admit_resident_page_with_capacity(job, &wrong, text_system, 0, 0)
                            .unwrap_err()
                            .error(),
                        ExactGeometryError::WrongPage(_)
                    ));
                    assert_eq!(owner.counts(), before);
                    let wrong_edge =
                        resident_text(source, text.key(), text.key().revision(), 1, 99);
                    assert!(matches!(
                        owner
                            .admit_resident_page_with_capacity(job, &wrong_edge, text_system, 0, 0,)
                            .unwrap_err()
                            .error(),
                        ExactGeometryError::WrongPage(_)
                    ));
                    assert_eq!(owner.counts(), before);
                    owner.admit_resident_page_with_capacity(
                        job,
                        &resident,
                        text_system,
                        capacity.0,
                        capacity.1,
                    )
                };
                let release = match &result {
                    Ok(admission) => admission.release(),
                    Err(failure) => failure.release(),
                };
                if let Some(expected) = expected_object {
                    assert_eq!(release.object_pages, vec![expected]);
                } else {
                    assert_eq!(release.pages, vec![text.key()]);
                }
                if attempt < 2 {
                    let admitted = result.unwrap();
                    required = (
                        admitted.admission_required_bytes(),
                        admitted.admission_required_items(),
                    );
                    assert_eq!(
                        admitted.progress(),
                        if object {
                            ExactGeometryProgress::IndexComplete
                        } else {
                            ExactGeometryProgress::NeedObjects
                        }
                    );
                } else {
                    let failure = result.unwrap_err();
                    assert!(matches!(
                        failure.error(),
                        ExactGeometryError::CapacityExceeded
                            | ExactGeometryError::Layout(
                                gpui::StreamingLayoutError::CapacityExceeded(_)
                            )
                    ));
                    assert_eq!(failure.release().jobs, vec![job]);
                    assert_eq!(owner.counts().active_job_items, 0);
                }
            }
        }
    });
}
