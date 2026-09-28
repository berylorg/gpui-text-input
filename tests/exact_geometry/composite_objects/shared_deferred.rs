use super::*;
use gpui_text_input::preparation_test_support::{
    enclosing_failure_peak, is_enclosing_capacity_refusal, owner_presentation_overlap,
    prepare_continuation_copy, prepare_deferred_tail, prepare_detached_inline, prepare_object_scan,
    prepare_response, prepare_response_with_enclosing,
};

fn check_continuation_copy(
    owner: &ExactGeometryOwner,
    job: GeometryJobKey,
    index: bool,
    display_bytes: usize,
) {
    let before = owner.counts();
    let baseline = (before.total_bytes(), before.total_items());
    let (raw, identity) =
        prepare_continuation_copy(owner, job, index, None, (usize::MAX, usize::MAX)).unwrap();
    assert_eq!(raw, identity);
    for current in [(0, 0), (baseline.0 + 8192, baseline.1 + 32)] {
        let mapped = (
            current.0 + raw.0 - baseline.0 - display_bytes,
            current.1 + raw.1 - baseline.1,
        );
        for _ in 0..2 {
            assert_eq!(
                prepare_continuation_copy(owner, job, index, Some(current), mapped).unwrap(),
                (raw, mapped)
            );
            assert_eq!(owner.counts(), before);
        }
        for limit in [(mapped.0 - 1, mapped.1), (mapped.0, mapped.1 - 1)] {
            let failure =
                prepare_continuation_copy(owner, job, index, Some(current), limit).unwrap_err();
            assert!(is_enclosing_capacity_refusal(&failure));
            assert_eq!(enclosing_failure_peak(&failure), Some(mapped));
            assert_eq!(failure.release(), &ExactGeometryRelease::default());
            assert_eq!(owner.counts(), before);
            assert!(prepare_continuation_copy(owner, job, index, Some(current), mapped).is_ok());
        }
    }
    for current in [(usize::MAX, 0), (0, usize::MAX)] {
        let failure =
            prepare_continuation_copy(owner, job, index, Some(current), (usize::MAX, usize::MAX))
                .unwrap_err();
        assert!(!is_enclosing_capacity_refusal(&failure));
        assert_eq!(enclosing_failure_peak(&failure), Some((0, 0)));
        assert_eq!(failure.release(), &ExactGeometryRelease::default());
        assert_eq!(owner.counts(), before);
    }
}

fn check_deferred_tail(
    owner: &ExactGeometryOwner,
    job: GeometryJobKey,
    index: bool,
    objects: &ObjectPage,
    display_bytes: usize,
) {
    let before = owner.counts();
    let (raw, identity, baseline) =
        prepare_deferred_tail(owner, job, index, objects, None, (usize::MAX, usize::MAX)).unwrap();
    assert_eq!(raw, identity);
    for current in [(0, 0), (baseline.0 + 8192, baseline.1 + 32)] {
        let mapped = (
            current.0 + raw.0 - baseline.0 - display_bytes,
            current.1 + raw.1 - baseline.1,
        );
        for _ in 0..2 {
            assert_eq!(
                prepare_deferred_tail(owner, job, index, objects, Some(current), mapped).unwrap(),
                (raw, mapped, baseline)
            );
            assert_eq!(owner.counts(), before);
        }
        for limit in [(mapped.0 - 1, mapped.1), (mapped.0, mapped.1 - 1)] {
            let failure = prepare_deferred_tail(owner, job, index, objects, Some(current), limit)
                .unwrap_err();
            assert!(is_enclosing_capacity_refusal(&failure));
            assert_eq!(enclosing_failure_peak(&failure), Some(mapped));
            assert_eq!(failure.release(), &ExactGeometryRelease::default());
            assert_eq!(owner.counts(), before);
            assert!(
                prepare_deferred_tail(owner, job, index, objects, Some(current), mapped).is_ok()
            );
        }
    }
    for limit in [(raw.0 - 1, raw.1), (raw.0, raw.1 - 1)] {
        let failure = prepare_deferred_tail(owner, job, index, objects, None, limit).unwrap_err();
        assert!(is_enclosing_capacity_refusal(&failure));
        assert_eq!(enclosing_failure_peak(&failure), Some(raw));
        assert_eq!(owner.counts(), before);
    }
    for current in [(usize::MAX, 0), (0, usize::MAX)] {
        let failure = prepare_deferred_tail(
            owner,
            job,
            index,
            objects,
            Some(current),
            (usize::MAX, usize::MAX),
        )
        .unwrap_err();
        assert!(!is_enclosing_capacity_refusal(&failure));
        assert_eq!(enclosing_failure_peak(&failure), Some((0, 0)));
        assert_eq!(failure.release(), &ExactGeometryRelease::default());
        assert_eq!(owner.counts(), before);
    }
}

fn check_detached_inline(
    owner: &ExactGeometryOwner,
    job: GeometryJobKey,
    index: bool,
    text: &RangePage,
    objects: &ObjectPage,
    text_system: &gpui::WindowTextSystem,
    display_bytes: usize,
) {
    let before = owner.counts();
    let probe = |current, limit| {
        prepare_detached_inline(
            owner,
            job,
            index,
            text,
            objects,
            text_system,
            current,
            limit,
        )
    };
    let (raw, identity, baseline) = probe(None, (usize::MAX, usize::MAX)).unwrap();
    assert_eq!(raw, identity);
    for current in [(0, 0), (baseline.0 + 8192, baseline.1 + 32)] {
        let mapped = (
            current.0 + raw.0 - baseline.0 - 2 * display_bytes,
            current.1 + raw.1 - baseline.1,
        );
        for _ in 0..2 {
            assert_eq!(
                probe(Some(current), mapped).unwrap(),
                (raw, mapped, baseline)
            );
            assert_eq!(owner.counts(), before);
        }
        for limit in [(mapped.0 - 1, mapped.1), (mapped.0, mapped.1 - 1)] {
            let failure = probe(Some(current), limit).unwrap_err();
            assert!(is_enclosing_capacity_refusal(&failure), "{failure:?}");
            let attempted = enclosing_failure_peak(&failure).unwrap();
            assert!(attempted.0 <= mapped.0 && attempted.1 <= mapped.1);
            assert!(attempted.0 > limit.0 || attempted.1 > limit.1);
            assert_eq!(failure.release(), &ExactGeometryRelease::default());
            assert_eq!(owner.counts(), before);
            assert!(probe(Some(current), mapped).is_ok());
        }
    }
    for current in [(usize::MAX, 0), (0, usize::MAX)] {
        let failure = probe(Some(current), (usize::MAX, usize::MAX)).unwrap_err();
        assert!(!is_enclosing_capacity_refusal(&failure));
        assert_eq!(enclosing_failure_peak(&failure), Some((0, 0)));
        assert_eq!(failure.release(), &ExactGeometryRelease::default());
        assert_eq!(owner.counts(), before);
    }
}

fn check_object_scan(
    owner: &ExactGeometryOwner,
    job: GeometryJobKey,
    index: bool,
    text: &RangePage,
    objects: &ObjectPage,
    text_system: &gpui::WindowTextSystem,
    display_bytes: usize,
) {
    let before = owner.counts();
    let probe = |current, limit| {
        prepare_object_scan(
            owner,
            job,
            index,
            text,
            objects,
            text_system,
            current,
            limit,
        )
    };
    let (raw, identity, baseline) = probe(None, (usize::MAX, usize::MAX)).unwrap();
    assert_eq!(raw, identity);
    for current in [(0, 0), (baseline.0 + 8192, baseline.1 + 32)] {
        let (enclosing_raw, mapped, _) = probe(Some(current), (usize::MAX, usize::MAX)).unwrap();
        assert_eq!(enclosing_raw, raw);
        assert_eq!(mapped.1, current.1 + raw.1 - baseline.1);
        let savings = current.0 + raw.0 - baseline.0 - mapped.0;
        assert_eq!(savings, display_bytes * if index { 1 } else { 2 });
        for _ in 0..2 {
            assert_eq!(
                probe(Some(current), mapped).unwrap(),
                (raw, mapped, baseline)
            );
            assert_eq!(owner.counts(), before);
        }
        for limit in [(mapped.0 - 1, mapped.1), (mapped.0, mapped.1 - 1)] {
            let failure = probe(Some(current), limit).unwrap_err();
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
            assert!(probe(Some(current), mapped).is_ok());
        }
    }
    for current in [(usize::MAX, 0), (0, usize::MAX)] {
        let failure = probe(Some(current), (usize::MAX, usize::MAX)).unwrap_err();
        assert!(!is_enclosing_capacity_refusal(&failure));
        assert_eq!(failure.release(), &ExactGeometryRelease::default());
        assert_eq!(owner.counts(), before);
    }
}

#[gpui::test]
fn returned_inline_overlap_uses_live_backing_before_target_metadata(cx: &mut TestAppContext) {
    use gpui_text_input::preparation_test_support::{
        fragment_presentation_overlap, shared_object_display,
    };
    with_text_system(cx, |text_system| {
        for display in ["", "shared é"] {
            let fact = InlineObjectFact::new(
                InlineObjectId::new(1),
                ByteOffset::new(0),
                InlineObjectOrder::new(1),
                "fallback".repeat(64),
                InlineObjectPresentation::new(1, display, px(80.), px(64.), px(40.), None, 1, true)
                    .unwrap(),
            );
            let leading = SourcePosition::new(
                ByteOffset::new(0),
                InlineObjectGap::before(fact.cursor().neighbor()),
            );
            let trailing = SourcePosition::new(
                ByteOffset::new(0),
                InlineObjectGap::after(fact.cursor().neighbor()),
            );
            let mut input = layout(64, 10000.);
            input.start_position = leading.into();
            let mut owner = ExactGeometryOwner::new(
                binding("", 1),
                PresentationGeneration::new(1),
                input.clone(),
                style(),
                ExactGeometryLimits::new(256, 32, 512 * 1024, 32 * 1024).unwrap(),
            )
            .unwrap();
            let job = start_index(&mut owner, 1);
            let text = page(&mut owner, job, "", 0, 0, 1);
            owner.admit_page(job, &text, text_system).unwrap();
            let objects = object_response(&mut owner, job, 1, vec![fact], true);
            let independent = objects.clone();
            let runs = || {
                if display.is_empty() {
                    Vec::new()
                } else {
                    vec![TextRun {
                        len: display.len(),
                        font: font(".SystemUIFont"),
                        color: black(),
                        background_color: None,
                        underline: None,
                        strikethrough: None,
                    }]
                }
            };
            for shared in [false, true] {
                let mut session = text_system.streaming_layout_session(input.clone()).unwrap();
                let returned = session
                    .admit_inline_object(gpui::StreamingInlineObject {
                        input_id: input.input_id,
                        segment_policy_id: input.segment_policy_id,
                        ordinal: 0,
                        id: objects.objects()[0].id().into(),
                        order: objects.objects()[0].order().into(),
                        leading: leading.into(),
                        trailing: trailing.into(),
                        presentation: if shared {
                            shared_object_display(&objects.objects()[0])
                        } else {
                            SharedString::from(display.to_owned())
                        },
                        runs: runs(),
                        width: px(80.),
                        height: px(64.),
                        baseline: px(40.),
                        background: None,
                    })
                    .unwrap();
                let expected = if shared { display.len() } else { 0 };
                assert_eq!(
                    fragment_presentation_overlap(&returned.fragments, &[&objects]),
                    Some(expected)
                );
                assert_eq!(
                    fragment_presentation_overlap(&returned.fragments, &[&objects, &objects]),
                    Some(expected)
                );
                assert_eq!(
                    fragment_presentation_overlap(&returned.fragments, &[&independent]),
                    Some(0)
                );
                assert_eq!(
                    fragment_presentation_overlap(&returned.fragments, &[]),
                    Some(0)
                );
                let mut charged_twice = returned.fragments.to_vec();
                charged_twice.extend(returned.fragments.iter().cloned());
                assert_eq!(
                    fragment_presentation_overlap(&charged_twice, &[&objects]),
                    Some(expected * 2)
                );
                assert_eq!(owner_presentation_overlap(&owner, &[&objects]), Some(0));
            }
            if !display.is_empty() {
                input.start_position = StreamingLayoutPosition::at(0);
                let mut session = text_system.streaming_layout_session(input.clone()).unwrap();
                let returned = session
                    .admit_text(gpui::StreamingTextSegment {
                        input_id: input.input_id,
                        segment_policy_id: input.segment_policy_id,
                        ordinal: 0,
                        logical_range: StreamingLayoutPosition::at(0)
                            ..StreamingLayoutPosition::at(display.len() as u64),
                        text: shared_object_display(&objects.objects()[0]),
                        runs: runs(),
                    })
                    .unwrap();
                assert_eq!(
                    fragment_presentation_overlap(&returned.fragments, &[&objects]),
                    Some(0)
                );
            }
        }
    });
}

#[gpui::test]
fn deferred_presentations_share_admitted_backing_through_preparation(cx: &mut TestAppContext) {
    with_text_system(cx, |text_system| {
        for (index, display) in [
            (false, ""),
            (true, ""),
            (false, "shared é"),
            (true, "shared é"),
        ] {
            let display = display.to_owned();
            let fact = |id| {
                InlineObjectFact::new(
                    InlineObjectId::new(id),
                    ByteOffset::new(0),
                    InlineObjectOrder::new(id),
                    "fallback".repeat(64),
                    InlineObjectPresentation::new(
                        id as u64,
                        display.clone(),
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
                binding("x", 1),
                PresentationGeneration::new(1),
                input,
                style(),
                ExactGeometryLimits::new(256, 32, 512 * 1024, 32 * 1024).unwrap(),
            )
            .unwrap();
            let target = BlockTarget::new(px(0.), px(80.), px(32.));
            if !index {
                let job = start_index(&mut owner, 1);
                let text = page(&mut owner, job, "x", 0, 1, 1);
                owner.admit_page(job, &text, text_system).unwrap();
                let objects = object_response_with_limit(
                    &mut owner,
                    job,
                    1,
                    3,
                    vec![first.clone(), fact(2), fact(3)],
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
            let text = page(&mut owner, job, "x", 0, 1, 10);
            owner.admit_page(job, &text, text_system).unwrap();
            let objects = object_response_with_limit(&mut owner, job, 10, 3, vec![first], false);
            let complete = ObjectPage::new(
                ObjectPageId::new(11),
                objects.key(),
                vec![fact(1), fact(2)],
                ObjectPageEdgeFact::EnvelopeBoundary,
                ObjectPageEdgeFact::EnvelopeBoundary,
                true,
                None,
            )
            .unwrap();
            check_object_scan(
                &owner,
                job,
                index,
                &text,
                &complete,
                text_system,
                display.len(),
            );
            let independent = objects.clone();
            let prepare = |owner: &ExactGeometryOwner, objects: &ObjectPage, id| {
                prepare_response(
                    owner,
                    job,
                    &text,
                    Some(objects),
                    text_system,
                    false,
                    index,
                    (
                        GeometryJobId::new(id),
                        PageRequestId::new(id),
                        ObjectRequestId::new(id),
                    ),
                    target,
                    (usize::MAX, usize::MAX),
                )
                .unwrap()
            };
            let before = owner.counts();
            check_continuation_copy(&owner, job, index, 0);
            check_deferred_tail(&owner, job, index, &objects, display.len());
            let enclosing = prepare_response_with_enclosing(
                &owner,
                job,
                &text,
                Some(&objects),
                text_system,
                false,
                index,
                (
                    GeometryJobId::new(20),
                    PageRequestId::new(20),
                    ObjectRequestId::new(20),
                ),
                target,
                (8192, 32),
                (usize::MAX, usize::MAX),
            )
            .unwrap();
            assert_eq!(
                enclosing.presentation_overlap(&[&objects]),
                Some(display.len())
            );
            assert_eq!(
                enclosing.required_capacity(),
                prepare(&owner, &objects, 20).required_capacity()
            );
            assert_eq!(owner.counts(), before);
            drop(enclosing);
            for _ in 0..2 {
                let prepared = prepare(&owner, &objects, 20);
                assert_eq!(
                    prepared.presentation_overlap(&[&objects]),
                    Some(display.len())
                );
                assert_eq!(prepared.presentation_overlap(&[&independent]), Some(0));
                assert_eq!(
                    prepared.presentation_overlap(&[&objects, &objects]),
                    Some(display.len())
                );
                assert_eq!(owner.counts(), before);
            }
            let prepared = prepare(&owner, &objects, 20);
            let next = prepared.object_request().unwrap();
            prepared.commit(&mut owner);
            assert_eq!(owner.counts().deferred_object_items, 4);
            check_continuation_copy(&owner, job, index, display.len());
            assert_eq!(
                owner_presentation_overlap(&owner, &[&objects]),
                Some(display.len())
            );
            assert_eq!(owner_presentation_overlap(&owner, &[&independent]), Some(0));

            let second = fact(2);
            let cursor = second.cursor();
            let following = ObjectPage::new(
                ObjectPageId::new(20),
                next.key(),
                vec![second],
                ObjectPageEdgeFact::Continues(objects.objects()[0].cursor()),
                ObjectPageEdgeFact::Continues(cursor),
                false,
                Some(cursor),
            )
            .unwrap();
            let before = owner.counts();
            let expected = display.len() * if index { 1 } else { 2 };
            check_detached_inline(
                &owner,
                job,
                index,
                &text,
                &following,
                text_system,
                display.len(),
            );
            let failure = prepare_detached_inline(
                &owner,
                job,
                index,
                &text,
                &objects,
                text_system,
                Some((8192, 32)),
                (usize::MAX, usize::MAX),
            )
            .unwrap_err();
            assert_eq!(failure.error(), &ExactGeometryError::SourceContract);
            assert!(!is_enclosing_capacity_refusal(&failure));
            assert_eq!(failure.release(), &ExactGeometryRelease::default());
            assert_eq!(owner.counts(), before);
            let enclosing = prepare_response_with_enclosing(
                &owner,
                job,
                &text,
                Some(&following),
                text_system,
                false,
                index,
                (
                    GeometryJobId::new(30),
                    PageRequestId::new(30),
                    ObjectRequestId::new(30),
                ),
                target,
                (8192, 32),
                (usize::MAX, usize::MAX),
            )
            .unwrap();
            assert_eq!(
                enclosing.required_capacity(),
                prepare(&owner, &following, 30).required_capacity()
            );
            assert_eq!(
                enclosing.presentation_overlap(&[&objects, &following]),
                Some(expected)
            );
            assert_eq!(owner.counts(), before);
            drop(enclosing);
            for _ in 0..2 {
                let prepared = prepare(&owner, &following, 30);
                assert_eq!(
                    prepared.presentation_overlap(&[&objects, &following]),
                    Some(expected)
                );
                assert_eq!(owner.counts(), before);
                assert_eq!(
                    owner_presentation_overlap(&owner, &[&objects]),
                    Some(display.len())
                );
            }
            prepare(&owner, &following, 30).commit(&mut owner);
            assert_eq!(owner.counts().deferred_object_items, 4);
            assert_eq!(
                owner_presentation_overlap(&owner, &[&objects, &following]),
                Some(expected)
            );
        }
    });
}
