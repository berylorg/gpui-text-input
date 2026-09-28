use super::*;
use gpui_text_input::preparation_test_support::{owner_presentation_overlap, prepare_response};

#[gpui::test]
fn deferred_presentations_share_admitted_backing_through_preparation(cx: &mut TestAppContext) {
    with_text_system(cx, |text_system| {
        for index in [false, true] {
            let display = "shared".to_owned();
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
            let mut input = layout(8, 10000.);
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
            let objects = object_response(&mut owner, job, 10, vec![first], false);
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
