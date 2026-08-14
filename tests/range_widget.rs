use std::sync::Arc;

use gpui::{
    EntityInputHandler, Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, SharedString,
    StreamingLayoutBinding, StreamingLayoutLimits, TextRun, black, font, point, px,
};
use gpui_scrollbar::ScrollbarStyle;
use gpui_text_input::{
    BindingId, ByteOffset, ByteRange, ClipboardLimits, ClipboardWriteOutcome, ExactGeometryLimits,
    LogicalExtent, MutationFragment, MutationFragmentPayload, MutationKind, MutationLimits,
    MutationOutcome, MutationProposal, PageDemandEnvelope, PageDirection, PageEdgeFact,
    PageFailure, PageId, PlatformRangeResult, RangeBinding, RangeHistoryPlan, RangePage,
    RangeRestorationSeed, RangeSelection, RangeTextInput, RangeTextInputConfig,
    RangeTextInputLimits, RangeTextInputRequest, ResidencyLimits, SegmentationLimits,
    SourceRevision, StreamingGeometryStyle, StreamingOversizePresentation, TextInputTheme,
    ensure_text_input_bindings,
};

fn binding(source: &str, revision: u64) -> RangeBinding {
    RangeBinding::new(
        BindingId::new(17),
        SourceRevision::new(revision),
        LogicalExtent::new(
            source.len() as u64,
            if source.is_empty() {
                0
            } else {
                source.bytes().filter(|byte| *byte == b'\n').count() as u64 + 1
            },
        ),
    )
}

#[gpui::test]
fn mounted_undo_and_redo_use_the_shared_exact_staged_transaction(cx: &mut gpui::TestAppContext) {
    cx.update(ensure_text_input_bindings);
    let original = "current";
    let undone = "prior";
    let (input, cx) = cx.add_window_view(|window, cx| {
        let input = RangeTextInput::new(config(original, 1), window, cx).unwrap();
        input.focus(window);
        input
    });
    assert!(drive_pages(&input, cx, original).is_empty());

    cx.simulate_keystrokes("ctrl-z");
    let intent = drive_pages(&input, cx, original)
        .into_iter()
        .find_map(|request| match request {
            RangeTextInputRequest::HistoryIntent(intent) => Some(intent),
            _ => None,
        })
        .expect("undo emits exact intent");
    let proposal = MutationProposal::new(
        intent.key(),
        MutationKind::Undo,
        ByteRange::from_u64(0, original.len() as u64).unwrap(),
        0,
    );
    input.update(cx, |input, cx| {
        input
            .submit_history_plan(
                RangeHistoryPlan::new(
                    intent,
                    proposal,
                    RangeSelection::caret(ByteOffset::new(undone.len() as u64)),
                ),
                cx,
            )
            .unwrap();
    });
    let preflight = drive_pages(&input, cx, original);
    assert!(preflight.iter().any(
        |request| matches!(request, RangeTextInputRequest::MutationPreflight(actual) if *actual == proposal)
    ));
    input.update(cx, |input, cx| {
        input.accept_mutation_preflight(intent.key(), cx).unwrap();
        input
            .stage_history_fragment(
                MutationFragment::new(
                    intent.key(),
                    0,
                    MutationFragmentPayload::Utf8 {
                        inserted_offset: 0,
                        text: undone.to_owned(),
                    },
                ),
                cx,
            )
            .unwrap();
        input
            .stage_history_fragment(
                MutationFragment::new(intent.key(), 1, MutationFragmentPayload::Terminal),
                cx,
            )
            .unwrap();
    });
    let staged = drive_pages(&input, cx, original);
    assert!(staged.iter().any(
        |request| matches!(request, RangeTextInputRequest::MutationCommit(key) if *key == intent.key())
    ));
    input.update(cx, |input, _| {
        input.admit_mutation_commit(intent.key()).unwrap()
    });
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input
                .settle_mutation(
                    intent.key(),
                    MutationOutcome::Committed(binding(undone, 2)),
                    window,
                    cx,
                )
                .unwrap();
        })
    });
    assert!(drive_pages(&input, cx, undone).is_empty());
    input.read_with(cx, |input, _| {
        assert_eq!(input.surface().unwrap().binding(), binding(undone, 2));
        assert_eq!(
            input.surface().unwrap().selection(),
            RangeSelection::caret(ByteOffset::new(5))
        );
    });

    cx.simulate_keystrokes("ctrl-y");
    let redo = drive_pages(&input, cx, undone)
        .into_iter()
        .find_map(|request| match request {
            RangeTextInputRequest::HistoryIntent(intent) => Some(intent),
            _ => None,
        })
        .expect("redo emits exact intent");
    let redo_proposal = MutationProposal::new(
        redo.key(),
        MutationKind::Redo,
        ByteRange::from_u64(0, undone.len() as u64).unwrap(),
        0,
    );
    input.update(cx, |input, cx| {
        input
            .submit_history_plan(
                RangeHistoryPlan::new(
                    redo,
                    redo_proposal,
                    RangeSelection::caret(ByteOffset::new(original.len() as u64)),
                ),
                cx,
            )
            .unwrap();
        input.accept_mutation_preflight(redo.key(), cx).unwrap();
        input
            .stage_history_fragment(
                MutationFragment::new(
                    redo.key(),
                    0,
                    MutationFragmentPayload::Utf8 {
                        inserted_offset: 0,
                        text: original.to_owned(),
                    },
                ),
                cx,
            )
            .unwrap();
        input
            .stage_history_fragment(
                MutationFragment::new(redo.key(), 1, MutationFragmentPayload::Terminal),
                cx,
            )
            .unwrap();
        input.admit_mutation_commit(redo.key()).unwrap();
    });
    let _ = drive_pages(&input, cx, undone);
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input
                .settle_mutation(
                    redo.key(),
                    MutationOutcome::Committed(binding(original, 3)),
                    window,
                    cx,
                )
                .unwrap();
        })
    });
    assert!(drive_pages(&input, cx, original).is_empty());
    assert_eq!(
        input.read_with(cx, |input, _| input.surface().unwrap().binding()),
        binding(original, 3)
    );
}

#[gpui::test]
fn nonresident_platform_query_replays_source_selected_pages_without_a_whole_source(
    cx: &mut gpui::TestAppContext,
) {
    let source = "0123456789".repeat(20);
    let (input, cx) = cx.add_window_view(|window, cx| {
        let input = RangeTextInput::new(config(&source, 1), window, cx).unwrap();
        input.focus(window);
        input
    });
    assert!(drive_pages(&input, cx, &source).is_empty());
    let pending = input.update(cx, |input, cx| {
        input.platform_text_for_range(150..160, cx).unwrap()
    });
    assert!(matches!(pending, PlatformRangeResult::Pending(_)));
    assert!(drive_pages(&input, cx, &source).is_empty());
    let ready = input.update(cx, |input, cx| {
        input.platform_text_for_range(150..160, cx).unwrap()
    });
    assert_eq!(ready, PlatformRangeResult::Ready("0123456789".to_owned()));
}

#[gpui::test]
fn recurrent_platform_page_rejects_old_result_before_touching_new_replay(
    cx: &mut gpui::TestAppContext,
) {
    let source = "0123456789".repeat(20);
    let (input, cx) = cx
        .add_window_view(|window, cx| RangeTextInput::new(config(&source, 1), window, cx).unwrap());
    assert!(drive_pages(&input, cx, &source).is_empty());

    let _ = input.update(cx, |input, cx| {
        input.platform_text_for_range(150..160, cx).unwrap()
    });
    let old = input
        .update(cx, |input, _| input.take_request())
        .and_then(|request| match request {
            RangeTextInputRequest::Page(page)
                if page.key().purpose() == gpui_text_input::PagePurpose::PlatformRange =>
            {
                Some(page)
            }
            _ => None,
        })
        .expect("old platform request");
    input.update(cx, |input, cx| {
        input
            .fail_page(old.key(), PageFailure::Cancelled, cx)
            .unwrap();
    });

    let _ = input.update(cx, |input, cx| {
        input.platform_text_for_range(150..160, cx).unwrap()
    });
    let new = input
        .update(cx, |input, _| input.take_request())
        .and_then(|request| match request {
            RangeTextInputRequest::Page(page)
                if page.key().purpose() == gpui_text_input::PagePurpose::PlatformRange =>
            {
                Some(page)
            }
            _ => None,
        })
        .expect("new platform request");
    assert_ne!(old.key(), new.key());

    let late = page_for(&source, 900, old);
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            assert!(matches!(
                input.deliver_page(late, window, cx),
                Err(gpui_text_input::RangeTextInputError::Stale)
            ));
        })
    });
    let current = page_for(&source, 901, new);
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.deliver_page(current, window, cx).unwrap();
        })
    });
    let _ = drive_pages(&input, cx, &source);
    assert_eq!(
        input.update(cx, |input, cx| input
            .platform_text_for_range(150..160, cx)
            .unwrap()),
        PlatformRangeResult::Ready("0123456789".to_owned())
    );
}

#[gpui::test]
fn nonorigin_utf16_query_crosses_multibyte_pages_exactly(cx: &mut gpui::TestAppContext) {
    let source = format!("{}TARGET{}", "🙂".repeat(24), "é".repeat(24));
    let target_start = "🙂".encode_utf16().count() * 24;
    let target_end = target_start + "TARGET".encode_utf16().count();
    let (input, cx) = cx
        .add_window_view(|window, cx| RangeTextInput::new(config(&source, 1), window, cx).unwrap());
    assert!(drive_pages(&input, cx, &source).is_empty());
    assert!(matches!(
        input.update(cx, |input, cx| {
            input
                .platform_text_for_range(target_start..target_end, cx)
                .unwrap()
        }),
        PlatformRangeResult::Pending(_)
    ));
    assert!(drive_pages(&input, cx, &source).is_empty());
    input.update(cx, |input, cx| {
        assert_eq!(
            input
                .platform_text_for_range(target_start..target_end, cx)
                .unwrap(),
            PlatformRangeResult::Ready("TARGET".to_owned())
        );
    });
}

#[gpui::test]
fn nonresident_marked_replacement_preserves_exact_composition_and_selection(
    cx: &mut gpui::TestAppContext,
) {
    let source = "abcdefghij".repeat(20);
    let inserted = "\u{00e9}\u{1f642}";
    let (input, cx) = cx.add_window_view(|window, cx| {
        let input = RangeTextInput::new(config(&source, 1), window, cx).unwrap();
        input.focus(window);
        input
    });
    assert!(drive_pages(&input, cx, &source).is_empty());

    input.update(cx, |input, cx| input.set_read_only(true, cx));
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.replace_and_mark_text_in_range(Some(150..160), inserted, Some(1..3), window, cx);
        })
    });
    assert!(input.update(cx, |input, _| input.take_request()).is_none());
    input.update(cx, |input, cx| input.set_read_only(false, cx));

    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.replace_and_mark_text_in_range(Some(150..160), inserted, Some(1..3), window, cx);
        })
    });
    let requests = drive_pages(&input, cx, &source);
    let proposal = requests
        .iter()
        .find_map(|request| match request {
            RangeTextInputRequest::MutationPreflight(proposal) => Some(*proposal),
            _ => None,
        })
        .expect("replay resolves to ordinary mutation preflight");
    assert_eq!(
        proposal.replacement(),
        ByteRange::from_u64(150, 160).unwrap()
    );
    input.update(cx, |input, cx| {
        input.accept_mutation_preflight(proposal.key(), cx).unwrap();
    });
    let staged = drive_pages(&input, cx, &source);
    assert!(staged.iter().any(
        |request| matches!(request, RangeTextInputRequest::MutationCommit(key) if *key == proposal.key())
    ));
    input.update(cx, |input, _| {
        input.admit_mutation_commit(proposal.key()).unwrap();
    });
    let successor = format!("{}{}{}", &source[..150], inserted, &source[160..]);
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input
                .settle_mutation(
                    proposal.key(),
                    MutationOutcome::Committed(binding(&successor, 2)),
                    window,
                    cx,
                )
                .unwrap();
        })
    });
    assert!(drive_pages(&input, cx, &successor).is_empty());
    input.read_with(cx, |input, _| {
        let surface = input.surface().unwrap();
        assert_eq!(
            surface.composition(),
            Some(ByteRange::from_u64(150, 156).unwrap())
        );
        assert_eq!(
            surface.selection(),
            RangeSelection {
                anchor: ByteOffset::new(152),
                head: ByteOffset::new(156),
            }
        );
    });
}

#[gpui::test]
fn unpublished_platform_value_blocks_restoration_seed(cx: &mut gpui::TestAppContext) {
    let source = "ready platform payload";
    let (input, cx) = cx
        .add_window_view(|window, cx| RangeTextInput::new(config(source, 1), window, cx).unwrap());
    assert!(drive_pages(&input, cx, source).is_empty());
    let _ = input.update(cx, |input, cx| {
        input.platform_text_for_range(0..5, cx).unwrap()
    });
    assert!(drive_pages(&input, cx, source).is_empty());
    input.read_with(cx, |input, _| {
        assert!(matches!(
            input.export_restoration(None),
            Err(gpui_text_input::RangeTextInputError::NotQuiescent)
        ));
    });
    input.update(cx, |input, cx| {
        assert_eq!(
            input.platform_text_for_range(0..5, cx).unwrap(),
            PlatformRangeResult::Ready("ready".to_owned())
        );
    });
    input.read_with(cx, |input, _| {
        assert!(input.export_restoration(None).is_ok());
    });
}

#[gpui::test]
fn admitted_commit_detaches_on_rebind_and_late_settlement_is_obsolete(
    cx: &mut gpui::TestAppContext,
) {
    let source = "base";
    let (input, cx) = cx
        .add_window_view(|window, cx| RangeTextInput::new(config(source, 1), window, cx).unwrap());
    assert!(drive_pages(&input, cx, source).is_empty());
    cx.update(|window, app| input.update(app, |input, _| input.focus(window)));
    cx.simulate_input("!");
    let requests = drive_pages(&input, cx, source);
    let proposal = requests
        .iter()
        .find_map(|request| match request {
            RangeTextInputRequest::MutationPreflight(proposal) => Some(*proposal),
            _ => None,
        })
        .unwrap();
    input.update(cx, |input, cx| {
        input.accept_mutation_preflight(proposal.key(), cx).unwrap()
    });
    let _ = drive_pages(&input, cx, source);
    input.update(cx, |input, _| {
        input.admit_mutation_commit(proposal.key()).unwrap()
    });
    let replacement = "other";
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input
                .rebind(binding(replacement, 2), None, window, cx)
                .unwrap();
        })
    });
    let lifecycle = drive_pages(&input, cx, replacement);
    assert!(lifecycle.iter().any(|request| matches!(request, RangeTextInputRequest::DetachedMutation(key) if *key == proposal.key())));
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            let settlement = input
                .settle_mutation(
                    proposal.key(),
                    gpui_text_input::MutationOutcome::Committed(binding("base!", 2)),
                    window,
                    cx,
                )
                .unwrap();
            assert!(matches!(
                settlement,
                gpui_text_input::MutationSettlement::Obsolete(_)
            ));
            assert_eq!(input.surface().unwrap().binding(), binding(replacement, 2));
        })
    });
}

#[gpui::test]
fn detached_commit_slot_is_reserved_before_admission_and_never_exceeds_cap(
    cx: &mut gpui::TestAppContext,
) {
    let source = "base";
    let mut configuration = config(source, 1);
    configuration.limits =
        RangeTextInputLimits::new(2 * 1024 * 1024, 32768, 32, 32, px(16.), 1).unwrap();
    let (input, cx) = cx.add_window_view(|window, cx| {
        let input = RangeTextInput::new(configuration, window, cx).unwrap();
        input.focus(window);
        input
    });
    assert!(drive_pages(&input, cx, source).is_empty());

    cx.simulate_input("!");
    let first = drive_pages(&input, cx, source)
        .iter()
        .find_map(|request| match request {
            RangeTextInputRequest::MutationPreflight(proposal) => Some(*proposal),
            _ => None,
        })
        .unwrap();
    input.update(cx, |input, cx| {
        input.accept_mutation_preflight(first.key(), cx).unwrap()
    });
    let _ = drive_pages(&input, cx, source);
    input.update(cx, |input, _| {
        input.admit_mutation_commit(first.key()).unwrap()
    });

    let rebound = "next";
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.rebind(binding(rebound, 2), None, window, cx).unwrap();
        })
    });
    let lifecycle = drive_pages(&input, cx, rebound);
    assert!(lifecycle.iter().any(
        |request| matches!(request, RangeTextInputRequest::DetachedMutation(key) if *key == first.key())
    ));

    cx.simulate_input("?");
    let second = drive_pages(&input, cx, rebound)
        .iter()
        .find_map(|request| match request {
            RangeTextInputRequest::MutationPreflight(proposal) => Some(*proposal),
            _ => None,
        })
        .unwrap();
    input.update(cx, |input, cx| {
        input.accept_mutation_preflight(second.key(), cx).unwrap()
    });
    let _ = drive_pages(&input, cx, rebound);
    input.update(cx, |input, _| {
        assert!(matches!(
            input.admit_mutation_commit(second.key()),
            Err(gpui_text_input::RangeTextInputError::DetachedCapacity)
        ));
    });

    let disposed =
        cx.update(|window, app| input.update(app, |input, cx| input.dispose(window, cx)));
    assert!(disposed.iter().any(
        |request| matches!(request, RangeTextInputRequest::CancelMutation(key) if *key == second.key())
    ));
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            assert!(matches!(
                input
                    .settle_mutation(first.key(), MutationOutcome::Cancelled, window, cx)
                    .unwrap(),
                gpui_text_input::MutationSettlement::Obsolete(_)
            ));
            assert!(input.is_quiescent());
        })
    });
}

#[gpui::test]
fn retained_prior_surface_is_paint_only_after_rebind(cx: &mut gpui::TestAppContext) {
    let source = "old publication";
    let replacement = "new publication";
    let (input, cx) = cx
        .add_window_view(|window, cx| RangeTextInput::new(config(source, 1), window, cx).unwrap());
    assert!(drive_pages(&input, cx, source).is_empty());

    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input
                .rebind(binding(replacement, 2), None, window, cx)
                .unwrap();
            assert_eq!(input.surface().unwrap().binding(), binding(source, 1));
            assert!(matches!(
                input.begin_clipboard(gpui_text_input::ClipboardKind::Copy, cx),
                Err(gpui_text_input::RangeTextInputError::Busy)
            ));
            assert!(matches!(
                input.platform_text_for_range(0..1, cx),
                Err(gpui_text_input::RangeTextInputError::Busy)
            ));
        });
    });

    assert!(drive_pages(&input, cx, replacement).is_empty());
    input.read_with(cx, |input, _| {
        assert_eq!(input.surface().unwrap().binding(), binding(replacement, 2));
    });
}

#[gpui::test]
fn obsolete_target_completion_cannot_publish_newer_scroll_intent(cx: &mut gpui::TestAppContext) {
    let source = &(0..80)
        .map(|line| format!("line-{line:02}\n"))
        .collect::<String>();
    let (input, cx) = cx
        .add_window_view(|window, cx| RangeTextInput::new(config(source, 1), window, cx).unwrap());
    assert!(drive_pages(&input, cx, source).is_empty());
    let prior_geometry = input.read_with(cx, |input, _| input.surface().unwrap().geometry_key());

    input.update(cx, |input, cx| {
        input.request_absolute_scroll(px(96.), cx).unwrap()
    });
    let obsolete = input
        .update(cx, |input, _| input.take_request())
        .and_then(|request| match request {
            RangeTextInputRequest::Page(page) => Some(page),
            _ => None,
        })
        .expect("first target page");
    input.update(cx, |input, cx| {
        input.request_absolute_scroll(px(160.), cx).unwrap()
    });
    let obsolete_page = page_for(source, 900, obsolete);
    let result = cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.deliver_page(obsolete_page, window, cx)
        })
    });
    assert!(result.is_err(), "obsolete target input must be rejected");
    input.read_with(cx, |input, _| {
        assert_eq!(input.surface().unwrap().geometry_key(), prior_geometry);
    });
    let _ = drive_pages(&input, cx, source);
    input.read_with(cx, |input, _| {
        assert!(input.surface().unwrap().scroll_block() >= px(96.));
    });
}

fn config(source: &str, revision: u64) -> RangeTextInputConfig {
    let layout = StreamingLayoutBinding {
        input_id: 11,
        segment_policy_id: 13,
        wrap_width: px(120.),
        font_size: px(12.),
        line_height: px(16.),
        limits: StreamingLayoutLimits {
            segment_bytes: 32,
            runs: 8,
            decorations: 8,
            glyphs: 256,
            wraps: 128,
            maps: 257,
            fragments: 1,
            retained_bytes: 256 * 1024,
        },
    };
    let run = TextRun {
        len: 0,
        font: font(".SystemUIFont"),
        color: black(),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    RangeTextInputConfig {
        binding: binding(source, revision),
        layout,
        style: StreamingGeometryStyle::new(
            run,
            StreamingOversizePresentation::new(
                SharedString::new(Arc::<str>::from("")),
                vec![],
                px(12.),
                px(16.),
                px(12.),
                None,
            ),
        ),
        geometry_limits: ExactGeometryLimits::new(32, 8, 512 * 1024, 8192).unwrap(),
        residency_limits: ResidencyLimits::new(8, 128 * 1024, 8, 256).unwrap(),
        mutation_limits: MutationLimits::new(8, 256).unwrap(),
        clipboard_limits: ClipboardLimits::new(1024, 32).unwrap(),
        segmentation_limits: SegmentationLimits::new(32, 64).unwrap(),
        limits: RangeTextInputLimits::new(2 * 1024 * 1024, 32768, 32, 32, px(16.), 4).unwrap(),
        viewport_extent: px(80.),
        overscan: px(32.),
        placeholder: SharedString::new_static("Value"),
        theme: TextInputTheme::default(),
        scrollbar_style: ScrollbarStyle::default(),
    }
}

fn page_for(source: &str, id: u64, request: gpui_text_input::PageRequest) -> RangePage {
    let key = request.key();
    let (start, end) = match key.demand() {
        PageDemandEnvelope::Adjacent {
            anchor,
            direction: PageDirection::Forward,
            max_payload_bytes,
        } => {
            let start = anchor.get() as usize;
            let mut end = start
                .saturating_add(max_payload_bytes as usize)
                .min(source.len());
            while end > start && !source.is_char_boundary(end) {
                end -= 1;
            }
            (start, end)
        }
        PageDemandEnvelope::Adjacent {
            anchor,
            direction: PageDirection::Backward,
            max_payload_bytes,
        } => {
            let end = anchor.get() as usize;
            let mut start = end.saturating_sub(max_payload_bytes as usize);
            while start < end && !source.is_char_boundary(start) {
                start += 1;
            }
            (start, end)
        }
        PageDemandEnvelope::Validation {
            candidate,
            max_payload_bytes,
        } => {
            let candidate = candidate.get() as usize;
            let mut start = candidate.saturating_sub((max_payload_bytes as usize) / 2);
            while start < candidate && !source.is_char_boundary(start) {
                start += 1;
            }
            let mut end = start
                .saturating_add(max_payload_bytes as usize)
                .min(source.len());
            while end > candidate && !source.is_char_boundary(end) {
                end -= 1;
            }
            (start, end)
        }
    };
    RangePage::new(
        PageId::new(id),
        key,
        ByteRange::from_u64(start as u64, end as u64).unwrap(),
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
    .unwrap()
}

fn drive_pages(
    input: &gpui::Entity<RangeTextInput>,
    cx: &mut gpui::VisualTestContext,
    source: &str,
) -> Vec<RangeTextInputRequest> {
    let mut other = Vec::new();
    let mut page_id = 1;
    for _ in 0..256 {
        let request = input.update(cx, |input, _| input.take_request());
        match request {
            Some(RangeTextInputRequest::Page(request)) => {
                let page = page_for(source, page_id, request);
                page_id += 1;
                cx.update(|window, app| {
                    input.update(app, |input, cx| {
                        input.deliver_page(page, window, cx).unwrap()
                    });
                });
            }
            Some(RangeTextInputRequest::ReleasePage(_))
            | Some(RangeTextInputRequest::CancelPage(_)) => {}
            Some(request) => other.push(request),
            None => break,
        }
    }
    other
}

#[gpui::test]
fn live_range_widget_builds_one_exact_surface_and_drives_ime_mutation(
    cx: &mut gpui::TestAppContext,
) {
    cx.update(ensure_text_input_bindings);
    let source = "hello\nworld";
    let (input, cx) = cx.add_window_view(|window, cx| {
        let input = RangeTextInput::new(config(source, 1), window, cx).unwrap();
        input.focus(window);
        input
    });
    assert!(drive_pages(&input, cx, source).is_empty());
    input.read_with(cx, |input, _| {
        let surface = input.surface().expect("coherent surface");
        assert_eq!(surface.binding(), binding(source, 1));
        assert_eq!(surface.caret().get(), 0);
        assert!(!surface.fragments().is_empty());
    });

    cx.simulate_input("!");
    let requests = drive_pages(&input, cx, source);
    let proposal = requests
        .iter()
        .find_map(|request| match request {
            RangeTextInputRequest::MutationPreflight(proposal) => Some(*proposal),
            _ => None,
        })
        .expect("typed mutation preflight");
    input.update(cx, |input, cx| {
        input.accept_mutation_preflight(proposal.key(), cx).unwrap()
    });
    let staged = drive_pages(&input, cx, source);
    assert!(
        staged
            .iter()
            .any(|request| matches!(request, RangeTextInputRequest::MutationFragment { .. }))
    );
    assert!(staged.iter().any(|request| matches!(request, RangeTextInputRequest::MutationCommit(key) if *key == proposal.key())));
}

#[gpui::test]
fn rejected_preflight_releases_widget_dispatch_and_one_fragment_limit_rejects_atomically(
    cx: &mut gpui::TestAppContext,
) {
    cx.update(ensure_text_input_bindings);
    let source = "base";
    let (input, cx) = cx.add_window_view(|window, cx| {
        let input = RangeTextInput::new(config(source, 1), window, cx).unwrap();
        input.focus(window);
        input
    });
    assert!(drive_pages(&input, cx, source).is_empty());
    cx.simulate_input("!");
    let requests = drive_pages(&input, cx, source);
    let proposal = requests
        .iter()
        .find_map(|request| match request {
            RangeTextInputRequest::MutationPreflight(proposal) => Some(*proposal),
            _ => None,
        })
        .unwrap();
    input.update(cx, |input, cx| {
        input.reject_mutation_preflight(proposal.key(), cx).unwrap();
        assert!(input.is_quiescent());
        assert_eq!(
            input.surface().unwrap().selection().range(),
            ByteRange::from_u64(0, 0).unwrap()
        );
    });

    cx.simulate_input("?");
    let requests = drive_pages(&input, cx, source);
    let staged = requests
        .iter()
        .find_map(|request| match request {
            RangeTextInputRequest::MutationPreflight(proposal) => Some(*proposal),
            _ => None,
        })
        .unwrap();
    input.update(cx, |input, cx| {
        input.accept_mutation_preflight(staged.key(), cx).unwrap();
        assert!(matches!(
            input.take_request(),
            Some(RangeTextInputRequest::MutationFragment { key, .. }) if key == staged.key()
        ));
        assert_eq!(
            input.reject_mutation_staging(staged.key(), cx).unwrap(),
            gpui_text_input::MutationSettlement::Current(
                gpui_text_input::MutationOutcome::Rejected
            )
        );
        assert!(input.take_request().is_none());
        assert!(input.is_quiescent());
        assert_eq!(input.surface().unwrap().binding(), binding(source, 1));
    });

    let mut limited = config(source, 2);
    limited.mutation_limits = MutationLimits::new(1, 256).unwrap();
    let (limited, cx) = cx.add_window_view(|window, cx| {
        let input = RangeTextInput::new(limited, window, cx).unwrap();
        input.focus(window);
        input
    });
    assert!(drive_pages(&limited, cx, source).is_empty());
    cx.simulate_input("!");
    assert!(drive_pages(&limited, cx, source).is_empty());
    limited.read_with(cx, |input, _| {
        assert!(input.is_quiescent());
        assert_eq!(input.surface().unwrap().binding(), binding(source, 2));
    });
}

#[gpui::test]
fn mounted_double_and_triple_click_select_exact_word_and_logical_line(
    cx: &mut gpui::TestAppContext,
) {
    cx.update(ensure_text_input_bindings);
    let source = "alpha beta\ngamma";
    let (input, cx) = cx.add_window_view(|window, cx| {
        let input = RangeTextInput::new(config(source, 1), window, cx).unwrap();
        input.focus(window);
        input
    });
    assert!(drive_pages(&input, cx, source).is_empty());
    cx.update(|window, cx| window.draw_and_present_for_test(cx));
    let click = input.read_with(cx, |input, _| {
        let position = input
            .surface()
            .unwrap()
            .position_for_offset(gpui_text_input::ByteOffset::new(7))
            .unwrap();
        point(position.x + px(1.), position.y + px(1.))
    });

    cx.simulate_event(MouseDownEvent {
        position: click,
        modifiers: Modifiers::none(),
        button: MouseButton::Left,
        click_count: 2,
        first_mouse: false,
    });
    cx.simulate_event(MouseUpEvent {
        position: click,
        modifiers: Modifiers::none(),
        button: MouseButton::Left,
        click_count: 2,
    });
    assert!(drive_pages(&input, cx, source).is_empty());
    input.read_with(cx, |input, _| {
        assert_eq!(
            input.surface().unwrap().selection().range(),
            ByteRange::from_u64(6, 10).unwrap()
        );
    });

    cx.simulate_event(MouseDownEvent {
        position: click,
        modifiers: Modifiers::none(),
        button: MouseButton::Left,
        click_count: 3,
        first_mouse: false,
    });
    cx.simulate_event(MouseUpEvent {
        position: click,
        modifiers: Modifiers::none(),
        button: MouseButton::Left,
        click_count: 3,
    });
    assert!(drive_pages(&input, cx, source).is_empty());
    input.read_with(cx, |input, _| {
        assert_eq!(
            input.surface().unwrap().selection().range(),
            ByteRange::from_u64(0, 11).unwrap()
        );
    });
}

#[gpui::test]
fn restoration_uses_validation_envelopes_and_imports_no_resident_page(
    cx: &mut gpui::TestAppContext,
) {
    let source = "alpha\nbeta";
    let (input, cx) = cx
        .add_window_view(|window, cx| RangeTextInput::new(config(source, 1), window, cx).unwrap());
    assert!(drive_pages(&input, cx, source).is_empty());
    let seed: RangeRestorationSeed =
        input.read_with(cx, |input, _| input.export_restoration(None).unwrap());
    input.update(cx, |input, cx| input.import_restoration(seed, cx).unwrap());
    let mut saw_validation = false;
    for _ in 0..256 {
        let request = input.update(cx, |input, _| input.take_request());
        match request {
            Some(RangeTextInputRequest::Page(request)) => {
                saw_validation |= matches!(
                    request.key().demand(),
                    PageDemandEnvelope::Validation { .. }
                );
                let page = page_for(source, 500, request);
                cx.update(|window, app| {
                    input.update(app, |input, cx| {
                        input.deliver_page(page, window, cx).unwrap()
                    })
                });
            }
            Some(RangeTextInputRequest::ReleasePage(_))
            | Some(RangeTextInputRequest::CancelPage(_)) => {}
            None => break,
            Some(_) => panic!("unexpected restoration request"),
        }
    }
    assert!(saw_validation);
    input.read_with(cx, |input, _| {
        let surface = input.surface().unwrap();
        assert_eq!(surface.binding(), seed.binding);
        assert_eq!(surface.caret(), seed.caret);
        assert_eq!(surface.selection(), seed.selection);
        assert_eq!(surface.scroll_source(), seed.scroll.source);
        assert_eq!(surface.scroll_intra_anchor(), seed.scroll.intra_anchor);
        assert_eq!(surface.viewport(), seed.viewport);
        assert_eq!(surface.overscan(), seed.overscan);
    });
}

#[gpui::test]
fn nonresident_platform_replacement_resolves_exact_range_before_preflight(
    cx: &mut gpui::TestAppContext,
) {
    let source = "abcdefghij".repeat(20);
    let (input, cx) = cx
        .add_window_view(|window, cx| RangeTextInput::new(config(&source, 1), window, cx).unwrap());
    assert!(drive_pages(&input, cx, &source).is_empty());
    input.update(cx, |input, cx| {
        input
            .replace_platform_range(150..160, "X".to_owned(), cx)
            .unwrap();
    });
    let requests = drive_pages(&input, cx, &source);
    let proposal = requests
        .iter()
        .find_map(|request| match request {
            RangeTextInputRequest::MutationPreflight(proposal) => Some(*proposal),
            _ => None,
        })
        .expect("replacement preflight after replay");
    assert_eq!(
        proposal.replacement(),
        ByteRange::from_u64(150, 160).unwrap()
    );
    input.read_with(cx, |input, _| {
        assert_eq!(input.surface().unwrap().binding(), binding(&source, 1))
    });
}

#[gpui::test]
fn invalid_restoration_boundary_rejects_seed_and_retains_prior_surface(
    cx: &mut gpui::TestAppContext,
) {
    let source = "éx";
    let (input, cx) = cx
        .add_window_view(|window, cx| RangeTextInput::new(config(source, 1), window, cx).unwrap());
    assert!(drive_pages(&input, cx, source).is_empty());
    let mut seed = input.read_with(cx, |input, _| input.export_restoration(None).unwrap());
    seed.caret = gpui_text_input::ByteOffset::new(1);
    seed.selection = gpui_text_input::RangeSelection::caret(seed.caret);
    input.update(cx, |input, cx| input.import_restoration(seed, cx).unwrap());
    let mut rejected = false;
    for page_id in 700..720 {
        let Some(request) = input.update(cx, |input, _| input.take_request()) else {
            break;
        };
        match request {
            RangeTextInputRequest::Page(request) => {
                let page = page_for(source, page_id, request);
                let result = cx.update(|window, app| {
                    input.update(app, |input, cx| input.deliver_page(page, window, cx))
                });
                if matches!(
                    result,
                    Err(gpui_text_input::RangeTextInputError::MalformedSeed)
                ) {
                    rejected = true;
                    break;
                }
            }
            RangeTextInputRequest::ReleasePage(_) => {}
            _ => panic!("unexpected validation request"),
        }
    }
    assert!(rejected);
    input.read_with(cx, |input, _| {
        assert_eq!(input.surface().unwrap().binding(), binding(source, 1))
    });
}

#[gpui::test]
fn estimated_absolute_scroll_records_intent_until_exact_index_completes(
    cx: &mut gpui::TestAppContext,
) {
    let source = "line of wrapped text\n".repeat(100);
    let (input, cx) = cx
        .add_window_view(|window, cx| RangeTextInput::new(config(&source, 1), window, cx).unwrap());
    input.update(cx, |input, cx| {
        input.request_absolute_scroll(px(200.), cx).unwrap()
    });
    let mut saw_estimate = false;
    for page_id in 900..1200 {
        let Some(request) = input.update(cx, |input, _| input.take_request()) else {
            break;
        };
        match request {
            RangeTextInputRequest::Page(request) => {
                let page = page_for(&source, page_id, request);
                cx.update(|window, app| {
                    input.update(app, |input, cx| {
                        input.deliver_page(page, window, cx).unwrap()
                    })
                });
                saw_estimate |= input.read_with(cx, |input, _| input.geometry_estimate().is_some());
            }
            RangeTextInputRequest::ReleasePage(_) | RangeTextInputRequest::CancelPage(_) => {}
            _ => panic!("unexpected scroll request"),
        }
    }
    assert!(saw_estimate);
    input.read_with(cx, |input, _| {
        let surface = input.surface().expect("exact target surface");
        assert_eq!(surface.quality(), gpui_text_input::GeometryQuality::Exact);
        assert!(surface.scroll_block() >= px(0.));
    });
}

#[gpui::test]
fn coherent_surface_accepts_exact_byte_and_item_caps_and_rejects_one_under(
    cx: &mut gpui::TestAppContext,
) {
    let source = "bounded surface";
    let (baseline, cx) = cx
        .add_window_view(|window, cx| RangeTextInput::new(config(source, 1), window, cx).unwrap());
    assert!(drive_pages(&baseline, cx, source).is_empty());
    let (surface_charge, admission_charge) = baseline.read_with(cx, |input, _| {
        (
            input.surface().unwrap().charge(),
            input.last_surface_admission_charge().unwrap(),
        )
    });
    assert!(admission_charge.bytes > surface_charge.bytes);
    assert!(admission_charge.items > surface_charge.items);

    let mut exact_config = config(source, 1);
    exact_config.limits.max_surface_bytes = admission_charge.bytes;
    exact_config.limits.max_surface_items = admission_charge.items;
    let (exact, cx) =
        cx.add_window_view(|window, cx| RangeTextInput::new(exact_config, window, cx).unwrap());
    assert!(drive_pages(&exact, cx, source).is_empty());
    exact.read_with(cx, |input, _| {
        assert_eq!(input.surface().unwrap().charge(), surface_charge);
        assert_eq!(
            input.last_surface_admission_charge(),
            Some(admission_charge)
        );
    });

    let mut under_config = config(source, 1);
    under_config.limits.max_surface_bytes = admission_charge.bytes - 1;
    under_config.limits.max_surface_items = admission_charge.items;
    let (under, cx) =
        cx.add_window_view(|window, cx| RangeTextInput::new(under_config, window, cx).unwrap());
    let mut rejected = false;
    for page_id in 1300..1400 {
        let Some(request) = under.update(cx, |input, _| input.take_request()) else {
            break;
        };
        match request {
            RangeTextInputRequest::Page(request) => {
                let page = page_for(source, page_id, request);
                let result = cx.update(|window, app| {
                    under.update(app, |input, cx| input.deliver_page(page, window, cx))
                });
                rejected |= matches!(
                    result,
                    Err(gpui_text_input::RangeTextInputError::SurfaceCapacity)
                );
            }
            RangeTextInputRequest::ReleasePage(_) | RangeTextInputRequest::CancelPage(_) => {}
            _ => panic!("unexpected cap request"),
        }
    }
    assert!(rejected);
    under.read_with(cx, |input, _| assert!(input.surface().is_none()));

    let mut item_under_config = config(source, 1);
    item_under_config.limits.max_surface_bytes = admission_charge.bytes;
    item_under_config.limits.max_surface_items = admission_charge.items - 1;
    let (item_under, cx) = cx
        .add_window_view(|window, cx| RangeTextInput::new(item_under_config, window, cx).unwrap());
    let mut item_rejected = false;
    for page_id in 1400..1500 {
        let Some(request) = item_under.update(cx, |input, _| input.take_request()) else {
            break;
        };
        match request {
            RangeTextInputRequest::Page(request) => {
                let page = page_for(source, page_id, request);
                let result = cx.update(|window, app| {
                    item_under.update(app, |input, cx| input.deliver_page(page, window, cx))
                });
                item_rejected |= matches!(
                    result,
                    Err(gpui_text_input::RangeTextInputError::SurfaceCapacity)
                );
            }
            RangeTextInputRequest::ReleasePage(_) | RangeTextInputRequest::CancelPage(_) => {}
            _ => panic!("unexpected item-cap request"),
        }
    }
    assert!(item_rejected);
    item_under.read_with(cx, |input, _| assert!(input.surface().is_none()));
}

#[gpui::test]
fn empty_coherent_surface_owns_and_paints_placeholder_across_widget_states(
    cx: &mut gpui::TestAppContext,
) {
    let (input, cx) =
        cx.add_window_view(|window, cx| RangeTextInput::new(config("", 1), window, cx).unwrap());
    assert!(drive_pages(&input, cx, "").is_empty());
    input.read_with(cx, |input, _| {
        assert_eq!(
            input
                .surface()
                .unwrap()
                .placeholder()
                .map(|value| value.as_ref()),
            Some("Value")
        );
    });
    input.update(cx, |input, cx| {
        input.set_enabled(false, cx);
        input.set_read_only(true, cx);
    });
    cx.update(|window, app| window.draw_and_present_for_test(app));
    input.read_with(cx, |input, _| {
        assert!(input.surface().unwrap().placeholder().is_some());
    });

    let text = "content";
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.rebind(binding(text, 2), None, window, cx).unwrap();
            assert!(input.surface().unwrap().placeholder().is_some());
        })
    });
    assert!(drive_pages(&input, cx, text).is_empty());
    input.read_with(cx, |input, _| {
        assert!(input.surface().unwrap().placeholder().is_none());
    });

    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.rebind(binding("", 3), None, window, cx).unwrap();
        })
    });
    assert!(drive_pages(&input, cx, "").is_empty());
    cx.update(|window, app| window.draw_and_present_for_test(app));
    input.read_with(cx, |input, _| {
        assert_eq!(
            input
                .surface()
                .unwrap()
                .placeholder()
                .map(|value| value.as_ref()),
            Some("Value")
        );
    });
}

#[gpui::test]
fn mounted_read_only_copy_and_single_flight_history_routes_are_typed(
    cx: &mut gpui::TestAppContext,
) {
    cx.update(ensure_text_input_bindings);
    let source = "alpha beta";
    let (input, cx) = cx.add_window_view(|window, cx| {
        let input = RangeTextInput::new(config(source, 1), window, cx).unwrap();
        input.focus(window);
        input
    });
    assert!(drive_pages(&input, cx, source).is_empty());

    input.update(cx, |input, cx| input.set_read_only(true, cx));
    cx.simulate_input("blocked");
    cx.simulate_keystrokes("ctrl-a ctrl-x ctrl-z");
    let blocked = drive_pages(&input, cx, source);
    assert!(!blocked.iter().any(|request| matches!(
        request,
        RangeTextInputRequest::MutationPreflight(_) | RangeTextInputRequest::HistoryIntent(_)
    )));

    cx.simulate_keystrokes("ctrl-c");
    let copied = drive_pages(&input, cx, source);
    let write = copied
        .iter()
        .find_map(|request| match request {
            RangeTextInputRequest::ClipboardWrite(write) => Some(write),
            _ => None,
        })
        .expect("read-only selection remains copyable");
    assert_eq!(write.text(), source);
    input.update(cx, |input, cx| {
        input
            .settle_clipboard_write(write.key(), ClipboardWriteOutcome::Written, cx)
            .unwrap();
        input.set_read_only(false, cx);
    });

    cx.simulate_keystrokes("ctrl-z ctrl-z");
    let history = drive_pages(&input, cx, source);
    let operations = history
        .iter()
        .filter_map(|request| match request {
            RangeTextInputRequest::HistoryIntent(intent) => Some(intent.key().operation()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(operations.len(), 1, "history dispatch is single-flight");

    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.rebind(binding(source, 2), None, window, cx).unwrap();
        })
    });
    let lifecycle = drive_pages(&input, cx, source);
    assert!(lifecycle.iter().any(|request| matches!(
        request,
        RangeTextInputRequest::CancelHistoryIntent(intent)
            if intent.key().operation() == operations[0]
    )));
}

#[gpui::test]
fn clipboard_reuses_concurrent_geometry_resident_page_without_stranding(
    cx: &mut gpui::TestAppContext,
) {
    cx.update(ensure_text_input_bindings);
    let source = "0123456789".repeat(12);
    let (input, cx) = cx.add_window_view(|window, cx| {
        let input = RangeTextInput::new(config(&source, 1), window, cx).unwrap();
        input.focus(window);
        input
    });
    assert!(drive_pages(&input, cx, &source).is_empty());
    cx.simulate_keystrokes("ctrl-a");
    assert!(drive_pages(&input, cx, &source).is_empty());

    input.update(cx, |input, cx| {
        input.request_absolute_scroll(px(0.), cx).unwrap()
    });
    let geometry = input
        .update(cx, |input, _| input.take_request())
        .and_then(|request| match request {
            RangeTextInputRequest::Page(page)
                if page.key().purpose() == gpui_text_input::PagePurpose::GeometryTarget =>
            {
                Some(page)
            }
            _ => None,
        })
        .expect("target page creates concurrent residency");
    let page = page_for(&source, 700, geometry);
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.deliver_page(page, window, cx).unwrap();
            input
                .begin_clipboard(gpui_text_input::ClipboardKind::Copy, cx)
                .unwrap();
        })
    });
    let requests = drive_pages(&input, cx, &source);
    let write = requests
        .iter()
        .find_map(|request| match request {
            RangeTextInputRequest::ClipboardWrite(write) => Some(write),
            _ => None,
        })
        .expect("resident first page advances clipboard to completion");
    assert_eq!(write.text(), source);
}

#[gpui::test]
fn history_plan_rejects_mismatch_and_admitted_conflict_settles_obsolete_after_detach(
    cx: &mut gpui::TestAppContext,
) {
    cx.update(ensure_text_input_bindings);
    let source = "history";
    let (input, cx) = cx.add_window_view(|window, cx| {
        let input = RangeTextInput::new(config(source, 1), window, cx).unwrap();
        input.focus(window);
        input
    });
    assert!(drive_pages(&input, cx, source).is_empty());
    cx.simulate_keystrokes("ctrl-z");
    let intent = drive_pages(&input, cx, source)
        .into_iter()
        .find_map(|request| match request {
            RangeTextInputRequest::HistoryIntent(intent) => Some(intent),
            _ => None,
        })
        .unwrap();
    let malformed = MutationProposal::new(
        intent.key(),
        MutationKind::Redo,
        ByteRange::from_u64(0, source.len() as u64).unwrap(),
        0,
    );
    input.update(cx, |input, cx| {
        assert!(matches!(
            input.submit_history_plan(
                RangeHistoryPlan::new(intent, malformed, RangeSelection::caret(ByteOffset::new(0))),
                cx,
            ),
            Err(gpui_text_input::RangeTextInputError::Stale)
        ));
    });
    let proposal = MutationProposal::new(
        intent.key(),
        MutationKind::Undo,
        ByteRange::from_u64(0, source.len() as u64).unwrap(),
        0,
    );
    input.update(cx, |input, cx| {
        input
            .submit_history_plan(
                RangeHistoryPlan::new(intent, proposal, RangeSelection::caret(ByteOffset::new(0))),
                cx,
            )
            .unwrap();
        input.reject_mutation_preflight(intent.key(), cx).unwrap();
    });
    let _ = drive_pages(&input, cx, source);

    cx.simulate_keystrokes("ctrl-z");
    let detached = drive_pages(&input, cx, source)
        .into_iter()
        .find_map(|request| match request {
            RangeTextInputRequest::HistoryIntent(intent) => Some(intent),
            _ => None,
        })
        .unwrap();
    let proposal = MutationProposal::new(
        detached.key(),
        MutationKind::Undo,
        ByteRange::from_u64(0, source.len() as u64).unwrap(),
        0,
    );
    input.update(cx, |input, cx| {
        input
            .submit_history_plan(
                RangeHistoryPlan::new(
                    detached,
                    proposal,
                    RangeSelection::caret(ByteOffset::new(0)),
                ),
                cx,
            )
            .unwrap();
        input.accept_mutation_preflight(detached.key(), cx).unwrap();
        input
            .stage_history_fragment(
                MutationFragment::new(detached.key(), 0, MutationFragmentPayload::Terminal),
                cx,
            )
            .unwrap();
    });
    let _ = drive_pages(&input, cx, source);
    input.update(cx, |input, _| {
        input.admit_mutation_commit(detached.key()).unwrap()
    });
    let replacement = "replacement";
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input
                .rebind(binding(replacement, 2), None, window, cx)
                .unwrap();
        })
    });
    let lifecycle = drive_pages(&input, cx, replacement);
    assert!(lifecycle.iter().any(
        |request| matches!(request, RangeTextInputRequest::DetachedMutation(key) if *key == detached.key())
    ));
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            assert!(matches!(
                input
                    .settle_mutation(detached.key(), MutationOutcome::Conflict, window, cx)
                    .unwrap(),
                gpui_text_input::MutationSettlement::Obsolete(MutationOutcome::Conflict)
            ));
        })
    });
}

#[gpui::test]
fn malformed_history_selection_rejects_before_commit_and_restores_quiescence(
    cx: &mut gpui::TestAppContext,
) {
    cx.update(ensure_text_input_bindings);
    let source = "history";
    let successor = "ok";
    let (input, cx) = cx.add_window_view(|window, cx| {
        let input = RangeTextInput::new(config(source, 1), window, cx).unwrap();
        input.focus(window);
        input
    });
    assert!(drive_pages(&input, cx, source).is_empty());

    cx.simulate_keystrokes("ctrl-z");
    let intent = drive_pages(&input, cx, source)
        .into_iter()
        .find_map(|request| match request {
            RangeTextInputRequest::HistoryIntent(intent) => Some(intent),
            _ => None,
        })
        .unwrap();
    let proposal = MutationProposal::new(
        intent.key(),
        MutationKind::Undo,
        ByteRange::from_u64(0, source.len() as u64).unwrap(),
        0,
    );
    input.update(cx, |input, cx| {
        input
            .submit_history_plan(
                RangeHistoryPlan::new(
                    intent,
                    proposal,
                    RangeSelection::caret(ByteOffset::new(successor.len() as u64 + 1)),
                ),
                cx,
            )
            .unwrap();
    });
    let preflight = drive_pages(&input, cx, source);
    assert!(preflight.iter().any(
        |request| matches!(request, RangeTextInputRequest::MutationPreflight(candidate) if *candidate == proposal)
    ));
    input.update(cx, |input, cx| {
        input.accept_mutation_preflight(intent.key(), cx).unwrap();
        input
            .stage_history_fragment(
                MutationFragment::new(
                    intent.key(),
                    0,
                    MutationFragmentPayload::Utf8 {
                        inserted_offset: 0,
                        text: successor.to_owned(),
                    },
                ),
                cx,
            )
            .unwrap();
    });
    let fragments = drive_pages(&input, cx, source);
    assert!(fragments.iter().any(
        |request| matches!(request, RangeTextInputRequest::MutationFragment { key, .. } if *key == intent.key())
    ));

    input.update(cx, |input, cx| {
        assert!(matches!(
            input.stage_history_fragment(
                MutationFragment::new(intent.key(), 1, MutationFragmentPayload::Terminal),
                cx,
            ),
            Err(gpui_text_input::RangeTextInputError::Contract(
                gpui_text_input::RangeContractError::ByteRangeOutsideExtent { byte_len: 2, .. }
            ))
        ));
        assert!(input.take_request().is_none());
        assert!(input.is_quiescent());
        assert_eq!(input.surface().unwrap().binding(), binding(source, 1));
    });
}

#[gpui::test]
fn impossible_history_successor_extent_rejects_before_commit(cx: &mut gpui::TestAppContext) {
    cx.update(ensure_text_input_bindings);
    let source = "\n\nx";
    let (input, cx) = cx.add_window_view(|window, cx| {
        let input = RangeTextInput::new(config(source, 1), window, cx).unwrap();
        input.focus(window);
        input
    });
    assert!(drive_pages(&input, cx, source).is_empty());

    cx.simulate_keystrokes("ctrl-z");
    let intent = drive_pages(&input, cx, source)
        .into_iter()
        .find_map(|request| match request {
            RangeTextInputRequest::HistoryIntent(intent) => Some(intent),
            _ => None,
        })
        .unwrap();
    let proposal = MutationProposal::new(
        intent.key(),
        MutationKind::Undo,
        ByteRange::from_u64(0, 2).unwrap(),
        0,
    );
    input.update(cx, |input, cx| {
        input
            .submit_history_plan(
                RangeHistoryPlan::new(intent, proposal, RangeSelection::caret(ByteOffset::new(0))),
                cx,
            )
            .unwrap();
    });
    let _ = drive_pages(&input, cx, source);
    input.update(cx, |input, cx| {
        input.accept_mutation_preflight(intent.key(), cx).unwrap();
        assert!(matches!(
            input.stage_history_fragment(
                MutationFragment::new(intent.key(), 0, MutationFragmentPayload::Terminal),
                cx,
            ),
            Err(gpui_text_input::RangeTextInputError::Mutation(
                gpui_text_input::MutationError::IncoherentSuccessor
            ))
        ));
        assert!(input.take_request().is_none());
        assert!(input.is_quiescent());
        assert_eq!(input.surface().unwrap().binding(), binding(source, 1));
    });
}

#[gpui::test]
fn history_accepts_successor_end_selection_at_exact_staging_caps(cx: &mut gpui::TestAppContext) {
    cx.update(ensure_text_input_bindings);
    let source = "old";
    let successor = "\u{00e9}";
    let mut configuration = config(source, 1);
    configuration.mutation_limits = MutationLimits::new(2, successor.len()).unwrap();
    let (input, cx) = cx.add_window_view(|window, cx| {
        let input = RangeTextInput::new(configuration, window, cx).unwrap();
        input.focus(window);
        input
    });
    assert!(drive_pages(&input, cx, source).is_empty());

    cx.simulate_keystrokes("ctrl-z");
    let intent = drive_pages(&input, cx, source)
        .into_iter()
        .find_map(|request| match request {
            RangeTextInputRequest::HistoryIntent(intent) => Some(intent),
            _ => None,
        })
        .unwrap();
    let proposal = MutationProposal::new(
        intent.key(),
        MutationKind::Undo,
        ByteRange::from_u64(0, source.len() as u64).unwrap(),
        0,
    );
    input.update(cx, |input, cx| {
        input
            .submit_history_plan(
                RangeHistoryPlan::new(
                    intent,
                    proposal,
                    RangeSelection::caret(ByteOffset::new(successor.len() as u64)),
                ),
                cx,
            )
            .unwrap();
    });
    let _ = drive_pages(&input, cx, source);
    input.update(cx, |input, cx| {
        input.accept_mutation_preflight(intent.key(), cx).unwrap();
        input
            .stage_history_fragment(
                MutationFragment::new(
                    intent.key(),
                    0,
                    MutationFragmentPayload::Utf8 {
                        inserted_offset: 0,
                        text: successor.to_owned(),
                    },
                ),
                cx,
            )
            .unwrap();
        input
            .stage_history_fragment(
                MutationFragment::new(intent.key(), 1, MutationFragmentPayload::Terminal),
                cx,
            )
            .unwrap();
    });
    let staged = drive_pages(&input, cx, source);
    assert!(staged.iter().any(
        |request| matches!(request, RangeTextInputRequest::MutationCommit(key) if *key == intent.key())
    ));
    input.update(cx, |input, _| {
        input.admit_mutation_commit(intent.key()).unwrap()
    });
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input
                .settle_mutation(
                    intent.key(),
                    MutationOutcome::Committed(binding(successor, 2)),
                    window,
                    cx,
                )
                .unwrap();
        })
    });
    assert!(drive_pages(&input, cx, successor).is_empty());
    input.read_with(cx, |input, _| {
        assert_eq!(
            input.surface().unwrap().selection(),
            RangeSelection::caret(ByteOffset::new(successor.len() as u64))
        );
        assert!(input.is_quiescent());
    });
}

#[gpui::test]
fn disposal_distinguishes_undispatched_and_dispatched_work(cx: &mut gpui::TestAppContext) {
    let source = "pending work";
    let (undispatched, cx) = cx
        .add_window_view(|window, cx| RangeTextInput::new(config(source, 1), window, cx).unwrap());
    let drained =
        cx.update(|window, app| undispatched.update(app, |input, cx| input.dispose(window, cx)));
    assert!(
        drained.is_empty(),
        "undispatched work needs no host cancellation"
    );

    let (dispatched, cx) = cx
        .add_window_view(|window, cx| RangeTextInput::new(config(source, 1), window, cx).unwrap());
    let page = dispatched
        .update(cx, |input, _| input.take_request())
        .and_then(|request| match request {
            RangeTextInputRequest::Page(page) => Some(page),
            _ => None,
        })
        .expect("initial exact index page");
    let drained =
        cx.update(|window, app| dispatched.update(app, |input, cx| input.dispose(window, cx)));
    assert!(drained.iter().any(
        |request| matches!(request, RangeTextInputRequest::CancelPage(key) if *key == page.key())
    ));
    assert!(!drained.iter().any(
        |request| matches!(request, RangeTextInputRequest::ReleasePage(key) if *key == page.key())
    ));
}

#[gpui::test]
fn disposed_widget_cannot_restart_mounted_geometry_or_restoration(cx: &mut gpui::TestAppContext) {
    let source = "mounted lifecycle";
    let configuration = config(source, 1);
    let layout = configuration.layout.clone();
    let style = configuration.style.clone();
    let (input, cx) =
        cx.add_window_view(|window, cx| RangeTextInput::new(configuration, window, cx).unwrap());
    assert!(drive_pages(&input, cx, source).is_empty());
    let seed = input.read_with(cx, |input, _| input.export_restoration(None).unwrap());

    cx.update(|window, app| {
        input.update(app, |input, cx| {
            assert!(input.dispose(window, cx).is_empty());
            assert!(matches!(
                input.set_layout(layout, style, cx),
                Err(gpui_text_input::RangeTextInputError::NotMounted)
            ));
            assert!(matches!(
                input.request_absolute_scroll(px(16.), cx),
                Err(gpui_text_input::RangeTextInputError::NotMounted)
            ));
            assert!(matches!(
                input.import_restoration(seed, cx),
                Err(gpui_text_input::RangeTextInputError::NotMounted)
            ));
            assert!(matches!(
                input.rebind(binding(source, 2), None, window, cx),
                Err(gpui_text_input::RangeTextInputError::NotMounted)
            ));
            assert!(input.take_request().is_none());
        });
    });
}

#[gpui::test]
fn failed_successor_geometry_retains_prior_surface_and_late_page_is_released(
    cx: &mut gpui::TestAppContext,
) {
    let source = "prior surface";
    let (input, cx) = cx
        .add_window_view(|window, cx| RangeTextInput::new(config(source, 1), window, cx).unwrap());
    assert!(drive_pages(&input, cx, source).is_empty());
    let prior = input.read_with(cx, |input, _| input.surface().unwrap().binding());

    let successor = "successor text";
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input
                .rebind(binding(successor, 2), None, window, cx)
                .unwrap();
        })
    });
    let request = input
        .update(cx, |input, _| input.take_request())
        .and_then(|request| match request {
            RangeTextInputRequest::Page(page) => Some(page),
            _ => None,
        })
        .expect("successor geometry page");
    input.update(cx, |input, cx| {
        input
            .fail_page(request.key(), PageFailure::Unavailable, cx)
            .unwrap();
        assert_eq!(input.surface().unwrap().binding(), prior);
    });

    let late = page_for(successor, 99, request);
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            assert!(input.deliver_page(late, window, cx).is_err());
        })
    });
    let lifecycle = (0..8)
        .filter_map(|_| input.update(cx, |input, _| input.take_request()))
        .collect::<Vec<_>>();
    assert!(
        lifecycle
            .iter()
            .any(|request| matches!(request, RangeTextInputRequest::ReleasePage(_)))
    );
    assert_eq!(
        input.read_with(cx, |input, _| input.surface().unwrap().binding()),
        prior
    );
}
