use super::*;
use gpui_text_input::preparation_test_support::{
    enclosing_failure_peak, is_enclosing_capacity_refusal, prepare_object_finalization,
};

#[gpui::test]
fn source_finalization_retains_shared_inline_credit(cx: &mut TestAppContext) {
    with_text_system(cx, |text_system| {
        for index in [false, true] {
            for display in ["", "shared é"] {
                let fact = || {
                    InlineObjectFact::new(
                        InlineObjectId::new(1),
                        ByteOffset::new(0),
                        InlineObjectOrder::new(1),
                        "fallback".repeat(64),
                        InlineObjectPresentation::new(
                            1,
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
                    InlineObjectGap::before(fact().cursor().neighbor()),
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
                if !index {
                    let job = start_index(&mut owner, 1);
                    let text = page(&mut owner, job, "xy", 0, 2, 1);
                    owner.admit_page(job, &text, text_system).unwrap();
                    let objects =
                        object_response_with_limit(&mut owner, job, 1, 3, vec![fact()], true);
                    owner
                        .admit_object_page(job, &text, &objects, text_system)
                        .unwrap();
                }
                let job = if index {
                    start_index(&mut owner, 10)
                } else {
                    owner
                        .request_block_target(
                            GeometryJobId::new(10),
                            BlockTarget::new(px(0.), px(10000.), px(32.)),
                        )
                        .unwrap()
                        .key()
                };
                let text = page(&mut owner, job, "xy", 0, 2, 10);
                owner.admit_page(job, &text, text_system).unwrap();
                let objects =
                    object_response_with_limit(&mut owner, job, 10, 3, vec![fact()], true);
                let before = owner.counts();
                let (raw, _, baseline) = prepare_object_finalization(
                    &owner,
                    job,
                    index,
                    &text,
                    &objects,
                    text_system,
                    None,
                    (usize::MAX, usize::MAX),
                )
                .unwrap();
                for current in [(0, 0), (baseline.0 + 8192, baseline.1 + 32)] {
                    let probe = |limit| {
                        prepare_object_finalization(
                            &owner,
                            job,
                            index,
                            &text,
                            &objects,
                            text_system,
                            Some(current),
                            limit,
                        )
                    };
                    let credit = if index { 0 } else { display.len() };
                    let expected = (
                        current.0 + raw.0 - baseline.0 - credit,
                        current.1 + raw.1 - baseline.1,
                    );
                    for _ in 0..2 {
                        let (actual_raw, peak, actual_baseline) = probe(expected).unwrap();
                        assert_eq!(actual_raw, raw);
                        assert_eq!(actual_baseline, baseline);
                        assert_eq!(peak, expected);
                        assert_eq!(owner.counts(), before);
                    }
                    for limit in [(expected.0 - 1, expected.1), (expected.0, expected.1 - 1)] {
                        let failure = probe(limit).unwrap_err();
                        if is_enclosing_capacity_refusal(&failure) {
                            let peak = enclosing_failure_peak(&failure).unwrap();
                            assert!(peak.0 > limit.0 || peak.1 > limit.1);
                        } else {
                            assert_eq!(
                                failure.error(),
                                &ExactGeometryError::Layout(
                                    gpui::StreamingLayoutError::CapacityExceeded(
                                        gpui::StreamingLayoutComponent::Total
                                    )
                                )
                            );
                        }
                        assert_eq!(failure.release(), &ExactGeometryRelease::default());
                        assert_eq!(owner.counts(), before);
                        assert!(probe(expected).is_ok());
                    }
                }
            }
        }
    });
}
