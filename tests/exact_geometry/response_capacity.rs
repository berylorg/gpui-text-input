use super::*;

const GENEROUS: (usize, usize) = (512 * 1024, 32 * 1024);

#[gpui::test]
fn text_response_ceiling_checks_coexistence_and_preserves_later_capacity(cx: &mut TestAppContext) {
    with_text_system(cx, |text_system| {
        let source = "a".repeat(200);
        let mut required = GENEROUS;
        for attempt in 0..6 {
            let capacity = match attempt {
                0 => (usize::MAX, usize::MAX),
                1 => required,
                2 => (required.0 - 1, required.1),
                3 => (required.0, required.1 - 1),
                4 => (0, required.1),
                _ => (required.0, 0),
            };
            let mut owner =
                owner_with_retained_items(&source, 16, 10000., 2, GENEROUS.0, GENEROUS.1, style())
                    .unwrap();
            let job = start_index(&mut owner, 1);
            let text = page(&mut owner, job, &source, 0, source.len(), 1);
            let before = owner.counts();
            let result =
                owner.admit_page_with_capacity(job, &text, text_system, capacity.0, capacity.1);
            if attempt < 2 {
                let admission = result.unwrap();
                assert_eq!(admission.progress(), ExactGeometryProgress::NeedObjects);
                required = (
                    admission.admission_required_bytes(),
                    admission.admission_required_items(),
                );
                assert_eq!(
                    required,
                    (
                        before.total_bytes() + text.retained_charge().bytes(),
                        before.total_items() + text.retained_charge().items(),
                    )
                );
            } else {
                let failure = result.unwrap_err();
                assert_eq!(failure.error(), &ExactGeometryError::CapacityExceeded);
                assert_eq!(
                    failure.stage(),
                    gpui_text_input::ExactGeometryFailureStage::PageCoexistence
                );
                assert_eq!(failure.release().jobs, vec![job]);
                assert_eq!(failure.release().pages, vec![text.key()]);
                assert_eq!(failure.release().counts, before_without_inputs(before));
                assert_eq!(owner.counts().active_job_items, 0);
                let next = start_index(&mut owner, 2);
                let next_text = page(&mut owner, next, &source, 0, source.len(), 2);
                assert_eq!(
                    admit_page_with_empty_objects(&mut owner, next, &next_text, text_system)
                        .unwrap()
                        .progress(),
                    ExactGeometryProgress::IndexComplete
                );
            }
        }
    });
}

fn before_without_inputs(
    mut counts: gpui_text_input::ExactGeometryCounts,
) -> gpui_text_input::ExactGeometryCounts {
    counts.owner_bytes = 0;
    counts.owner_items = 0;
    counts.input_bytes = 0;
    counts.input_items = 0;
    counts
}

#[gpui::test]
fn object_response_ceiling_reaches_scan_and_terminal_publication(cx: &mut TestAppContext) {
    with_text_system(cx, |text_system| {
        for (target, complete) in [(false, false), (false, true), (true, true)] {
            let source = "abcdefghijklmnopqrs";
            let mut required = GENEROUS;
            for attempt in 0..8 {
                let capacity = match attempt {
                    0 => (usize::MAX, usize::MAX),
                    1 => required,
                    2 => (required.0 - 1, required.1),
                    3 => (required.0, required.1 - 1),
                    4 => (0, required.1),
                    5 => (required.0, 0),
                    _ => (usize::MAX, usize::MAX),
                };
                let configured = match attempt {
                    6 => (required.0 - 1, GENEROUS.1),
                    7 => (GENEROUS.0, required.1 - 1),
                    _ => GENEROUS,
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
                let index = start_index(&mut owner, 1);
                let job = if target {
                    let text = page(&mut owner, index, source, 0, source.len(), 1);
                    admit_page_with_empty_objects(&mut owner, index, &text, text_system).unwrap();
                    owner
                        .request_block_target_anchored(
                            GeometryJobId::new(2),
                            BlockTarget::new(px(0.), px(10000.), px(0.)),
                            SourcePosition::new(ByteOffset::new(0), InlineObjectGap::NoObjects),
                        )
                        .unwrap()
                        .key()
                } else {
                    index
                };
                let prior_index = owner.index().map(|index| index.key());
                let text = page(
                    &mut owner,
                    job,
                    source,
                    0,
                    if complete { source.len() } else { 18 },
                    2,
                );
                owner.admit_page(job, &text, text_system).unwrap();
                let objects = empty_object_page(&mut owner, job, &text, 2);
                let result = owner.admit_object_page_with_capacity(
                    job,
                    &text,
                    &objects,
                    text_system,
                    capacity.0,
                    capacity.1,
                );
                if attempt < 2 {
                    let admission = result.unwrap();
                    assert_eq!(
                        admission.progress(),
                        if target {
                            ExactGeometryProgress::TargetComplete
                        } else if complete {
                            ExactGeometryProgress::IndexComplete
                        } else {
                            ExactGeometryProgress::Scanning
                        }
                    );
                    required = (
                        admission.admission_required_bytes(),
                        admission.admission_required_items(),
                    );
                    assert!(required.0 <= capacity.0 && required.1 <= capacity.1);
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
                    assert_eq!(failure.release().object_pages, vec![objects.key()]);
                    assert_eq!(owner.counts().active_job_items, 0);
                    assert_eq!(owner.index().map(|index| index.key()), prior_index);
                    assert!(owner.target().is_none());
                }
            }
        }
    });
}

#[gpui::test]
fn stale_responses_do_not_apply_zero_capacity_to_the_current_job(cx: &mut TestAppContext) {
    with_text_system(cx, |text_system| {
        let source = "abc";
        let mut owner = owner(source, 8, 2, GENEROUS.0, 32);
        let old_job = start_index(&mut owner, 1);
        let old_text = page(&mut owner, old_job, source, 0, source.len(), 1);
        owner.admit_page(old_job, &old_text, text_system).unwrap();
        let old_objects = empty_object_page(&mut owner, old_job, &old_text, 1);
        owner
            .admit_object_page(old_job, &old_text, &old_objects, text_system)
            .unwrap();
        let current = start_index(&mut owner, 2);
        let text = page(&mut owner, current, source, 0, source.len(), 2);
        let before = owner.counts();
        let stale = owner
            .admit_page_with_capacity(old_job, &old_text, text_system, 0, 0)
            .unwrap_err();
        assert_eq!(stale.error(), &ExactGeometryError::ObsoleteJob(old_job));
        assert_eq!(stale.release(), &ExactGeometryRelease::default());
        let wrong = owner
            .admit_page_with_capacity(current, &old_text, text_system, 0, 0)
            .unwrap_err();
        assert_eq!(
            wrong.error(),
            &ExactGeometryError::WrongPage(old_text.key())
        );
        assert_eq!(wrong.release(), &ExactGeometryRelease::default());
        assert_eq!(owner.counts(), before);
        owner.admit_page(current, &text, text_system).unwrap();
        let objects = empty_object_page(&mut owner, current, &text, 2);
        let before = owner.counts();
        let stale = owner
            .admit_object_page_with_capacity(old_job, &old_text, &old_objects, text_system, 0, 0)
            .unwrap_err();
        assert_eq!(stale.error(), &ExactGeometryError::ObsoleteJob(old_job));
        assert_eq!(stale.release(), &ExactGeometryRelease::default());
        let wrong = owner
            .admit_object_page_with_capacity(current, &text, &old_objects, text_system, 0, 0)
            .unwrap_err();
        assert_eq!(
            wrong.error(),
            &ExactGeometryError::WrongObjectPage(old_objects.key())
        );
        assert_eq!(wrong.release(), &ExactGeometryRelease::default());
        assert_eq!(owner.counts(), before);
        assert_eq!(
            owner
                .admit_object_page(current, &text, &objects, text_system)
                .unwrap()
                .progress(),
            ExactGeometryProgress::IndexComplete
        );
    });
}

#[gpui::test]
fn context_response_uses_the_call_ceiling_without_raising_configured_limits(
    cx: &mut TestAppContext,
) {
    with_text_system(cx, |text_system| {
        let source = format!("{}{}TARGET", "a".repeat(6144), "😀".repeat(24));
        let mut required = GENEROUS;
        for attempt in 0..5 {
            let capacity = match attempt {
                0 => (usize::MAX, usize::MAX),
                1 => required,
                2 => (required.0 - 1, required.1),
                3 => (required.0, required.1 - 1),
                _ => (usize::MAX, usize::MAX),
            };
            let configured = match attempt {
                4 => (required.0 - 1, GENEROUS.1),
                _ => GENEROUS,
            };
            let mut owner = ExactGeometryOwner::new(
                binding(&source, 1),
                PresentationGeneration::new(1),
                layout(8, 24.),
                style(),
                ExactGeometryLimits::new(6144, 2, configured.0, configured.1).unwrap(),
            )
            .unwrap();
            let job = start_index(&mut owner, 1);
            let (request, text) = precontext::reach_context_with_forward_cap(
                &mut owner,
                &source,
                job,
                text_system,
                8,
            );
            let result =
                owner.admit_page_with_capacity(job, &text, text_system, capacity.0, capacity.1);
            if attempt < 2 {
                let admission = result.unwrap();
                required = (
                    admission.admission_required_bytes(),
                    admission.admission_required_items(),
                );
                assert_eq!(admission.progress(), ExactGeometryProgress::Scanning);
            } else {
                let failure = result.unwrap_err();
                assert_eq!(failure.error(), &ExactGeometryError::CapacityExceeded);
                assert_eq!(failure.release().jobs, vec![job]);
                assert_eq!(failure.release().pages, vec![request.key()]);
                assert_eq!(owner.counts().active_job_items, 0);
            }
        }
    });
}
