use super::*;

fn next_ascii_demand_page(
    owner: &mut ExactGeometryOwner,
    job: GeometryJobKey,
    source: &str,
    page_bytes: usize,
    id: u64,
) -> (RangePage, usize) {
    let request = owner.request_page(job, PageRequestId::new(id)).unwrap();
    let gpui_text_input::PageDemandEnvelope::Adjacent {
        anchor,
        direction: gpui_text_input::PageDirection::Forward,
        max_payload_bytes,
    } = request.key().demand()
    else {
        panic!("canonical target issued a non-forward page demand")
    };
    let start = anchor.get() as usize;
    let end = start
        .saturating_add(page_bytes.min(max_payload_bytes as usize))
        .min(source.len());
    let range = ByteRange::from_u64(start as u64, end as u64).unwrap();
    let page = RangePage::new(
        PageId::new(id),
        request.key(),
        range,
        source[start..end].to_owned(),
        vec![],
        if start == 0 {
            PageEdgeFact::DocumentBoundary
        } else {
            PageEdgeFact::Continues
        },
        if end == source.len() {
            PageEdgeFact::DocumentBoundary
        } else {
            PageEdgeFact::Continues
        },
        end == source.len(),
    )
    .unwrap();
    (page, start)
}

fn drive_ascii_job_from_demands(
    owner: &mut ExactGeometryOwner,
    text_system: &WindowTextSystem,
    source: &str,
    job: GeometryJobKey,
    page_bytes: usize,
    first_request_id: u64,
) -> (ExactGeometryProgress, usize) {
    let mut request_id = first_request_id;
    let mut first_start = None;
    loop {
        let (page, start) = next_ascii_demand_page(owner, job, source, page_bytes, request_id);
        first_start.get_or_insert(start);
        let admission = admit_page_with_empty_objects(owner, job, &page, text_system).unwrap();
        if admission.progress() != ExactGeometryProgress::Scanning {
            return (admission.progress(), first_start.unwrap());
        }
        request_id += 1;
    }
}

#[gpui::test]
fn canonical_partitions_preserve_aggregates_and_every_checkpoint_cursor(cx: &mut TestAppContext) {
    with_text_system(cx, |text_system| {
        let source = "a\u{301}bc\ndefghijklmnop\nq";
        let whole = scan_index(text_system, source, &[source.len()], 5, 8, 256 * 1024, 32);
        let split = scan_index(
            text_system,
            source,
            &[1, 3, 6, 10, 15, source.len()],
            5,
            8,
            256 * 1024,
            32,
        );
        let left = whole.index().unwrap();
        let right = split.index().unwrap();
        assert_eq!(left.aggregate(), right.aggregate());
        let facts = |owner: &ExactGeometryOwner| {
            owner
                .index()
                .unwrap()
                .checkpoints()
                .iter()
                .map(|checkpoint| {
                    assert_eq!(
                        checkpoint.source().byte_offset.get(),
                        checkpoint.cursor_offset() as u64
                    );
                    let source_offset = checkpoint.source().byte_offset.get() as usize;
                    assert_eq!(
                        checkpoint.logical_line(),
                        source[..source_offset].matches('\n').count() as u64,
                    );
                    (
                        checkpoint.source(),
                        checkpoint.visual_lines(),
                        checkpoint.logical_line(),
                        checkpoint.segment(),
                        checkpoint.is_terminal(),
                    )
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(facts(&whole), facts(&split));
    });
}

#[gpui::test]
fn newline_checkpoints_publish_post_newline_line_and_segment_state(cx: &mut TestAppContext) {
    with_text_system(cx, |text_system| {
        let source = "a\nb\nc";
        let owner = scan_index(
            text_system,
            source,
            &[1, 2, 3, 4, source.len()],
            16,
            16,
            256 * 1024,
            16,
        );
        let facts = owner
            .index()
            .unwrap()
            .checkpoints()
            .iter()
            .map(|checkpoint| {
                (
                    checkpoint.source().byte_offset.get(),
                    checkpoint.cursor_offset() as u64,
                    checkpoint.logical_line(),
                    checkpoint.segment(),
                    checkpoint.is_terminal(),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            facts,
            vec![
                (0, 0, 0, 0, false),
                (2, 2, 1, 2, false),
                (4, 4, 2, 4, false),
                (5, 5, 2, 6, true),
            ]
        );
    });
}

#[gpui::test]
fn ordinary_target_retains_explicit_composite_line_and_source_boundaries(cx: &mut TestAppContext) {
    with_text_system(cx, |text_system| {
        let source = "a\nb";
        let mut owner = scan_index(text_system, source, &[source.len()], 16, 16, 256 * 1024, 16);
        let target = owner
            .request_block_target(
                GeometryJobId::new(2),
                BlockTarget::new(px(0.), px(100.), px(0.)),
            )
            .unwrap();
        assert_eq!(target.progress(), ExactGeometryProgress::Scanning);
        assert_eq!(
            drive_ascii_job(
                &mut owner,
                text_system,
                source,
                target.key(),
                0,
                source.len(),
                2,
            ),
            ExactGeometryProgress::TargetComplete
        );
        let fragments = owner.target().unwrap().fragments();
        let boundaries = fragments
            .iter()
            .filter_map(|fragment| match fragment {
                StreamingLayoutFragment::Boundary(fragment) => Some(fragment),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(boundaries.len(), 2);
        assert_eq!(boundaries[0].kind, StreamingBoundaryKind::LogicalLine);
        assert_eq!(boundaries[1].kind, StreamingBoundaryKind::EndOfSource);
        assert!(fragments.iter().all(|fragment| {
            let maps = match fragment {
                StreamingLayoutFragment::Text(fragment) => fragment.maps(),
                StreamingLayoutFragment::OversizeAtom(fragment) => fragment.maps(),
                StreamingLayoutFragment::Boundary(fragment) => fragment.maps(),
                StreamingLayoutFragment::InlineObject(_) => return false,
            };
            maps.iter().all(|map| {
                map.logical_position
                    == StreamingLayoutPosition::at(map.logical_position.byte_offset)
            })
        }));
    });
}

#[gpui::test]
fn target_resolves_independently_expected_leading_visual_line_sources(cx: &mut TestAppContext) {
    with_text_system(cx, |text_system| {
        let source = "abc\ndef\nghi";
        let mut owner = owner_with(source, 32, 512., 16, 256 * 1024, 16, style()).unwrap();
        let index = start_index(&mut owner, 1);
        assert_eq!(
            drive_ascii_job(&mut owner, text_system, source, index, 0, source.len(), 1),
            ExactGeometryProgress::IndexComplete
        );
        for (ix, (block, expected_source, expected_predecessor)) in [
            (0., 0_u64, 0_u64),
            (7., 0, 0),
            (14., 4, 0),
            (21., 4, 0),
            (28., 8, 4),
            (140., 11, 11),
        ]
        .into_iter()
        .enumerate()
        {
            let target = BlockTarget::new(px(block), px(0.), px(0.));
            let start = owner
                .request_block_target(GeometryJobId::new(ix as u64 + 2), target)
                .unwrap();
            if start.progress() == ExactGeometryProgress::Scanning {
                let (progress, demand_start) = drive_ascii_job_from_demands(
                    &mut owner,
                    text_system,
                    source,
                    start.key(),
                    source.len(),
                    10 + ix as u64,
                );
                assert_eq!(progress, ExactGeometryProgress::TargetComplete);
                assert_eq!(demand_start as u64, expected_predecessor);
            }
            assert_eq!(
                owner.target().unwrap().predecessor().byte_offset.get(),
                expected_predecessor
            );
            assert_eq!(
                owner.target().unwrap().target_source().byte_offset.get(),
                expected_source
            );
        }
    });
}

#[gpui::test]
fn soft_wrap_target_has_literal_leading_anchor_and_viewport_overscan_maps(cx: &mut TestAppContext) {
    with_text_system(cx, |text_system| {
        let source = "abcdefghij";
        let mut owner = owner_with(source, 2, 12., 16, 256 * 1024, 16, style()).unwrap();
        let index = start_index(&mut owner, 1);
        assert_eq!(
            drive_ascii_job(&mut owner, text_system, source, index, 0, source.len(), 1),
            ExactGeometryProgress::IndexComplete
        );
        let requested_target = BlockTarget::new(px(14.), px(14.), px(14.));
        let start = owner
            .request_block_target(GeometryJobId::new(2), requested_target)
            .unwrap();
        let (progress, predecessor) = drive_ascii_job_from_demands(
            &mut owner,
            text_system,
            source,
            start.key(),
            source.len(),
            2,
        );
        assert_eq!(progress, ExactGeometryProgress::TargetComplete);
        let target = owner.target().unwrap();
        // The 12px wrap width fits exactly two 6px test-font glyphs. The absolute target begins on
        // the second soft-wrapped visual line, so its independently known leading source is byte 2.
        assert_eq!(predecessor, 0);
        assert_eq!(target.target_source().byte_offset.get(), 2);
        assert_eq!(target.source_end().byte_offset.get(), 10);

        // The viewport is [14, 28) and its overscan is [28, 42). Only canonical segments 2..4 and
        // 4..6 intersect that literal block window; the following 6..8 segment starts at 42px and
        // must not be retained. These source/map facts are fixed expectations, not a second owner.
        assert_eq!(
            fragment_facts(target.fragments()),
            vec![
                (
                    2..4,
                    vec![
                        (2, 0_f32.to_bits(), 14_f32.to_bits()),
                        (3, 6_f32.to_bits(), 14_f32.to_bits()),
                        (4, 12_f32.to_bits(), 14_f32.to_bits()),
                    ],
                ),
                (
                    4..6,
                    vec![
                        (4, 0_f32.to_bits(), 28_f32.to_bits()),
                        (5, 6_f32.to_bits(), 28_f32.to_bits()),
                        (6, 12_f32.to_bits(), 28_f32.to_bits()),
                    ],
                ),
            ]
        );
    });
}

#[gpui::test]
fn anchors_before_and_after_nonzero_target_preserve_requested_output_window(
    cx: &mut TestAppContext,
) {
    with_text_system(cx, |text_system| {
        let source = "a\nb\nc\nd\ne\nf\ng\nh\n";
        let requested = BlockTarget::new(px(1.), px(10.), px(0.));
        let run = |anchor: Option<SourcePosition>| {
            let mut owner = scan_index(text_system, source, &[source.len()], 2, 2, 256 * 1024, 16);
            let start = match anchor {
                Some(anchor) => owner
                    .request_block_target_anchored(GeometryJobId::new(2), requested, anchor)
                    .unwrap(),
                None => owner
                    .request_block_target(GeometryJobId::new(2), requested)
                    .unwrap(),
            };
            assert_eq!(
                drive_ascii_job(&mut owner, text_system, source, start.key(), 0, 2, 10),
                ExactGeometryProgress::TargetComplete,
            );
            let publication = owner.target().unwrap();
            (
                publication.target_source(),
                publication.source_end(),
                fragment_facts(publication.fragments()),
            )
        };

        let ordinary = run(None);
        let before = run(Some(SourcePosition::new(
            ByteOffset::new(0),
            InlineObjectGap::NoObjects,
        )));
        let after = run(Some(SourcePosition::new(
            ByteOffset::new(source.len() as u64),
            InlineObjectGap::NoObjects,
        )));

        assert_eq!(before.0, ordinary.0);
        assert_eq!(before.1, ordinary.1);
        assert_eq!(before.2, ordinary.2);
        assert_eq!(after.0, ordinary.0);
        assert_eq!(after.2, ordinary.2);
        assert!(after.1.byte_offset > ordinary.1.byte_offset);
        assert!(!ordinary.2.is_empty());
    });
}

#[gpui::test]
fn target_inside_tall_atom_line_resolves_that_lines_leading_source(cx: &mut TestAppContext) {
    with_text_system(cx, |text_system| {
        let source = "a\nXXXX\nb";
        let font = font(".SystemUIFont");
        let text_run = TextRun {
            len: 0,
            font: font.clone(),
            color: black(),
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let style = StreamingGeometryStyle::new(
            text_run,
            StreamingOversizePresentation::new(
                SharedString::new_static(""),
                vec![],
                px(12.),
                px(42.),
                px(0.),
                None,
            ),
        );
        let atom_range = ByteRange::from_u64(2, 6).unwrap();
        let mut owner = owner_with(source, 16, 512., 16, 256 * 1024, 16, style).unwrap();
        let index = start_index(&mut owner, 1);
        let index_page = page_with_atoms(
            &mut owner,
            index,
            source,
            0,
            source.len(),
            1,
            vec![AtomFact::new(
                AtomId::new(1),
                atom_range,
                atom_range,
                "atom",
            )],
        );
        assert_eq!(
            admit_page_with_empty_objects(&mut owner, index, &index_page, text_system)
                .unwrap()
                .progress(),
            ExactGeometryProgress::IndexComplete
        );
        let start = owner
            .request_block_target(
                GeometryJobId::new(2),
                BlockTarget::new(px(28.), px(1.), px(0.)),
            )
            .unwrap();
        let target_page = page_with_atoms(
            &mut owner,
            start.key(),
            source,
            2,
            source.len(),
            2,
            vec![AtomFact::new(
                AtomId::new(1),
                atom_range,
                atom_range,
                "atom",
            )],
        );
        admit_page_with_empty_objects(&mut owner, start.key(), &target_page, text_system).unwrap();
        assert_eq!(owner.target().unwrap().target_source().byte_offset.get(), 2);
    });
}

#[gpui::test]
fn sparse_gap_discards_pre_window_output_and_matches_dense_canonical_target(
    cx: &mut TestAppContext,
) {
    with_text_system(cx, |text_system| {
        let source = "x\n".repeat(1_200);
        let dense_replacement_cap = 4 * 1024 * 1024;
        let build = |checkpoint_cap| {
            let mut owner = owner(&source, 8, checkpoint_cap, dense_replacement_cap, 8);
            let job = start_index(&mut owner, 1);
            assert_eq!(
                drive_ascii_job(&mut owner, text_system, &source, job, 0, 128, 1),
                ExactGeometryProgress::IndexComplete
            );
            owner
        };
        let mut sparse = build(3);
        let mut dense = build(1_500);
        let target = BlockTarget::new(px(14. * 900.), px(28.), px(14.));

        let sparse_start = sparse
            .request_block_target(GeometryJobId::new(2), target)
            .unwrap();
        let dense_start = dense
            .request_block_target(GeometryJobId::new(2), target)
            .unwrap();
        assert_eq!(sparse_start.progress(), ExactGeometryProgress::Scanning);
        assert_eq!(dense_start.progress(), ExactGeometryProgress::Scanning);

        let (sparse_progress, sparse_predecessor) = drive_ascii_job_from_demands(
            &mut sparse,
            text_system,
            &source,
            sparse_start.key(),
            128,
            100,
        );
        assert_eq!(sparse_progress, ExactGeometryProgress::TargetComplete);
        let (dense_progress, dense_predecessor) = drive_ascii_job_from_demands(
            &mut dense,
            text_system,
            &source,
            dense_start.key(),
            128,
            100,
        );
        assert_eq!(dense_progress, ExactGeometryProgress::TargetComplete);
        assert_eq!(sparse_predecessor, 0);
        assert!(dense_predecessor > 1_700);
        let sparse_target = sparse.target().unwrap();
        let dense_target = dense.target().unwrap();
        assert_eq!(sparse_target.predecessor().byte_offset.get(), 0);
        assert_eq!(
            dense_target.predecessor().byte_offset.get(),
            dense_predecessor as u64
        );
        assert_eq!(sparse_target.target_source(), dense_target.target_source());
        assert_eq!(
            fragment_facts(sparse_target.fragments()),
            fragment_facts(dense_target.fragments())
        );
        assert_eq!(sparse_target.charge(), dense_target.charge());
        assert!(sparse_target.fragments().len() <= 8);
        assert!(sparse_target.source_end().byte_offset.get() < source.len() as u64);
    });
}

#[gpui::test]
fn checkpoint_replay_matches_origin_across_cap_flush_and_split_grapheme(cx: &mut TestAppContext) {
    with_text_system(cx, |text_system| {
        let source = "abcdefe\u{301}ghijklmnopqrstuvwxyz\nq";
        let build = |checkpoint_cap| {
            scan_index(
                text_system,
                source,
                &[7, 9, source.len()],
                6,
                checkpoint_cap,
                256 * 1024,
                16,
            )
        };
        let mut replay = build(16);
        let mut origin = build(2);
        let checkpoint = replay
            .index()
            .unwrap()
            .checkpoints()
            .iter()
            .find(|checkpoint| checkpoint.source().byte_offset.get() == 6)
            .expect("cap flush checkpoint before split grapheme")
            .clone();
        assert_eq!(checkpoint.cursor_offset(), 6);
        assert!(!checkpoint.is_terminal());
        let target = BlockTarget::new(checkpoint.resume_block_offset() + px(14.), px(28.), px(14.));
        let replay_start = replay
            .request_block_target(GeometryJobId::new(2), target)
            .unwrap();
        let origin_start = origin
            .request_block_target(GeometryJobId::new(2), target)
            .unwrap();
        assert_eq!(origin.index().unwrap().checkpoints().len(), 2);

        for (owner, start, expected_start, first_id) in [
            (&mut replay, replay_start, 6_usize, 20),
            (&mut origin, origin_start, 0, 30),
        ] {
            let job = start.key();
            let (first, page_start) = next_ascii_demand_page(
                owner,
                job,
                source,
                7_usize.saturating_sub(expected_start),
                first_id,
            );
            assert_eq!(page_start, expected_start);
            let mut progress =
                admit_page_with_empty_objects(owner, job, &first, text_system).unwrap();
            let mut id = first_id + 1;
            while progress.progress() == ExactGeometryProgress::Scanning {
                let (next, _) = next_ascii_demand_page(owner, job, source, 8, id);
                progress = admit_page_with_empty_objects(owner, job, &next, text_system).unwrap();
                id += 1;
            }
            assert_eq!(progress.progress(), ExactGeometryProgress::TargetComplete);
        }
        assert_eq!(
            replay.target().unwrap().target_source(),
            origin.target().unwrap().target_source()
        );
        assert_eq!(
            fragment_facts(replay.target().unwrap().fragments()),
            fragment_facts(origin.target().unwrap().fragments())
        );
    });
}

#[gpui::test]
fn arbitrarily_long_logical_line_keeps_fixed_scan_and_target_residency(cx: &mut TestAppContext) {
    with_text_system(cx, |text_system| {
        let source = "abcdefghij".repeat(2_000);
        let mut owner = owner(&source, 8, 4, 512 * 1024, 8);
        let index_job = start_index(&mut owner, 1);
        let mut start = 0;
        let mut id = 1;
        let mut peak_scan = 0;
        while start < source.len() {
            let end = (start + 128).min(source.len());
            let next = page(&mut owner, index_job, &source, start, end, id);
            admit_page_with_empty_objects(&mut owner, index_job, &next, text_system).unwrap();
            peak_scan = peak_scan.max(owner.counts().scan_buffer_bytes);
            start = end;
            id += 1;
        }
        assert!(peak_scan <= 16);
        assert_eq!(owner.index().unwrap().checkpoints().len(), 4);
        assert_eq!(owner.counts().output_items, 0);

        let target = BlockTarget::new(px(14. * 1_000.), px(28.), px(14.));
        let target_start = owner
            .request_block_target(GeometryJobId::new(2), target)
            .unwrap();
        let (progress, predecessor) = drive_ascii_job_from_demands(
            &mut owner,
            text_system,
            &source,
            target_start.key(),
            128,
            id,
        );
        assert_eq!(progress, ExactGeometryProgress::TargetComplete);
        assert_eq!(
            owner.target().unwrap().predecessor().byte_offset.get(),
            predecessor as u64
        );
        assert!(owner.target().unwrap().fragments().len() <= 8);
        assert!(owner.target().unwrap().source_end().byte_offset.get() < source.len() as u64);
    });
}

#[gpui::test]
fn oversized_grapheme_and_cross_page_atom_remain_compact_exact_ranges(cx: &mut TestAppContext) {
    with_text_system(cx, |text_system| {
        let source = format!("x{}abcdefy", "\u{301}".repeat(20));
        let atom_start = 41_u64;
        let atom_end = 47_u64;
        let atom_range = ByteRange::from_u64(atom_start, atom_end).unwrap();
        let mut owner = owner(&source, 8, 4, 256 * 1024, 16);
        let job = start_index(&mut owner, 1);
        let partitions = [15usize, 31, 44, source.len()];
        let mut start = 0;
        for (ix, end) in partitions.into_iter().enumerate() {
            let page_range = ByteRange::from_u64(start as u64, end as u64).unwrap();
            let atoms = atom_range
                .intersection(page_range)
                .filter(|range| !range.is_empty())
                .map(|fragment| {
                    vec![AtomFact::new(
                        AtomId::new(8),
                        atom_range,
                        fragment,
                        "bounded fallback",
                    )]
                })
                .unwrap_or_default();
            let next = page_with_atoms(&mut owner, job, &source, start, end, ix as u64 + 1, atoms);
            admit_page_with_empty_objects(&mut owner, job, &next, text_system).unwrap();
            start = end;
        }
        let index = owner.index().unwrap();
        assert_eq!(
            index
                .checkpoints()
                .first()
                .unwrap()
                .source()
                .byte_offset
                .get(),
            0
        );
        assert_eq!(
            index
                .checkpoints()
                .last()
                .unwrap()
                .source()
                .byte_offset
                .get(),
            source.len() as u64
        );
        assert!(
            index
                .checkpoints()
                .iter()
                .all(|checkpoint| checkpoint.source().byte_offset.get()
                    == checkpoint.cursor_offset() as u64)
        );
        assert_eq!(owner.counts().active_atom_bytes, 0);
    });
}
