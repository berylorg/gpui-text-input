use std::mem::size_of;

use super::*;

fn assert_terminal_failure(
    failure: &gpui_text_input::ExactGeometryFailure,
    job: GeometryJobKey,
    page: gpui_text_input::PageRequestKey,
) {
    assert_eq!(failure.release().jobs, vec![job]);
    assert_eq!(failure.release().pages, vec![page]);
    assert!(failure.release().counts.active_job_bytes > 0);
    assert_eq!(
        failure.release().counts.pending_page_bytes,
        size_of::<gpui_text_input::PageRequestKey>()
    );
}

fn subtract_counts(
    total: gpui_text_input::ExactGeometryCounts,
    base: gpui_text_input::ExactGeometryCounts,
) -> gpui_text_input::ExactGeometryCounts {
    gpui_text_input::ExactGeometryCounts {
        owner_items: total.owner_items - base.owner_items,
        owner_bytes: total.owner_bytes - base.owner_bytes,
        input_items: total.input_items - base.input_items,
        input_bytes: total.input_bytes - base.input_bytes,
        desired_target_items: total.desired_target_items - base.desired_target_items,
        desired_target_bytes: total.desired_target_bytes - base.desired_target_bytes,
        active_job_items: total.active_job_items - base.active_job_items,
        active_job_bytes: total.active_job_bytes - base.active_job_bytes,
        pending_page_items: total.pending_page_items - base.pending_page_items,
        pending_page_bytes: total.pending_page_bytes - base.pending_page_bytes,
        scan_buffer_items: total.scan_buffer_items - base.scan_buffer_items,
        scan_buffer_bytes: total.scan_buffer_bytes - base.scan_buffer_bytes,
        active_atom_items: total.active_atom_items - base.active_atom_items,
        active_atom_bytes: total.active_atom_bytes - base.active_atom_bytes,
        checkpoints: total.checkpoints - base.checkpoints,
        checkpoint_bytes: total.checkpoint_bytes - base.checkpoint_bytes,
        continuation_items: total.continuation_items - base.continuation_items,
        continuation_bytes: total.continuation_bytes - base.continuation_bytes,
        output_items: total.output_items - base.output_items,
        output_record_bytes: total.output_record_bytes - base.output_record_bytes,
        output_payload_bytes: total.output_payload_bytes - base.output_payload_bytes,
        publication_items: total.publication_items - base.publication_items,
        publication_bytes: total.publication_bytes - base.publication_bytes,
    }
}

#[gpui::test]
fn borrowed_page_and_checkpoint_peaks_report_direct_exact_cap_and_release(cx: &mut TestAppContext) {
    with_text_system(cx, |text_system| {
        let source = "abcdefghij".repeat(40);
        let exercise = |cap: usize| {
            let mut owner = owner(&source, 8, 16, cap, 16);
            let job = start_index(&mut owner, 1);
            let next = page(&mut owner, job, &source, 0, 128, 1);
            let result = owner.admit_page(job, &next, text_system);
            (owner, job, next.key(), result)
        };

        let (_, _, _, accepted) = exercise(512 * 1024);
        let accepted = accepted.unwrap();
        let required = accepted.admission_required_bytes();
        assert!(required > 128);
        let (_, _, _, exact) = exercise(required);
        assert_eq!(exact.unwrap().admission_required_bytes(), required);

        let (mut rejected, job, page, failure) = exercise(required - 1);
        let failure = failure.unwrap_err();
        assert_eq!(failure.error(), &ExactGeometryError::CapacityExceeded);
        assert_eq!(
            failure.stage(),
            gpui_text_input::ExactGeometryFailureStage::Checkpoint
        );
        assert_eq!(failure.admission_required_bytes(), required);
        assert_terminal_failure(&failure, job, page);
        assert!(128 < required - 1, "page payload fits the cap in isolation");
        let late = RangePage::new(
            PageId::new(99),
            page,
            ByteRange::from_u64(0, 128).unwrap(),
            source[..128].to_owned(),
            vec![],
            PageEdgeFact::DocumentBoundary,
            PageEdgeFact::Continues,
            false,
        )
        .unwrap();
        let late = rejected.admit_page(job, &late, text_system).unwrap_err();
        assert_eq!(late.error(), &ExactGeometryError::ObsoleteJob(job));
        assert_eq!(late.release(), &ExactGeometryRelease::default());
    });
}

#[gpui::test]
fn borrowed_page_payload_can_fail_initial_combined_live_peak(cx: &mut TestAppContext) {
    with_text_system(cx, |text_system| {
        let source = format!("a{}b", "\u{301}".repeat(100));
        let first_end = 127;
        let exercise = |cap: usize| {
            let mut owner = owner(&source, 8, 8, cap, 16);
            let base = owner.counts();
            let job = start_index(&mut owner, 1);
            let next = page(&mut owner, job, &source, 0, first_end, 1);
            let active = subtract_counts(owner.counts(), base);
            let result = owner.admit_page(job, &next, text_system);
            (owner, job, next.key(), active, result)
        };
        let (_, _, _, _, accepted) = exercise(512 * 1024);
        let required = accepted.unwrap().admission_required_bytes();
        let (_, _, _, _, exact) = exercise(required);
        assert_eq!(exact.unwrap().admission_required_bytes(), required);
        let (_, job, page, active, failure) = exercise(required - 1);
        let failure = failure.unwrap_err();
        assert_eq!(failure.error(), &ExactGeometryError::CapacityExceeded);
        assert_eq!(
            failure.stage(),
            gpui_text_input::ExactGeometryFailureStage::PageCoexistence
        );
        assert_eq!(failure.admission_required_bytes(), required);
        assert!(first_end < required - 1);
        assert_terminal_failure(&failure, job, page);
        assert_eq!(failure.release().counts, active);
    });
}

#[gpui::test]
fn gpui_semantic_item_peak_accepts_exact_cap_and_rejects_one_under_atomically(
    cx: &mut TestAppContext,
) {
    with_text_system(cx, |text_system| {
        let source = "wrapped semantic item accounting";
        let exercise = |item_cap: usize| {
            let mut owner =
                owner_with_retained_items(source, 8, 24., 8, 256 * 1024, item_cap, style())
                    .unwrap();
            let job = start_index(&mut owner, 1);
            let next = page(&mut owner, job, source, 0, source.len(), 1);
            let result = owner.admit_page(job, &next, text_system);
            (owner, job, next.key(), result)
        };

        let (accepted_owner, _, _, accepted) = exercise(usize::MAX);
        let required = accepted.unwrap().admission_required_items();
        assert!(required > accepted_owner.counts().total_items());
        let (_, _, _, exact) = exercise(required);
        assert_eq!(exact.unwrap().admission_required_items(), required);

        let (rejected, job, page, failure) = exercise(required - 1);
        let failure = failure.unwrap_err();
        assert_eq!(failure.error(), &ExactGeometryError::CapacityExceeded);
        assert_eq!(failure.admission_required_items(), required);
        assert_terminal_failure(&failure, job, page);
        assert!(rejected.index().is_none());
    });
}

#[gpui::test]
fn window_scan_and_finalize_errors_release_named_terminal_state(cx: &mut TestAppContext) {
    let source = "abcdefghij";
    let mut window_owner = owner(source, 4, 8, 256 * 1024, 16);
    let window_base = window_owner.counts();
    let job = start_index(&mut window_owner, 1);
    let first = page(&mut window_owner, job, source, 0, 5, 1);
    {
        let first_window = cx.add_empty_window();
        first_window.update(|window, _| {
            assert_eq!(
                window_owner
                    .admit_page(job, &first, window.text_system())
                    .unwrap()
                    .progress(),
                ExactGeometryProgress::Scanning
            );
        });
    }
    let second = page(&mut window_owner, job, source, 5, source.len(), 2);
    let window_active = subtract_counts(window_owner.counts(), window_base);
    let second_window = cx.add_empty_window();
    second_window.update(|window, _| {
        let failure = window_owner
            .admit_page(job, &second, window.text_system())
            .unwrap_err();
        assert_eq!(failure.error(), &ExactGeometryError::SourceContract);
        assert_eq!(
            failure.stage(),
            gpui_text_input::ExactGeometryFailureStage::WindowIdentity
        );
        assert_terminal_failure(&failure, job, second.key());
        assert_eq!(failure.release().counts, window_active);
    });

    second_window.update(|window, _| {
        let mut bad_layout = layout(8, 64.);
        bad_layout.limits.maps = 1;
        let mut layout_owner = ExactGeometryOwner::new(
            binding("a\nb", 1),
            bad_layout,
            style(),
            ExactGeometryLimits::new(16, 8, 256 * 1024, 16 * 1024).unwrap(),
        )
        .unwrap();
        let layout_base = layout_owner.counts();
        let job = start_index(&mut layout_owner, 1);
        let next = page(&mut layout_owner, job, "a\nb", 0, 3, 1);
        let mut layout_active = subtract_counts(layout_owner.counts(), layout_base);
        layout_active.scan_buffer_items = 1;
        let failure = layout_owner
            .admit_page(job, &next, window.text_system())
            .unwrap_err();
        assert!(matches!(failure.error(), ExactGeometryError::Layout(_)));
        assert_eq!(
            failure.stage(),
            gpui_text_input::ExactGeometryFailureStage::Scan
        );
        assert_terminal_failure(&failure, job, next.key());
        assert_eq!(failure.release().counts, layout_active);

        let source = "xxxx";
        let mut owner = owner(source, 8, 8, 256 * 1024, 16);
        let atom_base = owner.counts();
        let job = start_index(&mut owner, 1);
        let atom_range = ByteRange::from_u64(0, 6).unwrap();
        let fragment = ByteRange::from_u64(0, 4).unwrap();
        let next = page_with_atoms(
            &mut owner,
            job,
            source,
            0,
            4,
            1,
            vec![AtomFact::new(AtomId::new(7), atom_range, fragment, "atom")],
        );
        let mut expected_release = subtract_counts(owner.counts(), atom_base);
        expected_release.active_atom_items = 1;
        expected_release.active_atom_bytes =
            size_of::<gpui_text_input::AtomId>() + size_of::<ByteRange>();
        let failure = owner
            .admit_page(job, &next, window.text_system())
            .unwrap_err();
        assert_eq!(failure.error(), &ExactGeometryError::SourceContract);
        assert_eq!(
            failure.stage(),
            gpui_text_input::ExactGeometryFailureStage::Finalize
        );
        assert_terminal_failure(&failure, job, next.key());
        assert_eq!(failure.release().counts, expected_release);
    });
}

#[gpui::test]
fn publication_replacement_peak_is_exact_and_preserves_prior_on_one_under(cx: &mut TestAppContext) {
    with_text_system(cx, |text_system| {
        let source = "a\nb\nc";
        let exercise = |cap: usize| {
            let mut owner = owner(source, 16, 16, cap, 16);
            let first = start_index(&mut owner, 1);
            let first_page = page(&mut owner, first, source, 0, source.len(), 1);
            owner.admit_page(first, &first_page, text_system).unwrap();
            let prior = owner.index().unwrap().key();
            let second = start_index(&mut owner, 2);
            let second_page = page(&mut owner, second, source, 0, source.len(), 2);
            let result = owner.admit_page(second, &second_page, text_system);
            (owner, prior, second, second_page.key(), result)
        };

        let (accepted_owner, prior, _, page, accepted) = exercise(512 * 1024);
        let accepted = accepted.unwrap();
        let required = accepted.admission_required_bytes();
        assert!(accepted_owner.counts().total_bytes() < required);
        assert_eq!(accepted.release().jobs, vec![prior]);
        assert_eq!(accepted.release().pages, vec![page]);
        assert!(accepted.release().counts.publication_bytes > 0);
        assert!(accepted.release().counts.checkpoint_bytes > 0);
        assert!(accepted.release().counts.active_job_bytes > 0);

        let (_, prior, _, _, exact) = exercise(required);
        let exact = exact.unwrap();
        assert_eq!(exact.admission_required_bytes(), required);
        assert_eq!(exact.release().jobs, vec![prior]);

        let (rejected, prior, second, page, failure) = exercise(required - 1);
        let failure = failure.unwrap_err();
        assert_eq!(failure.error(), &ExactGeometryError::CapacityExceeded);
        assert_eq!(
            failure.stage(),
            gpui_text_input::ExactGeometryFailureStage::Publication
        );
        assert_eq!(failure.admission_required_bytes(), required);
        assert_terminal_failure(&failure, second, page);
        assert_eq!(rejected.index().unwrap().key(), prior);
    });
}

#[gpui::test]
fn target_and_terminal_fast_path_replacements_report_prior_publications(cx: &mut TestAppContext) {
    with_text_system(cx, |text_system| {
        let source = "a\nb";
        let mut owner = owner(source, 16, 16, 256 * 1024, 16);
        let index = start_index(&mut owner, 1);
        let index_page = page(&mut owner, index, source, 0, source.len(), 1);
        owner.admit_page(index, &index_page, text_system).unwrap();

        let first = owner
            .request_block_target(
                GeometryJobId::new(2),
                BlockTarget::new(px(0.), px(1.), px(0.)),
            )
            .unwrap();
        let first_page = page(&mut owner, first.key(), source, 0, source.len(), 2);
        owner
            .admit_page(first.key(), &first_page, text_system)
            .unwrap();
        let prior_target = owner.target().unwrap().key();

        let second = owner
            .request_block_target(
                GeometryJobId::new(3),
                BlockTarget::new(px(14.), px(1.), px(0.)),
            )
            .unwrap();
        let second_page = page(&mut owner, second.key(), source, 0, source.len(), 3);
        let replacement = owner
            .admit_page(second.key(), &second_page, text_system)
            .unwrap();
        assert_eq!(replacement.release().jobs, vec![prior_target]);
        assert!(replacement.release().counts.output_record_bytes > 0);
        assert!(replacement.release().counts.active_job_bytes > 0);
        let second_target = owner.target().unwrap().key();

        let terminal = owner
            .request_block_target(
                GeometryJobId::new(4),
                BlockTarget::new(px(140.), px(1.), px(0.)),
            )
            .unwrap();
        assert_eq!(terminal.progress(), ExactGeometryProgress::TargetComplete);
        assert_eq!(terminal.release().jobs, vec![second_target]);
        assert!(terminal.release().counts.publication_bytes > 0);
    });
}
