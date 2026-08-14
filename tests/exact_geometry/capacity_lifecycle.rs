use std::mem::size_of;

use super::*;

#[gpui::test]
fn empty_source_completes_origin_and_terminal_without_page(cx: &mut TestAppContext) {
    with_text_system(cx, |_| {
        let mut owner = owner("", 8, 4, 256 * 1024, 8);
        let start = owner.start_index(GeometryJobId::new(1)).unwrap();
        assert_eq!(start.progress(), ExactGeometryProgress::IndexComplete);
        let index = owner.index().unwrap();
        assert_eq!(index.aggregate().visual_lines(), 0);
        assert_eq!(index.aggregate().content_height(), px(0.));
        assert_eq!(index.checkpoints().len(), 2);
        assert_eq!(index.checkpoints()[0].source().get(), 0);
        assert!(!index.checkpoints()[0].is_terminal());
        assert!(index.checkpoints()[1].is_terminal());
        assert_eq!(index.checkpoints()[1].cursor_offset(), 0);
        assert_eq!(
            owner.request_page(start.key(), PageRequestId::new(1)),
            Err(ExactGeometryError::ObsoleteJob(start.key()))
        );
    });
}

#[gpui::test]
fn construction_accounts_style_collections_features_fallbacks_and_direct_exact_cap(
    cx: &mut TestAppContext,
) {
    with_text_system(cx, |_| {
        let base_font = font("AccountingFamily");
        let base_style = style_with_font(base_font, "oversize");
        let base = owner_with("abc", 16, 64., 4, 256 * 1024, 8, base_style).unwrap();

        let mut rich_font = font("AccountingFamily");
        rich_font.features = FontFeatures(Arc::new(vec![
            ("liga".to_owned(), 1),
            ("calt".to_owned(), 0),
        ]));
        rich_font.fallbacks = Some(FontFallbacks::from_fonts(vec![
            "Fallback One".to_owned(),
            "Fallback Two".to_owned(),
        ]));
        let rich_style = style_with_font(rich_font.clone(), "oversize");
        let rich = owner_with("abc", 16, 64., 4, 256 * 1024, 8, rich_style.clone()).unwrap();
        let per_run_delta = 2 * (size_of::<(String, u32)>() + 4)
            + size_of::<Vec<String>>()
            + size_of::<String>()
            + "Fallback One".len()
            + size_of::<String>()
            + "Fallback Two".len();
        assert_eq!(
            rich.counts().input_bytes - base.counts().input_bytes,
            per_run_delta * 2,
        );

        let required = rich.counts().total_bytes();
        let exact = owner_with("abc", 16, 64., 4, required, 8, rich_style.clone()).unwrap();
        assert_eq!(exact.counts().total_bytes(), required);
        assert!(matches!(
            owner_with("abc", 16, 64., 4, required - 1, 8, rich_style),
            Err(ExactGeometryError::CapacityExceeded)
        ));

        let rich_style = style_with_font(rich_font, "oversize");
        let item_probe = owner_with_retained_items(
            "abc",
            16,
            64.,
            4,
            256 * 1024,
            usize::MAX,
            rich_style.clone(),
        )
        .unwrap();
        let required_items = item_probe.counts().total_items();
        let exact_items = owner_with_retained_items(
            "abc",
            16,
            64.,
            4,
            256 * 1024,
            required_items,
            rich_style.clone(),
        )
        .unwrap();
        assert_eq!(exact_items.counts().total_items(), required_items);
        assert!(matches!(
            owner_with_retained_items(
                "abc",
                16,
                64.,
                4,
                256 * 1024,
                required_items - 1,
                rich_style,
            ),
            Err(ExactGeometryError::CapacityExceeded)
        ));
    });
}

#[gpui::test]
fn layout_replacement_accounts_old_and_candidate_inputs_concurrently(cx: &mut TestAppContext) {
    with_text_system(cx, |_| {
        let source = "abc";
        let old_style = style_with_font(font("OldFamily"), "old");
        let mut new_font = font("NewFamily");
        new_font.features = FontFeatures(Arc::new(vec![("liga".to_owned(), 1)]));
        new_font.fallbacks = Some(FontFallbacks::from_fonts(vec!["Fallback".to_owned()]));
        let new_style = style_with_font(new_font, "new presentation");

        let mut probe = owner_with(source, 16, 64., 4, 256 * 1024, 8, old_style.clone()).unwrap();
        let next_layout = layout(32, 32.);
        let required = probe
            .set_layout_required_bytes(&next_layout, &new_style)
            .unwrap();
        probe.set_layout(next_layout, new_style.clone()).unwrap();

        let mut exact = owner_with(source, 16, 64., 4, required, 8, old_style.clone()).unwrap();
        let release = exact
            .set_layout(layout(32, 32.), new_style.clone())
            .unwrap();
        assert_eq!(exact.retained_high_water_bytes(), required);
        assert!(release.counts.input_bytes > 0);

        let mut rejected = owner_with(source, 16, 64., 4, required - 1, 8, old_style).unwrap();
        let prior_key = rejected.key();
        let prior_counts = rejected.counts();
        assert_eq!(
            rejected.set_layout(layout(32, 32.), new_style),
            Err(ExactGeometryError::CapacityExceeded)
        );
        assert_eq!(rejected.key(), prior_key);
        assert_eq!(rejected.counts(), prior_counts);
    });
}

#[gpui::test]
fn desired_target_identity_replacement_cancel_epoch_and_dispose_are_observable(
    cx: &mut TestAppContext,
) {
    with_text_system(cx, |text_system| {
        let source = "abcdefghij".repeat(20);
        let mut owner = owner(source.as_str(), 8, 4, 256 * 1024, 8);
        let first = owner
            .request_block_target(
                GeometryJobId::new(1),
                BlockTarget::new(px(14.), px(28.), px(14.)),
            )
            .unwrap();
        assert_eq!(first.progress(), ExactGeometryProgress::PendingIndex);
        assert_eq!(owner.desired_target_key(), Some(first.key()));
        let replacement = owner
            .request_block_target(
                GeometryJobId::new(2),
                BlockTarget::new(px(28.), px(28.), px(14.)),
            )
            .unwrap();
        assert_eq!(replacement.release().jobs, vec![first.key()]);
        assert!(replacement.release().counts.desired_target_bytes > 0);
        let cancelled = owner.cancel(replacement.key()).unwrap();
        assert_eq!(cancelled.jobs, vec![replacement.key()]);
        assert_eq!(owner.desired_target_key(), None);

        let pending = owner
            .request_block_target(
                GeometryJobId::new(3),
                BlockTarget::new(px(42.), px(28.), px(14.)),
            )
            .unwrap();
        let index = owner.start_index(GeometryJobId::new(4)).unwrap();
        assert_eq!(index.progress(), ExactGeometryProgress::Scanning);
        assert_eq!(
            drive_ascii_job(
                &mut owner,
                text_system,
                source.as_str(),
                index.key(),
                0,
                128,
                1,
            ),
            ExactGeometryProgress::IndexComplete
        );
        let resumed = owner.start_pending_target().unwrap();
        assert_eq!(resumed.key(), pending.key());
        assert_eq!(resumed.progress(), ExactGeometryProgress::Scanning);
        owner.cancel(resumed.key()).unwrap();
        let later = owner.start_index(GeometryJobId::new(5)).unwrap();
        owner.cancel(later.key()).unwrap();

        let old_key = owner.key();
        let release = owner
            .rebind(binding(source.as_str(), 2))
            .expect("rebind releases old publications");
        assert!(release.jobs.contains(&index.key()));
        assert_ne!(owner.key(), old_key);
        assert_eq!(owner.key().epoch().get(), old_key.epoch().get() + 1);

        let pending_after_rebind = owner
            .request_block_target(
                GeometryJobId::new(6),
                BlockTarget::new(px(14.), px(28.), px(14.)),
            )
            .unwrap();
        let release = owner
            .set_layout(layout(6, 32.), style())
            .expect("epoch replacement releases desired target");
        assert!(release.jobs.contains(&pending_after_rebind.key()));
        assert_eq!(owner.desired_target_key(), None);

        let pending_before_dispose = owner
            .request_block_target(
                GeometryJobId::new(7),
                BlockTarget::new(px(14.), px(28.), px(14.)),
            )
            .unwrap();
        let release = owner.dispose();
        assert!(release.jobs.contains(&pending_before_dispose.key()));
        assert!(release.counts.input_bytes > 0);
        assert_eq!(owner.counts().input_bytes, 0);
        assert_eq!(owner.counts().total_bytes(), owner.counts().owner_bytes);
        assert!(matches!(
            owner.start_index(GeometryJobId::new(8)),
            Err(ExactGeometryError::Disposed)
        ));
    });
}

#[gpui::test]
fn cancellation_failure_late_pages_dispose_and_drop_release_owned_state(cx: &mut TestAppContext) {
    with_text_system(cx, |text_system| {
        let source = "abcdefghij";
        let mut owner = owner(source, 4, 4, 256 * 1024, 8);
        let base = owner.counts();
        let cancelled = start_index(&mut owner, 1);
        let request = owner
            .request_page(cancelled, PageRequestId::new(1))
            .unwrap();
        let late = RangePage::new(
            PageId::new(1),
            request.key(),
            ByteRange::from_u64(0, 5).unwrap(),
            source[..5].to_owned(),
            vec![],
            PageEdgeFact::DocumentBoundary,
            PageEdgeFact::Continues,
            false,
        )
        .unwrap();
        let active_counts = owner.counts();
        let release = owner.cancel(cancelled).unwrap();
        assert_eq!(release.pages, vec![request.key()]);
        assert_eq!(
            release.counts.total_bytes(),
            active_counts.total_bytes() - base.total_bytes()
        );
        assert_eq!(owner.counts(), base);
        let late_failure = owner.admit_page(cancelled, &late, text_system).unwrap_err();
        assert_eq!(
            late_failure.error(),
            &ExactGeometryError::ObsoleteJob(cancelled)
        );
        assert_eq!(late_failure.release(), &ExactGeometryRelease::default());

        let failed = start_index(&mut owner, 2);
        let request = owner.request_page(failed, PageRequestId::new(2)).unwrap();
        let release = owner.fail_page(failed, request.key()).unwrap();
        assert_eq!(release.jobs, vec![failed]);
        assert_eq!(release.pages, vec![request.key()]);
        assert_eq!(owner.counts(), base);

        let active = start_index(&mut owner, 3);
        let request = owner.request_page(active, PageRequestId::new(3)).unwrap();
        let release = owner.dispose();
        assert!(release.jobs.contains(&active));
        assert_eq!(release.pages, vec![request.key()]);
        assert_eq!(owner.counts().input_bytes, 0);

        let feature_records = Arc::new(vec![("liga".to_owned(), 1)]);
        let baseline_refs = Arc::strong_count(&feature_records);
        let mut tracked_font = font("TrackedFamily");
        tracked_font.features = FontFeatures(feature_records.clone());
        let tracked_style = style_with_font(tracked_font, "");
        {
            let dropped = owner_with(source, 8, 24., 4, 256 * 1024, 8, tracked_style).unwrap();
            assert!(dropped.counts().input_bytes > 0);
            assert_eq!(Arc::strong_count(&feature_records), baseline_refs + 1);
        }
        assert_eq!(Arc::strong_count(&feature_records), baseline_refs);
    });
}

#[gpui::test]
fn over_extent_page_is_a_terminal_source_contract_failure(cx: &mut TestAppContext) {
    with_text_system(cx, |text_system| {
        let mut owner = owner("a", 8, 4, 256 * 1024, 8);
        let base = owner.counts();
        let job = start_index(&mut owner, 1);
        let request = owner.request_page(job, PageRequestId::new(1)).unwrap();
        let active = owner.counts();
        let malformed = RangePage::new(
            PageId::new(1),
            request.key(),
            ByteRange::from_u64(0, 2).unwrap(),
            "a\n".to_owned(),
            vec![],
            PageEdgeFact::DocumentBoundary,
            PageEdgeFact::Continues,
            false,
        )
        .unwrap();

        let failure = owner.admit_page(job, &malformed, text_system).unwrap_err();
        assert_eq!(failure.error(), &ExactGeometryError::SourceContract);
        assert_eq!(
            failure.stage(),
            gpui_text_input::ExactGeometryFailureStage::PageCoexistence
        );
        assert_eq!(failure.release().jobs, vec![job]);
        assert_eq!(failure.release().pages, vec![request.key()]);
        assert_eq!(
            failure.release().counts.active_job_bytes,
            active.active_job_bytes
        );
        assert_eq!(
            failure.release().counts.pending_page_bytes,
            active.pending_page_bytes
        );
        assert_eq!(owner.counts(), base);
        assert!(owner.index().is_none());
        assert!(owner.target().is_none());
        assert_eq!(owner.desired_target_key(), None);
        assert_eq!(owner.estimate(), None);
        assert_eq!(
            owner.request_page(job, PageRequestId::new(2)),
            Err(ExactGeometryError::ObsoleteJob(job))
        );
    });
}
