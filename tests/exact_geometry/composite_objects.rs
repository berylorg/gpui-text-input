use super::*;

fn object(id: u128, anchor: u64, order: u128, width: f32) -> InlineObjectFact {
    let presentation = InlineObjectPresentation::new(
        id as u64,
        "",
        px(width),
        px(14.),
        px(10.),
        None,
        id as u64,
        true,
    )
    .unwrap();
    InlineObjectFact::new(
        InlineObjectId::new(id),
        ByteOffset::new(anchor),
        InlineObjectOrder::new(order),
        format!("object-{id}"),
        presentation,
    )
}

fn composite_owner(first: &InlineObjectFact, wrap_width: f32) -> ExactGeometryOwner {
    let mut composite_layout = layout(8, wrap_width);
    composite_layout.start_position = SourcePosition::new(
        ByteOffset::new(0),
        InlineObjectGap::before(first.cursor().neighbor()),
    )
    .into();
    ExactGeometryOwner::new(
        binding("", 1),
        PresentationGeneration::new(1),
        composite_layout,
        style(),
        ExactGeometryLimits::new(256, 32, 512 * 1024, 32 * 1024).unwrap(),
    )
    .unwrap()
}

fn object_response(
    owner: &mut ExactGeometryOwner,
    job: GeometryJobKey,
    id: u64,
    objects: Vec<InlineObjectFact>,
    complete: bool,
) -> ObjectPage {
    let request = owner
        .request_object_page(job, ObjectRequestId::new(id), 1, 64 * 1024)
        .unwrap();
    let preceding = request.key().demand().cursor().map_or(
        ObjectPageEdgeFact::EnvelopeBoundary,
        ObjectPageEdgeFact::Continues,
    );
    let continuation = (!complete).then(|| objects.last().unwrap().cursor());
    let following = continuation.map_or(
        ObjectPageEdgeFact::EnvelopeBoundary,
        ObjectPageEdgeFact::Continues,
    );
    ObjectPage::new(
        ObjectPageId::new(id),
        request.key(),
        objects,
        preceding,
        following,
        complete,
        continuation,
    )
    .unwrap()
}

#[gpui::test]
fn same_anchor_objects_continue_across_pages_and_checkpoint_each_composite_gap(
    cx: &mut TestAppContext,
) {
    with_text_system(cx, |text_system| {
        let first = object(1, 0, 10, 10.);
        let second = object(2, 0, 20, 20.);
        let third = object(3, 0, 30, 30.);
        let mut owner = composite_owner(&first, 24.);
        let job = start_index(&mut owner, 1);
        let text_page = page(&mut owner, job, "", 0, 0, 1);
        assert_eq!(
            owner
                .admit_page(job, &text_page, text_system)
                .unwrap()
                .progress(),
            ExactGeometryProgress::NeedObjects
        );

        for (id, fact, complete, progress) in [
            (1, first.clone(), false, ExactGeometryProgress::NeedObjects),
            (2, second.clone(), false, ExactGeometryProgress::NeedObjects),
            (3, third.clone(), true, ExactGeometryProgress::IndexComplete),
        ] {
            let response = object_response(&mut owner, job, id, vec![fact], complete);
            let admission = owner
                .admit_object_page(job, &text_page, &response, text_system)
                .unwrap();
            assert_eq!(admission.progress(), progress);
        }

        let checkpoints = owner.index().unwrap().checkpoints();
        assert!(checkpoints.len() >= 4);
        assert!(checkpoints.iter().any(|checkpoint| {
            checkpoint.source()
                == SourcePosition::new(
                    ByteOffset::new(0),
                    InlineObjectGap::between(first.cursor().neighbor(), second.cursor().neighbor())
                        .unwrap(),
                )
                && checkpoint.object_cursor() == Some(first.cursor())
        }));
        assert!(checkpoints.iter().any(|checkpoint| {
            checkpoint.source()
                == SourcePosition::new(
                    ByteOffset::new(0),
                    InlineObjectGap::between(second.cursor().neighbor(), third.cursor().neighbor())
                        .unwrap(),
                )
                && checkpoint.object_cursor() == Some(second.cursor())
        }));
    });
}

#[gpui::test]
fn object_at_resident_page_edge_follows_buffered_cross_page_grapheme(cx: &mut TestAppContext) {
    with_text_system(cx, |text_system| {
        let source = "e\u{301}x";
        let fact = object(7, 3, 10, 12.);
        let mut owner = owner(source, 8, 16, 512 * 1024, 32);
        let job = start_index(&mut owner, 1);

        let first_page = page(&mut owner, job, source, 0, 1, 1);
        let first_objects = owner.admit_page(job, &first_page, text_system).unwrap();
        assert_eq!(first_objects.progress(), ExactGeometryProgress::NeedObjects);
        let empty = object_response(&mut owner, job, 1, vec![], true);
        assert_eq!(
            owner
                .admit_object_page(job, &first_page, &empty, text_system)
                .unwrap()
                .progress(),
            ExactGeometryProgress::Scanning
        );

        let second_page = page(&mut owner, job, source, 1, source.len(), 2);
        assert_eq!(
            owner
                .admit_page(job, &second_page, text_system)
                .unwrap()
                .progress(),
            ExactGeometryProgress::NeedObjects
        );
        let objects = object_response(&mut owner, job, 2, vec![fact.clone()], true);
        assert_eq!(
            owner
                .admit_object_page(job, &second_page, &objects, text_system)
                .unwrap()
                .progress(),
            ExactGeometryProgress::IndexComplete
        );
        assert!(
            owner
                .index()
                .unwrap()
                .checkpoints()
                .iter()
                .any(|checkpoint| {
                    checkpoint.object_cursor() == Some(fact.cursor())
                        && checkpoint.source().gap
                            == InlineObjectGap::after(fact.cursor().neighbor())
                })
        );
    });
}

#[gpui::test]
fn cancelled_object_request_cannot_satisfy_identical_new_demand(cx: &mut TestAppContext) {
    with_text_system(cx, |text_system| {
        let first = object(41, 0, 10, 10.);
        let mut owner = composite_owner(&first, 24.);

        let old_job = start_index(&mut owner, 1);
        let old_text = page(&mut owner, old_job, "", 0, 0, 1);
        assert_eq!(
            owner
                .admit_page(old_job, &old_text, text_system)
                .unwrap()
                .progress(),
            ExactGeometryProgress::NeedObjects
        );
        let old_page = object_response(&mut owner, old_job, 1, vec![first.clone()], true);
        let old_key = old_page.key();
        let cancelled = owner.cancel(old_job).unwrap();
        assert_eq!(cancelled.object_pages, vec![old_key]);

        let new_job = start_index(&mut owner, 2);
        let new_text = page(&mut owner, new_job, "", 0, 0, 2);
        assert_eq!(
            owner
                .admit_page(new_job, &new_text, text_system)
                .unwrap()
                .progress(),
            ExactGeometryProgress::NeedObjects
        );
        assert_eq!(
            owner.request_object_page(new_job, ObjectRequestId::new(1), 1, 64 * 1024),
            Err(ExactGeometryError::IdNotMonotonic)
        );
        let new_page = object_response(&mut owner, new_job, 2, vec![first], true);
        let late = owner
            .admit_object_page(new_job, &new_text, &old_page, text_system)
            .unwrap_err();
        assert_eq!(late.error(), &ExactGeometryError::WrongObjectPage(old_key));
        assert!(late.release().jobs.is_empty());
        assert_eq!(
            owner
                .admit_object_page(new_job, &new_text, &new_page, text_system)
                .unwrap()
                .progress(),
            ExactGeometryProgress::IndexComplete
        );
    });
}
