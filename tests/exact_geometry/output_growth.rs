use super::*;

#[gpui::test]
fn index_layout_peak_includes_discarded_fragment_records(cx: &mut TestAppContext) {
    with_text_system(cx, |text_system| {
        let source = "abcdefghijklmnopqrs";
        let mut session = text_system
            .streaming_layout_session(layout(16, 10000.))
            .unwrap();
        let layout = session
            .admit_text(gpui::StreamingTextSegment {
                input_id: 41,
                segment_policy_id: 73,
                ordinal: 0,
                logical_range: StreamingLayoutPosition::at(0)..StreamingLayoutPosition::at(16),
                text: SharedString::new(source[..16].to_owned()),
                runs: vec![TextRun {
                    len: 16,
                    font: font(".SystemUIFont"),
                    color: black(),
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                }],
            })
            .unwrap();
        let mut required = (512 * 1024, 32 * 1024);
        for attempt in 0..4 {
            let (bytes, items) = match attempt {
                0 | 1 => required,
                2 => (required.0 - 1, required.1),
                _ => (required.0, required.1 - 1),
            };
            let mut owner =
                owner_with_retained_items(source, 16, 10000., 2, bytes, items, style()).unwrap();
            let job = start_index(&mut owner, 1);
            let page = page(&mut owner, job, source, 0, 18, 1);
            assert_eq!(
                owner
                    .admit_page(job, &page, text_system)
                    .unwrap()
                    .progress(),
                ExactGeometryProgress::NeedObjects
            );
            let objects = empty_object_page(&mut owner, job, &page, 1);
            let before = owner.counts();
            let expected = (
                before.total_bytes()
                    + page.retained_charge().bytes()
                    + objects.retained_charge().bytes()
                    + 8
                    + layout.charge.total().unwrap()
                    + std::mem::size_of::<StreamingLayoutFragment>()
                    + 3 * std::mem::size_of::<gpui_text_input::ExactGeometryCheckpoint>(),
                before.total_items()
                    + page.retained_charge().items()
                    + 1
                    + layout.item_charge.total().unwrap()
                    + 1
                    + 3,
            );
            let result = owner.admit_object_page(job, &page, &objects, text_system);
            if attempt < 2 {
                let admission = result.unwrap();
                assert_eq!(admission.progress(), ExactGeometryProgress::Scanning);
                required = (
                    admission.admission_required_bytes(),
                    admission.admission_required_items(),
                );
                assert_eq!(required, expected);
                assert_eq!(owner.counts().output_record_bytes, 0);
            } else {
                let failure = result.unwrap_err();
                assert_eq!(failure.error(), &ExactGeometryError::CapacityExceeded);
                assert_eq!(
                    failure.stage(),
                    gpui_text_input::ExactGeometryFailureStage::Checkpoint
                );
                assert_eq!(failure.release().jobs, vec![job]);
                assert_eq!(failure.release().object_pages, vec![objects.key()]);
                assert_eq!(failure.release().counts.checkpoints, 1);
                assert_eq!(owner.counts().active_job_items, 0);
                assert!(owner.index().is_none());
            }
        }
    });
}

#[gpui::test]
fn target_fragment_backing_is_admitted_before_initial_and_repeated_growth(cx: &mut TestAppContext) {
    with_text_system(cx, |text_system| {
        for fragments in [1, 5, 9] {
            let source = format!("{}\nb\nc\nd\n", "a".repeat(fragments * 16 + 3));
            let mut required = (0, 0);
            let mut accepted_records = 0;
            for attempt in 0..4 {
                let (bytes, items) = match attempt {
                    0 => (512 * 1024, 32 * 1024),
                    1 => required,
                    2 => (required.0 - 1, required.1),
                    _ => (required.0, required.1 - 1),
                };
                let mut owner =
                    owner_with_retained_items(&source, 16, 10000., 4, bytes, items, style())
                        .unwrap();
                let index = start_index(&mut owner, 1);
                let index_page = page(&mut owner, index, &source, 0, source.len(), 1);
                assert_eq!(
                    admit_page_with_empty_objects(&mut owner, index, &index_page, text_system)
                        .unwrap()
                        .progress(),
                    ExactGeometryProgress::IndexComplete
                );
                let job = owner
                    .request_block_target_anchored(
                        GeometryJobId::new(2),
                        BlockTarget::new(px(0.), px(10000.), px(0.)),
                        SourcePosition::new(ByteOffset::new(0), InlineObjectGap::NoObjects),
                    )
                    .unwrap()
                    .key();
                let page = page(&mut owner, job, &source, 0, fragments * 16 + 2, 2);
                let result = admit_page_with_empty_objects(&mut owner, job, &page, text_system);
                if attempt < 2 {
                    let admission = result.unwrap();
                    assert_eq!(admission.progress(), ExactGeometryProgress::Scanning);
                    required = (
                        admission.admission_required_bytes(),
                        admission.admission_required_items(),
                    );
                    accepted_records = owner.counts().output_record_bytes;
                    assert!(accepted_records > 0);
                } else {
                    let failure = result.unwrap_err();
                    assert!(matches!(
                        failure.error(),
                        ExactGeometryError::CapacityExceeded
                            | ExactGeometryError::Layout(
                                gpui::StreamingLayoutError::CapacityExceeded(
                                    gpui::StreamingLayoutComponent::Total
                                )
                            )
                    ));
                    assert_eq!(
                        failure.stage(),
                        gpui_text_input::ExactGeometryFailureStage::Scan
                    );
                    assert!(
                        if attempt == 2 {
                            failure.release().counts.output_record_bytes < accepted_records
                        } else {
                            failure.release().counts.output_record_bytes <= accepted_records
                        },
                        "fragments={fragments} attempt={attempt}"
                    );
                    assert_eq!(failure.release().jobs, vec![job]);
                    assert_eq!(owner.counts().active_job_items, 0);
                    assert!(owner.index().is_some());
                    assert!(owner.target().is_none());
                }
            }
        }
    });
}

#[gpui::test]
fn target_object_presentation_backing_is_admitted_before_initial_and_repeated_growth(
    cx: &mut TestAppContext,
) {
    with_text_system(cx, |text_system| {
        for retained in [1, 5, 9] {
            let facts = (1..=retained + 1)
                .map(|id| {
                    InlineObjectFact::new(
                        InlineObjectId::new(id as u128),
                        ByteOffset::new(0),
                        InlineObjectOrder::new(id as u128),
                        "",
                        InlineObjectPresentation::new(
                            id as u64,
                            "",
                            px(10.),
                            px(14.),
                            px(10.),
                            None,
                            0,
                            false,
                        )
                        .unwrap(),
                    )
                })
                .collect::<Vec<_>>();
            let mut required = (0, 0);
            let mut accepted_records = 0;
            for attempt in 0..4 {
                let (bytes, items) = match attempt {
                    0 => (512 * 1024, 32 * 1024),
                    1 => required,
                    2 => (required.0 - 1, required.1),
                    _ => (required.0, required.1 - 1),
                };
                let mut layout = layout(32, 10000.);
                layout.start_position = SourcePosition::new(
                    ByteOffset::new(0),
                    InlineObjectGap::before(facts[0].cursor().neighbor()),
                )
                .into();
                let mut owner = ExactGeometryOwner::new(
                    binding("", 1),
                    PresentationGeneration::new(1),
                    layout,
                    style(),
                    ExactGeometryLimits::new(256, 4, bytes, items).unwrap(),
                )
                .unwrap();
                let index = start_index(&mut owner, 1);
                let index_page = page(&mut owner, index, "", 0, 0, 1);
                owner.admit_page(index, &index_page, text_system).unwrap();
                let index_objects = objects(&mut owner, index, 1, facts.clone(), true);
                assert_eq!(
                    owner
                        .admit_object_page(index, &index_page, &index_objects, text_system)
                        .unwrap()
                        .progress(),
                    ExactGeometryProgress::IndexComplete
                );
                let job = owner
                    .request_block_target_anchored(
                        GeometryJobId::new(2),
                        BlockTarget::new(px(0.), px(10000.), px(0.)),
                        SourcePosition::new(
                            ByteOffset::new(0),
                            InlineObjectGap::before(facts[0].cursor().neighbor()),
                        ),
                    )
                    .unwrap()
                    .key();
                let page = page(&mut owner, job, "", 0, 0, 2);
                owner.admit_page(job, &page, text_system).unwrap();
                let response = objects(&mut owner, job, 2, facts.clone(), false);
                let result = owner.admit_object_page(job, &page, &response, text_system);
                if attempt < 2 {
                    let admission = result.unwrap();
                    assert_eq!(admission.progress(), ExactGeometryProgress::NeedObjects);
                    required = (
                        admission.admission_required_bytes(),
                        admission.admission_required_items(),
                    );
                    accepted_records = owner.counts().output_record_bytes;
                } else {
                    let failure = result.unwrap_err();
                    assert_eq!(failure.error(), &ExactGeometryError::CapacityExceeded);
                    assert_eq!(
                        failure.stage(),
                        gpui_text_input::ExactGeometryFailureStage::Scan
                    );
                    assert!(
                        if attempt == 2 {
                            failure.release().counts.output_record_bytes < accepted_records
                        } else {
                            failure.release().counts.output_record_bytes <= accepted_records
                        },
                        "retained={retained} attempt={attempt}"
                    );
                    assert_eq!(failure.release().jobs, vec![job]);
                    assert_eq!(owner.counts().active_job_items, 0);
                    assert!(owner.index().is_some());
                    assert!(owner.target().is_none());
                }
            }
        }
    });
}

fn objects(
    owner: &mut ExactGeometryOwner,
    job: GeometryJobKey,
    id: u64,
    facts: Vec<InlineObjectFact>,
    complete: bool,
) -> ObjectPage {
    let request = owner
        .request_object_page(job, ObjectRequestId::new(id), facts.len(), 64 * 1024)
        .unwrap();
    let continuation = (!complete).then(|| facts.last().unwrap().cursor());
    ObjectPage::new(
        ObjectPageId::new(id),
        request.key(),
        facts,
        ObjectPageEdgeFact::EnvelopeBoundary,
        continuation.map_or(
            ObjectPageEdgeFact::EnvelopeBoundary,
            ObjectPageEdgeFact::Continues,
        ),
        complete,
        continuation,
    )
    .unwrap()
}
