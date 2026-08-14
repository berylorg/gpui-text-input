use super::*;

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
                    assert_eq!(checkpoint.source().get(), checkpoint.cursor_offset() as u64);
                    let source_offset = checkpoint.source().get() as usize;
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
                    checkpoint.source().get(),
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
                (2, 2, 1, 1, false),
                (4, 4, 2, 2, false),
                (5, 5, 2, 3, true),
            ]
        );
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
        for (ix, (block, expected_source)) in [
            (0., 0_u64),
            (7., 0),
            (14., 4),
            (21., 4),
            (28., 8),
            (140., 11),
        ]
        .into_iter()
        .enumerate()
        {
            let start = owner
                .request_block_target(
                    GeometryJobId::new(ix as u64 + 2),
                    BlockTarget::new(px(block), px(0.), px(0.)),
                )
                .unwrap();
            let predecessor = owner
                .index()
                .unwrap()
                .checkpoints()
                .iter()
                .rev()
                .find(|checkpoint| {
                    checkpoint.source().get() == 0 || checkpoint.resume_block_offset() <= px(block)
                })
                .unwrap()
                .source();
            if start.progress() == ExactGeometryProgress::Scanning {
                assert_eq!(
                    drive_ascii_job(
                        &mut owner,
                        text_system,
                        source,
                        start.key(),
                        predecessor.get() as usize,
                        source.len(),
                        10 + ix as u64,
                    ),
                    ExactGeometryProgress::TargetComplete
                );
            }
            assert_eq!(
                owner.target().unwrap().target_source().get(),
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
        let start = owner
            .request_block_target(
                GeometryJobId::new(2),
                BlockTarget::new(px(14.), px(14.), px(14.)),
            )
            .unwrap();
        let predecessor = owner
            .index()
            .unwrap()
            .checkpoints()
            .iter()
            .rev()
            .find(|checkpoint| {
                checkpoint.source().get() == 0 || checkpoint.resume_block_offset() <= px(14.)
            })
            .unwrap()
            .source()
            .get() as usize;
        assert_eq!(
            drive_ascii_job(
                &mut owner,
                text_system,
                source,
                start.key(),
                predecessor,
                source.len(),
                2,
            ),
            ExactGeometryProgress::TargetComplete
        );
        let target = owner.target().unwrap();
        // The 12px wrap width fits exactly two 6px test-font glyphs. The absolute target begins on
        // the second soft-wrapped visual line, so its independently known leading source is byte 2.
        assert_eq!(predecessor, 2);
        assert_eq!(target.target_source().get(), 2);
        assert_eq!(target.source_end().get(), 10);

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
            owner
                .admit_page(index, &index_page, text_system)
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
        owner
            .admit_page(start.key(), &target_page, text_system)
            .unwrap();
        assert_eq!(owner.target().unwrap().target_source().get(), 2);
    });
}

#[gpui::test]
fn sparse_gap_discards_pre_window_output_and_matches_dense_canonical_target(
    cx: &mut TestAppContext,
) {
    with_text_system(cx, |text_system| {
        let source = "x\n".repeat(1_200);
        let build = |checkpoint_cap| {
            let mut owner = owner(&source, 8, checkpoint_cap, 512 * 1024, 8);
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

        let sparse_predecessor = sparse
            .index()
            .unwrap()
            .checkpoints()
            .iter()
            .rev()
            .find(|checkpoint| {
                checkpoint.source().get() == 0
                    || checkpoint.resume_block_offset() <= target.block_offset()
            })
            .unwrap()
            .source();
        let dense_predecessor = dense
            .index()
            .unwrap()
            .checkpoints()
            .iter()
            .rev()
            .find(|checkpoint| {
                checkpoint.source().get() == 0
                    || checkpoint.resume_block_offset() <= target.block_offset()
            })
            .unwrap()
            .source();
        assert_eq!(sparse_predecessor.get(), 0);
        assert!(dense_predecessor.get() > 1_700);

        assert_eq!(
            drive_ascii_job(
                &mut sparse,
                text_system,
                &source,
                sparse_start.key(),
                sparse_predecessor.get() as usize,
                128,
                100,
            ),
            ExactGeometryProgress::TargetComplete
        );
        assert_eq!(
            drive_ascii_job(
                &mut dense,
                text_system,
                &source,
                dense_start.key(),
                dense_predecessor.get() as usize,
                128,
                100,
            ),
            ExactGeometryProgress::TargetComplete
        );
        let sparse_target = sparse.target().unwrap();
        let dense_target = dense.target().unwrap();
        assert_eq!(sparse_target.predecessor(), sparse_predecessor);
        assert_eq!(dense_target.predecessor(), dense_predecessor);
        assert_eq!(sparse_target.target_source(), dense_target.target_source());
        assert_eq!(
            fragment_facts(sparse_target.fragments()),
            fragment_facts(dense_target.fragments())
        );
        assert_eq!(sparse_target.charge(), dense_target.charge());
        assert!(sparse_target.fragments().len() <= 8);
        assert!(sparse_target.source_end().get() < source.len() as u64);
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
            .find(|checkpoint| checkpoint.source().get() == 6)
            .expect("cap flush checkpoint before split grapheme")
            .clone();
        assert_eq!(checkpoint.cursor_offset(), 6);
        assert!(!checkpoint.is_terminal());
        let target = BlockTarget::new(checkpoint.resume_block_offset(), px(28.), px(14.));
        let replay_start = replay
            .request_block_target(GeometryJobId::new(2), target)
            .unwrap();
        let origin_start = origin
            .request_block_target(GeometryJobId::new(2), target)
            .unwrap();
        let replay_predecessor = replay
            .index()
            .unwrap()
            .checkpoints()
            .iter()
            .rev()
            .find(|candidate| {
                candidate.source().get() == 0
                    || candidate.resume_block_offset() <= target.block_offset()
            })
            .unwrap()
            .source();
        assert_eq!(replay_predecessor, checkpoint.source());
        assert_eq!(origin.index().unwrap().checkpoints().len(), 2);

        for (owner, start, predecessor, first_id) in [
            (&mut replay, replay_start, replay_predecessor, 20),
            (&mut origin, origin_start, ByteOffset::new(0), 30),
        ] {
            let job = start.key();
            let mut page_start = predecessor.get() as usize;
            let first_end = 7;
            let first = page(owner, job, source, page_start, first_end, first_id);
            let mut progress = owner.admit_page(job, &first, text_system).unwrap();
            page_start = first_end;
            let mut id = first_id + 1;
            while progress.progress() == ExactGeometryProgress::Scanning {
                let end = page_start.saturating_add(8).min(source.len());
                let next = page(owner, job, source, page_start, end, id);
                progress = owner.admit_page(job, &next, text_system).unwrap();
                page_start = end;
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
            owner.admit_page(index_job, &next, text_system).unwrap();
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
        let predecessor = owner
            .index()
            .unwrap()
            .checkpoints()
            .iter()
            .rev()
            .find(|checkpoint| {
                checkpoint.source().get() == 0
                    || checkpoint.resume_block_offset() <= target.block_offset()
            })
            .unwrap()
            .source();
        assert_eq!(
            drive_ascii_job(
                &mut owner,
                text_system,
                &source,
                target_start.key(),
                predecessor.get() as usize,
                128,
                id,
            ),
            ExactGeometryProgress::TargetComplete
        );
        assert!(owner.target().unwrap().fragments().len() <= 8);
        assert!(owner.target().unwrap().source_end().get() < source.len() as u64);
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
            owner.admit_page(job, &next, text_system).unwrap();
            start = end;
        }
        let index = owner.index().unwrap();
        assert_eq!(index.checkpoints().first().unwrap().source().get(), 0);
        assert_eq!(
            index.checkpoints().last().unwrap().source().get(),
            source.len() as u64
        );
        assert!(
            index
                .checkpoints()
                .iter()
                .all(|checkpoint| checkpoint.source().get() == checkpoint.cursor_offset() as u64)
        );
        assert_eq!(owner.counts().active_atom_bytes, 0);
    });
}
