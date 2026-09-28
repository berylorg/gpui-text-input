use super::*;

fn pending_text(
    source: &str,
    start: usize,
    end: usize,
    bytes: usize,
    items: usize,
    text_system: &WindowTextSystem,
) -> (ExactGeometryOwner, GeometryJobKey, RangePage, ObjectPage) {
    let mut owner = owner_with_retained_items(source, 32, 64., 4, bytes, items, style()).unwrap();
    let job = start_index(&mut owner, 1);
    if start != 0 {
        let first = page(&mut owner, job, source, 0, start, 1);
        assert_eq!(
            admit_page_with_empty_objects(&mut owner, job, &first, text_system)
                .unwrap()
                .progress(),
            ExactGeometryProgress::Scanning
        );
    }
    let page = page(&mut owner, job, source, start, end, 2);
    assert_eq!(
        owner
            .admit_page(job, &page, text_system)
            .unwrap()
            .progress(),
        ExactGeometryProgress::NeedObjects
    );
    let objects = empty_object_page(&mut owner, job, &page, 2);
    (owner, job, page, objects)
}

fn growth_case(
    text_system: &WindowTextSystem,
    source: &str,
    start: usize,
    end: usize,
    growth: usize,
    refused_storage: usize,
) {
    let (probe, _, page, objects) =
        pending_text(source, start, end, 256 * 1024, usize::MAX, text_system);
    let bytes = probe.counts().total_bytes()
        + page.retained_charge().bytes()
        + objects.retained_charge().bytes()
        + growth;
    let items = probe.counts().total_items() + page.retained_charge().items() + 2;
    for (byte_cap, item_cap, success) in [
        (bytes, items, true),
        (bytes - 1, items, false),
        (bytes, items - 1, false),
    ] {
        if start != 0 && item_cap < items {
            continue;
        }
        let (mut owner, job, page, objects) =
            pending_text(source, start, end, byte_cap, item_cap, text_system);
        let result = owner.admit_object_page(job, &page, &objects, text_system);
        if success {
            let admission = result.unwrap();
            assert_eq!(admission.progress(), ExactGeometryProgress::Scanning);
            assert_eq!(admission.admission_required_bytes(), bytes);
            assert_eq!(admission.admission_required_items(), items);
            assert!(owner.counts().scan_buffer_bytes >= growth);
        } else {
            let failure = result.unwrap_err();
            assert_eq!(failure.error(), &ExactGeometryError::CapacityExceeded);
            assert_eq!(
                failure.stage(),
                gpui_text_input::ExactGeometryFailureStage::Scan
            );
            assert_eq!(
                failure.release().counts.scan_buffer_bytes,
                if byte_cap < bytes { refused_storage } else { 0 }
            );
            assert_eq!(owner.counts().active_job_items, 0);
            assert_eq!(owner.counts().scan_buffer_bytes, 0);
            assert!(owner.index().is_none());
            assert!(owner.target().is_none());
        }
    }
}

#[gpui::test]
fn cross_page_grapheme_growth_is_admitted_before_storage_changes(cx: &mut TestAppContext) {
    with_text_system(cx, |text_system| {
        growth_case(
            text_system,
            "a\u{301}\u{301}\u{301}\u{301}\u{301}z",
            0,
            9,
            9,
            0,
        );
    });
}

#[gpui::test]
fn segment_growth_charges_the_live_grapheme_before_storage_changes(cx: &mut TestAppContext) {
    with_text_system(cx, |text_system| {
        growth_case(text_system, "abcdefghijklmnop", 0, 8, 16, 8);
    });
}

#[gpui::test]
fn cross_page_growth_charges_old_and_replacement_text_storage(cx: &mut TestAppContext) {
    with_text_system(cx, |text_system| {
        let source = format!("a{}z", "\u{301}".repeat(12));
        growth_case(text_system, &source, 9, 17, 18, 9);
        growth_case(
            text_system,
            "abcdefghijklmnopqrstuvwxyz012345",
            8,
            16,
            16,
            16,
        );
    });
}

#[gpui::test]
fn text_conversion_refusal_preserves_scanner_storage_until_release(cx: &mut TestAppContext) {
    with_text_system(cx, |text_system| {
        let source = "a\nz";
        let (probe, _, page, objects) =
            pending_text(source, 0, source.len(), 256 * 1024, usize::MAX, text_system);
        let segment_capacity = 8;
        let newline_capacity = 8;
        let conversion_peak = probe.counts().total_bytes()
            + page.retained_charge().bytes()
            + objects.retained_charge().bytes()
            + segment_capacity
            + 1
            + newline_capacity
            + std::mem::size_of::<TextRun>();
        let conversion_items = probe.counts().total_items()
            + page.retained_charge().items()
            + 3;
        for (bytes, items, released_text) in [
            (conversion_peak - 1, usize::MAX, segment_capacity),
            (conversion_peak, usize::MAX, 0),
            (256 * 1024, conversion_items - 1, segment_capacity),
            (256 * 1024, conversion_items, 0),
        ] {
            let (mut owner, job, page, objects) =
                pending_text(source, 0, source.len(), bytes, items, text_system);
            let failure = owner
                .admit_object_page(job, &page, &objects, text_system)
                .unwrap_err();
            assert!(
                matches!(failure.error(), ExactGeometryError::CapacityExceeded)
                    || (released_text == 0
                        && matches!(
                            failure.error(),
                            ExactGeometryError::Layout(
                                gpui::StreamingLayoutError::CapacityExceeded(
                                    gpui::StreamingLayoutComponent::Total
                                )
                            )
                        ))
            );
            assert_eq!(
                failure.release().counts.scan_buffer_bytes,
                released_text + newline_capacity
            );
            assert_eq!(owner.counts().active_job_items, 0);
            assert!(owner.index().is_none());
        }
    });
}

#[gpui::test]
fn oversize_presentation_respects_exact_byte_and_item_capacity(cx: &mut TestAppContext) {
    with_text_system(cx, |text_system| {
        let source = format!("a{}z", "\u{301}".repeat(20));
        let mut required_bytes = 0;
        let mut required_items = 0;
        for case in 0..4 {
            let (bytes, items) = match case {
                0 => (256 * 1024, usize::MAX),
                1 => (required_bytes, required_items),
                2 => (required_bytes - 1, usize::MAX),
                _ => (256 * 1024, required_items - 1),
            };
            let run = TextRun {
                len: 3,
                font: font(".SystemUIFont"),
                color: black(),
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            let geometry_style = StreamingGeometryStyle::new(
                run.clone(),
                StreamingOversizePresentation::new(
                    SharedString::new(Arc::<str>::from("...")),
                    vec![run],
                    px(32.),
                    px(20.),
                    px(14.),
                    None,
                ),
            );
            let mut owner = owner_with_retained_items(
                &source,
                8,
                64.,
                4,
                bytes,
                items,
                geometry_style,
            )
            .unwrap();
            let job = start_index(&mut owner, 1);
            let page = page(&mut owner, job, &source, 0, source.len(), 1);
            let result = admit_page_with_empty_objects(&mut owner, job, &page, text_system);
            if case < 2 {
                let admission = result.unwrap();
                assert_eq!(admission.progress(), ExactGeometryProgress::IndexComplete);
                required_bytes = admission.admission_required_bytes();
                required_items = admission.admission_required_items();
                assert!(owner.index().is_some());
            } else {
                let failure = result.unwrap_err();
                assert_eq!(failure.error(), &ExactGeometryError::CapacityExceeded);
                assert!(owner.index().is_none());
            }
            assert_eq!(owner.counts().active_job_items, 0);
            assert_eq!(owner.counts().active_atom_bytes, 0);
            assert_eq!(owner.counts().scan_buffer_bytes, 0);
        }
    });
}

fn rollover_owner(
    source: &str,
    bytes: usize,
    text_system: &WindowTextSystem,
) -> ExactGeometryOwner {
    let mut layout = layout(768, 10000.);
    layout.limits.glyphs = 2048;
    layout.limits.maps = 2048;
    let mut owner = ExactGeometryOwner::new(
        binding(source, 1),
        PresentationGeneration::new(1),
        layout,
        style(),
        ExactGeometryLimits::new(4096, 4, bytes, 64 * 1024).unwrap(),
    )
    .unwrap();
    let job = start_index(&mut owner, 1);
    let page = page(&mut owner, job, source, 0, source.len(), 1);
    assert_eq!(
        admit_page_with_empty_objects(&mut owner, job, &page, text_system)
            .unwrap()
            .progress(),
        ExactGeometryProgress::IndexComplete
    );
    owner
}

#[gpui::test]
fn target_segment_rollover_keeps_grapheme_charged_during_layout(cx: &mut TestAppContext) {
    with_text_system(cx, |text_system| {
        let grapheme = format!("a{}", "\u{301}".repeat(256));
        let source = format!("{grapheme}{grapheme}zz");
        let mut required = 0;
        for (cap, success) in [(256 * 1024, true), (0, true), (0, false)] {
            let cap = if cap != 0 {
                cap
            } else {
                required - usize::from(!success)
            };
            let mut owner = rollover_owner(&source, cap, text_system);
            let job = owner
                .request_block_target(
                    GeometryJobId::new(2),
                    BlockTarget::new(px(0.), px(10000.), px(0.)),
                )
                .unwrap()
                .key();
            let page = page(&mut owner, job, &source, 0, source.len() - 1, 2);
            let result = admit_page_with_empty_objects(&mut owner, job, &page, text_system);
            if success {
                let admission = result.unwrap();
                assert_eq!(admission.progress(), ExactGeometryProgress::Scanning);
                required = admission.admission_required_bytes();
                assert!(owner.counts().output_payload_bytes > 0);
                assert!(owner.counts().scan_buffer_bytes >= grapheme.len());
            } else {
                let failure = result.unwrap_err();
                assert_eq!(failure.error(), &ExactGeometryError::CapacityExceeded);
                assert_eq!(
                    failure.stage(),
                    gpui_text_input::ExactGeometryFailureStage::Scan
                );
                assert_eq!(failure.release().counts.output_payload_bytes, 0);
                assert!(failure.release().counts.scan_buffer_bytes >= grapheme.len());
                assert_eq!(owner.counts().active_job_items, 0);
                assert!(owner.target().is_none());
            }
        }
    });
}
