use super::*;

#[gpui::test]
fn configured_layout_total_failure_keeps_its_class_when_item_capacity_is_clamped(
    cx: &mut TestAppContext,
) {
    with_text_system(cx, |text_system| {
        let first = object(1, 0, 1, 16.);
        let mut input = layout(8, 10000.);
        input.start_position = SourcePosition::new(
            ByteOffset::new(0),
            InlineObjectGap::before(first.cursor().neighbor()),
        )
        .into();
        input.limits.retained_bytes = std::mem::size_of::<gpui::StreamingLayoutContinuation>();
        let mut owner = ExactGeometryOwner::new(
            binding("", 1),
            PresentationGeneration::new(1),
            input,
            style(),
            ExactGeometryLimits::new(256, 2, 512 * 1024, 512).unwrap(),
        )
        .unwrap();
        let job = start_index(&mut owner, 1);
        let text = page(&mut owner, job, "", 0, 0, 1);
        assert_eq!(
            owner
                .admit_page(job, &text, text_system)
                .unwrap()
                .progress(),
            ExactGeometryProgress::NeedObjects
        );
        let objects = object_response(&mut owner, job, 1, vec![first], true);
        let failure = owner
            .admit_object_page(job, &text, &objects, text_system)
            .unwrap_err();
        assert_eq!(
            failure.error(),
            &ExactGeometryError::Layout(gpui::StreamingLayoutError::CapacityExceeded(
                gpui::StreamingLayoutComponent::Total
            ))
        );
        assert_eq!(failure.release().jobs, vec![job]);
        assert_eq!(failure.release().object_pages, vec![objects.key()]);
        assert_eq!(owner.counts().active_job_items, 0);
        assert!(owner.index().is_none());
    });
}

#[gpui::test]
fn layout_return_capacity_is_reserved_before_continuation_changes(cx: &mut TestAppContext) {
    with_text_system(cx, |text_system| {
        let mut required = (512 * 1024, 32 * 1024);
        for attempt in 0..4 {
            let (bytes, items) = match attempt {
                0 | 1 => required,
                2 => (required.0 - 1, required.1),
                _ => (required.0, required.1 - 1),
            };
            let first = object(1, 0, 1, 16.);
            let second = object(2, 0, 2, 16.);
            let leading = SourcePosition::new(
                ByteOffset::new(0),
                InlineObjectGap::before(first.cursor().neighbor()),
            );
            let trailing = SourcePosition::new(
                ByteOffset::new(0),
                InlineObjectGap::between(first.cursor().neighbor(), second.cursor().neighbor())
                    .unwrap(),
            );
            let mut input = layout(8, 10000.);
            input.start_position = leading.into();
            let mut session = text_system.streaming_layout_session(input.clone()).unwrap();
            let returned = session
                .admit_inline_object(gpui::StreamingInlineObject {
                    input_id: input.input_id,
                    segment_policy_id: input.segment_policy_id,
                    ordinal: 0,
                    id: first.id().into(),
                    order: first.order().into(),
                    leading: leading.into(),
                    trailing: trailing.into(),
                    presentation: SharedString::default(),
                    runs: Vec::new(),
                    width: px(16.),
                    height: px(14.),
                    baseline: px(10.),
                    background: None,
                })
                .unwrap();
            let mut owner = ExactGeometryOwner::new(
                binding("", 1),
                PresentationGeneration::new(1),
                input,
                style(),
                ExactGeometryLimits::new(256, 2, bytes, items).unwrap(),
            )
            .unwrap();
            let job = start_index(&mut owner, 1);
            let text = page(&mut owner, job, "", 0, 0, 1);
            assert_eq!(
                owner
                    .admit_page(job, &text, text_system)
                    .unwrap()
                    .progress(),
                ExactGeometryProgress::NeedObjects
            );
            let objects =
                object_response_with_limit(&mut owner, job, 1, 2, vec![first, second], false);
            let before = owner.counts();
            let expected = (
                before.total_bytes()
                    + text.retained_charge().bytes()
                    + objects.retained_charge().bytes()
                    + returned.charge.total().unwrap()
                    + std::mem::size_of::<StreamingLayoutFragment>(),
                before.total_items()
                    + text.retained_charge().items()
                    + objects.retained_charge().objects()
                    + 1
                    + returned.item_charge.total().unwrap()
                    + 1
                    + 7
                    - before.continuation_items,
            );
            if attempt == 0 {
                required = expected;
                assert_eq!(
                    owner
                        .admit_object_page(job, &text, &objects, text_system)
                        .unwrap()
                        .progress(),
                    ExactGeometryProgress::NeedObjects
                );
                continue;
            }
            let failure = owner
                .admit_object_page(job, &text, &objects, text_system)
                .unwrap_err();
            if attempt == 1 {
                assert_eq!(failure.error(), &ExactGeometryError::CapacityExceeded);
                assert_eq!(
                    failure.stage(),
                    gpui_text_input::ExactGeometryFailureStage::Checkpoint
                );
                assert_eq!(failure.release().counts.continuation_items, 7);
            } else {
                assert_eq!(
                    failure.error(),
                    &ExactGeometryError::Layout(gpui::StreamingLayoutError::CapacityExceeded(
                        gpui::StreamingLayoutComponent::Total
                    ))
                );
                assert_eq!(
                    failure.stage(),
                    gpui_text_input::ExactGeometryFailureStage::Scan
                );
                assert_eq!(
                    failure.release().counts.continuation_items,
                    before.continuation_items
                );
                assert!(failure.admission_required_bytes() <= bytes);
                assert!(failure.admission_required_items() <= items);
            }
            assert_eq!(failure.release().jobs, vec![job]);
            assert_eq!(failure.release().object_pages, vec![objects.key()]);
            assert_eq!(failure.release().counts.checkpoints, 1);
            assert_eq!(owner.counts().active_job_items, 0);
            assert!(owner.index().is_none());
        }
    });
}
