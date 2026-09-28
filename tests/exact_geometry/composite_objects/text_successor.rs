use super::*;
use gpui_text_input::preparation_test_support::{
    enclosing_failure_peak, is_enclosing_capacity_refusal, prepare_response,
    prepare_response_with_enclosing,
};

#[gpui::test]
fn text_successor_consumes_deferred_custody_before_preparation(cx: &mut TestAppContext) {
    with_text_system(cx, |text_system| {
        for index in [false, true] {
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
                let first = fact(1);
                let mut input = layout(16, 10000.);
                input.start_position = SourcePosition::new(
                    ByteOffset::new(0),
                    InlineObjectGap::before(first.cursor().neighbor()),
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
                let target = BlockTarget::new(px(0.), px(10000.), px(32.));
                if !index {
                    let job = start_index(&mut owner, 1);
                    let text = page(&mut owner, job, "xy", 0, 2, 1);
                    owner.admit_page(job, &text, text_system).unwrap();
                    let objects = object_response_with_limit(
                        &mut owner,
                        job,
                        1,
                        3,
                        vec![first.clone(), fact(2)],
                        true,
                    );
                    owner
                        .admit_object_page(job, &text, &objects, text_system)
                        .unwrap();
                }
                let job = if index {
                    start_index(&mut owner, 10)
                } else {
                    owner
                        .request_block_target(GeometryJobId::new(10), target)
                        .unwrap()
                        .key()
                };
                let text = page(&mut owner, job, "xy", 0, 1, 10);
                owner.admit_page(job, &text, text_system).unwrap();
                let objects =
                    object_response_with_limit(&mut owner, job, 10, 3, vec![first], false);
                let ids = |id| {
                    (
                        GeometryJobId::new(id),
                        PageRequestId::new(id),
                        ObjectRequestId::new(id),
                    )
                };
                let prepared = prepare_response(
                    &owner,
                    job,
                    &text,
                    Some(&objects),
                    text_system,
                    false,
                    index,
                    ids(20),
                    target,
                    (usize::MAX, usize::MAX),
                )
                .unwrap();
                let next = prepared.object_request().unwrap();
                assert!(prepared.page_request().is_none());
                prepared.commit(&mut owner);
                assert_eq!(owner.counts().deferred_object_items, 4);
                let following = ObjectPage::new(
                    ObjectPageId::new(20),
                    next.key(),
                    vec![fact(2)],
                    ObjectPageEdgeFact::Continues(objects.objects()[0].cursor()),
                    ObjectPageEdgeFact::EnvelopeBoundary,
                    true,
                    None,
                )
                .unwrap();
                let before = owner.counts();
                let prepared = prepare_response(
                    &owner,
                    job,
                    &text,
                    Some(&following),
                    text_system,
                    false,
                    index,
                    ids(30),
                    target,
                    (usize::MAX, usize::MAX),
                )
                .unwrap();
                let next = prepared.page_request().unwrap();
                assert_eq!(owner.counts(), before);
                prepared.commit(&mut owner);
                let before = owner.counts();
                assert_eq!(before.deferred_object_items, 0);
                assert_eq!(before.deferred_object_bytes, 0);
                let text = super::super::precontext::response_with_atoms("xy", 30, next, 1, &[]);
                let baseline = (
                    before.total_bytes() + text.retained_charge().bytes(),
                    before.total_items() + text.retained_charge().items(),
                );
                for resident in [false, true] {
                    let raw = prepare_response(
                        &owner,
                        job,
                        &text,
                        None,
                        text_system,
                        resident,
                        index,
                        ids(40),
                        target,
                        (usize::MAX, usize::MAX),
                    )
                    .unwrap()
                    .required_capacity();
                    for current in [(0, 0), (baseline.0 + 8192, baseline.1 + 32)] {
                        let expected = (
                            current.0 + raw.0 - baseline.0,
                            current.1 + raw.1 - baseline.1,
                        );
                        let probe = |limit| {
                            prepare_response_with_enclosing(
                                &owner,
                                job,
                                &text,
                                None,
                                text_system,
                                resident,
                                index,
                                ids(40),
                                target,
                                current,
                                limit,
                            )
                        };
                        for _ in 0..2 {
                            let prepared = probe(expected).unwrap();
                            assert_eq!(prepared.enclosing_peak(), Some(expected));
                            assert_eq!(prepared.required_capacity(), raw);
                            assert!(prepared.object_request().is_some());
                            assert_eq!(owner.counts(), before);
                        }
                        for limit in [(expected.0 - 1, expected.1), (expected.0, expected.1 - 1)] {
                            let failure = probe(limit).unwrap_err();
                            assert!(is_enclosing_capacity_refusal(&failure));
                            let attempted = enclosing_failure_peak(&failure).unwrap();
                            assert!(attempted.0 > limit.0 || attempted.1 > limit.1);
                            assert_eq!(failure.release(), &ExactGeometryRelease::default());
                            assert_eq!(owner.counts(), before);
                            assert!(probe(expected).is_ok());
                        }
                    }
                }
            }
        }
    });
}
