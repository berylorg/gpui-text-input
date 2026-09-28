use super::*;

fn check_continuation_capacity(
    text_system: &WindowTextSystem,
    deferred: bool,
    prepare: impl Fn(usize, usize) -> (ExactGeometryOwner, GeometryJobKey, RangePage, ObjectPage),
) {
    let (mut probe, job, page, objects) = prepare(512 * 1024, 32 * 1024);
    let admission = probe
        .admit_object_page(job, &page, &objects, text_system)
        .unwrap();
    let bytes = admission.admission_required_bytes();
    let items = admission.admission_required_items();
    for (byte_cap, item_cap, success) in [
        (bytes, items, true),
        (bytes - 1, items, false),
        (bytes, items - 1, false),
    ] {
        let (mut owner, job, page, objects) = prepare(byte_cap, item_cap);
        let result = owner.admit_object_page(job, &page, &objects, text_system);
        if success {
            let admission = result.unwrap();
            assert_eq!(admission.admission_required_bytes(), bytes);
            assert_eq!(admission.admission_required_items(), items);
            assert_eq!(
                admission.progress(),
                if deferred {
                    ExactGeometryProgress::NeedObjects
                } else {
                    ExactGeometryProgress::Scanning
                }
            );
            assert_eq!(owner.counts().active_atom_items, usize::from(!deferred));
            assert_eq!(
                owner.counts().deferred_object_items,
                4 * usize::from(deferred)
            );
        } else {
            let failure = result.unwrap_err();
            assert_eq!(failure.error(), &ExactGeometryError::CapacityExceeded);
            assert_eq!(
                failure.stage(),
                gpui_text_input::ExactGeometryFailureStage::Scan
            );
            assert_eq!(failure.release().counts.active_atom_bytes, 0);
            assert_eq!(failure.release().counts.active_atom_items, 0);
            assert_eq!(failure.release().counts.deferred_object_bytes, 0);
            assert_eq!(failure.release().counts.deferred_object_items, 0);
            assert_eq!(failure.release().jobs, vec![job]);
            assert_eq!(owner.counts().active_job_items, 0);
            assert!(owner.index().is_none());
            assert!(owner.target().is_none());
        }
    }
}

#[gpui::test]
fn cross_page_atom_storage_is_admitted_before_allocation(cx: &mut TestAppContext) {
    with_text_system(cx, |text_system| {
        check_continuation_capacity(text_system, false, |bytes, items| {
            let source = "opaque atom";
            let mut owner =
                owner_with_retained_items(source, 32, 64., 4, bytes, items, style()).unwrap();
            let job = start_index(&mut owner, 1);
            let page = page_with_atoms(
                &mut owner,
                job,
                source,
                0,
                4,
                1,
                vec![AtomFact::new(
                    AtomId::new(1),
                    ByteRange::from_u64(0, source.len() as u64).unwrap(),
                    ByteRange::from_u64(0, 4).unwrap(),
                    "fallback",
                )],
            );
            assert_eq!(
                owner
                    .admit_page(job, &page, text_system)
                    .unwrap()
                    .progress(),
                ExactGeometryProgress::NeedObjects
            );
            let objects = empty_object_page(&mut owner, job, &page, 1);
            (owner, job, page, objects)
        });
    });
}

#[gpui::test]
fn deferred_object_storage_and_payload_are_admitted_before_cloning(cx: &mut TestAppContext) {
    with_text_system(cx, |text_system| {
        let fact = InlineObjectFact::new(
            InlineObjectId::new(1),
            ByteOffset::new(0),
            InlineObjectOrder::new(1),
            "fallback".repeat(64),
            InlineObjectPresentation::new(
                1,
                "visible".repeat(64),
                px(20.),
                px(14.),
                px(10.),
                None,
                1,
                true,
            )
            .unwrap(),
        );
        check_continuation_capacity(text_system, true, |bytes, items| {
            let mut binding_layout = layout(32, 64.);
            binding_layout.start_position = SourcePosition::new(
                ByteOffset::new(0),
                InlineObjectGap::before(fact.cursor().neighbor()),
            )
            .into();
            let mut owner = ExactGeometryOwner::new(
                binding("", 1),
                PresentationGeneration::new(1),
                binding_layout,
                style(),
                ExactGeometryLimits::new(32, 4, bytes, items).unwrap(),
            )
            .unwrap();
            let job = start_index(&mut owner, 1);
            let page = page(&mut owner, job, "", 0, 0, 1);
            assert_eq!(
                owner
                    .admit_page(job, &page, text_system)
                    .unwrap()
                    .progress(),
                ExactGeometryProgress::NeedObjects
            );
            let request = owner
                .request_object_page(job, ObjectRequestId::new(1), 1, 64 * 1024)
                .unwrap();
            let objects = ObjectPage::new(
                ObjectPageId::new(1),
                request.key(),
                vec![fact.clone()],
                ObjectPageEdgeFact::EnvelopeBoundary,
                ObjectPageEdgeFact::Continues(fact.cursor()),
                false,
                Some(fact.cursor()),
            )
            .unwrap();
            (owner, job, page, objects)
        });
    });
}
