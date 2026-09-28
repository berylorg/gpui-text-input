use super::*;

#[path = "composite_objects/continuation_capacity.rs"]
mod continuation_capacity;
#[path = "composite_objects/returned_capacity.rs"]
mod returned_capacity;
#[cfg(feature = "test-support")]
#[path = "composite_objects/shared_deferred.rs"]
mod shared_deferred;
#[cfg(feature = "test-support")]
#[path = "composite_objects/source_finalization.rs"]
mod source_finalization;
#[cfg(feature = "test-support")]
#[path = "composite_objects/terminal_target.rs"]
mod terminal_target;
#[cfg(feature = "test-support")]
#[path = "composite_objects/text_successor.rs"]
mod text_successor;

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
    object_response_with_limit(owner, job, id, 1, objects, complete)
}

fn object_response_with_limit(
    owner: &mut ExactGeometryOwner,
    job: GeometryJobKey,
    id: u64,
    max_objects: usize,
    objects: Vec<InlineObjectFact>,
    complete: bool,
) -> ObjectPage {
    let request = owner
        .request_object_page(job, ObjectRequestId::new(id), max_objects, 64 * 1024)
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
fn inline_style_storage_is_admitted_before_layout(cx: &mut TestAppContext) {
    with_text_system(cx, |text_system| {
        for deferred in [false, true] {
            for display in ["x", ""] {
                let mut required = (512 * 1024, 32 * 1024);
                let mut reservation = (0, 0);
                for attempt in 0..7 {
                    let (bytes, items) = match attempt {
                        0 | 1 => required,
                        2 => (required.0 - 1, required.1),
                        3 => (required.0, required.1 - 1),
                        4 => reservation,
                        5 => (reservation.0 - 1, reservation.1),
                        _ => (reservation.0, reservation.1 - 1),
                    };
                    let first = InlineObjectFact::new(
                        InlineObjectId::new(1),
                        ByteOffset::new(0),
                        InlineObjectOrder::new(1),
                        "object".repeat(1024),
                        InlineObjectPresentation::new(
                            1,
                            display,
                            px(16.),
                            px(20.),
                            px(14.),
                            None,
                            1,
                            true,
                        )
                        .unwrap(),
                    );
                    let mut input = layout(8, 10000.);
                    input.start_position = SourcePosition::new(
                        ByteOffset::new(0),
                        InlineObjectGap::before(first.cursor().neighbor()),
                    )
                    .into();
                    // A layout error distinguishes entry into GPUI from the earlier allocation guard.
                    input.limits.maps = 1;
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
                    let objects = if deferred {
                        let initial = object_response(&mut owner, job, 1, vec![first], false);
                        assert_eq!(
                            owner
                                .admit_object_page(job, &text, &initial, text_system)
                                .unwrap()
                                .progress(),
                            ExactGeometryProgress::NeedObjects
                        );
                        let next = InlineObjectFact::new(
                            InlineObjectId::new(2),
                            ByteOffset::new(0),
                            InlineObjectOrder::new(2),
                            "next".repeat(4096),
                            InlineObjectPresentation::new(
                                2,
                                "",
                                px(16.),
                                px(20.),
                                px(14.),
                                None,
                                2,
                                true,
                            )
                            .unwrap(),
                        );
                        object_response_with_limit(
                            &mut owner,
                            job,
                            2,
                            4,
                            vec![
                                next,
                                object(3, 0, 3, 16.),
                                object(4, 0, 4, 16.),
                                object(5, 0, 5, 16.),
                            ],
                            true,
                        )
                    } else {
                        object_response(&mut owner, job, 1, vec![first], true)
                    };
                    let before = owner.counts();
                    let run_count = usize::from(!display.is_empty());
                    let expected = (
                        before.total_bytes()
                            + text.retained_charge().bytes()
                            + objects.retained_charge().bytes()
                            + run_count * std::mem::size_of::<TextRun>(),
                        before.total_items()
                            + text.retained_charge().items()
                            + objects.retained_charge().objects()
                            + 1
                            + run_count,
                    );
                    let failure = owner
                        .admit_object_page(job, &text, &objects, text_system)
                        .unwrap_err();
                    let peak = if attempt <= 1 || attempt >= 4 {
                        (
                            expected.0
                                + std::mem::size_of::<StreamingLayoutFragment>()
                                + std::mem::size_of::<gpui::StreamingLayoutContinuation>(),
                            expected.1 + 6 + 2 * usize::from(deferred),
                        )
                    } else {
                        expected
                    };
                    assert_eq!(failure.admission_required_bytes(), peak.0);
                    assert_eq!(failure.admission_required_items(), peak.1);
                    if attempt == 0 {
                        assert!(matches!(
                            failure.error(),
                            ExactGeometryError::Layout(
                                gpui::StreamingLayoutError::CapacityExceeded(
                                    gpui::StreamingLayoutComponent::Maps
                                )
                            )
                        ));
                        required = expected;
                        reservation = peak;
                    } else if attempt == 4 {
                        assert!(
                            matches!(failure.error(), ExactGeometryError::Layout(_)),
                            "{:?}",
                            failure.error()
                        );
                    } else {
                        assert_eq!(failure.error(), &ExactGeometryError::CapacityExceeded);
                    }
                    assert_eq!(
                        failure.stage(),
                        if display.is_empty() && (2..4).contains(&attempt) {
                            gpui_text_input::ExactGeometryFailureStage::PageCoexistence
                        } else {
                            gpui_text_input::ExactGeometryFailureStage::Scan
                        }
                    );
                    assert_eq!(failure.release().jobs, vec![job]);
                    assert_eq!(failure.release().object_pages, vec![objects.key()]);
                    assert_eq!(failure.release().counts.output_record_bytes, 0);
                    assert_eq!(owner.counts().active_job_items, 0);
                    assert!(owner.index().is_none());
                }
            }
        }
    });
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
fn anchored_predecessors_respect_before_between_and_after_same_anchor_gaps(
    cx: &mut TestAppContext,
) {
    with_text_system(cx, |text_system| {
        let first = object(11, 0, 10, 10.);
        let second = object(12, 0, 20, 20.);
        let between = SourcePosition::new(
            ByteOffset::new(0),
            InlineObjectGap::between(first.cursor().neighbor(), second.cursor().neighbor())
                .unwrap(),
        );
        let build = || {
            let mut owner = composite_owner(&first, 24.);
            let job = start_index(&mut owner, 1);
            let text_page = page(&mut owner, job, "", 0, 0, 1);
            assert_eq!(
                owner
                    .admit_page(job, &text_page, text_system)
                    .unwrap()
                    .progress(),
                ExactGeometryProgress::NeedObjects,
            );
            let objects = object_response_with_limit(
                &mut owner,
                job,
                1,
                2,
                vec![first.clone(), second.clone()],
                true,
            );
            assert_eq!(
                owner
                    .admit_object_page(job, &text_page, &objects, text_system)
                    .unwrap()
                    .progress(),
                ExactGeometryProgress::IndexComplete,
            );
            owner
        };

        for (job_id, anchor, expected_predecessor, replay) in [
            (
                2,
                SourcePosition::new(
                    ByteOffset::new(0),
                    InlineObjectGap::before(first.cursor().neighbor()),
                ),
                SourcePosition::new(
                    ByteOffset::new(0),
                    InlineObjectGap::before(first.cursor().neighbor()),
                ),
                vec![first.clone(), second.clone()],
            ),
            (
                3,
                between,
                SourcePosition::new(
                    ByteOffset::new(0),
                    InlineObjectGap::before(first.cursor().neighbor()),
                ),
                vec![first.clone(), second.clone()],
            ),
            (
                4,
                SourcePosition::new(
                    ByteOffset::new(0),
                    InlineObjectGap::after(second.cursor().neighbor()),
                ),
                between,
                vec![second.clone()],
            ),
        ] {
            let mut owner = build();
            let start = owner
                .request_block_target_anchored(
                    GeometryJobId::new(job_id),
                    BlockTarget::new(px(28.), px(14.), px(0.)),
                    anchor,
                )
                .unwrap();
            let expected_items = match expected_predecessor.gap {
                InlineObjectGap::NoObjects => 3,
                InlineObjectGap::Before(_) | InlineObjectGap::After(_) => 5,
                InlineObjectGap::Between { .. } => 7,
            };
            assert_eq!(owner.counts().continuation_items, expected_items);
            let text_page = page(&mut owner, start.key(), "", 0, 0, job_id + 10);
            assert_eq!(
                owner
                    .admit_page(start.key(), &text_page, text_system)
                    .unwrap()
                    .progress(),
                ExactGeometryProgress::NeedObjects,
            );
            let objects =
                object_response_with_limit(&mut owner, start.key(), job_id + 10, 2, replay, true);
            assert_eq!(
                owner
                    .admit_object_page(start.key(), &text_page, &objects, text_system)
                    .unwrap()
                    .progress(),
                ExactGeometryProgress::TargetComplete,
            );
            assert_eq!(owner.target().unwrap().predecessor(), expected_predecessor);
        }
    });
}

#[gpui::test]
fn anchored_target_predecessor_stays_before_viewport_visible_same_anchor_objects(
    cx: &mut TestAppContext,
) {
    with_text_system(cx, |text_system| {
        let source = "x".repeat(40);
        let first = object(4, 32, 10, 144.);
        let second = object(5, 32, 20, 144.);
        let mut owner = owner_with(&source, 32, 240., 32, 512 * 1024, 32, style()).unwrap();
        let index = start_index(&mut owner, 1);
        let leading_text = page(&mut owner, index, &source, 0, 32, 1);
        assert_eq!(
            owner
                .admit_page(index, &leading_text, text_system)
                .unwrap()
                .progress(),
            ExactGeometryProgress::NeedObjects
        );
        let leading_objects = object_response_with_limit(
            &mut owner,
            index,
            1,
            2,
            vec![first.clone(), second.clone()],
            true,
        );
        assert_eq!(
            owner
                .admit_object_page(index, &leading_text, &leading_objects, text_system)
                .unwrap()
                .progress(),
            ExactGeometryProgress::Scanning
        );
        let terminal_text = page(&mut owner, index, &source, 32, 40, 2);
        assert_eq!(
            owner
                .admit_page(index, &terminal_text, text_system)
                .unwrap()
                .progress(),
            ExactGeometryProgress::NeedObjects
        );
        let terminal_objects = object_response(&mut owner, index, 2, vec![], true);
        assert_eq!(
            owner
                .admit_object_page(index, &terminal_text, &terminal_objects, text_system)
                .unwrap()
                .progress(),
            ExactGeometryProgress::IndexComplete
        );

        let origin = {
            let checkpoints = owner.index().unwrap().checkpoints();
            checkpoints.first().unwrap().source()
        };
        let anchor = SourcePosition::new(
            ByteOffset::new(32),
            InlineObjectGap::after(second.cursor().neighbor()),
        );
        let target = owner
            .request_block_target_anchored(
                GeometryJobId::new(2),
                BlockTarget::new(px(0.), px(80.), px(14.)),
                anchor,
            )
            .unwrap();
        assert_eq!(target.progress(), ExactGeometryProgress::Scanning);
        let target_leading_text = page(&mut owner, target.key(), &source, 0, 32, 3);
        assert_eq!(
            owner
                .admit_page(target.key(), &target_leading_text, text_system)
                .unwrap()
                .progress(),
            ExactGeometryProgress::NeedObjects
        );
        let target_leading_objects = object_response_with_limit(
            &mut owner,
            target.key(),
            3,
            2,
            vec![first.clone(), second.clone()],
            true,
        );
        assert_eq!(
            owner
                .admit_object_page(
                    target.key(),
                    &target_leading_text,
                    &target_leading_objects,
                    text_system,
                )
                .unwrap()
                .progress(),
            ExactGeometryProgress::Scanning
        );
        let target_terminal_text = page(&mut owner, target.key(), &source, 32, 40, 4);
        assert_eq!(
            owner
                .admit_page(target.key(), &target_terminal_text, text_system)
                .unwrap()
                .progress(),
            ExactGeometryProgress::NeedObjects
        );
        let target_terminal_objects = object_response(&mut owner, target.key(), 4, vec![], true);
        assert_eq!(
            owner
                .admit_object_page(
                    target.key(),
                    &target_terminal_text,
                    &target_terminal_objects,
                    text_system,
                )
                .unwrap()
                .progress(),
            ExactGeometryProgress::TargetComplete
        );

        let publication = owner.target().unwrap();
        assert_eq!(publication.predecessor(), origin);
        assert_eq!(publication.source_end().byte_offset, ByteOffset::new(40));
    });
}

#[gpui::test]
fn end_anchor_after_object_scans_boundary_proof_before_target_publication(cx: &mut TestAppContext) {
    with_text_system(cx, |text_system| {
        let first = object(1, 0, 10, 10.);
        let mut owner = composite_owner(&first, 24.);
        let index = start_index(&mut owner, 1);
        let index_text = page(&mut owner, index, "", 0, 0, 1);
        assert_eq!(
            owner
                .admit_page(index, &index_text, text_system)
                .unwrap()
                .progress(),
            ExactGeometryProgress::NeedObjects
        );
        let index_objects = object_response(&mut owner, index, 1, vec![first.clone()], true);
        assert_eq!(
            owner
                .admit_object_page(index, &index_text, &index_objects, text_system)
                .unwrap()
                .progress(),
            ExactGeometryProgress::IndexComplete
        );

        let after = SourcePosition::new(
            ByteOffset::new(0),
            InlineObjectGap::after(first.cursor().neighbor()),
        );
        let target = owner
            .request_block_target_anchored(
                GeometryJobId::new(2),
                BlockTarget::new(px(0.), px(28.), px(14.)),
                after,
            )
            .unwrap();
        assert_eq!(target.progress(), ExactGeometryProgress::Scanning);
        let target_text = page(&mut owner, target.key(), "", 0, 0, 2);
        assert_eq!(
            owner
                .admit_page(target.key(), &target_text, text_system)
                .unwrap()
                .progress(),
            ExactGeometryProgress::NeedObjects
        );
        let target_objects =
            object_response(&mut owner, target.key(), 2, vec![first.clone()], true);
        assert_eq!(
            owner
                .admit_object_page(target.key(), &target_text, &target_objects, text_system)
                .unwrap()
                .progress(),
            ExactGeometryProgress::TargetComplete
        );
        assert!(owner.target().unwrap().fragments().iter().any(|fragment| {
            matches!(fragment, gpui::StreamingLayoutFragment::InlineObject(_))
        }));
    });
}

#[gpui::test]
fn earlier_anchor_retains_only_leading_object_context_and_requested_visual_line(
    cx: &mut TestAppContext,
) {
    with_text_system(cx, |text_system| {
        let source = "aa\nbb\ncc\ndd\nee\nff\ngg\nhh\nii\njj\n";
        let inline = object(31, 6, 10, 10.);
        let requested = BlockTarget::new(px(42.), px(80.), px(14.));
        let anchor = SourcePosition::new(ByteOffset::new(0), InlineObjectGap::NoObjects);

        let run = |pending: bool| {
            let mut owner = owner_with(source, 2, 512., 32, 512 * 1024, 32, style()).unwrap();
            let index = start_index(&mut owner, 1);
            if pending {
                let start = owner
                    .request_block_target_anchored(GeometryJobId::new(2), requested, anchor)
                    .unwrap();
                assert_eq!(start.progress(), ExactGeometryProgress::PendingIndex);
            }
            let index_text = page(&mut owner, index, source, 0, source.len(), 1);
            assert_eq!(
                owner
                    .admit_page(index, &index_text, text_system)
                    .unwrap()
                    .progress(),
                ExactGeometryProgress::NeedObjects,
            );
            let index_objects =
                object_response_with_limit(&mut owner, index, 1, 4, vec![inline.clone()], true);
            assert_eq!(
                owner
                    .admit_object_page(index, &index_text, &index_objects, text_system)
                    .unwrap()
                    .progress(),
                ExactGeometryProgress::IndexComplete,
            );
            let start = if pending {
                owner.start_pending_target().unwrap()
            } else {
                owner
                    .request_block_target_anchored(GeometryJobId::new(2), requested, anchor)
                    .unwrap()
            };
            let target_text = page(&mut owner, start.key(), source, 0, source.len(), 10);
            assert_eq!(
                owner
                    .admit_page(start.key(), &target_text, text_system)
                    .unwrap()
                    .progress(),
                ExactGeometryProgress::NeedObjects,
            );
            let target_objects = object_response_with_limit(
                &mut owner,
                start.key(),
                10,
                4,
                vec![inline.clone()],
                true,
            );
            assert_eq!(
                owner
                    .admit_object_page(start.key(), &target_text, &target_objects, text_system)
                    .unwrap()
                    .progress(),
                ExactGeometryProgress::TargetComplete,
            );
            owner
                .target()
                .unwrap()
                .fragments()
                .iter()
                .map(|fragment| match fragment {
                    gpui::StreamingLayoutFragment::Text(fragment) => {
                        let range = fragment.logical_range();
                        ("text", range.start.byte_offset, range.end.byte_offset)
                    }
                    gpui::StreamingLayoutFragment::OversizeAtom(fragment) => (
                        "atom",
                        fragment.logical_range.start.byte_offset,
                        fragment.logical_range.end.byte_offset,
                    ),
                    gpui::StreamingLayoutFragment::InlineObject(fragment) => (
                        "object",
                        fragment.leading.byte_offset,
                        fragment.trailing.byte_offset,
                    ),
                    gpui::StreamingLayoutFragment::Boundary(fragment) => {
                        let first = fragment
                            .maps()
                            .first()
                            .unwrap()
                            .logical_position
                            .byte_offset;
                        let last = fragment.maps().last().unwrap().logical_position.byte_offset;
                        ("boundary", first, last)
                    }
                })
                .collect::<Vec<_>>()
        };

        let direct = run(false);
        let pending = run(true);
        assert_eq!(direct, pending);
        assert_eq!(
            direct,
            vec![
                ("object", 6, 6),
                ("boundary", 8, 9),
                ("text", 9, 11),
                ("boundary", 11, 12),
                ("text", 12, 14),
                ("boundary", 14, 15),
                ("text", 15, 17),
                ("boundary", 17, 18),
                ("text", 18, 20),
                ("boundary", 20, 21),
                ("text", 21, 23),
                ("boundary", 23, 24),
                ("text", 24, 26),
                ("boundary", 26, 27),
                ("text", 27, 29),
                ("boundary", 29, 30),
            ],
        );
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
