use super::*;

#[gpui::test]
fn initial_continuation_counts_match_gpui_gap_witnesses(cx: &mut TestAppContext) {
    with_text_system(cx, |text_system| {
        let first = object(1, 0, 1, 16.);
        let second = object(2, 0, 2, 16.);
        for (gap, expected) in [
            (InlineObjectGap::NoObjects, 3),
            (InlineObjectGap::before(first.cursor().neighbor()), 5),
            (
                InlineObjectGap::between(first.cursor().neighbor(), second.cursor().neighbor())
                    .unwrap(),
                7,
            ),
            (InlineObjectGap::after(second.cursor().neighbor()), 5),
        ] {
            let mut input = layout(8, 10000.);
            input.start_position = SourcePosition::new(ByteOffset::new(0), gap).into();
            let session = text_system.streaming_layout_session(input.clone()).unwrap();
            assert_eq!(session.retained_item_charge().total().unwrap(), expected);
            let mut owner = ExactGeometryOwner::new(
                binding("", 1),
                PresentationGeneration::new(1),
                input,
                style(),
                ExactGeometryLimits::new(256, 2, 512 * 1024, 512).unwrap(),
            )
            .unwrap();
            start_index(&mut owner, 1);
            assert_eq!(owner.counts().continuation_items, expected);
        }
    });
}

#[gpui::test]
fn pristine_object_origin_admits_gap_witnesses_before_mutation(cx: &mut TestAppContext) {
    with_text_system(cx, |text_system| {
        let mut required = 512;
        for attempt in 0..3 {
            let items = if attempt == 2 { required - 1 } else { required };
            let mut owner = ExactGeometryOwner::new(
                binding("", 1),
                PresentationGeneration::new(1),
                layout(8, 10000.),
                style(),
                ExactGeometryLimits::new(256, 2, 512 * 1024, items).unwrap(),
            )
            .unwrap();
            let job = start_index(&mut owner, 1);
            let text = page(&mut owner, job, "", 0, 0, 1);
            owner.admit_page(job, &text, text_system).unwrap();
            let objects = object_response(&mut owner, job, 1, vec![object(1, 0, 1, 16.)], true);
            assert_eq!(owner.counts().continuation_items, 3);
            let expected = owner.counts().total_items()
                + text.retained_charge().items()
                + objects.retained_charge().objects()
                + 1
                + 2;
            if attempt == 0 {
                required = expected;
                owner
                    .admit_object_page(job, &text, &objects, text_system)
                    .unwrap();
                continue;
            }
            let failure = owner
                .admit_object_page(job, &text, &objects, text_system)
                .unwrap_err();
            assert_eq!(failure.error(), &ExactGeometryError::CapacityExceeded);
            assert_eq!(
                failure.stage(),
                gpui_text_input::ExactGeometryFailureStage::Scan
            );
            assert_eq!(
                failure.release().counts.continuation_items,
                if attempt == 1 { 5 } else { 3 }
            );
            assert_eq!(failure.release().jobs, vec![job]);
            assert_eq!(failure.release().object_pages, vec![objects.key()]);
            assert_eq!(owner.counts().active_job_items, 0);
            assert!(owner.index().is_none());
        }
    });
}
