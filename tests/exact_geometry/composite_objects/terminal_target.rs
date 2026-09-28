use super::*;
use gpui_text_input::preparation_test_support::{
    enclosing_failure_peak, is_enclosing_capacity_refusal, prepare_object_scan, prepare_response,
    prepare_response_with_enclosing,
};

#[allow(clippy::too_many_arguments)]
fn check_terminal_response(
    owner: &ExactGeometryOwner,
    job: GeometryJobKey,
    index: bool,
    text: &RangePage,
    objects: &ObjectPage,
    text_system: &gpui::WindowTextSystem,
    target: BlockTarget,
    display_bytes: usize,
) {
    let before = owner.counts();
    let (_, _, baseline) = prepare_object_scan(
        owner,
        job,
        index,
        text,
        objects,
        text_system,
        None,
        (usize::MAX, usize::MAX),
    )
    .unwrap();
    let ids = (
        GeometryJobId::new(30),
        PageRequestId::new(30),
        ObjectRequestId::new(30),
    );
    for resident in [false, true] {
        let raw = prepare_response(
            owner,
            job,
            text,
            Some(objects),
            text_system,
            resident,
            index,
            ids,
            target,
            (usize::MAX, usize::MAX),
        )
        .unwrap()
        .required_capacity();
        for current in [(0, 0), (baseline.0 + 8192, baseline.1 + 32)] {
            let probe = |limit| {
                prepare_response_with_enclosing(
                    owner,
                    job,
                    text,
                    Some(objects),
                    text_system,
                    resident,
                    index,
                    ids,
                    target,
                    current,
                    limit,
                )
            };
            let prepared = probe((usize::MAX, usize::MAX)).unwrap();
            let peak = prepared.enclosing_peak().unwrap();
            assert_eq!(prepared.required_capacity(), raw);
            assert!(prepared.page_request().is_none());
            assert!(prepared.object_request().is_none());
            assert!(!prepared.terminal_index() && prepared.terminal_target());
            assert_eq!(peak.1, current.1 + raw.1 - baseline.1);
            let uncredited = current.0 + raw.0 - baseline.0;
            assert!(peak.0 <= uncredited);
            if display_bytes > 0 {
                assert_eq!(
                    peak.0,
                    uncredited - display_bytes,
                    "{peak:?} {raw:?} {baseline:?}"
                );
            } else {
                assert_eq!(peak.0, uncredited);
            }
            drop(prepared);
            for _ in 0..2 {
                let prepared = probe(peak).unwrap();
                assert_eq!(prepared.enclosing_peak(), Some(peak));
                assert_eq!(prepared.required_capacity(), raw);
                assert_eq!(owner.counts(), before);
            }
            for limit in [(peak.0 - 1, peak.1), (peak.0, peak.1 - 1)] {
                let failure = probe(limit).unwrap_err();
                if is_enclosing_capacity_refusal(&failure) {
                    let attempted = enclosing_failure_peak(&failure).unwrap();
                    assert!(attempted.0 > limit.0 || attempted.1 > limit.1);
                } else {
                    assert_eq!(
                        failure.error(),
                        &ExactGeometryError::Layout(gpui::StreamingLayoutError::CapacityExceeded(
                            gpui::StreamingLayoutComponent::Total
                        ))
                    );
                }
                assert_eq!(failure.release(), &ExactGeometryRelease::default());
                assert_eq!(owner.counts(), before);
                assert!(probe(peak).is_ok());
            }
        }
    }
}

#[gpui::test]
fn terminal_target_publication_retains_shared_inline_credit(cx: &mut TestAppContext) {
    with_text_system(cx, |text_system| {
        for prior_output in [false, true] {
            for display in ["", "shared é"] {
                let fact = |id| {
                    InlineObjectFact::new(
                        InlineObjectId::new(id),
                        ByteOffset::new(0),
                        InlineObjectOrder::new(id),
                        "fallback".repeat(64),
                        InlineObjectPresentation::new(
                            id as u64,
                            display,
                            px(20.),
                            px(64.),
                            px(40.),
                            None,
                            1,
                            true,
                        )
                        .unwrap(),
                    )
                };
                let mut input = layout(16, 10000.);
                input.start_position = SourcePosition::new(
                    ByteOffset::new(0),
                    InlineObjectGap::before(fact(1).cursor().neighbor()),
                )
                .into();
                let mut owner = ExactGeometryOwner::new(
                    binding("xy", 1),
                    PresentationGeneration::new(1),
                    input,
                    style(),
                    ExactGeometryLimits::new(256, 32, 512 * 1024, 32 * 1024).unwrap(),
                )
                .unwrap();
                {
                    let job = start_index(&mut owner, 1);
                    let text = page(&mut owner, job, "xy", 0, 2, 1);
                    owner.admit_page(job, &text, text_system).unwrap();
                    let objects = object_response_with_limit(
                        &mut owner,
                        job,
                        1,
                        4,
                        vec![fact(1), fact(2), fact(3)],
                        true,
                    );
                    owner
                        .admit_object_page(job, &text, &objects, text_system)
                        .unwrap();
                }
                let job = owner
                    .request_block_target(
                        GeometryJobId::new(10),
                        BlockTarget::new(px(0.), px(10000.), px(32.)),
                    )
                    .unwrap()
                    .key();
                let text = page(&mut owner, job, "xy", 0, 2, 10);
                owner.admit_page(job, &text, text_system).unwrap();
                if prior_output {
                    let first = object_response_with_limit(
                        &mut owner,
                        job,
                        10,
                        2,
                        vec![fact(1), fact(2)],
                        false,
                    );
                    owner
                        .admit_object_page(job, &text, &first, text_system)
                        .unwrap();
                }
                let (id, facts, credited) = if prior_output {
                    (11, vec![fact(3)], 2)
                } else {
                    (10, vec![fact(1), fact(2), fact(3)], 3)
                };
                let objects = object_response_with_limit(&mut owner, job, id, 4, facts, true);
                check_terminal_response(
                    &owner,
                    job,
                    false,
                    &text,
                    &objects,
                    text_system,
                    BlockTarget::new(px(0.), px(10000.), px(32.)),
                    credited * display.len(),
                );
            }
        }
    });
}
