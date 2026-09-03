use std::{cell::RefCell, rc::Rc, sync::Arc};

#[cfg(feature = "test-support")]
use std::num::NonZeroUsize;

#[path = "range_widget/propagation.rs"]
mod propagation;
#[path = "range_widget/range_widget_legacy_contracts.rs"]
mod range_widget_legacy_contracts;
#[path = "range_widget/range_widget_protocol.rs"]
mod range_widget_protocol;

use gpui::{
    EntityInputHandler, Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, ScrollDelta,
    ScrollWheelEvent, SharedString, StreamingLayoutBinding, StreamingLayoutFragment,
    StreamingLayoutLimits, StreamingLayoutPosition, TextRun, black, font, point, px,
};
use gpui_scrollbar::ScrollbarStyle;
use gpui_text_input::{
    AtomFact, AtomId, BindingId, ByteOffset, ByteRange, ClipboardLimits, ClipboardProvenanceLimits,
    ClipboardProvenancePolicy, ClipboardWriteOutcome, ExactGeometryLimits, ExactGeometryOwner,
    InlineObjectFact, InlineObjectGap, InlineObjectId, InlineObjectNeighbor, InlineObjectOrder,
    InlineObjectPresentation, InlineObjectSurfaceDismissal, LogicalExtent, MutationBeginRequest,
    MutationCursor, MutationKey, MutationKind, MutationLimits, MutationPositions, MutationProposal,
    ObjectDemand, ObjectDemandEnvelope, ObjectDirection, ObjectPage, ObjectPageEdgeFact,
    ObjectPageId, ObjectPurpose, ObjectRequestId, ObjectResidency, ObjectResidencyLimits,
    PageDemand, PageDemandEnvelope, PageDirection, PageEdgeFact, PageFailure, PageId, PagePurpose,
    PageRequestId, PlatformRangeResult, PresentationGeneration, RangeBinding, RangeHistoryFrontier,
    RangePage, RangeResidency, RangeRestorationScrollAnchor, RangeRestorationSeed, RangeSelection,
    RangeSourceSelection, RangeSurfaceHit, RangeTextInput, RangeTextInputConfig,
    RangeTextInputEvent, RangeTextInputLimits, RangeTextInputRequest, ResidencyLimits,
    SegmentationLimits, SourcePosition, SourceRange, SourceRevision, StreamingGeometryStyle,
    StreamingOversizePresentation, TextInputAtomClipboardPolicy, TextInputEnterKey,
    TextInputRichPastePolicy, TextInputTheme, ensure_text_input_bindings,
};

#[gpui::test]
fn clipboard_begin_exact_peak_follows_the_ordinary_request_path(cx: &mut gpui::TestAppContext) {
    let source = "a";
    let start = ordinary_position(0);
    let end = ordinary_position(1);
    let selection = SourceRange::new(start, end).unwrap();
    let predecessor = MutationPositions::new(end, start, end);
    let clipboard_limits = ClipboardLimits::new_composite(64, 4, 1, 4096)
        .unwrap()
        .with_provenance(ClipboardProvenancePolicy::Stream(
            ClipboardProvenanceLimits::new(2, 4096).unwrap(),
        ));
    let configured = |bytes, items| {
        let mut configuration = config(source, 1);
        configuration.clipboard_limits = clipboard_limits;
        configuration.limits.max_surface_bytes = bytes;
        configuration.limits.max_surface_items = items;
        configuration
    };

    let (probe, cx) = cx.add_window_view(|window, cx| {
        RangeTextInput::new(configured(16 * 1024 * 1024, 2 * 1024 * 1024), window, cx).unwrap()
    });
    assert!(drive_pages(&probe, cx, source).is_empty());

    let begin = |input: &gpui::Entity<RangeTextInput>, cx: &mut gpui::VisualTestContext| {
        let (text, objects) = admitted_sources(source, 1, &[start, end]);
        input.update(cx, |input, cx| {
            input.begin_composite_clipboard(
                gpui_text_input::ClipboardKind::Copy,
                selection,
                predecessor,
                &text,
                &objects,
                cx,
            )
        })
    };

    begin(&probe, cx).unwrap();
    assert!(matches!(
        probe.update(cx, |input, _| input.take_request()),
        Some(RangeTextInputRequest::ObjectPage(_))
    ));
    let high_water = probe.read_with(cx, |input, _| input.realization_diagnostics().high_water);
    let exact_bytes = high_water.owned_bytes;
    let exact_items = high_water.owned_items;

    let (exact, cx) = cx.add_window_view(|window, cx| {
        RangeTextInput::new(configured(exact_bytes, exact_items), window, cx).unwrap()
    });
    assert!(drive_pages(&exact, cx, source).is_empty());
    begin(&exact, cx).unwrap();
    assert!(matches!(
        exact.update(cx, |input, _| input.take_request()),
        Some(RangeTextInputRequest::ObjectPage(_))
    ));
}

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

fn ordinary_range(range: ByteRange) -> SourceRange {
    SourceRange::new(
        ordinary_position(range.start().get()),
        ordinary_position(range.end().get()),
    )
    .unwrap()
}

fn ordinary_position(offset: u64) -> SourcePosition {
    SourcePosition::new(ByteOffset::new(offset), InlineObjectGap::NoObjects)
}

#[derive(Debug, PartialEq)]
struct RangeSurfaceFingerprint {
    binding: RangeBinding,
    geometry: gpui_text_input::GeometryKey,
    selection: RangeSourceSelection,
    scroll_source: ByteOffset,
    scroll_block: gpui::Pixels,
    charge: gpui_text_input::RangeSurfaceCharge,
}

#[derive(Debug, PartialEq)]
struct RangePublicationFingerprint {
    surface: RangeSurfaceFingerprint,
    admission: Option<gpui_text_input::RangeSurfaceCharge>,
}

fn range_publication_fingerprint(
    input: &gpui::Entity<RangeTextInput>,
    cx: &mut gpui::VisualTestContext,
) -> RangePublicationFingerprint {
    input.read_with(cx, |input, _| range_publication_fingerprint_from(input))
}

fn range_publication_fingerprint_from(input: &RangeTextInput) -> RangePublicationFingerprint {
    let surface = input.surface().unwrap();
    RangePublicationFingerprint {
        surface: RangeSurfaceFingerprint {
            binding: surface.binding(),
            geometry: surface.geometry_key(),
            selection: surface.selection(),
            scroll_source: surface.scroll_source(),
            scroll_block: surface.scroll_block(),
            charge: surface.charge(),
        },
        admission: input.last_surface_admission_charge(),
    }
}

fn restoration_seed(source: &str, revision: u64, position: SourcePosition) -> RangeRestorationSeed {
    RangeRestorationSeed {
        binding: binding(source, revision),
        caret: position,
        selection: RangeSourceSelection::caret(position),
        scroll: RangeRestorationScrollAnchor {
            position,
            intra_anchor: px(0.),
        },
        history: None,
    }
}

#[test]
fn restoration_seed_is_copy_only_compact_logical_state() {
    assert!(!std::mem::needs_drop::<RangeRestorationSeed>());
    assert!(std::mem::size_of::<RangeRestorationSeed>() <= 512);
}

fn object_neighbor(id: u128, order: u128) -> InlineObjectNeighbor {
    InlineObjectNeighbor::new(InlineObjectId::new(id), InlineObjectOrder::new(order))
}

fn object_fact(id: u128, anchor: u64, order: u128) -> InlineObjectFact {
    object_fact_with_activation(id, anchor, order, true)
}

fn object_fact_with_fallback(
    id: u128,
    anchor: u64,
    order: u128,
    fallback: String,
) -> InlineObjectFact {
    InlineObjectFact::new(
        InlineObjectId::new(id),
        ByteOffset::new(anchor),
        InlineObjectOrder::new(order),
        fallback,
        InlineObjectPresentation::new(
            id as u64,
            SharedString::new_static(""),
            px(10.),
            px(10.),
            px(0.),
            None,
            0,
            true,
        )
        .unwrap(),
    )
}

fn object_fact_with_activation(
    id: u128,
    anchor: u64,
    order: u128,
    activation_eligible: bool,
) -> InlineObjectFact {
    object_fact_with_width_and_activation(id, anchor, order, px(10.), activation_eligible)
}

fn object_fact_with_width_and_activation(
    id: u128,
    anchor: u64,
    order: u128,
    width: gpui::Pixels,
    activation_eligible: bool,
) -> InlineObjectFact {
    InlineObjectFact::new(
        InlineObjectId::new(id),
        ByteOffset::new(anchor),
        InlineObjectOrder::new(order),
        format!("[{id}]"),
        InlineObjectPresentation::new(
            id as u64,
            SharedString::new_static(""),
            width,
            px(10.),
            px(0.),
            None,
            0,
            activation_eligible,
        )
        .unwrap(),
    )
}

fn admitted_sources(
    source: &str,
    revision: u64,
    positions: &[SourcePosition],
) -> (RangeResidency, ObjectResidency) {
    admitted_sources_with_facts(source, revision, positions, &[])
}

fn admitted_sources_with_facts(
    source: &str,
    revision: u64,
    positions: &[SourcePosition],
    facts: &[InlineObjectFact],
) -> (RangeResidency, ObjectResidency) {
    let binding = binding(source, revision);
    let mut text_residency = RangeResidency::new(
        binding,
        ResidencyLimits::new(8, 128 * 1024, 8, 128 * 1024).unwrap(),
    );
    let PageDemand::Requested(text_request) = text_residency
        .demand(
            PageRequestId::new(91_000 + revision),
            PagePurpose::Caret,
            PageDemandEnvelope::Adjacent {
                anchor: ByteOffset::new(0),
                direction: PageDirection::Forward,
                max_payload_bytes: (source.len() as u64).max(4),
            },
        )
        .unwrap()
    else {
        panic!("expected text request")
    };
    let text = RangePage::new(
        PageId::new(91_000 + revision),
        text_request.key(),
        ByteRange::from_u64(0, source.len() as u64).unwrap(),
        source.to_owned(),
        vec![],
        PageEdgeFact::DocumentBoundary,
        PageEdgeFact::DocumentBoundary,
        true,
    )
    .unwrap();
    text_residency.admit(text).unwrap();
    let mut objects = ObjectResidency::new(
        binding,
        PresentationGeneration::new(1),
        ObjectResidencyLimits::new(8, 16, 128 * 1024, 64 * 1024, 8, 16, 128 * 1024).unwrap(),
    );
    let mut offsets = Vec::new();
    for position in positions {
        if offsets.contains(&position.byte_offset) {
            continue;
        }
        offsets.push(position.byte_offset);
        let demand = ObjectDemandEnvelope::anchor(
            position.byte_offset,
            None,
            ObjectDirection::Forward,
            facts
                .iter()
                .filter(|fact| fact.anchor() == position.byte_offset)
                .count()
                .max(1),
            4096,
        )
        .unwrap();
        let ObjectDemand::Requested(request) = objects
            .demand(
                ObjectRequestId::new(92_000 + revision + offsets.len() as u64),
                ObjectPurpose::MutationSuccessor,
                demand,
            )
            .unwrap()
        else {
            panic!("expected object request")
        };
        let page = ObjectPage::new(
            ObjectPageId::new(92_000 + revision + offsets.len() as u64),
            request.key(),
            facts
                .iter()
                .filter(|fact| fact.anchor() == position.byte_offset)
                .cloned()
                .collect(),
            ObjectPageEdgeFact::EnvelopeBoundary,
            ObjectPageEdgeFact::EnvelopeBoundary,
            true,
            None,
        )
        .unwrap();
        let proofs = text_residency
            .prove_object_page_anchors(binding, &page)
            .unwrap();
        objects.admit(page, proofs).unwrap();
    }
    (text_residency, objects)
}

fn admit_ordinary_edit_positions(
    input: &mut RangeTextInput,
    source: &str,
    revision: u64,
    offsets: &[u64],
) {
    let positions: Vec<_> = offsets.iter().copied().map(ordinary_position).collect();
    let (text, objects) = admitted_sources(source, revision, &positions);
    input
        .admit_edit_positions(&positions, &text, &objects)
        .unwrap();
}

#[gpui::test]
fn host_mutation_protocol_rejects_undo_and_redo_before_operation_claim(
    cx: &mut gpui::TestAppContext,
) {
    let source = "history protocol boundary";
    let configuration = config(source, 1);
    let coordinator = configuration.settlement_coordinator.clone();
    let (input, cx) =
        cx.add_window_view(|window, cx| RangeTextInput::new(configuration, window, cx).unwrap());
    assert!(drive_pages(&input, cx, source).is_empty());
    let current = input.read_with(cx, |input, _| input.surface().unwrap().selection().head);
    let (text, objects) = admitted_sources(source, 1, &[current]);
    let base = binding(source, 1);
    let before = range_publication_fingerprint(&input, cx);
    let proposal = |kind, operation| {
        MutationBeginRequest::new(
            MutationProposal::new(
                MutationKey::new(base.binding(), base.revision(), operation),
                kind,
                MutationPositions::collapsed(current),
                SourceRange::new(current, current).unwrap(),
                0,
            ),
            MutationCursor::new(0),
            MutationCursor::new(0),
        )
    };

    for kind in [MutationKind::Undo, MutationKind::Redo] {
        input.update(cx, |input, cx| {
            let operation = input.lease_host_operation().unwrap();
            let operation_id = operation.operation();
            assert!(matches!(
                input.begin_host_mutation(
                    operation,
                    proposal(kind, operation_id),
                    &[current],
                    &text,
                    &objects,
                    cx
                ),
                Err(gpui_text_input::RangeTextInputError::UnsupportedMutationKind)
            ));
            assert!(input.take_request().is_none());
        });
        assert_eq!(range_publication_fingerprint(&input, cx), before);
        assert_eq!(coordinator.retained_count(), 0);
    }

    input.update(cx, |input, cx| {
        let operation = input.lease_host_operation().unwrap();
        let operation_id = operation.operation();
        let edit = proposal(MutationKind::Edit, operation_id);
        assert_eq!(
            input
                .begin_host_mutation(operation, edit, &[current], &text, &objects, cx)
                .unwrap(),
            edit.proposal().key()
        );
        assert!(matches!(
            input.take_request(),
            Some(RangeTextInputRequest::MutationBegin(request)) if request == edit
        ));
    });
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
                Err(gpui_text_input::RangeTextInputError::PageResponseRejected(
                    _
                ))
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
        admit_ordinary_edit_positions(input, source, 1, &[0]);
    });
    input.read_with(cx, |input, _| {
        assert!(input.export_restoration(None).is_ok());
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
            assert_eq!(input.clipboard_counts(), Default::default());
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
fn unpublished_surface_does_not_admit_normal_clipboard_custody(cx: &mut gpui::TestAppContext) {
    let source = "unpublished";
    let (input, cx) = cx
        .add_window_view(|window, cx| RangeTextInput::new(config(source, 1), window, cx).unwrap());
    input.update(cx, |input, cx| {
        assert!(input.surface().is_none());
        for kind in [
            gpui_text_input::ClipboardKind::Copy,
            gpui_text_input::ClipboardKind::Cut,
        ] {
            assert!(matches!(
                input.begin_clipboard(kind, cx),
                Err(gpui_text_input::RangeTextInputError::Busy)
            ));
        }
        assert_eq!(input.clipboard_counts(), Default::default());
    });
    assert!(drive_pages(&input, cx, source).is_empty());
}

fn assert_normal_clipboard_blocked_without_custody(
    input: &gpui::Entity<RangeTextInput>,
    cx: &mut gpui::TestAppContext,
) {
    let before = input.read_with(cx, |input, _| {
        let ownership = input.realization_diagnostics().current;
        (
            input.surface().map(|surface| surface.binding()),
            input.surface().map(|surface| surface.selection()),
            ownership.queued_requests,
            ownership.response_custody_count,
            ownership.response_custody_bytes,
            ownership.response_custody_items,
            ownership.clipboard_bytes,
            ownership.clipboard_items,
        )
    });
    input.update(cx, |input, cx| {
        for kind in [
            gpui_text_input::ClipboardKind::Copy,
            gpui_text_input::ClipboardKind::Cut,
        ] {
            assert!(matches!(
                input.begin_clipboard(kind, cx),
                Err(gpui_text_input::RangeTextInputError::Busy)
            ));
        }
        assert_eq!(input.clipboard_counts(), Default::default());
    });
    input.read_with(cx, |input, _| {
        let ownership = input.realization_diagnostics().current;
        assert_eq!(input.surface().map(|surface| surface.binding()), before.0);
        assert_eq!(input.surface().map(|surface| surface.selection()), before.1);
        assert_eq!(ownership.queued_requests, before.2);
        assert_eq!(ownership.response_custody_count, before.3);
        assert_eq!(ownership.response_custody_bytes, before.4);
        assert_eq!(ownership.response_custody_items, before.5);
        assert_eq!(ownership.clipboard_bytes, before.6);
        assert_eq!(ownership.clipboard_items, before.7);
        assert_eq!(input.clipboard_counts(), Default::default());
    });
}

#[gpui::test]
fn pending_history_does_not_admit_normal_clipboard_custody_or_deletion(
    cx: &mut gpui::TestAppContext,
) {
    cx.update(ensure_text_input_bindings);
    let source = "pending history";
    let (input, cx) = cx.add_window_view(|window, cx| {
        let input = RangeTextInput::new(config(source, 1), window, cx).unwrap();
        input.focus(window);
        input
    });
    assert!(drive_pages(&input, cx, source).is_empty());
    cx.simulate_keystrokes("ctrl-a");
    assert!(drive_pages(&input, cx, source).is_empty());
    input.update(cx, |input, _| {
        let prior = input.history_frontier();
        input
            .set_history_frontier(
                prior,
                RangeHistoryFrontier {
                    binding: binding(source, 1),
                    id: prior.id + 1,
                    undo_available: true,
                    redo_available: false,
                },
            )
            .unwrap();
    });
    cx.simulate_keystrokes("ctrl-z");
    assert!(matches!(
        input.update(cx, |input, _| input.take_request()),
        Some(RangeTextInputRequest::HistoryIntent(intent))
            if intent.kind() == MutationKind::Undo
    ));
    input.update(cx, |input, cx| {
        for kind in [
            gpui_text_input::ClipboardKind::Copy,
            gpui_text_input::ClipboardKind::Cut,
        ] {
            assert!(matches!(
                input.begin_clipboard(kind, cx),
                Err(gpui_text_input::RangeTextInputError::Busy)
            ));
        }
        assert_eq!(input.clipboard_counts(), Default::default());
        assert!(input.take_request().is_none());
    });
    cx.update(|window, app| input.update(app, |input, cx| input.dispose(window, cx)));
}

#[gpui::test]
fn empty_rebind_with_canonical_selection_completes_prepared_publication(
    cx: &mut gpui::TestAppContext,
) {
    let source = "prior publication";
    let prior = binding(source, 1);
    let successor = binding("", 2);
    let position = SourcePosition::new(ByteOffset::new(0), InlineObjectGap::NoObjects);
    let selection = RangeSourceSelection {
        anchor: position,
        head: position,
    };
    let (input, cx) = cx
        .add_window_view(|window, cx| RangeTextInput::new(config(source, 1), window, cx).unwrap());
    assert!(drive_pages(&input, cx, source).is_empty());

    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input
                .rebind(successor, Some(selection), window, cx)
                .unwrap();
            assert_eq!(input.surface().unwrap().binding(), prior);
        });
    });

    assert!(drive_pages(&input, cx, "").is_empty());
    input.read_with(cx, |input, _| {
        assert!(input.is_quiescent());
        let surface = input.surface().unwrap();
        assert_eq!(surface.binding(), successor);
        assert_eq!(surface.selection(), selection);
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
    assert!(matches!(
        input.update(cx, |input, _| input.take_request()),
        Some(RangeTextInputRequest::CancelPage(key)) if key == obsolete.key()
    ));
    let second = input
        .update(cx, |input, _| input.take_request())
        .and_then(|request| match request {
            RangeTextInputRequest::Page(page) => Some(page),
            _ => None,
        })
        .expect("second target page follows its predecessor cancellation");
    input.update(cx, |input, cx| {
        input.request_absolute_scroll(px(224.), cx).unwrap()
    });
    assert!(matches!(
        input.update(cx, |input, _| input.take_request()),
        Some(RangeTextInputRequest::CancelPage(key)) if key == second.key()
    ));
    let final_request = input
        .update(cx, |input, _| input.take_request())
        .and_then(|request| match request {
            RangeTextInputRequest::Page(page) => Some(page),
            _ => None,
        })
        .expect("third target page follows the second cancellation");
    let obsolete_page = page_for(source, 900, obsolete);
    let result = cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.deliver_page(obsolete_page, window, cx)
        })
    });
    assert!(result.is_err(), "obsolete target input must be rejected");
    let second_page = page_for(source, 901, second);
    assert!(
        cx.update(|window, app| {
            input.update(app, |input, cx| input.deliver_page(second_page, window, cx))
        })
        .is_err()
    );
    input.read_with(cx, |input, _| {
        assert_eq!(input.surface().unwrap().geometry_key(), prior_geometry);
    });
    let final_page = page_for(source, 902, final_request);
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.deliver_page(final_page, window, cx).unwrap()
        })
    });
    let _ = drive_pages(&input, cx, source);
    input.read_with(cx, |input, _| {
        let block = input.surface().unwrap().scroll_block();
        assert_eq!(block, px(216.));
        assert!(input.is_quiescent());
    });
}

fn config(source: &str, revision: u64) -> RangeTextInputConfig {
    let layout = StreamingLayoutBinding {
        input_id: 11,
        segment_policy_id: 13,
        start_position: StreamingLayoutPosition::at(0),
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
            retained_items: 4096,
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
        presentation_generation: PresentationGeneration::new(1),
        enter_key: TextInputEnterKey::InsertNewline,
        atom_clipboard_policy: TextInputAtomClipboardPolicy::PlainText,
        rich_paste_policy: TextInputRichPastePolicy::PlainText,
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
        object_residency_limits: ObjectResidencyLimits::new(
            4,
            32,
            128 * 1024,
            64 * 1024,
            4,
            32,
            128 * 1024,
        )
        .unwrap(),
        mutation_limits: MutationLimits::new(8, 256).unwrap(),
        clipboard_limits: ClipboardLimits::new(1024, 32).unwrap(),
        segmentation_limits: SegmentationLimits::new(32, 64).unwrap(),
        limits: RangeTextInputLimits::new(2 * 1024 * 1024, 32768, 8, px(80.), 32, 32, px(16.))
            .unwrap(),
        settlement_coordinator: gpui_text_input::RangeSettlementCoordinator::new(4).unwrap(),
        viewport_extent: px(80.),
        overscan: px(32.),
        placeholder: SharedString::new_static("Value"),
        theme: TextInputTheme::default(),
        scrollbar_style: ScrollbarStyle::default(),
    }
}

fn replacement_geometry_style() -> StreamingGeometryStyle {
    let presentation = Arc::<str>::from("r".repeat(64 * 1024));
    let presentation_len = presentation.len();
    StreamingGeometryStyle::new(
        TextRun {
            len: 0,
            font: font("ReplacementFamily"),
            color: black(),
            background_color: None,
            underline: None,
            strikethrough: None,
        },
        StreamingOversizePresentation::new(
            SharedString::new(presentation),
            vec![TextRun {
                len: presentation_len,
                font: font("ReplacementFamily"),
                color: black(),
                background_color: None,
                underline: None,
                strikethrough: None,
            }],
            px(12.),
            px(16.),
            px(12.),
            None,
        ),
    )
}

fn one_under_geometry_replacement_config(source: &str) -> RangeTextInputConfig {
    let mut configuration = config(source, 1);
    configuration.layout.limits.segment_bytes = 64 * 1024;
    configuration.style = replacement_geometry_style();
    let limits = configuration.geometry_limits;
    let mut probe = ExactGeometryOwner::new(
        configuration.binding,
        configuration.presentation_generation,
        configuration.layout.clone(),
        configuration.style.clone(),
        limits,
    )
    .unwrap();
    probe
        .start_index(gpui_text_input::GeometryJobId::new(1))
        .unwrap();
    let counts = probe.counts();
    let replacement_peak = counts.total_bytes() + counts.input_bytes;
    configuration.geometry_limits = ExactGeometryLimits::new(
        limits.max_page_bytes(),
        limits.max_checkpoints(),
        replacement_peak - 1,
        limits.max_retained_items(),
    )
    .unwrap();
    configuration
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

fn page_for_with_local_atom(
    source: &str,
    id: u64,
    request: gpui_text_input::PageRequest,
    atom: AtomId,
    fallback: &str,
) -> RangePage {
    let key = request.key();
    let base = page_for(source, id, request);
    RangePage::new(
        base.id(),
        key,
        base.range(),
        base.text().to_owned(),
        vec![AtomFact::new(atom, base.range(), base.range(), fallback)],
        base.preceding(),
        base.following(),
        base.end_of_source(),
    )
    .unwrap()
}

fn page_for_split_atom(
    source: &str,
    id: u64,
    request: gpui_text_input::PageRequest,
    atom: AtomId,
    global_range: ByteRange,
    fallback: &str,
) -> RangePage {
    let key = request.key();
    let PageDemandEnvelope::Adjacent {
        anchor,
        direction: PageDirection::Forward,
        max_payload_bytes,
    } = key.demand()
    else {
        return page_for(source, id, request);
    };
    let start = anchor.get() as usize;
    let end = start
        .saturating_add(4)
        .min(start.saturating_add(max_payload_bytes as usize))
        .min(source.len());
    let range = ByteRange::from_u64(start as u64, end as u64).unwrap();
    let atoms = global_range
        .intersection(range)
        .filter(|fragment| !fragment.is_empty())
        .map(|fragment| vec![AtomFact::new(atom, global_range, fragment, fallback)])
        .unwrap_or_default();
    RangePage::new(
        PageId::new(id),
        key,
        range,
        source[start..end].to_owned(),
        atoms,
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

fn malformed_geometry_text_page(
    id: u64,
    request: gpui_text_input::PageRequest,
    source_len: usize,
) -> RangePage {
    let PageDemandEnvelope::Adjacent {
        anchor, direction, ..
    } = request.key().demand()
    else {
        panic!("geometry requests adjacent text")
    };
    let (start, end) = match direction {
        PageDirection::Forward => (anchor.get(), anchor.get() + 1),
        PageDirection::Backward => (anchor.get() - 1, anchor.get()),
    };
    assert!(
        end < source_len as u64,
        "fixture must claim a premature source end"
    );
    RangePage::new(
        PageId::new(id),
        request.key(),
        ByteRange::from_u64(start, end).unwrap(),
        "x".to_owned(),
        vec![],
        if start == 0 {
            PageEdgeFact::DocumentBoundary
        } else {
            PageEdgeFact::Continues
        },
        PageEdgeFact::DocumentBoundary,
        true,
    )
    .unwrap()
}

fn restoration_object_page(
    request: gpui_text_input::ObjectRequest,
    facts: &[InlineObjectFact],
    id: u64,
) -> ObjectPage {
    let demand = request.key().demand();
    let eligible = facts
        .iter()
        .filter(|fact| demand.contains_anchor(fact.anchor()))
        .filter(|fact| match demand.direction() {
            ObjectDirection::Forward => demand.cursor().is_none_or(|cursor| fact.cursor() > cursor),
            ObjectDirection::Backward => {
                demand.cursor().is_none_or(|cursor| fact.cursor() < cursor)
            }
        })
        .collect::<Vec<_>>();
    let count = eligible.len().min(demand.max_objects());
    let start = match demand.direction() {
        ObjectDirection::Forward => 0,
        ObjectDirection::Backward => eligible.len() - count,
    };
    let objects = eligible[start..]
        .iter()
        .map(|fact| (*fact).clone())
        .collect::<Vec<_>>();
    let complete = count == eligible.len();
    let continuation = (!complete).then(|| match demand.direction() {
        ObjectDirection::Forward => objects.last().unwrap().cursor(),
        ObjectDirection::Backward => objects.first().unwrap().cursor(),
    });
    let cursor_edge = demand.cursor().map_or(
        ObjectPageEdgeFact::EnvelopeBoundary,
        ObjectPageEdgeFact::Continues,
    );
    let (preceding, following) = match demand.direction() {
        ObjectDirection::Forward => (
            cursor_edge,
            continuation.map_or(
                ObjectPageEdgeFact::EnvelopeBoundary,
                ObjectPageEdgeFact::Continues,
            ),
        ),
        ObjectDirection::Backward => (
            continuation.map_or(
                ObjectPageEdgeFact::EnvelopeBoundary,
                ObjectPageEdgeFact::Continues,
            ),
            cursor_edge,
        ),
    };
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

fn take_clipboard_provenance_page(
    input: &gpui::Entity<RangeTextInput>,
    cx: &mut gpui::VisualTestContext,
    source: &str,
    facts: &[InlineObjectFact],
    page_id: &mut u64,
) -> gpui_text_input::ClipboardProvenancePage {
    try_take_clipboard_provenance_page(input, cx, source, facts, page_id).unwrap()
}

fn take_request_after_scheduled_frames(
    input: &gpui::Entity<RangeTextInput>,
    cx: &mut gpui::VisualTestContext,
    boundary: &str,
) -> RangeTextInputRequest {
    for _ in 0..256 {
        if let Some(request) = input.update(cx, |input, _| input.take_request()) {
            return request;
        }
        cx.update(|window, app| window.draw(app).clear());
        cx.run_until_parked();
    }
    panic!(
        "{boundary} did not dispatch within the bounded frame drive: {:?}",
        input.read_with(cx, |input, _| input.realization_diagnostics())
    )
}

fn try_take_clipboard_provenance_page(
    input: &gpui::Entity<RangeTextInput>,
    cx: &mut gpui::VisualTestContext,
    source: &str,
    facts: &[InlineObjectFact],
    page_id: &mut u64,
) -> Result<gpui_text_input::ClipboardProvenancePage, gpui_text_input::RangeTextInputError> {
    for _ in 0..256 {
        let Some(request) = input.update(cx, |input, _| input.take_request()) else {
            cx.update(|window, app| window.draw(app).clear());
            cx.run_until_parked();
            continue;
        };
        match request {
            RangeTextInputRequest::Page(request)
                if request.key().purpose() == PagePurpose::Clipboard =>
            {
                let page = page_for(source, *page_id, request);
                *page_id += 1;
                cx.update(|window, app| {
                    input.update(app, |input, cx| input.deliver_page(page, window, cx))
                })?;
            }
            RangeTextInputRequest::ObjectPage(request)
                if request.key().purpose() == ObjectPurpose::Clipboard =>
            {
                let page = restoration_object_page(request, facts, *page_id);
                *page_id += 1;
                cx.update(|window, app| {
                    input.update(app, |input, cx| {
                        input.deliver_object_page_in_window(page, window, cx)
                    })
                })?;
            }
            RangeTextInputRequest::ClipboardProvenancePage(page) => return Ok(page),
            RangeTextInputRequest::ReleasePage(_) | RangeTextInputRequest::ReleaseObjectPage(_) => {
            }
            other => panic!("unexpected clipboard request: {other:?}"),
        }
    }
    let capacity_rejected = input.read_with(cx, |input, _| {
        let diagnostics = input.realization_diagnostics();
        let counts = input.clipboard_counts();
        diagnostics.last_response_rejection
            == Some(gpui_text_input::RangeResponseRejectionClass::ResidencyCapacity)
            && counts.pending_object_pages != 0
            && counts.retained_object_facts != 0
    });
    if capacity_rejected {
        return Err(gpui_text_input::RangeTextInputError::SurfaceCapacity);
    }
    panic!(
        "clipboard provenance drive exhausted: {:?}",
        input.read_with(cx, |input, _| input.realization_diagnostics())
    )
}

fn begin_clipboard_to_provenance(
    input: &gpui::Entity<RangeTextInput>,
    cx: &mut gpui::VisualTestContext,
    source: &str,
    facts: &[InlineObjectFact],
    page_id: &mut u64,
) -> Result<gpui_text_input::ClipboardProvenancePage, gpui_text_input::RangeTextInputError> {
    let start = ordinary_position(0);
    let end = ordinary_position(source.len() as u64);
    let selection = SourceRange::new(start, end).unwrap();
    let (text, objects) = admitted_sources(source, 1, &[start, end]);
    input.update(cx, |input, cx| {
        input.begin_composite_clipboard(
            gpui_text_input::ClipboardKind::Copy,
            selection,
            MutationPositions::new(end, start, end),
            &text,
            &objects,
            cx,
        )
    })?;
    try_take_clipboard_provenance_page(input, cx, source, facts, page_id)
}

struct SplitAtomClipboardAttempt {
    key: gpui_text_input::ClipboardKey,
    provenance:
        Result<gpui_text_input::ClipboardProvenancePage, gpui_text_input::RangeTextInputError>,
    delivered_pages: Vec<gpui_text_input::PageRequestKey>,
    released_pages: Vec<gpui_text_input::PageRequestKey>,
    delivered_object_pages: Vec<gpui_text_input::ObjectRequestKey>,
    released_object_pages: Vec<gpui_text_input::ObjectRequestKey>,
}

fn assert_exact_clipboard_text_response_releases(
    clipboard: gpui_text_input::ClipboardKey,
    delivered: &[gpui_text_input::PageRequestKey],
    released: &[gpui_text_input::PageRequestKey],
) {
    assert!(!delivered.is_empty());
    assert_eq!(released.len(), delivered.len());
    for (index, key) in delivered.iter().enumerate() {
        assert_eq!(key.purpose(), PagePurpose::Clipboard);
        assert_eq!(key.binding(), clipboard.binding());
        assert_eq!(key.revision(), clipboard.revision());
        assert!(!delivered[..index].contains(key));
        assert_eq!(
            released.iter().filter(|released| *released == key).count(),
            1
        );
    }
    assert!(released.iter().all(|key| delivered.contains(key)));
}

fn assert_exact_clipboard_object_response_releases(
    clipboard: gpui_text_input::ClipboardKey,
    delivered: &[gpui_text_input::ObjectRequestKey],
    released: &[gpui_text_input::ObjectRequestKey],
) {
    assert_eq!(delivered.len(), 1);
    assert_eq!(released.len(), delivered.len());
    for (index, key) in delivered.iter().enumerate() {
        assert_eq!(key.purpose(), ObjectPurpose::Clipboard);
        assert_eq!(key.binding(), clipboard.binding());
        assert_eq!(key.revision(), clipboard.revision());
        assert!(!delivered[..index].contains(key));
        assert_eq!(
            released.iter().filter(|released| *released == key).count(),
            1
        );
    }
    assert!(released.iter().all(|key| delivered.contains(key)));
}

fn begin_split_atom_clipboard_to_provenance(
    input: &gpui::Entity<RangeTextInput>,
    cx: &mut gpui::VisualTestContext,
    source: &str,
    selection_end: u64,
    facts: &[InlineObjectFact],
    atom: AtomId,
    global_range: ByteRange,
    fallback: &str,
    page_id: &mut u64,
) -> Result<SplitAtomClipboardAttempt, gpui_text_input::RangeTextInputError> {
    let start = ordinary_position(0);
    let end = ordinary_position(selection_end);
    let selection = SourceRange::new(start, end).unwrap();
    let (text, objects) = admitted_sources(source, 1, &[start, end]);
    let mut delivered_pages = Vec::new();
    let mut released_pages = Vec::new();
    let mut delivered_object_pages = Vec::new();
    let mut released_object_pages = Vec::new();
    let key = input.update(cx, |input, cx| {
        input.begin_composite_clipboard(
            gpui_text_input::ClipboardKind::Copy,
            selection,
            MutationPositions::new(end, start, end),
            &text,
            &objects,
            cx,
        )
    })?;
    loop {
        let Some(request) = input.update(cx, |input, _| input.take_request()) else {
            let capacity_rejected = input.read_with(cx, |input, _| {
                let counts = input.clipboard_counts();
                counts.pending_object_pages == 0
                    && counts.retained_object_facts == 0
                    && counts.retained_provenance_items == 0
                    && counts.retained_provenance_bytes > 1024 * 1024
                    && counts.staged_bytes == 64
            });
            if capacity_rejected {
                return Ok(SplitAtomClipboardAttempt {
                    key,
                    provenance: Err(gpui_text_input::RangeTextInputError::SurfaceCapacity),
                    delivered_pages,
                    released_pages,
                    delivered_object_pages,
                    released_object_pages,
                });
            }
            cx.update(|window, app| window.draw(app).clear());
            cx.run_until_parked();
            continue;
        };
        match request {
            RangeTextInputRequest::Page(request)
                if request.key().purpose() == PagePurpose::Clipboard =>
            {
                delivered_pages.push(request.key());
                let page =
                    page_for_split_atom(source, *page_id, request, atom, global_range, fallback);
                *page_id += 1;
                let result = cx.update(|window, app| {
                    input.update(app, |input, cx| input.deliver_page(page, window, cx))
                });
                if let Err(error) = result {
                    return Ok(SplitAtomClipboardAttempt {
                        key,
                        provenance: Err(error),
                        delivered_pages,
                        released_pages,
                        delivered_object_pages,
                        released_object_pages,
                    });
                }
            }
            RangeTextInputRequest::ObjectPage(request)
                if request.key().purpose() == ObjectPurpose::Clipboard =>
            {
                delivered_object_pages.push(request.key());
                let page = restoration_object_page(request, facts, *page_id);
                *page_id += 1;
                let result = cx.update(|window, app| {
                    input.update(app, |input, cx| {
                        input.deliver_object_page_in_window(page, window, cx)
                    })
                });
                if let Err(error) = result {
                    return Ok(SplitAtomClipboardAttempt {
                        key,
                        provenance: Err(error),
                        delivered_pages,
                        released_pages,
                        delivered_object_pages,
                        released_object_pages,
                    });
                }
            }
            RangeTextInputRequest::ClipboardProvenancePage(page) => {
                return Ok(SplitAtomClipboardAttempt {
                    key,
                    provenance: Ok(page),
                    delivered_pages,
                    released_pages,
                    delivered_object_pages,
                    released_object_pages,
                });
            }
            RangeTextInputRequest::ReleasePage(key) => released_pages.push(key),
            RangeTextInputRequest::ReleaseObjectPage(key) => released_object_pages.push(key),
            other => panic!("unexpected split-atom clipboard request: {other:?}"),
        }
    }
}

fn hold_same_revision_geometry_target(
    input: &gpui::Entity<RangeTextInput>,
    cx: &mut gpui::VisualTestContext,
    target: gpui::Pixels,
) -> gpui_text_input::PageRequest {
    input.update(cx, |input, cx| {
        input.request_absolute_scroll(target, cx).unwrap()
    });
    match take_request_after_scheduled_frames(input, cx, "same-revision geometry target") {
        RangeTextInputRequest::Page(request)
            if request.key().purpose() == PagePurpose::GeometryTarget =>
        {
            request
        }
        other => panic!("unexpected same-revision geometry target request: {other:?}"),
    }
}

fn drain_rebound_surface_strict(
    input: &gpui::Entity<RangeTextInput>,
    cx: &mut gpui::VisualTestContext,
    source: &str,
) -> Vec<RangeTextInputRequest> {
    let mut lifecycle = Vec::new();
    let mut observed_quiescent = false;
    for _ in 0..256 {
        let Some(request) = input.update(cx, |input, _| input.take_request()) else {
            let quiescent = input.read_with(cx, |input, _| input.is_quiescent());
            if quiescent && observed_quiescent {
                return lifecycle;
            }
            observed_quiescent = quiescent;
            cx.update(|window, app| window.draw(app).clear());
            cx.run_until_parked();
            continue;
        };
        observed_quiescent = false;
        match request {
            RangeTextInputRequest::Page(request)
                if request.key().purpose() != PagePurpose::Clipboard =>
            {
                let page = page_for(source, request.key().id().get(), request);
                cx.update(|window, app| {
                    input.update(app, |input, cx| {
                        input.deliver_page(page, window, cx).unwrap()
                    })
                });
            }
            RangeTextInputRequest::ObjectPage(request)
                if request.key().purpose() != ObjectPurpose::Clipboard =>
            {
                let page = restoration_object_page(request, &[], request.key().id().get());
                cx.update(|window, app| {
                    input.update(app, |input, cx| {
                        input
                            .deliver_object_page_in_window(page, window, cx)
                            .unwrap()
                    })
                });
            }
            request @ (RangeTextInputRequest::ReleasePage(_)
            | RangeTextInputRequest::CancelPage(_)
            | RangeTextInputRequest::ReleaseObjectPage(_)
            | RangeTextInputRequest::CancelObjectPage(_)
            | RangeTextInputRequest::CancelClipboardProvenancePage(_)
            | RangeTextInputRequest::CancelClipboardWrite(_)) => lifecycle.push(request),
            other => panic!("unexpected rebound lifecycle request: {other:?}"),
        }
    }
    panic!(
        "rebound lifecycle did not become quiescent: {:?}",
        input.read_with(cx, |input, _| input.realization_diagnostics())
    );
}

fn drain_terminal_lifecycle_strict(
    input: &gpui::Entity<RangeTextInput>,
    cx: &mut gpui::VisualTestContext,
) -> Vec<RangeTextInputRequest> {
    let mut lifecycle = Vec::new();
    let mut observed_quiescent = false;
    for _ in 0..256 {
        let Some(request) = input.update(cx, |input, _| input.take_request()) else {
            let quiescent = input.read_with(cx, |input, _| input.is_quiescent());
            if quiescent && observed_quiescent {
                return lifecycle;
            }
            observed_quiescent = quiescent;
            cx.update(|window, app| window.draw(app).clear());
            cx.run_until_parked();
            continue;
        };
        observed_quiescent = false;
        match request {
            request @ (RangeTextInputRequest::ReleasePage(_)
            | RangeTextInputRequest::CancelPage(_)
            | RangeTextInputRequest::ReleaseObjectPage(_)
            | RangeTextInputRequest::CancelObjectPage(_)
            | RangeTextInputRequest::CancelClipboardProvenancePage(_)
            | RangeTextInputRequest::CancelClipboardWrite(_)) => lifecycle.push(request),
            other => panic!("unexpected terminal lifecycle request: {other:?}"),
        }
    }
    panic!(
        "terminal lifecycle did not become quiescent: {:?}",
        input.read_with(cx, |input, _| input.realization_diagnostics())
    );
}

fn forward_object_page_with_limit(
    request: gpui_text_input::ObjectRequest,
    facts: &[InlineObjectFact],
    id: u64,
    page_limit: usize,
) -> ObjectPage {
    let demand = request.key().demand();
    assert_eq!(demand.direction(), ObjectDirection::Forward);
    let eligible = facts
        .iter()
        .filter(|fact| demand.contains_anchor(fact.anchor()))
        .filter(|fact| demand.cursor().is_none_or(|cursor| fact.cursor() > cursor))
        .collect::<Vec<_>>();
    let count = eligible.len().min(page_limit).min(demand.max_objects());
    let objects = eligible[..count]
        .iter()
        .map(|fact| (*fact).clone())
        .collect::<Vec<_>>();
    let complete = count == eligible.len();
    let continuation = (!complete).then(|| objects.last().unwrap().cursor());
    ObjectPage::new(
        ObjectPageId::new(id),
        request.key(),
        objects,
        demand.cursor().map_or(
            ObjectPageEdgeFact::EnvelopeBoundary,
            ObjectPageEdgeFact::Continues,
        ),
        continuation.map_or(
            ObjectPageEdgeFact::EnvelopeBoundary,
            ObjectPageEdgeFact::Continues,
        ),
        complete,
        continuation,
    )
    .unwrap()
}

fn restoration_validation_page_with_fallback(
    request: gpui_text_input::PageRequest,
    fallback: &str,
    id: u64,
) -> RangePage {
    let range = ByteRange::from_u64(0, 1).unwrap();
    RangePage::new(
        PageId::new(id),
        request.key(),
        range,
        "x".to_owned(),
        vec![AtomFact::new(AtomId::new(id), range, range, fallback)],
        PageEdgeFact::DocumentBoundary,
        PageEdgeFact::DocumentBoundary,
        true,
    )
    .unwrap()
}

fn queue_empty_clipboard_cut(
    input: &gpui::Entity<RangeTextInput>,
    cx: &mut gpui::VisualTestContext,
) -> gpui_text_input::ClipboardKey {
    let position = ordinary_position(0);
    let (text, objects) = admitted_sources("", 1, &[position]);
    input.update(cx, |input, cx| {
        input
            .begin_composite_clipboard(
                gpui_text_input::ClipboardKind::Cut,
                SourceRange::new(position, position).unwrap(),
                MutationPositions::collapsed(position),
                &text,
                &objects,
                cx,
            )
            .unwrap()
    })
}

fn validate_restoration_to_first_geometry_page(
    input: &gpui::Entity<RangeTextInput>,
    cx: &mut gpui::VisualTestContext,
    source: &str,
    seed: RangeRestorationSeed,
) -> gpui_text_input::PageRequest {
    input.update(cx, |input, cx| input.import_restoration(seed, cx).unwrap());
    for id in 71_000..71_100 {
        match input.update(cx, |input, _| input.take_request()).unwrap() {
            RangeTextInputRequest::Page(request)
                if request.key().purpose() == PagePurpose::Restoration =>
            {
                let page = page_for(source, id, request);
                cx.update(|window, app| {
                    input.update(app, |input, cx| {
                        input.deliver_page(page, window, cx).unwrap()
                    })
                });
            }
            RangeTextInputRequest::Page(request)
                if matches!(
                    request.key().purpose(),
                    PagePurpose::GeometryIndex | PagePurpose::GeometryTarget
                ) =>
            {
                return request;
            }
            RangeTextInputRequest::ObjectPage(request) => {
                let page = restoration_object_page(request, &[], id);
                cx.update(|window, app| {
                    input.update(app, |input, cx| {
                        input
                            .deliver_object_page_in_window(page, window, cx)
                            .unwrap_or_else(|error| {
                                panic!(
                                    "object page delivery failed: {error:?}; diagnostics: {:?}",
                                    input.realization_diagnostics()
                                )
                            })
                    })
                });
            }
            RangeTextInputRequest::ReleasePage(_) | RangeTextInputRequest::ReleaseObjectPage(_) => {
            }
            other => panic!("unexpected restoration request: {other:?}"),
        }
    }
    panic!("restoration did not begin geometry")
}

fn validate_restoration_to_first_object_page(
    input: &gpui::Entity<RangeTextInput>,
    cx: &mut gpui::VisualTestContext,
    source: &str,
    seed: RangeRestorationSeed,
) -> gpui_text_input::ObjectRequest {
    input.update(cx, |input, cx| input.import_restoration(seed, cx).unwrap());
    for id in 83_000..83_100 {
        match input.update(cx, |input, _| input.take_request()).unwrap() {
            RangeTextInputRequest::Page(request)
                if request.key().purpose() == PagePurpose::Restoration =>
            {
                let page = page_for(source, id, request);
                cx.update(|window, app| {
                    input.update(app, |input, cx| {
                        input.deliver_page(page, window, cx).unwrap()
                    })
                });
            }
            RangeTextInputRequest::ObjectPage(request) => return request,
            RangeTextInputRequest::ReleasePage(_) => {}
            other => panic!("unexpected restoration request: {other:?}"),
        }
    }
    panic!("restoration did not begin object validation")
}

fn drive_pages(
    input: &gpui::Entity<RangeTextInput>,
    cx: &mut gpui::VisualTestContext,
    source: &str,
) -> Vec<RangeTextInputRequest> {
    let mut other = Vec::new();
    let mut observed_quiescent = false;
    for _ in 0..256 {
        let request = input.update(cx, |input, _| input.take_request());
        let had_request = request.is_some();
        match request {
            Some(RangeTextInputRequest::Page(request)) => {
                observed_quiescent = false;
                let page = page_for(source, request.key().id().get(), request);
                cx.update(|window, app| {
                    input.update(app, |input, cx| {
                        input.deliver_page(page, window, cx).unwrap()
                    });
                });
            }
            Some(RangeTextInputRequest::ObjectPage(request)) => {
                observed_quiescent = false;
                let page = restoration_object_page(request, &[], request.key().id().get());
                cx.update(|window, app| {
                    input.update(app, |input, cx| {
                        input
                            .deliver_object_page_in_window(page, window, cx)
                            .unwrap_or_else(|error| {
                                panic!(
                                    "driven object page failed: {error:?}; diagnostics: {:?}",
                                    input.realization_diagnostics()
                                )
                            })
                    })
                });
            }
            Some(RangeTextInputRequest::ReleasePage(_))
            | Some(RangeTextInputRequest::CancelPage(_))
            | Some(RangeTextInputRequest::ReleaseObjectPage(_))
            | Some(RangeTextInputRequest::CancelObjectPage(_)) => {
                observed_quiescent = false;
            }
            Some(request) => {
                observed_quiescent = false;
                other.push(request);
            }
            None => {
                let quiescent = input.read_with(cx, |input, _| input.is_quiescent());
                if quiescent && observed_quiescent {
                    break;
                }
                observed_quiescent = quiescent;
            }
        }
        if had_request {
            continue;
        }
        if input.read_with(cx, |input, _| input.is_quiescent()) {
            break;
        }
        cx.update(|window, app| window.draw(app).clear());
        cx.run_until_parked();
    }
    other
}

fn drive_pages_with_split_atom_to_quiescence(
    input: &gpui::Entity<RangeTextInput>,
    cx: &mut gpui::VisualTestContext,
    source: &str,
    atom: AtomId,
    global_range: ByteRange,
    fallback: &str,
) {
    let mut observed_quiescent = false;
    for _ in 0..256 {
        let quiescent = input.read_with(cx, |input, _| input.is_quiescent());
        if quiescent && observed_quiescent {
            return;
        }
        observed_quiescent = quiescent;
        let request = input.update(cx, |input, _| input.take_request());
        let had_request = request.is_some();
        match request {
            Some(RangeTextInputRequest::Page(request)) => {
                observed_quiescent = false;
                let page = page_for_split_atom(
                    source,
                    request.key().id().get(),
                    request,
                    atom,
                    global_range,
                    fallback,
                );
                cx.update(|window, app| {
                    input.update(app, |input, cx| {
                        input.deliver_page(page, window, cx).unwrap()
                    })
                });
            }
            Some(RangeTextInputRequest::ObjectPage(request)) => {
                observed_quiescent = false;
                let page = restoration_object_page(request, &[], request.key().id().get());
                cx.update(|window, app| {
                    input.update(app, |input, cx| {
                        input
                            .deliver_object_page_in_window(page, window, cx)
                            .unwrap()
                    })
                });
            }
            Some(RangeTextInputRequest::ReleasePage(_))
            | Some(RangeTextInputRequest::CancelPage(_))
            | Some(RangeTextInputRequest::ReleaseObjectPage(_))
            | Some(RangeTextInputRequest::CancelObjectPage(_)) => {
                observed_quiescent = false;
            }
            Some(request) => panic!("unexpected split-atom surface request: {request:?}"),
            None => {}
        }
        if !had_request {
            cx.update(|window, app| window.draw(app).clear());
            cx.run_until_parked();
        }
    }
    panic!(
        "split-atom quiescence drive exhausted: {:?}",
        input.read_with(cx, |input, _| input.realization_diagnostics())
    );
}

#[derive(Clone, Debug, PartialEq)]
struct FirstSurfaceFacts {
    requests: usize,
    viewport: ByteRange,
    caret: Option<gpui::Point<gpui::Pixels>>,
    hit: Option<ByteOffset>,
    surface_charge: gpui_text_input::RangeSurfaceCharge,
    owned_high_water: (usize, usize),
    visual_lines: u64,
    content_height: gpui::Pixels,
}

fn drive_first_local_surface(
    input: &gpui::Entity<RangeTextInput>,
    cx: &mut gpui::VisualTestContext,
    source: &str,
    first_page_id: u64,
) -> FirstSurfaceFacts {
    let mut requests = 0;
    let mut page_id = first_page_id;
    for _ in 0..128 {
        if input.read_with(cx, |input, _| input.is_surface_current_and_interactive()) {
            break;
        }
        let mut batch = Vec::new();
        while let Some(request) = input.update(cx, |input, _| input.take_request()) {
            batch.push(request);
        }
        assert!(
            !batch.is_empty(),
            "local target must retain one bounded successor"
        );
        let cancelled_pages = batch
            .iter()
            .filter_map(|request| match request {
                RangeTextInputRequest::CancelPage(key) => Some(*key),
                _ => None,
            })
            .collect::<Vec<_>>();
        let cancelled_objects = batch
            .iter()
            .filter_map(|request| match request {
                RangeTextInputRequest::CancelObjectPage(key) => Some(*key),
                _ => None,
            })
            .collect::<Vec<_>>();
        for request in batch {
            match request {
                RangeTextInputRequest::Page(request) => {
                    if cancelled_pages.contains(&request.key()) {
                        continue;
                    }
                    assert_eq!(request.key().purpose(), PagePurpose::GeometryTarget);
                    let request_key = request.key();
                    requests += 1;
                    let page = page_for(source, page_id, request);
                    page_id += 1;
                    cx.update(|window, app| {
                    input.update(app, |input, cx| {
                        input.deliver_page(page, window, cx).unwrap_or_else(|error| {
                            panic!(
                                "first-surface text request {requests} {request_key:?} failed: {error:?}; diagnostics: {:?}",
                                input.realization_diagnostics()
                            )
                        })
                    })
                });
                }
                RangeTextInputRequest::ObjectPage(request) => {
                    if cancelled_objects.contains(&request.key()) {
                        continue;
                    }
                    assert_eq!(request.key().purpose(), ObjectPurpose::GeometryTarget);
                    requests += 1;
                    let page = restoration_object_page(request, &[], page_id);
                    page_id += 1;
                    cx.update(|window, app| {
                        input.update(app, |input, cx| {
                            input
                                .deliver_object_page_in_window(page, window, cx)
                                .unwrap()
                        })
                    });
                }
                RangeTextInputRequest::ReleasePage(_)
                | RangeTextInputRequest::ReleaseObjectPage(_)
                | RangeTextInputRequest::CancelPage(_)
                | RangeTextInputRequest::CancelObjectPage(_) => {}
                other => panic!("unexpected first-surface request: {other:?}"),
            }
        }
    }
    input.read_with(cx, |input, _| {
        assert!(input.is_surface_current_and_interactive());
        let surface = input.surface().unwrap();
        assert_eq!(
            surface.quality(),
            gpui_text_input::GeometryQuality::Estimated
        );
        let caret = surface.position_for_offset(ByteOffset::new(0));
        let hit = caret.and_then(|caret| surface.hit_test(point(caret.x, caret.y + px(1.))));
        let diagnostics = input.realization_diagnostics();
        assert_eq!(diagnostics.current.pending_index_intents, 1);
        FirstSurfaceFacts {
            requests,
            viewport: surface.viewport(),
            caret,
            hit,
            surface_charge: surface.charge(),
            owned_high_water: (
                diagnostics.high_water.owned_bytes,
                diagnostics.high_water.owned_items,
            ),
            visual_lines: surface.visual_lines(),
            content_height: surface.content_height(),
        }
    })
}

#[gpui::test]
fn target_first_surface_is_local_estimated_delayed_and_epoch_ready(cx: &mut gpui::TestAppContext) {
    let prefix = "visible prefix line\n".repeat(24);
    let small = format!("{prefix}{}", "small tail\n".repeat(48));
    let medium = format!("{prefix}{}", "medium tail\n".repeat(80));
    let large = format!("{prefix}{}", "large tail\n".repeat(160));
    let (input, cx) = cx
        .add_window_view(|window, cx| RangeTextInput::new(config(&small, 1), window, cx).unwrap());

    let small_first = drive_first_local_surface(&input, cx, &small, 120_000);
    assert_eq!(small_first.hit, Some(ByteOffset::new(0)));
    assert!(small_first.requests > 0);
    while let Some(request) = input.update(cx, |input, _| input.take_request()) {
        assert!(matches!(
            request,
            RangeTextInputRequest::ReleasePage(_) | RangeTextInputRequest::ReleaseObjectPage(_)
        ));
    }

    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.rebind(binding(&medium, 2), None, window, cx).unwrap();
            assert!(!input.is_surface_current_and_interactive());
        })
    });
    let medium_first = drive_first_local_surface(&input, cx, &medium, 121_000);
    assert_eq!(medium_first.requests, small_first.requests);
    assert_eq!(medium_first.viewport, small_first.viewport);
    assert_eq!(medium_first.caret, small_first.caret);
    assert_eq!(medium_first.hit, small_first.hit);
    assert_eq!(medium_first.surface_charge, small_first.surface_charge);

    while let Some(request) = input.update(cx, |input, _| input.take_request()) {
        assert!(matches!(
            request,
            RangeTextInputRequest::ReleasePage(_) | RangeTextInputRequest::ReleaseObjectPage(_)
        ));
    }
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.rebind(binding(&large, 3), None, window, cx).unwrap();
            assert!(!input.is_surface_current_and_interactive());
        })
    });
    let large_first = drive_first_local_surface(&input, cx, &large, 121_500);
    assert_eq!(large_first.requests, medium_first.requests);
    assert_eq!(large_first.viewport, medium_first.viewport);
    assert_eq!(large_first.caret, medium_first.caret);
    assert_eq!(large_first.hit, medium_first.hit);
    assert_eq!(large_first.surface_charge, medium_first.surface_charge);
    assert_eq!(large_first.owned_high_water, medium_first.owned_high_water);
    while let Some(request) = input.update(cx, |input, _| input.take_request()) {
        assert!(matches!(
            request,
            RangeTextInputRequest::ReleasePage(_) | RangeTextInputRequest::ReleaseObjectPage(_)
        ));
    }
    cx.update(|window, app| window.draw(app).clear());
    cx.run_until_parked();
    let delayed = input
        .update(cx, |input, _| input.take_request())
        .expect("later prepaint quantum starts the background index");
    let RangeTextInputRequest::Page(delayed) = delayed else {
        panic!("background index must begin with a text page")
    };
    assert_eq!(delayed.key().purpose(), PagePurpose::GeometryIndex);
    input.read_with(cx, |input, _| {
        assert!(input.is_surface_current_and_interactive());
        assert!(input.geometry_estimate().is_some());
        assert_eq!(
            input.surface().unwrap().quality(),
            gpui_text_input::GeometryQuality::Estimated
        );
    });
    let page = page_for(&large, 122_000, delayed);
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.deliver_page(page, window, cx).unwrap()
        })
    });
    input.read_with(cx, |input, _| {
        assert!(input.is_surface_current_and_interactive());
        assert!(input.geometry_estimate().is_some());
        let diagnostics = input.realization_diagnostics();
        assert_eq!(diagnostics.current.active_geometry_jobs, 1);
        assert_eq!(
            diagnostics.current.pending_geometry_pages
                + diagnostics.current.pending_geometry_objects,
            1
        );
    });
    for _ in 0..4 {
        assert!(drive_pages(&input, cx, &large).is_empty());
        if input.read_with(cx, |input, _| {
            input.surface().unwrap().quality() == gpui_text_input::GeometryQuality::Exact
        }) {
            break;
        }
    }
    input.read_with(cx, |input, _| {
        let surface = input.surface().unwrap();
        assert_eq!(surface.quality(), gpui_text_input::GeometryQuality::Exact);
        assert!(surface.visual_lines() > large_first.visual_lines);
        assert!(surface.content_height() > large_first.content_height);
        assert!(input.is_surface_current_and_interactive());
    });

    let mut replacement = config(&large, 3);
    replacement.layout.wrap_width = px(72.);
    input.update(cx, |input, cx| {
        input
            .set_layout(replacement.layout, replacement.style, cx)
            .unwrap();
        assert!(!input.is_surface_current_and_interactive());
    });
    let layout_first = drive_first_local_surface(&input, cx, &large, 123_000);
    assert!(layout_first.requests > 0);
    input.update(cx, |input, cx| {
        input.request_absolute_scroll(px(16.), cx).unwrap();
        assert!(!input.is_surface_current_and_interactive());
    });

    let released =
        cx.update(|window, app| input.update(app, |input, cx| input.dispose(window, cx)));
    assert!(!released.is_empty());
    input.read_with(cx, |input, _| {
        let diagnostics = input.realization_diagnostics();
        assert_eq!(diagnostics.current.pending_index_intents, 0);
        assert_eq!(diagnostics.current.active_geometry_jobs, 0);
        assert_eq!(diagnostics.current.candidates, 0);
        assert_eq!(diagnostics.current.pending_geometry_pages, 0);
        assert_eq!(diagnostics.current.pending_geometry_objects, 0);
        assert_eq!(diagnostics.current.response_custody_count, 0);
        assert!(input.surface().is_none());
        assert!(!input.is_surface_current_and_interactive());
    });
}

#[gpui::test]
fn target_first_background_index_is_semantically_quiescent_and_disposes_exact_custody(
    cx: &mut gpui::TestAppContext,
) {
    let source = "visible target line\n".repeat(192);
    let (input, cx) = cx
        .add_window_view(|window, cx| RangeTextInput::new(config(&source, 1), window, cx).unwrap());

    let first = drive_first_local_surface(&input, cx, &source, 123_500);
    assert!(first.requests > 0);
    while let Some(request) = input.update(cx, |input, _| input.take_request()) {
        assert!(matches!(
            request,
            RangeTextInputRequest::ReleasePage(_) | RangeTextInputRequest::ReleaseObjectPage(_)
        ));
    }

    cx.update(|window, app| window.draw(app).clear());
    cx.run_until_parked();
    let index = input
        .update(cx, |input, _| input.take_request())
        .and_then(|request| match request {
            RangeTextInputRequest::Page(request)
                if request.key().purpose() == PagePurpose::GeometryIndex =>
            {
                Some(request)
            }
            _ => None,
        })
        .expect("locally current surface starts one delayed background index page");
    let key = index.key();
    let late = page_for(&source, 123_900, index);

    input.read_with(cx, |input, _| {
        assert!(input.is_surface_current_and_interactive());
        assert!(input.is_semantically_quiescent());
        assert!(!input.is_quiescent());
    });

    let released =
        cx.update(|window, app| input.update(app, |input, cx| input.dispose(window, cx)));
    assert_eq!(
        released
            .iter()
            .filter(|request| matches!(request, RangeTextInputRequest::CancelPage(cancelled) if *cancelled == key))
            .count(),
        1
    );
    assert!(released.iter().all(|request| matches!(
        request,
        RangeTextInputRequest::CancelPage(_)
            | RangeTextInputRequest::ReleasePage(_)
            | RangeTextInputRequest::CancelObjectPage(_)
            | RangeTextInputRequest::ReleaseObjectPage(_)
            | RangeTextInputRequest::CancelClipboardWrite(_)
    )));
    input.read_with(cx, |input, _| {
        let diagnostics = input.realization_diagnostics();
        assert_eq!(diagnostics.current.pending_index_intents, 0);
        assert_eq!(diagnostics.current.active_geometry_jobs, 0);
        assert_eq!(diagnostics.current.pending_geometry_pages, 0);
        assert_eq!(diagnostics.current.pending_geometry_objects, 0);
        assert_eq!(diagnostics.current.response_custody_count, 0);
        assert!(input.is_semantically_quiescent());
        assert!(input.is_quiescent());
    });

    let rejected = cx
        .update(|window, app| input.update(app, |input, cx| input.deliver_page(late, window, cx)));
    assert!(matches!(
        rejected,
        Err(gpui_text_input::RangeTextInputError::PageResponseRejected(
            _
        ))
    ));
}

fn drive_pages_with_objects(
    input: &gpui::Entity<RangeTextInput>,
    cx: &mut gpui::VisualTestContext,
    source: &str,
    facts: &[InlineObjectFact],
) {
    let mut observed_quiescent = false;
    for _ in 0..512 {
        let request = input.update(cx, |input, _| input.take_request());
        let had_request = request.is_some();
        match request {
            Some(RangeTextInputRequest::Page(request)) => {
                observed_quiescent = false;
                let page = page_for(source, request.key().id().get(), request);
                cx.update(|window, app| {
                    input.update(app, |input, cx| {
                        input.deliver_page(page, window, cx).unwrap()
                    })
                });
            }
            Some(RangeTextInputRequest::ObjectPage(request)) => {
                observed_quiescent = false;
                let page = restoration_object_page(request, facts, request.key().id().get());
                cx.update(|window, app| {
                    input.update(app, |input, cx| {
                        input
                            .deliver_object_page_in_window(page, window, cx)
                            .unwrap()
                    })
                });
            }
            Some(RangeTextInputRequest::ReleasePage(_))
            | Some(RangeTextInputRequest::CancelPage(_))
            | Some(RangeTextInputRequest::ReleaseObjectPage(_))
            | Some(RangeTextInputRequest::CancelObjectPage(_)) => {
                observed_quiescent = false;
            }
            None => {
                let quiescent = input.read_with(cx, |input, _| input.is_quiescent());
                if quiescent && observed_quiescent {
                    break;
                }
                observed_quiescent = quiescent;
            }
            Some(request) => panic!("unexpected object geometry request: {request:?}"),
        }
        if !had_request {
            cx.update(|window, app| window.draw(app).clear());
            cx.run_until_parked();
        }
    }
    input.read_with(cx, |input, _| {
        assert!(
            input.is_quiescent(),
            "object geometry drive exhausted without quiescing: {:?}",
            input.realization_diagnostics()
        );
    });
}

fn drive_attached_inline_object_surface_requests(
    input: &gpui::Entity<RangeTextInput>,
    cx: &mut gpui::VisualTestContext,
    source: &str,
    facts: &[InlineObjectFact],
) {
    let mut observed_idle_cycles = 0;
    let mut awaiting_idle_observation = false;
    for _ in 0..64 {
        match input.update(cx, |input, _| input.take_request()) {
            Some(RangeTextInputRequest::Page(request)) => {
                let page = page_for(source, request.key().id().get(), request);
                cx.update(|window, app| {
                    input.update(app, |input, cx| {
                        input.deliver_page(page, window, cx).unwrap()
                    })
                });
                observed_idle_cycles = 0;
                awaiting_idle_observation = false;
            }
            Some(RangeTextInputRequest::ObjectPage(request)) => {
                let page = restoration_object_page(request, facts, request.key().id().get());
                cx.update(|window, app| {
                    input.update(app, |input, cx| {
                        input
                            .deliver_object_page_in_window(page, window, cx)
                            .unwrap()
                    })
                });
                observed_idle_cycles = 0;
                awaiting_idle_observation = false;
            }
            Some(RangeTextInputRequest::ReleasePage(_))
            | Some(RangeTextInputRequest::CancelPage(_))
            | Some(RangeTextInputRequest::ReleaseObjectPage(_))
            | Some(RangeTextInputRequest::CancelObjectPage(_)) => {
                observed_idle_cycles = 0;
                awaiting_idle_observation = false;
            }
            None => {
                if awaiting_idle_observation {
                    observed_idle_cycles += 1;
                    if observed_idle_cycles == 2 {
                        return;
                    }
                }
                cx.update(|window, app| window.draw(app).clear());
                cx.run_until_parked();
                awaiting_idle_observation = true;
            }
            Some(request) => panic!("unexpected attached surface request: {request:?}"),
        }
    }
    panic!("attached surface request servicing exceeded its 64-step bound");
}

fn drive_pages_with_limited_objects(
    input: &gpui::Entity<RangeTextInput>,
    cx: &mut gpui::VisualTestContext,
    source: &str,
    facts: &[InlineObjectFact],
    page_limit: usize,
) {
    let mut next_page_id = 98_000;
    let mut observed_quiescent = false;
    for _ in 0..512 {
        let request = input.update(cx, |input, _| input.take_request());
        let had_request = request.is_some();
        match request {
            Some(RangeTextInputRequest::Page(request)) => {
                observed_quiescent = false;
                let page = page_for(source, next_page_id, request);
                next_page_id += 1;
                cx.update(|window, app| {
                    input.update(app, |input, cx| {
                        input.deliver_page(page, window, cx).unwrap()
                    })
                });
            }
            Some(RangeTextInputRequest::ObjectPage(request)) => {
                observed_quiescent = false;
                let page = forward_object_page_with_limit(request, facts, next_page_id, page_limit);
                next_page_id += 1;
                cx.update(|window, app| {
                    input.update(app, |input, cx| {
                        input
                            .deliver_object_page_in_window(page, window, cx)
                            .unwrap()
                    })
                });
            }
            Some(RangeTextInputRequest::ReleasePage(_))
            | Some(RangeTextInputRequest::CancelPage(_))
            | Some(RangeTextInputRequest::ReleaseObjectPage(_))
            | Some(RangeTextInputRequest::CancelObjectPage(_)) => {
                observed_quiescent = false;
            }
            None => {
                let quiescent = input.read_with(cx, |input, _| input.is_quiescent());
                if quiescent && observed_quiescent {
                    break;
                }
                observed_quiescent = quiescent;
            }
            Some(request) => panic!("unexpected limited object geometry request: {request:?}"),
        }
        if !had_request {
            cx.update(|window, app| window.draw(app).clear());
            cx.run_until_parked();
        }
    }
    input.read_with(cx, |input, _| {
        assert!(
            input.is_quiescent(),
            "limited object geometry drive exhausted without quiescing: {:?}",
            input.realization_diagnostics()
        );
    });
}

fn drive_pages_observing_cancel(
    input: &gpui::Entity<RangeTextInput>,
    cx: &mut gpui::VisualTestContext,
    source: &str,
    cancelled: gpui_text_input::PageRequestKey,
) -> (Vec<RangeTextInputRequest>, usize) {
    let mut other = Vec::new();
    let mut cancellations = 0;
    let mut page_id = 81_000;
    for _ in 0..256 {
        cx.update(|window, app| window.draw(app).clear());
        cx.run_until_parked();
        match input.update(cx, |input, _| input.take_request()) {
            Some(RangeTextInputRequest::Page(request)) => {
                let page = page_for(source, page_id, request);
                page_id += 1;
                cx.update(|window, app| {
                    input.update(app, |input, cx| {
                        input.deliver_page(page, window, cx).unwrap()
                    })
                });
            }
            Some(RangeTextInputRequest::ObjectPage(request)) => {
                let page = restoration_object_page(request, &[], page_id);
                page_id += 1;
                cx.update(|window, app| {
                    input.update(app, |input, cx| {
                        input
                            .deliver_object_page_in_window(page, window, cx)
                            .unwrap()
                    })
                });
            }
            Some(RangeTextInputRequest::CancelPage(key)) if key == cancelled => {
                cancellations += 1;
            }
            Some(RangeTextInputRequest::ReleasePage(_))
            | Some(RangeTextInputRequest::ReleaseObjectPage(_))
            | Some(RangeTextInputRequest::CancelObjectPage(_)) => {}
            Some(request) => other.push(request),
            None => break,
        }
    }
    (other, cancellations)
}

fn restoration_events(
    input: &gpui::Entity<RangeTextInput>,
    cx: &mut gpui::VisualTestContext,
) -> Rc<RefCell<Vec<RangeTextInputEvent>>> {
    let events = Rc::new(RefCell::new(Vec::new()));
    let captured = events.clone();
    cx.cx.update(|cx| {
        cx.subscribe(input, move |_, event: &RangeTextInputEvent, _| {
            captured.borrow_mut().push(event.clone());
        })
        .detach();
    });
    events
}

#[gpui::test]
fn coherent_surface_clamps_ordinary_leading_and_trailing_line_whitespace(
    cx: &mut gpui::TestAppContext,
) {
    cx.update(ensure_text_input_bindings);
    let source = "alpha\nomega";
    let (input, cx) = cx
        .add_window_view(|window, cx| RangeTextInput::new(config(source, 1), window, cx).unwrap());
    assert!(drive_pages(&input, cx, source).is_empty());

    input.read_with(cx, |input, _| {
        let surface = input.surface().expect("coherent surface");
        let source_start = ByteOffset::new(0);
        let first_line_end = ByteOffset::new(5);
        let source_end = ByteOffset::new(source.len() as u64);
        assert_eq!(surface.viewport().start(), source_start);
        assert_eq!(surface.viewport().end(), source_end);

        let start = surface.position_for_offset(source_start).unwrap();
        assert_eq!(
            surface.hit_test(point(start.x - px(32.), start.y + px(1.))),
            Some(surface.viewport().start())
        );

        let line_end = surface.position_for_offset(first_line_end).unwrap();
        assert_eq!(
            surface.hit_test(point(line_end.x + px(32.), line_end.y + px(1.))),
            Some(first_line_end)
        );

        let end = surface.position_for_offset(source_end).unwrap();
        assert_eq!(
            surface.hit_test(point(end.x + px(32.), end.y + px(1.))),
            Some(source_end)
        );
    });
}

#[gpui::test]
fn coherent_surface_clamps_atom_first_ordinary_whitespace_to_exact_source_edges(
    cx: &mut gpui::TestAppContext,
) {
    cx.update(ensure_text_input_bindings);
    let source = format!("x{}", "\u{301}".repeat(20));
    let (input, cx) = cx
        .add_window_view(|window, cx| RangeTextInput::new(config(&source, 1), window, cx).unwrap());
    assert!(drive_pages(&input, cx, &source).is_empty());

    input.read_with(cx, |input, _| {
        let surface = input.surface().expect("coherent surface");
        let source_start = ByteOffset::new(0);
        let source_end = ByteOffset::new(source.len() as u64);
        assert_eq!(surface.viewport().start(), source_start);
        assert_eq!(surface.viewport().end(), source_end);

        let StreamingLayoutFragment::OversizeAtom(atom) = &surface.fragments()[0] else {
            panic!("oversized first grapheme must publish an atom-first surface")
        };
        assert_eq!(atom.logical_range.start, StreamingLayoutPosition::at(0));
        assert_eq!(
            atom.logical_range.end,
            StreamingLayoutPosition::at(source.len() as u64)
        );
        let block = atom.bounds.origin.y + px(1.);
        assert_eq!(
            surface.hit_test(point(atom.bounds.origin.x - px(32.), block)),
            Some(surface.viewport().start())
        );
        assert_eq!(
            surface.hit_test(point(
                atom.bounds.origin.x + atom.bounds.size.width + px(32.),
                block,
            )),
            Some(source_end)
        );
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
            input
                .surface()
                .unwrap()
                .platform_selection()
                .unwrap()
                .range(),
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
            input
                .surface()
                .unwrap()
                .platform_selection()
                .unwrap()
                .range(),
            ByteRange::from_u64(0, 11).unwrap()
        );
    });
}

#[gpui::test]
fn restoration_uses_validation_envelopes_and_imports_no_resident_page(
    cx: &mut gpui::TestAppContext,
) {
    let source = "alpha\nbeta";
    let first = object_neighbor(50, 10);
    let second = object_neighbor(51, 20);
    let mut object_config = config(source, 1);
    object_config.layout.start_position =
        SourcePosition::new(ByteOffset::new(0), InlineObjectGap::before(first)).into();
    let (input, cx) =
        cx.add_window_view(|window, cx| RangeTextInput::new(object_config, window, cx).unwrap());
    let restored = SourcePosition::new(
        ByteOffset::new(0),
        InlineObjectGap::between(first, second).unwrap(),
    );
    let seed = restoration_seed(source, 1, restored);
    let facts = [object_fact(50, 0, 10), object_fact(51, 0, 20)];
    drive_pages_with_objects(&input, cx, source, &facts);
    input.read_with(cx, |input, _| {
        assert!(input.surface().is_some());
        assert!(input.is_quiescent());
    });
    input.update(cx, |input, cx| input.import_restoration(seed, cx).unwrap());
    let mut saw_validation = false;
    let mut saw_object_validation = false;
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
            Some(RangeTextInputRequest::ObjectPage(request)) => {
                saw_object_validation = true;
                let page = restoration_object_page(request, &facts, 600);
                cx.update(|window, app| {
                    input.update(app, |input, cx| {
                        input
                            .deliver_object_page_in_window(page, window, cx)
                            .unwrap()
                    })
                });
            }
            Some(RangeTextInputRequest::ReleasePage(_))
            | Some(RangeTextInputRequest::CancelPage(_))
            | Some(RangeTextInputRequest::ReleaseObjectPage(_))
            | Some(RangeTextInputRequest::CancelObjectPage(_)) => {}
            None => break,
            Some(_) => panic!("unexpected restoration request"),
        }
    }
    assert!(saw_validation);
    assert!(saw_object_validation);
    input.read_with(cx, |input, _| {
        let surface = input.surface().unwrap();
        assert_eq!(surface.binding(), seed.binding);
        assert_eq!(surface.caret(), seed.caret);
        assert_eq!(surface.selection().anchor, seed.selection.anchor);
        assert_eq!(surface.selection().head, seed.selection.head);
        assert_eq!(surface.scroll_source(), seed.scroll.position.byte_offset);
        assert_eq!(surface.scroll_position(), seed.scroll.position);
        assert_eq!(surface.source_caret(), seed.caret);
        assert_eq!(surface.source_selection(), seed.selection);
        assert_eq!(surface.scroll_intra_anchor(), seed.scroll.intra_anchor);
        assert_eq!(surface.realized_objects().len(), 2);
        assert_eq!(surface.realized_object_gaps().len(), 3);
        let caret_gap = surface
            .realized_object_gaps()
            .iter()
            .find(|gap| gap.position() == seed.caret)
            .unwrap();
        assert_eq!(
            surface.caret_bounds(px(16.)),
            Some(caret_gap.caret_bounds())
        );
        assert_eq!(input.export_restoration(None).unwrap(), seed);
    });
}

#[gpui::test]
fn restoration_coalesces_offsets_and_proves_before_between_after_before_publication(
    cx: &mut gpui::TestAppContext,
) {
    let source = "ab";
    let (input, cx) = cx
        .add_window_view(|window, cx| RangeTextInput::new(config(source, 1), window, cx).unwrap());
    assert!(drive_pages(&input, cx, source).is_empty());
    let first = object_neighbor(61, 10);
    let second = object_neighbor(62, 20);
    let before = SourcePosition::new(ByteOffset::new(1), InlineObjectGap::before(first));
    let between = SourcePosition::new(
        ByteOffset::new(1),
        InlineObjectGap::between(first, second).unwrap(),
    );
    let after = SourcePosition::new(ByteOffset::new(1), InlineObjectGap::after(second));
    let seed = RangeRestorationSeed {
        binding: binding(source, 1),
        caret: between,
        selection: RangeSourceSelection {
            anchor: before,
            head: between,
        },
        scroll: RangeRestorationScrollAnchor {
            position: after,
            intra_anchor: px(0.),
        },
        history: None,
    };
    let facts = [object_fact(61, 1, 10), object_fact(62, 1, 20)];
    input.update(cx, |input, cx| input.import_restoration(seed, cx).unwrap());

    let mut text_validations = 0;
    let mut object_validations = 0;
    for id in 800..820 {
        let request = input.update(cx, |input, _| input.take_request()).unwrap();
        match request {
            RangeTextInputRequest::Page(request) => {
                assert!(matches!(
                    request.key().demand(),
                    PageDemandEnvelope::Validation { .. }
                ));
                text_validations += 1;
                let page = page_for(source, id, request);
                cx.update(|window, app| {
                    input.update(app, |input, cx| {
                        input.deliver_page(page, window, cx).unwrap()
                    })
                });
            }
            RangeTextInputRequest::ObjectPage(request) => {
                object_validations += 1;
                let page = restoration_object_page(request, &facts, id);
                input.update(cx, |input, cx| input.deliver_object_page(page, cx).unwrap());
            }
            RangeTextInputRequest::ReleasePage(_) | RangeTextInputRequest::ReleaseObjectPage(_) => {
                continue;
            }
            other => panic!("unexpected validation request: {other:?}"),
        }
        input.read_with(cx, |input, _| assert!(input.surface().is_none()));
        if object_validations == 3 {
            break;
        }
    }
    assert_eq!(text_validations, 1);
    assert_eq!(object_validations, 3);
    drive_pages_with_objects(&input, cx, source, &facts);
    input.read_with(cx, |input, _| {
        assert!(input.is_quiescent());
        let surface = input.surface().unwrap();
        assert_eq!(
            surface
                .realized_object_gaps()
                .iter()
                .map(|gap| gap.position())
                .collect::<Vec<_>>(),
            [before, between, after]
        );
        let gap_bounds = [before, between, after].map(|position| {
            surface
                .realized_object_gaps()
                .iter()
                .find(|gap| gap.position() == position)
                .unwrap()
                .caret_bounds()
        });
        assert!(gap_bounds[0].origin.x < gap_bounds[1].origin.x);
        assert!(gap_bounds[1].origin.x < gap_bounds[2].origin.x);
        assert_eq!(
            [before, between, after]
                .map(|position| { surface.position_for_source_position(position).unwrap() }),
            gap_bounds.map(|bounds| bounds.origin)
        );
        let objects = surface.realized_objects();
        assert_eq!(objects[0].leading(), before);
        assert_eq!(objects[0].trailing(), between);
        assert_eq!(objects[1].leading(), between);
        assert_eq!(objects[1].trailing(), after);
        assert_eq!(objects[0].leading_caret_bounds(), gap_bounds[0]);
        assert_eq!(objects[0].trailing_caret_bounds(), gap_bounds[1]);
        assert_eq!(objects[1].leading_caret_bounds(), gap_bounds[1]);
        assert_eq!(objects[1].trailing_caret_bounds(), gap_bounds[2]);
        assert_eq!(surface.caret_bounds(px(16.)), Some(gap_bounds[1]));
        assert_eq!(input.export_restoration(None).unwrap(), seed)
    });
}

#[gpui::test]
fn forged_nonadjacent_restoration_gap_is_rejected_without_publication(
    cx: &mut gpui::TestAppContext,
) {
    let source = "x";
    let (input, cx) = cx
        .add_window_view(|window, cx| RangeTextInput::new(config(source, 1), window, cx).unwrap());
    assert!(drive_pages(&input, cx, source).is_empty());
    let forged = SourcePosition::new(
        ByteOffset::new(0),
        InlineObjectGap::between(object_neighbor(70, 10), object_neighbor(72, 30)).unwrap(),
    );
    let seed = restoration_seed(source, 1, forged);
    let facts = [
        object_fact(70, 0, 10),
        object_fact(71, 0, 20),
        object_fact(72, 0, 30),
    ];
    input.update(cx, |input, cx| input.import_restoration(seed, cx).unwrap());
    let text = input.update(cx, |input, _| input.take_request()).unwrap();
    let RangeTextInputRequest::Page(request) = text else {
        panic!("text validation")
    };
    let page = page_for(source, 900, request);
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.deliver_page(page, window, cx).unwrap()
        })
    });
    let object = (0..3)
        .find_map(|_| {
            input
                .update(cx, |input, _| input.take_request())
                .filter(|request| matches!(request, RangeTextInputRequest::ObjectPage(_)))
        })
        .unwrap();
    let RangeTextInputRequest::ObjectPage(request) = object else {
        panic!("object validation")
    };
    let page = restoration_object_page(request, &facts, 901);
    assert!(matches!(
        input.update(cx, |input, cx| input.deliver_object_page(page, cx)),
        Err(gpui_text_input::RangeTextInputError::MalformedSeed)
    ));
    input.read_with(cx, |input, _| assert!(input.surface().is_none()));
}

#[gpui::test]
fn restoration_mismatch_failure_and_lifecycle_cancel_release_exact_work(
    cx: &mut gpui::TestAppContext,
) {
    let source = "cancel";
    let (input, cx) = cx
        .add_window_view(|window, cx| RangeTextInput::new(config(source, 1), window, cx).unwrap());
    assert!(drive_pages(&input, cx, source).is_empty());
    let seed = restoration_seed(source, 1, ordinary_position(0));
    let mut stale = seed;
    stale.binding = binding(source, 2);
    input.update(cx, |input, cx| {
        assert!(matches!(
            input.import_restoration(stale, cx),
            Err(gpui_text_input::RangeTextInputError::MalformedSeed)
        ));
        input.import_restoration(seed, cx).unwrap();
    });
    let request = input.update(cx, |input, _| input.take_request()).unwrap();
    let RangeTextInputRequest::Page(page) = request else {
        panic!("text validation")
    };
    input.update(cx, |input, cx| {
        input
            .fail_page(page.key(), PageFailure::Unavailable, cx)
            .unwrap();
        assert!(input.surface().is_none());
    });
    input.update(cx, |input, cx| input.import_restoration(seed, cx).unwrap());
    let request = input.update(cx, |input, _| input.take_request()).unwrap();
    let RangeTextInputRequest::Page(page) = request else {
        panic!("text validation")
    };
    let page = page_for(source, 950, page);
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.deliver_page(page, window, cx).unwrap()
        })
    });
    let request = (0..3)
        .find_map(|_| {
            input
                .update(cx, |input, _| input.take_request())
                .filter(|request| matches!(request, RangeTextInputRequest::ObjectPage(_)))
        })
        .unwrap();
    let RangeTextInputRequest::ObjectPage(object) = request else {
        panic!("object validation")
    };
    let drained = cx.update(|window, app| input.update(app, |input, cx| input.dispose(window, cx)));
    assert!(drained.iter().any(|request| {
        matches!(request, RangeTextInputRequest::CancelObjectPage(key) if *key == object.key())
    }));
    input.read_with(cx, |input, _| {
        assert!(matches!(
            input.export_restoration(None),
            Err(gpui_text_input::RangeTextInputError::NotMounted)
        ));
        assert!(input.surface().is_none());
    });
}

#[gpui::test]
fn post_validation_restoration_geometry_failure_rejects_once_and_can_retry(
    cx: &mut gpui::TestAppContext,
) {
    let source = "restore geometry";
    let (input, cx) = cx
        .add_window_view(|window, cx| RangeTextInput::new(config(source, 1), window, cx).unwrap());
    assert!(drive_pages(&input, cx, source).is_empty());
    let events = Rc::new(RefCell::new(Vec::new()));
    let captured = events.clone();
    cx.cx.update(|cx| {
        cx.subscribe(&input, move |_, event: &RangeTextInputEvent, _| {
            captured.borrow_mut().push(event.clone());
        })
        .detach();
    });
    let seed = restoration_seed(source, 1, ordinary_position(0));
    let geometry = validate_restoration_to_first_geometry_page(&input, cx, source, seed);
    input.update(cx, |input, cx| {
        input
            .fail_page(geometry.key(), PageFailure::Unavailable, cx)
            .unwrap()
    });
    assert!(drive_pages(&input, cx, source).is_empty());
    input.read_with(cx, |input, _| {
        assert!(input.surface().is_none());
        assert!(
            input.is_quiescent(),
            "restoration rejection did not retire: {:?}",
            input.realization_diagnostics()
        );
    });
    assert_eq!(
        events
            .borrow()
            .iter()
            .filter(|event| matches!(event, RangeTextInputEvent::RestorationRejected))
            .count(),
        1
    );

    input.update(cx, |input, cx| input.import_restoration(seed, cx).unwrap());
    assert!(drive_pages(&input, cx, source).is_empty());
    input.read_with(cx, |input, _| {
        assert_eq!(input.export_restoration(None).unwrap(), seed);
        assert!(input.is_quiescent());
    });
    assert_eq!(
        events
            .borrow()
            .iter()
            .filter(|event| matches!(event, RangeTextInputEvent::RestorationRejected))
            .count(),
        1
    );
}

#[gpui::test]
fn post_validation_restoration_geometry_failure_rejects_once_and_retries_validation(
    cx: &mut gpui::TestAppContext,
) {
    let source = "restore geometry retry admission";
    let (input, cx) = cx
        .add_window_view(|window, cx| RangeTextInput::new(config(source, 1), window, cx).unwrap());
    assert!(drive_pages(&input, cx, source).is_empty());
    let events = Rc::new(RefCell::new(Vec::new()));
    let captured = events.clone();
    cx.cx.update(|cx| {
        cx.subscribe(&input, move |_, event: &RangeTextInputEvent, _| {
            captured.borrow_mut().push(event.clone());
        })
        .detach();
    });
    let seed = restoration_seed(source, 1, ordinary_position(0));
    let index = validate_restoration_to_first_geometry_page(&input, cx, source, seed);
    assert_eq!(index.key().purpose(), PagePurpose::GeometryIndex);
    input.update(cx, |input, cx| {
        input
            .fail_page(index.key(), PageFailure::Unavailable, cx)
            .unwrap()
    });
    assert!(drive_pages(&input, cx, source).is_empty());
    input.read_with(cx, |input, _| {
        assert!(input.surface().is_none());
        assert!(input.is_quiescent());
    });
    assert_eq!(
        events
            .borrow()
            .iter()
            .filter(|event| matches!(event, RangeTextInputEvent::RestorationRejected))
            .count(),
        1
    );

    input.update(cx, |input, cx| input.import_restoration(seed, cx).unwrap());
    let request = input
        .update(cx, |input, _| input.take_request())
        .expect("fresh restoration validation request");
    let RangeTextInputRequest::Page(request) = request else {
        panic!("fresh restoration retry must dispatch a text page")
    };
    assert_eq!(request.key().purpose(), PagePurpose::Restoration);
    assert!(matches!(
        request.key().demand(),
        PageDemandEnvelope::Validation { .. }
    ));
    assert_eq!(
        events
            .borrow()
            .iter()
            .filter(|event| matches!(event, RangeTextInputEvent::RestorationRejected))
            .count(),
        1
    );
}

#[gpui::test]
fn post_validation_select_all_rejects_restoration_once_and_runs_ordinary_target(
    cx: &mut gpui::TestAppContext,
) {
    cx.update(ensure_text_input_bindings);
    let source = "ordinary selection after restoration";
    let (input, cx) = cx.add_window_view(|window, cx| {
        let input = RangeTextInput::new(config(source, 1), window, cx).unwrap();
        input.focus(window);
        input
    });
    assert!(drive_pages(&input, cx, source).is_empty());
    let events = Rc::new(RefCell::new(Vec::new()));
    let captured = events.clone();
    cx.cx.update(|cx| {
        cx.subscribe(&input, move |_, event: &RangeTextInputEvent, _| {
            captured.borrow_mut().push(event.clone());
        })
        .detach();
    });
    let seed = restoration_seed(source, 1, ordinary_position(0));
    let restoration_geometry =
        validate_restoration_to_first_geometry_page(&input, cx, source, seed);
    assert_eq!(
        restoration_geometry.key().purpose(),
        PagePurpose::GeometryIndex
    );
    let restoration_geometry_key = restoration_geometry.key();
    let late_restoration_geometry = page_for(source, 80_999, restoration_geometry);

    cx.simulate_keystrokes("ctrl-a");
    assert_eq!(
        events
            .borrow()
            .iter()
            .filter(|event| matches!(event, RangeTextInputEvent::RestorationRejected))
            .count(),
        1
    );
    assert!(matches!(
        input.update(cx, |input, _| input.take_request()),
        Some(RangeTextInputRequest::CancelPage(key)) if key == restoration_geometry_key
    ));
    let late_result = cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.deliver_page(late_restoration_geometry, window, cx)
        })
    });
    let Err(gpui_text_input::RangeTextInputError::PageResponseRejected(released)) = late_result
    else {
        panic!("cancelled restoration geometry response was not released: {late_result:?}")
    };
    assert_eq!(released.key(), restoration_geometry_key);
    let ordinary_geometry = input
        .update(cx, |input, _| input.take_request())
        .expect("ordinary Select All geometry proceeds after restoration cancellation");
    let RangeTextInputRequest::Page(ordinary_geometry) = ordinary_geometry else {
        panic!("ordinary Select All must restart text geometry")
    };
    assert_eq!(
        ordinary_geometry.key().purpose(),
        PagePurpose::GeometryIndex
    );
    assert_ne!(ordinary_geometry.key(), restoration_geometry_key);
    let ordinary_geometry = page_for(source, 81_000, ordinary_geometry);
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.deliver_page(ordinary_geometry, window, cx).unwrap()
        })
    });
    let (requests, cancellations) =
        drive_pages_observing_cancel(&input, cx, source, restoration_geometry_key);
    assert!(requests.is_empty());
    assert_eq!(cancellations, 0);
    input.read_with(cx, |input, _| {
        assert_eq!(
            input.surface().unwrap().platform_selection().unwrap(),
            RangeSelection {
                anchor: ByteOffset::new(0),
                head: ByteOffset::new(source.len() as u64),
            }
        );
        assert!(input.is_quiescent());
    });
    assert_eq!(
        events
            .borrow()
            .iter()
            .filter(|event| matches!(event, RangeTextInputEvent::RestorationRejected))
            .count(),
        1
    );

    input.update(cx, |input, cx| input.import_restoration(seed, cx).unwrap());
    assert!(drive_pages(&input, cx, source).is_empty());
    input.read_with(cx, |input, _| {
        assert_eq!(input.export_restoration(None).unwrap(), seed);
        assert!(input.is_quiescent());
    });
    assert_eq!(
        events
            .borrow()
            .iter()
            .filter(|event| matches!(event, RangeTextInputEvent::RestorationRejected))
            .count(),
        1
    );
}

#[gpui::test]
fn post_validation_host_scroll_busy_preserves_restoration_and_geometry(
    cx: &mut gpui::TestAppContext,
) {
    let source = &(0..80)
        .map(|line| format!("line-{line:02}\n"))
        .collect::<String>();
    let (input, cx) = cx
        .add_window_view(|window, cx| RangeTextInput::new(config(source, 1), window, cx).unwrap());
    assert!(drive_pages(&input, cx, source).is_empty());
    let events = Rc::new(RefCell::new(Vec::new()));
    let captured = events.clone();
    cx.cx.update(|cx| {
        cx.subscribe(&input, move |_, event: &RangeTextInputEvent, _| {
            captured.borrow_mut().push(event.clone());
        })
        .detach();
    });
    let seed = restoration_seed(source, 1, ordinary_position(0));
    let restoration_geometry =
        validate_restoration_to_first_geometry_page(&input, cx, source, seed);

    assert!(matches!(
        input.update(cx, |input, cx| input.request_absolute_scroll(px(96.), cx)),
        Err(gpui_text_input::RangeTextInputError::Busy)
    ));
    let page = page_for(source, 84_000, restoration_geometry);
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.deliver_page(page, window, cx).unwrap()
        })
    });
    assert!(drive_pages(&input, cx, source).is_empty());
    input.read_with(cx, |input, _| {
        assert_eq!(input.export_restoration(None).unwrap(), seed);
        assert!(input.is_quiescent());
    });
    assert_eq!(
        events
            .borrow()
            .iter()
            .filter(|event| matches!(event, RangeTextInputEvent::RestorationRejected))
            .count(),
        0
    );
}

#[gpui::test]
fn pre_validation_queued_scroll_busy_preserves_validation_without_cancellation(
    cx: &mut gpui::TestAppContext,
) {
    let source = &(0..80)
        .map(|line| format!("line-{line:02}\n"))
        .collect::<String>();
    let (input, cx) = cx
        .add_window_view(|window, cx| RangeTextInput::new(config(source, 1), window, cx).unwrap());
    assert!(drive_pages(&input, cx, source).is_empty());
    let events = Rc::new(RefCell::new(Vec::new()));
    let captured = events.clone();
    cx.cx.update(|cx| {
        cx.subscribe(&input, move |_, event: &RangeTextInputEvent, _| {
            captured.borrow_mut().push(event.clone());
        })
        .detach();
    });
    let seed = restoration_seed(source, 1, ordinary_position(0));
    input.update(cx, |input, cx| input.import_restoration(seed, cx).unwrap());
    assert!(matches!(
        input.update(cx, |input, cx| input.request_absolute_scroll(px(96.), cx)),
        Err(gpui_text_input::RangeTextInputError::Busy)
    ));

    let first = input.update(cx, |input, _| input.take_request()).unwrap();
    let RangeTextInputRequest::Page(first) = first else {
        panic!("queued validation remains unchanged")
    };
    assert_eq!(first.key().purpose(), PagePurpose::Restoration);
    let page = page_for(source, 84_000, first);
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.deliver_page(page, window, cx).unwrap()
        })
    });
    assert!(drive_pages(&input, cx, source).is_empty());
    input.read_with(cx, |input, _| {
        assert_eq!(input.export_restoration(None).unwrap(), seed);
        assert!(input.is_quiescent());
    });
    assert_eq!(
        events
            .borrow()
            .iter()
            .filter(|event| matches!(event, RangeTextInputEvent::RestorationRejected))
            .count(),
        0
    );
}

#[gpui::test]
fn pre_validation_dispatched_scroll_busy_preserves_text_and_object_custody(
    cx: &mut gpui::TestAppContext,
) {
    let source = &(0..80)
        .map(|line| format!("line-{line:02}\n"))
        .collect::<String>();
    let seed = restoration_seed(source, 1, ordinary_position(0));

    let (text_input, cx) = cx
        .add_window_view(|window, cx| RangeTextInput::new(config(source, 1), window, cx).unwrap());
    assert!(drive_pages(&text_input, cx, source).is_empty());
    let text_events = Rc::new(RefCell::new(Vec::new()));
    let captured = text_events.clone();
    cx.cx.update(|cx| {
        cx.subscribe(&text_input, move |_, event: &RangeTextInputEvent, _| {
            captured.borrow_mut().push(event.clone());
        })
        .detach();
    });
    text_input.update(cx, |input, cx| input.import_restoration(seed, cx).unwrap());
    let text = text_input
        .update(cx, |input, _| input.take_request())
        .and_then(|request| match request {
            RangeTextInputRequest::Page(page) => Some(page),
            _ => None,
        })
        .expect("text validation is dispatched");
    assert!(matches!(
        text_input.update(cx, |input, cx| input.request_absolute_scroll(px(96.), cx)),
        Err(gpui_text_input::RangeTextInputError::Busy)
    ));
    let late = page_for(source, 84_100, text);
    cx.update(|window, app| {
        text_input.update(app, |input, cx| {
            input.deliver_page(late, window, cx).unwrap()
        })
    });
    assert!(drive_pages(&text_input, cx, source).is_empty());
    text_input.read_with(cx, |input, _| {
        assert_eq!(input.export_restoration(None).unwrap(), seed);
        assert!(input.is_quiescent());
    });
    assert_eq!(
        text_events
            .borrow()
            .iter()
            .filter(|event| matches!(event, RangeTextInputEvent::RestorationRejected))
            .count(),
        0
    );

    let (object_input, cx) = cx
        .add_window_view(|window, cx| RangeTextInput::new(config(source, 1), window, cx).unwrap());
    assert!(drive_pages(&object_input, cx, source).is_empty());
    let object_events = Rc::new(RefCell::new(Vec::new()));
    let captured = object_events.clone();
    cx.cx.update(|cx| {
        cx.subscribe(&object_input, move |_, event: &RangeTextInputEvent, _| {
            captured.borrow_mut().push(event.clone());
        })
        .detach();
    });
    let object = validate_restoration_to_first_object_page(&object_input, cx, source, seed);
    assert!(matches!(
        object_input.update(cx, |input, cx| input.request_absolute_scroll(px(96.), cx)),
        Err(gpui_text_input::RangeTextInputError::Busy)
    ));
    let late = restoration_object_page(object, &[], 84_101);
    object_input
        .update(cx, |input, cx| input.deliver_object_page(late, cx))
        .unwrap();
    assert!(drive_pages(&object_input, cx, source).is_empty());
    object_input.read_with(cx, |input, _| {
        assert_eq!(input.export_restoration(None).unwrap(), seed);
        assert!(input.is_quiescent());
    });
    assert_eq!(
        object_events
            .borrow()
            .iter()
            .filter(|event| matches!(event, RangeTextInputEvent::RestorationRejected))
            .count(),
        0
    );
}

#[gpui::test]
fn pre_validation_rebind_and_dispose_distinguish_queued_from_dispatched_requests(
    cx: &mut gpui::TestAppContext,
) {
    let source = "restoration lifecycle";
    let seed = restoration_seed(source, 1, ordinary_position(0));

    let (queued_rebind, cx) = cx
        .add_window_view(|window, cx| RangeTextInput::new(config(source, 1), window, cx).unwrap());
    assert!(drive_pages(&queued_rebind, cx, source).is_empty());
    let queued_rebind_events = restoration_events(&queued_rebind, cx);
    queued_rebind.update(cx, |input, cx| input.import_restoration(seed, cx).unwrap());
    cx.update(|window, app| {
        queued_rebind.update(app, |input, cx| {
            input.rebind(binding(source, 2), None, window, cx).unwrap()
        })
    });
    let first = queued_rebind
        .update(cx, |input, _| input.take_request())
        .expect("rebind starts ordinary successor geometry");
    let RangeTextInputRequest::Page(first) = first else {
        panic!("queued validation needs no host cancellation")
    };
    assert!(
        matches!(
            first.key().purpose(),
            PagePurpose::GeometryIndex | PagePurpose::GeometryTarget
        ),
        "unexpected first rebind page purpose: {:?}",
        first.key().purpose()
    );
    let page = page_for(source, 84_200, first);
    cx.update(|window, app| {
        queued_rebind.update(app, |input, cx| {
            input.deliver_page(page, window, cx).unwrap()
        })
    });
    assert!(drive_pages(&queued_rebind, cx, source).is_empty());
    queued_rebind.read_with(cx, |input, _| assert!(input.is_quiescent()));
    assert_eq!(
        queued_rebind_events
            .borrow()
            .iter()
            .filter(|event| matches!(event, RangeTextInputEvent::RestorationRejected))
            .count(),
        1
    );

    let (queued_dispose, cx) = cx
        .add_window_view(|window, cx| RangeTextInput::new(config(source, 1), window, cx).unwrap());
    assert!(drive_pages(&queued_dispose, cx, source).is_empty());
    let queued_dispose_events = restoration_events(&queued_dispose, cx);
    queued_dispose.update(cx, |input, cx| input.import_restoration(seed, cx).unwrap());
    let drained =
        cx.update(|window, app| queued_dispose.update(app, |input, cx| input.dispose(window, cx)));
    assert!(!drained.iter().any(|request| matches!(
        request,
        RangeTextInputRequest::CancelPage(_)
            | RangeTextInputRequest::CancelObjectPage(_)
            | RangeTextInputRequest::Page(_)
            | RangeTextInputRequest::ObjectPage(_)
    )));
    queued_dispose.read_with(cx, |input, _| assert!(input.is_quiescent()));
    assert_eq!(
        queued_dispose_events
            .borrow()
            .iter()
            .filter(|event| matches!(event, RangeTextInputEvent::RestorationRejected))
            .count(),
        1
    );

    let (dispatched_rebind, cx) = cx
        .add_window_view(|window, cx| RangeTextInput::new(config(source, 1), window, cx).unwrap());
    assert!(drive_pages(&dispatched_rebind, cx, source).is_empty());
    let dispatched_rebind_events = restoration_events(&dispatched_rebind, cx);
    dispatched_rebind.update(cx, |input, cx| input.import_restoration(seed, cx).unwrap());
    let validation = dispatched_rebind
        .update(cx, |input, _| input.take_request())
        .and_then(|request| match request {
            RangeTextInputRequest::Page(page) => Some(page),
            _ => None,
        })
        .expect("validation text page is dispatched");
    cx.update(|window, app| {
        dispatched_rebind.update(app, |input, cx| {
            input.rebind(binding(source, 2), None, window, cx).unwrap()
        })
    });
    let first_after_rebind = dispatched_rebind.update(cx, |input, _| input.take_request());
    assert!(
        matches!(
            first_after_rebind,
            Some(RangeTextInputRequest::CancelPage(key)) if key == validation.key()
        ),
        "unexpected first dispatched rebind effect: {first_after_rebind:?}"
    );
    let late = page_for(source, 84_201, validation);
    let late_result = cx.update(|window, app| {
        dispatched_rebind.update(app, |input, cx| input.deliver_page(late, window, cx))
    });
    assert!(
        matches!(
            late_result,
            Err(gpui_text_input::RangeTextInputError::Stale)
        ),
        "{late_result:?}"
    );
    assert!(drive_pages(&dispatched_rebind, cx, source).is_empty());
    dispatched_rebind.read_with(cx, |input, _| assert!(input.is_quiescent()));
    assert_eq!(
        dispatched_rebind_events
            .borrow()
            .iter()
            .filter(|event| matches!(event, RangeTextInputEvent::RestorationRejected))
            .count(),
        1
    );

    let (dispatched_dispose, cx) = cx
        .add_window_view(|window, cx| RangeTextInput::new(config(source, 1), window, cx).unwrap());
    assert!(drive_pages(&dispatched_dispose, cx, source).is_empty());
    let dispatched_dispose_events = restoration_events(&dispatched_dispose, cx);
    dispatched_dispose.update(cx, |input, cx| input.import_restoration(seed, cx).unwrap());
    let validation = dispatched_dispose
        .update(cx, |input, _| input.take_request())
        .and_then(|request| match request {
            RangeTextInputRequest::Page(page) => Some(page),
            _ => None,
        })
        .expect("validation text page is dispatched");
    let late = page_for(source, 84_202, validation.clone());
    let drained = cx.update(|window, app| {
        dispatched_dispose.update(app, |input, cx| input.dispose(window, cx))
    });
    assert_eq!(
        drained
            .iter()
            .filter(|request| matches!(
                request,
                RangeTextInputRequest::CancelPage(key) if *key == validation.key()
            ))
            .count(),
        1
    );
    assert!(matches!(
        cx.update(|window, app| {
            dispatched_dispose.update(app, |input, cx| input.deliver_page(late, window, cx))
        }),
        Err(gpui_text_input::RangeTextInputError::PageResponseRejected(
            _
        ))
    ));
    assert!(
        dispatched_dispose
            .update(cx, |input, _| input.take_request())
            .is_none()
    );
    dispatched_dispose.read_with(cx, |input, _| assert!(input.is_quiescent()));
    assert_eq!(
        dispatched_dispose_events
            .borrow()
            .iter()
            .filter(|event| matches!(event, RangeTextInputEvent::RestorationRejected))
            .count(),
        1
    );
}

#[gpui::test]
fn post_validation_restoration_rebind_and_dispose_cancel_and_reject_once(
    cx: &mut gpui::TestAppContext,
) {
    let source = "restore cancellation";
    fn assert_semantic_owners(
        input: &RangeTextInput,
        pending_index_intents: usize,
        empty_response_custody: (usize, usize),
    ) {
        let current = input.realization_diagnostics().current;
        let (resident_pages, resident_page_bytes, resident_objects, resident_object_bytes) =
            input.surface().map_or((0, 0, 0, 0), |surface| {
                (
                    surface.pages().len(),
                    surface
                        .pages()
                        .iter()
                        .map(|page| page.retained_charge().bytes())
                        .sum::<usize>(),
                    surface
                        .object_pages()
                        .iter()
                        .map(|page| page.retained_charge().objects())
                        .sum::<usize>(),
                    surface
                        .object_pages()
                        .iter()
                        .map(|page| page.retained_charge().bytes())
                        .sum::<usize>(),
                )
            });
        assert_eq!(current.resident_pages, resident_pages);
        assert_eq!(current.resident_page_bytes, resident_page_bytes);
        assert_eq!(current.resident_objects, resident_objects);
        assert_eq!(current.resident_object_bytes, resident_object_bytes);
        assert_eq!(current.pending_page_bytes, 0);
        assert_eq!(current.pending_object_bytes, 0);
        assert_eq!(current.clipboard_bytes, 0);
        assert_eq!(current.clipboard_items, 0);
        assert_eq!(current.request_payload_bytes, 0);
        assert_eq!(current.request_payload_items, 0);
        assert_eq!(current.deferred_response_bytes, 0);
        assert_eq!(current.deferred_response_items, 0);
        let expected_response_custody = if input.surface().is_some() {
            empty_response_custody
        } else {
            (0, 0)
        };
        assert_eq!(current.response_custody_bytes, expected_response_custody.0);
        assert_eq!(current.response_custody_items, expected_response_custody.1);
        assert_eq!(current.response_processing_bytes, 0);
        assert_eq!(current.candidate_bytes, 0);
        assert_eq!(current.candidate_items, 0);
        assert_eq!(current.pending_geometry_record_bytes, 0);
        assert_eq!(current.pending_geometry_record_items, 0);
        assert_eq!(current.pending_configuration_bytes, 0);
        assert_eq!(current.pending_configuration_items, 0);
        assert_eq!(current.pending_index_intents, pending_index_intents);
        assert_eq!(current.active_geometry_jobs, 0);
        assert_eq!(current.pending_page_requests, 0);
        assert_eq!(current.dispatched_page_requests, 0);
        assert_eq!(current.pending_object_requests, 0);
        assert_eq!(current.dispatched_object_requests, 0);
        assert_eq!(current.pending_geometry_pages, 0);
        assert_eq!(current.pending_geometry_objects, 0);
        assert_eq!(current.resident_geometry_page_waits, 0);
        assert_eq!(current.coalesced_geometry_page_waits, 0);
        assert_eq!(current.index_geometry_page_waits, 0);
        assert_eq!(current.target_geometry_page_waits, 0);
        assert_eq!(current.deferred_geometry_responses, 0);
        assert_eq!(current.response_custody_count, 0);
        assert_eq!(current.response_processing_items, 0);
        assert_eq!(current.candidates, 0);
        assert_eq!(current.scheduled_continuations, 0);
        assert_eq!(current.queued_requests, 0);
        assert_eq!(current.pending_target_intents, 0);
        assert_eq!(current.pending_layout_intents, 0);
        assert_eq!(current.pending_presentation_intents, 0);
        assert_eq!(current.pending_rebind_intents, 0);
        assert_eq!(current.page_alias_waits, 0);
        assert_eq!(input.clipboard_counts(), Default::default());
        if input.surface().is_none() {
            assert_eq!(
                current.geometry_bytes,
                std::mem::size_of::<ExactGeometryOwner>()
            );
            assert_eq!(current.geometry_items, 1);
            assert_eq!(current.owned_bytes, std::mem::size_of::<RangeTextInput>());
            assert_eq!(current.owned_items, 4);
            assert_eq!(current.request_storage_bytes, 0);
            assert_eq!(current.request_storage_items, 0);
            assert_eq!(current.page_alias_storage_bytes, 0);
            assert_eq!(current.page_alias_storage_items, 0);
            assert_eq!(current.dispatched_record_bytes, 0);
            assert_eq!(current.dispatched_record_items, 0);
            assert_eq!(current.checkpoints, 0);
        }
    }
    let (input, cx) = cx
        .add_window_view(|window, cx| RangeTextInput::new(config(source, 1), window, cx).unwrap());
    let empty_response_custody = input.read_with(cx, |input, _| {
        let current = input.realization_diagnostics().current;
        assert_eq!(current.response_custody_count, 0);
        (
            current.response_custody_bytes,
            current.response_custody_items,
        )
    });
    assert!(drive_pages(&input, cx, source).is_empty());
    let mut predecessor_text_dispatches = Vec::new();
    let mut predecessor_object_dispatches = Vec::new();
    let predecessor_object_release_keys = Vec::new();
    let mut predecessor_text_releases = Vec::new();
    let mut predecessor_object_releases = Vec::new();
    let events = Rc::new(RefCell::new(Vec::new()));
    let captured = events.clone();
    cx.cx.update(|cx| {
        cx.subscribe(&input, move |_, event: &RangeTextInputEvent, _| {
            captured.borrow_mut().push(event.clone());
        })
        .detach();
    });
    let seed = restoration_seed(source, 1, ordinary_position(0));
    input.update(cx, |input, cx| input.import_restoration(seed, cx).unwrap());
    let geometry = 'validation: {
        for page_id in 71_000..71_100 {
            match input.update(cx, |input, _| input.take_request()).unwrap() {
                RangeTextInputRequest::Page(request)
                    if request.key().purpose() == PagePurpose::Restoration =>
                {
                    let key = request.key();
                    assert_eq!(key.revision(), SourceRevision::new(1));
                    assert!(!predecessor_text_dispatches.contains(&key));
                    predecessor_text_dispatches.push(key);
                    let page = page_for(source, page_id, request);
                    assert_eq!(page.key(), key);
                    cx.update(|window, app| {
                        input.update(app, |input, cx| {
                            input.deliver_page(page, window, cx).unwrap()
                        })
                    });
                }
                RangeTextInputRequest::Page(request)
                    if matches!(
                        request.key().purpose(),
                        PagePurpose::GeometryIndex | PagePurpose::GeometryTarget
                    ) =>
                {
                    break 'validation request;
                }
                RangeTextInputRequest::ObjectPage(request) => {
                    let key = request.key();
                    assert_eq!(key.revision(), SourceRevision::new(1));
                    assert!(!predecessor_object_dispatches.contains(&key));
                    predecessor_object_dispatches.push(key);
                    let page = restoration_object_page(request, &[], page_id);
                    assert_eq!(page.key(), key);
                    cx.update(|window, app| {
                        input.update(app, |input, cx| {
                            input
                                .deliver_object_page_in_window(page, window, cx)
                                .unwrap()
                        })
                    });
                }
                RangeTextInputRequest::ReleasePage(key) => {
                    assert!(predecessor_text_dispatches.contains(&key));
                    assert!(!predecessor_text_releases.contains(&key));
                    predecessor_text_releases.push(key);
                }
                RangeTextInputRequest::ReleaseObjectPage(key) => {
                    assert!(predecessor_object_release_keys.contains(&key));
                    assert!(!predecessor_object_releases.contains(&key));
                    predecessor_object_releases.push(key);
                }
                other => panic!("unexpected restoration validation request: {other:?}"),
            }
        }
        panic!("restoration did not begin geometry within its bounded validation drive")
    };
    let restoration_geometry_key = geometry.key();
    let late_restoration_geometry = page_for(source, 72_999, geometry);
    let duplicate_restoration_geometry = late_restoration_geometry.clone();
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.rebind(binding(source, 2), None, window, cx).unwrap()
        })
    });
    assert_eq!(
        events
            .borrow()
            .iter()
            .filter(|event| matches!(event, RangeTextInputEvent::RestorationRejected))
            .count(),
        1
    );
    let mut exact_cancellations = 0;
    let mut successor_text_dispatches = Vec::new();
    let mut successor_text_releases = Vec::new();
    let mut successor_object_dispatches = Vec::new();
    let mut successor_object_releases = Vec::new();
    let mut observed_delayed_index_cut = false;
    let mut reached_quiescence = false;
    let mut page_id = 73_000;
    for _ in 0..256 {
        match input.update(cx, |input, _| input.take_request()) {
            Some(RangeTextInputRequest::CancelPage(key)) if key == restoration_geometry_key => {
                exact_cancellations += 1;
            }
            Some(RangeTextInputRequest::Page(request)) => {
                let key = request.key();
                assert_eq!(key.binding(), BindingId::new(17));
                assert_eq!(key.revision(), SourceRevision::new(2));
                assert!(!successor_text_dispatches.contains(&key));
                successor_text_dispatches.push(key);
                let page = page_for(source, page_id, request);
                assert_eq!(page.key(), key);
                page_id += 1;
                cx.update(|window, app| {
                    input.update(app, |input, cx| {
                        input.deliver_page(page, window, cx).unwrap()
                    })
                });
            }
            Some(RangeTextInputRequest::ObjectPage(request)) => {
                let key = request.key();
                assert_eq!(key.binding(), BindingId::new(17));
                assert_eq!(key.revision(), SourceRevision::new(2));
                assert!(!successor_object_dispatches.contains(&key));
                successor_object_dispatches.push(key);
                let page = restoration_object_page(request, &[], page_id);
                assert_eq!(page.key(), key);
                page_id += 1;
                cx.update(|window, app| {
                    input.update(app, |input, cx| {
                        input
                            .deliver_object_page_in_window(page, window, cx)
                            .unwrap()
                    })
                });
            }
            Some(RangeTextInputRequest::ReleasePage(key))
                if key.revision() == SourceRevision::new(2) =>
            {
                successor_text_releases.push(key)
            }
            Some(RangeTextInputRequest::ReleaseObjectPage(key))
                if key.revision() == SourceRevision::new(2) =>
            {
                successor_object_releases.push(key)
            }
            Some(RangeTextInputRequest::ReleasePage(key)) => {
                assert!(predecessor_text_dispatches.contains(&key));
                assert!(!predecessor_text_releases.contains(&key));
                predecessor_text_releases.push(key)
            }
            Some(RangeTextInputRequest::ReleaseObjectPage(key)) => {
                assert!(predecessor_object_release_keys.contains(&key));
                assert!(!predecessor_object_releases.contains(&key));
                predecessor_object_releases.push(key)
            }
            Some(other) => panic!("unexpected rebind request: {other:?}"),
            None => {
                if input.read_with(cx, |input, _| input.is_quiescent()) {
                    reached_quiescence = true;
                    break;
                }
                if !observed_delayed_index_cut {
                    input.read_with(cx, |input, _| {
                        assert_semantic_owners(input, 1, empty_response_custody);
                        assert_eq!(input.surface().unwrap().binding(), binding(source, 2));
                        assert!(input.is_surface_current_and_interactive());
                        assert!(input.is_semantically_quiescent());
                    });
                    observed_delayed_index_cut = true;
                }
                cx.update(|window, app| window.draw(app).clear());
                cx.run_until_parked();
            }
        }
    }
    assert!(
        reached_quiescence,
        "rebind lifecycle exceeded its bounded drive"
    );
    assert!(observed_delayed_index_cut);
    assert_eq!(exact_cancellations, 1);
    assert!(!predecessor_text_dispatches.is_empty());
    assert_eq!(
        predecessor_text_releases.len(),
        predecessor_text_dispatches.len(),
        "predecessor text releases {predecessor_text_releases:?} did not match dispatched keys {predecessor_text_dispatches:?}"
    );
    assert_eq!(
        predecessor_object_releases.len(),
        predecessor_object_release_keys.len(),
        "predecessor object releases {predecessor_object_releases:?} did not match expected keys {predecessor_object_release_keys:?}"
    );
    assert!(!predecessor_object_dispatches.is_empty());
    assert!(predecessor_object_dispatches
        .iter()
        .all(|key| key.purpose() == ObjectPurpose::Restoration));
    assert!(predecessor_text_dispatches.iter().all(|key| {
        predecessor_text_releases
            .iter()
            .filter(|released| *released == key)
            .count()
            == 1
    }));
    assert!(predecessor_object_release_keys.iter().all(|key| {
        predecessor_object_releases
            .iter()
            .filter(|released| *released == key)
            .count()
            == 1
    }));
    assert!(!successor_text_dispatches.is_empty());
    assert!(!successor_object_dispatches.is_empty());
    assert_eq!(
        successor_text_releases.len(),
        successor_text_dispatches.len()
    );
    assert_eq!(
        successor_object_releases.len(),
        successor_object_dispatches.len()
    );
    for key in &successor_text_dispatches {
        assert_eq!(
            successor_text_releases
                .iter()
                .filter(|released| *released == key)
                .count(),
            1
        );
    }
    for key in &successor_object_dispatches {
        assert_eq!(
            successor_object_releases
                .iter()
                .filter(|released| *released == key)
                .count(),
            1
        );
    }
    assert!(successor_text_releases
        .iter()
        .all(|key| successor_text_dispatches.contains(key)));
    assert!(successor_object_releases
        .iter()
        .all(|key| successor_object_dispatches.contains(key)));
    input.read_with(cx, |input, _| {
        assert_semantic_owners(input, 0, empty_response_custody);
        assert_eq!(input.surface().unwrap().binding(), binding(source, 2));
        assert!(input.is_semantically_quiescent());
        assert!(input.is_quiescent());
    });
    let successor_publication = range_publication_fingerprint(&input, cx);
    for obsolete in [late_restoration_geometry, duplicate_restoration_geometry] {
        let expected_return = obsolete.clone();
        let events_before = events.borrow().clone();
        let ownership_before = input.read_with(cx, |input, _| {
            assert_semantic_owners(input, 0, empty_response_custody);
            assert!(input.is_semantically_quiescent());
            assert!(input.is_quiescent());
            input.realization_diagnostics().current
        });
        let rejected = cx.update(|window, app| {
            input.update(app, |input, cx| input.deliver_page(obsolete, window, cx))
        });
        let Err(gpui_text_input::RangeTextInputError::PageResponseRejected(returned)) = rejected
        else {
            panic!("obsolete restoration geometry payload was not returned: {rejected:?}")
        };
        assert_eq!(returned, expected_return);
        assert_eq!(returned.key(), restoration_geometry_key);
        assert_eq!(
            range_publication_fingerprint(&input, cx),
            successor_publication
        );
        assert!(input.update(cx, |input, _| input.take_request()).is_none());
        input.read_with(cx, |input, _| {
            assert_semantic_owners(input, 0, empty_response_custody);
            assert_eq!(input.realization_diagnostics().current, ownership_before);
            assert!(input.is_semantically_quiescent());
            assert!(input.is_quiescent());
        });
        assert_eq!(events.borrow().as_slice(), events_before.as_slice());
        assert_eq!(
            events
                .borrow()
                .iter()
                .filter(|event| matches!(event, RangeTextInputEvent::RestorationRejected))
                .count(),
            1
        );
    }
    assert_eq!(
        events
            .borrow()
            .iter()
            .filter(|event| matches!(event, RangeTextInputEvent::RestorationRejected))
            .count(),
        1
    );

    let (disposed, cx) = cx
        .add_window_view(|window, cx| RangeTextInput::new(config(source, 1), window, cx).unwrap());
    assert!(drive_pages(&disposed, cx, source).is_empty());
    let dispose_events = Rc::new(RefCell::new(Vec::new()));
    let captured = dispose_events.clone();
    cx.cx.update(|cx| {
        cx.subscribe(&disposed, move |_, event: &RangeTextInputEvent, _| {
            captured.borrow_mut().push(event.clone());
        })
        .detach();
    });
    let geometry = validate_restoration_to_first_geometry_page(&disposed, cx, source, seed);
    let restoration_geometry_key = geometry.key();
    let late_restoration_geometry = page_for(source, 73_999, geometry);
    let duplicate_restoration_geometry = late_restoration_geometry.clone();
    let drained =
        cx.update(|window, app| disposed.update(app, |input, cx| input.dispose(window, cx)));
    assert_eq!(
        drained
            .iter()
            .filter(|request| matches!(
                request,
                RangeTextInputRequest::CancelPage(key) if *key == restoration_geometry_key
            ))
            .count(),
        1
    );
    assert_eq!(
        dispose_events
            .borrow()
            .iter()
            .filter(|event| matches!(event, RangeTextInputEvent::RestorationRejected))
            .count(),
        1
    );
    for obsolete in [late_restoration_geometry, duplicate_restoration_geometry] {
        let expected_return = obsolete.clone();
        let events_before = dispose_events.borrow().clone();
        let ownership_before = disposed.read_with(cx, |input, _| {
            assert_semantic_owners(input, 0, empty_response_custody);
            let current = input.realization_diagnostics().current;
            assert!(input.surface().is_none());
            assert!(input.is_semantically_quiescent());
            assert!(input.is_quiescent());
            current
        });
        let rejected = cx.update(|window, app| {
            disposed.update(app, |input, cx| input.deliver_page(obsolete, window, cx))
        });
        let Err(gpui_text_input::RangeTextInputError::PageResponseRejected(returned)) = rejected
        else {
            panic!("disposed restoration geometry payload was not returned: {rejected:?}")
        };
        assert_eq!(returned, expected_return);
        assert_eq!(returned.key(), restoration_geometry_key);
        assert!(disposed
            .update(cx, |input, _| input.take_request())
            .is_none());
        disposed.read_with(cx, |input, _| {
            assert_semantic_owners(input, 0, empty_response_custody);
            let current = input.realization_diagnostics().current;
            assert_eq!(current, ownership_before);
            assert!(input.surface().is_none());
            assert!(input.is_semantically_quiescent());
            assert!(input.is_quiescent());
        });
        assert_eq!(dispose_events.borrow().as_slice(), events_before.as_slice());
        assert_eq!(
            dispose_events
                .borrow()
                .iter()
                .filter(|event| matches!(event, RangeTextInputEvent::RestorationRejected))
                .count(),
            1
        );
    }
    disposed.read_with(cx, |input, _| {
        assert_semantic_owners(input, 0, empty_response_custody);
        assert!(input.surface().is_none());
        assert!(input.is_semantically_quiescent());
        assert!(input.is_quiescent());
    });
    assert_eq!(
        dispose_events
            .borrow()
            .iter()
            .filter(|event| matches!(event, RangeTextInputEvent::RestorationRejected))
            .count(),
        1
    );
}

#[gpui::test]
fn restoration_validation_retained_byte_cap_accepts_exact_and_rejects_one_over(
    cx: &mut gpui::TestAppContext,
) {
    let source = "x";
    let seed = restoration_seed(source, 1, ordinary_position(0));
    let mut exact_config = config(source, 1);
    exact_config.residency_limits = ResidencyLimits::new(8, 5, 8, 256).unwrap();
    let (exact, cx) =
        cx.add_window_view(|window, cx| RangeTextInput::new(exact_config, window, cx).unwrap());
    assert!(drive_pages(&exact, cx, source).is_empty());
    exact.update(cx, |input, cx| input.import_restoration(seed, cx).unwrap());
    let request = exact.update(cx, |input, _| input.take_request()).unwrap();
    let RangeTextInputRequest::Page(request) = request else {
        panic!("restoration text validation")
    };
    let page = restoration_validation_page_with_fallback(request, "1234", 72_000);
    assert_eq!(page.retained_bytes(), 5);
    cx.update(|window, app| {
        exact.update(app, |input, cx| {
            input.deliver_page(page, window, cx).unwrap()
        })
    });
    assert!(drive_pages(&exact, cx, source).is_empty());
    exact.read_with(cx, |input, _| {
        assert_eq!(input.export_restoration(None).unwrap(), seed);
        assert!(input.is_quiescent());
    });

    let mut over_config = config(source, 1);
    over_config.residency_limits = ResidencyLimits::new(8, 5, 8, 256).unwrap();
    let (over, cx) =
        cx.add_window_view(|window, cx| RangeTextInput::new(over_config, window, cx).unwrap());
    assert!(drive_pages(&over, cx, source).is_empty());
    let events = Rc::new(RefCell::new(Vec::new()));
    let captured = events.clone();
    cx.cx.update(|cx| {
        cx.subscribe(&over, move |_, event: &RangeTextInputEvent, _| {
            captured.borrow_mut().push(event.clone());
        })
        .detach();
    });
    over.update(cx, |input, cx| input.import_restoration(seed, cx).unwrap());
    let request = over.update(cx, |input, _| input.take_request()).unwrap();
    let RangeTextInputRequest::Page(request) = request else {
        panic!("restoration text validation")
    };
    let page = restoration_validation_page_with_fallback(request, "12345", 72_001);
    assert_eq!(page.retained_bytes(), 6);
    assert!(matches!(
        cx.update(|window, app| over.update(app, |input, cx| input.deliver_page(page, window, cx))),
        Err(gpui_text_input::RangeTextInputError::SurfaceCapacity)
    ));
    assert!(drive_pages(&over, cx, source).is_empty());
    over.read_with(cx, |input, _| {
        assert!(input.surface().is_none());
        assert!(input.is_quiescent());
    });
    assert_eq!(
        events
            .borrow()
            .iter()
            .filter(|event| matches!(event, RangeTextInputEvent::RestorationRejected))
            .count(),
        1
    );
}

#[gpui::test]
fn dispatched_clipboard_write_cancels_exactly_once_and_late_success_cannot_delete(
    cx: &mut gpui::TestAppContext,
) {
    let (rebound, cx) =
        cx.add_window_view(|window, cx| RangeTextInput::new(config("", 1), window, cx).unwrap());
    assert!(drive_pages(&rebound, cx, "").is_empty());
    let key = queue_empty_clipboard_cut(&rebound, cx);
    assert!(matches!(
        rebound.update(cx, |input, _| input.take_request()),
        Some(RangeTextInputRequest::ClipboardWrite(write)) if write.key() == key
    ));
    cx.update(|window, app| {
        rebound.update(app, |input, cx| {
            input.rebind(binding("", 2), None, window, cx).unwrap()
        })
    });
    let requests = drive_pages(&rebound, cx, "");
    assert_eq!(
        requests
            .iter()
            .filter(|request| matches!(
                request,
                RangeTextInputRequest::CancelClipboardWrite(cancelled) if *cancelled == key
            ))
            .count(),
        1
    );
    rebound.update(cx, |input, cx| {
        assert!(matches!(
            input.settle_clipboard_write(key, ClipboardWriteOutcome::Written, cx),
            Err(gpui_text_input::RangeTextInputError::Stale)
        ));
        assert!(input.is_quiescent());
    });

    let (disposed, cx) =
        cx.add_window_view(|window, cx| RangeTextInput::new(config("", 1), window, cx).unwrap());
    assert!(drive_pages(&disposed, cx, "").is_empty());
    let key = queue_empty_clipboard_cut(&disposed, cx);
    assert!(matches!(
        disposed.update(cx, |input, _| input.take_request()),
        Some(RangeTextInputRequest::ClipboardWrite(write)) if write.key() == key
    ));
    let drained =
        cx.update(|window, app| disposed.update(app, |input, cx| input.dispose(window, cx)));
    assert_eq!(
        drained
            .iter()
            .filter(|request| matches!(
                request,
                RangeTextInputRequest::CancelClipboardWrite(cancelled) if *cancelled == key
            ))
            .count(),
        1
    );
    disposed.update(cx, |input, cx| {
        assert!(matches!(
            input.settle_clipboard_write(key, ClipboardWriteOutcome::Written, cx),
            Err(gpui_text_input::RangeTextInputError::Stale)
        ));
        assert!(input.is_quiescent());
    });
}

#[gpui::test]
fn provenance_page_dispatch_acknowledgement_and_write_failure_release_exactly(
    cx: &mut gpui::TestAppContext,
) {
    let source = "ab";
    let mut configuration = config(source, 1);
    configuration.clipboard_limits = ClipboardLimits::new_composite(32, 4, 2, 64 * 1024)
        .unwrap()
        .with_provenance(ClipboardProvenancePolicy::Stream(
            ClipboardProvenanceLimits::new(1, 4096).unwrap(),
        ));
    let (input, cx) =
        cx.add_window_view(|window, cx| RangeTextInput::new(configuration, window, cx).unwrap());
    assert!(drive_pages(&input, cx, source).is_empty());

    let start = ordinary_position(0);
    let end = ordinary_position(2);
    let selection = SourceRange::new(start, end).unwrap();
    let (text, objects) = admitted_sources(source, 1, &[start, end]);
    input.update(cx, |input, cx| {
        input
            .begin_composite_clipboard(
                gpui_text_input::ClipboardKind::Copy,
                selection,
                MutationPositions::new(end, start, end),
                &text,
                &objects,
                cx,
            )
            .unwrap();
    });
    let facts = [object_fact(301, 1, 1), object_fact(302, 1, 2)];
    let mut page_id = 80_000;
    let mut provenance_pages = Vec::new();
    let mut write = None;
    for _ in 0..256 {
        let request = take_request_after_scheduled_frames(&input, cx, "clipboard progress request");
        match request {
            RangeTextInputRequest::Page(request)
                if request.key().purpose() == PagePurpose::Clipboard =>
            {
                let page = page_for(source, page_id, request);
                page_id += 1;
                cx.update(|window, app| {
                    input.update(app, |input, cx| {
                        input.deliver_page(page, window, cx).unwrap()
                    })
                });
            }
            RangeTextInputRequest::ObjectPage(request)
                if request.key().purpose() == ObjectPurpose::Clipboard =>
            {
                let page = restoration_object_page(request, &facts, page_id);
                page_id += 1;
                cx.update(|window, app| {
                    input.update(app, |input, cx| {
                        input
                            .deliver_object_page_in_window(page, window, cx)
                            .unwrap()
                    })
                });
            }
            RangeTextInputRequest::ClipboardProvenancePage(page) => {
                provenance_pages.push(page.clone());
                input.update(cx, |input, cx| {
                    input
                        .acknowledge_clipboard_provenance_page(page, cx)
                        .unwrap()
                });
            }
            RangeTextInputRequest::ClipboardWrite(request) => {
                write = Some(request);
                break;
            }
            RangeTextInputRequest::ReleasePage(_) | RangeTextInputRequest::ReleaseObjectPage(_) => {
            }
            other => panic!("unexpected clipboard request: {other:?}"),
        }
    }
    let write = write.expect("clipboard write within bounded request drive");

    assert_eq!(provenance_pages.len(), 2);
    assert_eq!(write.text(), "a[301][302]b");
    let closure = write.provenance().expect("provenance closure");
    assert_eq!(closure.page_count(), 2);
    assert_eq!(closure.item_count(), 2);
    assert_eq!(closure.fallback_bytes(), 10);
    assert_eq!(closure.output_bytes(), 12);
    input.update(cx, |input, cx| {
        assert_eq!(
            input
                .settle_clipboard_write(write.key(), ClipboardWriteOutcome::Failed, cx)
                .unwrap(),
            gpui_text_input::ClipboardCompletion::WriteFailed
        );
    });
    let lifecycle = drive_pages(&input, cx, source);
    assert!(lifecycle.iter().all(|request| matches!(
        request,
        RangeTextInputRequest::ReleasePage(_) | RangeTextInputRequest::ReleaseObjectPage(_)
    )));
    input.read_with(cx, |input, _| {
        assert_eq!(input.clipboard_counts(), Default::default())
    });
}

#[gpui::test]
fn dispatched_provenance_page_rebinds_with_one_exact_cancel_and_rejects_late_ack(
    cx: &mut gpui::TestAppContext,
) {
    let source = "ab";
    let mut configuration = config(source, 1);
    configuration.clipboard_limits = ClipboardLimits::new_composite(32, 4, 1, 64 * 1024)
        .unwrap()
        .with_provenance(ClipboardProvenancePolicy::Stream(
            ClipboardProvenanceLimits::new(1, 4096).unwrap(),
        ));
    let (input, cx) =
        cx.add_window_view(|window, cx| RangeTextInput::new(configuration, window, cx).unwrap());
    assert!(drive_pages(&input, cx, source).is_empty());

    let start = ordinary_position(0);
    let end = ordinary_position(2);
    let selection = SourceRange::new(start, end).unwrap();
    let (text, objects) = admitted_sources(source, 1, &[start, end]);
    input.update(cx, |input, cx| {
        input
            .begin_composite_clipboard(
                gpui_text_input::ClipboardKind::Copy,
                selection,
                MutationPositions::new(end, start, end),
                &text,
                &objects,
                cx,
            )
            .unwrap();
    });
    let facts = [object_fact(401, 1, 1)];
    let mut page_id = 81_000;
    let mut provenance = None;
    for _ in 0..256 {
        match take_request_after_scheduled_frames(&input, cx, "clipboard progress request") {
            RangeTextInputRequest::Page(request)
                if request.key().purpose() == PagePurpose::Clipboard =>
            {
                let page = page_for(source, page_id, request);
                page_id += 1;
                cx.update(|window, app| {
                    input.update(app, |input, cx| {
                        input.deliver_page(page, window, cx).unwrap()
                    })
                });
            }
            RangeTextInputRequest::ObjectPage(request)
                if request.key().purpose() == ObjectPurpose::Clipboard =>
            {
                let page = restoration_object_page(request, &facts, page_id);
                page_id += 1;
                cx.update(|window, app| {
                    input.update(app, |input, cx| {
                        input
                            .deliver_object_page_in_window(page, window, cx)
                            .unwrap()
                    })
                });
            }
            RangeTextInputRequest::ClipboardProvenancePage(page) => {
                provenance = Some(page);
                break;
            }
            RangeTextInputRequest::ReleasePage(_) | RangeTextInputRequest::ReleaseObjectPage(_) => {
            }
            other => panic!("unexpected clipboard request: {other:?}"),
        }
    }
    let provenance = provenance.expect("provenance page within bounded request drive");
    let provenance_key = provenance.key();

    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.rebind(binding(source, 2), None, window, cx).unwrap()
        })
    });
    let requests = drive_pages(&input, cx, source);
    assert_eq!(
        requests
            .iter()
            .filter(|request| matches!(
                request,
                RangeTextInputRequest::CancelClipboardProvenancePage(key) if *key == provenance_key
            ))
            .count(),
        1
    );
    input.update(cx, |input, cx| {
        assert!(matches!(
            input.acknowledge_clipboard_provenance_page(provenance, cx),
            Err(gpui_text_input::RangeTextInputError::Stale)
        ));
        assert_eq!(input.clipboard_counts(), Default::default());
    });
}

#[gpui::test]
fn stale_page_zero_cannot_clear_current_operation_dispatch(cx: &mut gpui::TestAppContext) {
    let source = "ab";
    let mut configuration = config(source, 1);
    configuration.clipboard_limits = ClipboardLimits::new_composite(64, 4, 2, 64 * 1024)
        .unwrap()
        .with_provenance(ClipboardProvenancePolicy::Stream(
            ClipboardProvenanceLimits::new(1, 4096).unwrap(),
        ));
    let (input, cx) =
        cx.add_window_view(|window, cx| RangeTextInput::new(configuration, window, cx).unwrap());
    assert!(drive_pages(&input, cx, source).is_empty());

    let start = ordinary_position(0);
    let end = ordinary_position(2);
    let selection = SourceRange::new(start, end).unwrap();
    let predecessor = MutationPositions::new(end, start, end);
    let (text, objects) = admitted_sources(source, 1, &[start, end]);
    let mut page_id = 82_000;

    input.update(cx, |input, cx| {
        input
            .begin_composite_clipboard(
                gpui_text_input::ClipboardKind::Copy,
                selection,
                predecessor,
                &text,
                &objects,
                cx,
            )
            .unwrap();
    });
    let old_page =
        take_clipboard_provenance_page(&input, cx, source, &[object_fact(501, 1, 1)], &mut page_id);
    input.update(cx, |input, cx| {
        input
            .acknowledge_clipboard_provenance_page(old_page.clone(), cx)
            .unwrap()
    });
    let mut old_write = None;
    for _ in 0..256 {
        match take_request_after_scheduled_frames(&input, cx, "first clipboard write") {
            RangeTextInputRequest::ClipboardWrite(write) => {
                old_write = Some(write);
                break;
            }
            RangeTextInputRequest::Page(request)
                if request.key().purpose() == PagePurpose::Clipboard =>
            {
                let page = page_for(source, page_id, request);
                page_id += 1;
                cx.update(|window, app| {
                    input.update(app, |input, cx| {
                        input.deliver_page(page, window, cx).unwrap()
                    })
                });
            }
            RangeTextInputRequest::ObjectPage(request)
                if request.key().purpose() == ObjectPurpose::Clipboard =>
            {
                let page = restoration_object_page(request, &[object_fact(501, 1, 1)], page_id);
                page_id += 1;
                cx.update(|window, app| {
                    input.update(app, |input, cx| {
                        input
                            .deliver_object_page_in_window(page, window, cx)
                            .unwrap()
                    })
                });
            }
            RangeTextInputRequest::ReleasePage(_) | RangeTextInputRequest::ReleaseObjectPage(_) => {
            }
            other => panic!("unexpected first clipboard request: {other:?}"),
        }
    }
    let old_write = old_write.expect("first clipboard write within bounded request drive");
    input.update(cx, |input, cx| {
        input
            .settle_clipboard_write(old_write.key(), ClipboardWriteOutcome::Failed, cx)
            .unwrap();
    });

    input.update(cx, |input, cx| {
        input
            .begin_composite_clipboard(
                gpui_text_input::ClipboardKind::Copy,
                selection,
                predecessor,
                &text,
                &objects,
                cx,
            )
            .unwrap();
    });
    let current_page = take_clipboard_provenance_page(
        &input,
        cx,
        source,
        &[object_fact(601, 1, 1), object_fact(602, 1, 2)],
        &mut page_id,
    );
    assert_eq!(old_page.key().page_ordinal(), 0);
    assert_eq!(current_page.key().page_ordinal(), 0);
    assert_ne!(old_page.key().clipboard(), current_page.key().clipboard());
    input.update(cx, |input, cx| {
        assert!(matches!(
            input.acknowledge_clipboard_provenance_page(old_page, cx),
            Err(gpui_text_input::RangeTextInputError::Stale)
        ));
        input
            .acknowledge_clipboard_provenance_page(current_page, cx)
            .unwrap();
    });
    let next_page = take_clipboard_provenance_page(
        &input,
        cx,
        source,
        &[object_fact(601, 1, 1), object_fact(602, 1, 2)],
        &mut page_id,
    );
    let next_key = next_page.key();

    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.rebind(binding(source, 2), None, window, cx).unwrap()
        })
    });
    let requests = drive_pages(&input, cx, source);
    assert_eq!(
        requests
            .iter()
            .filter(|request| matches!(
                request,
                RangeTextInputRequest::CancelClipboardProvenancePage(key) if *key == next_key
            ))
            .count(),
        1
    );
    input.update(cx, |input, cx| {
        assert!(matches!(
            input.acknowledge_clipboard_provenance_page(next_page, cx),
            Err(gpui_text_input::RangeTextInputError::Stale)
        ));
        assert_eq!(input.clipboard_counts(), Default::default());
    });
}

#[gpui::test]
fn clipboard_ownership_is_exact_across_objects_shared_page_write_and_release(
    cx: &mut gpui::TestAppContext,
) {
    let source = "ab";
    let facts = [
        object_fact_with_fallback(701, 1, 1, String::new()),
        object_fact_with_fallback(702, 1, 2, "x".repeat(512 * 1024)),
    ];
    let configured = |max_surface_bytes| {
        let mut configuration = config(source, 1);
        configuration.limits.max_surface_bytes = max_surface_bytes;
        configuration.limits.max_surface_items = 2 * 1024 * 1024;
        configuration.clipboard_limits =
            ClipboardLimits::new_composite(1024 * 1024, 4, 2, 1024 * 1024)
                .unwrap()
                .with_provenance(ClipboardProvenancePolicy::Stream(
                    ClipboardProvenanceLimits::new(2, 4096).unwrap(),
                ));
        configuration.object_residency_limits =
            ObjectResidencyLimits::new(8, 64, 1024 * 1024, 1024 * 1024, 8, 64, 1024 * 1024)
                .unwrap();
        configuration
    };

    let (probe, cx) = cx.add_window_view(|window, cx| {
        RangeTextInput::new(configured(4 * 1024 * 1024), window, cx).unwrap()
    });
    assert!(drive_pages(&probe, cx, source).is_empty());
    let before_peak = probe.read_with(cx, |input, _| {
        input.realization_diagnostics().high_water.owned_bytes
    });
    let mut page_id = 83_000;
    let page = begin_clipboard_to_provenance(&probe, cx, source, &facts, &mut page_id).unwrap();
    assert_eq!(page.items().len(), 2);
    assert_eq!(
        page.items()[0].output_range().start(),
        page.items()[0].output_range().end()
    );
    probe.read_with(cx, |input, _| {
        let counts = input.clipboard_counts();
        let diagnostics = input.realization_diagnostics();
        assert_eq!(counts.retained_object_facts, 0);
        assert_eq!(counts.retained_provenance_items, 2);
        assert_eq!(diagnostics.current.clipboard_bytes, counts.owned_bytes);
        assert_eq!(diagnostics.current.clipboard_items, counts.owned_items);
        assert_eq!(diagnostics.current.request_payload_bytes, 0);
        assert_eq!(diagnostics.high_water.request_payload_bytes, 0);
        assert!(diagnostics.high_water.owned_bytes > before_peak);
    });
    probe.update(cx, |input, cx| {
        input
            .acknowledge_clipboard_provenance_page(page, cx)
            .unwrap()
    });
    while probe.read_with(cx, |input, _| input.clipboard_counts().staged_bytes) != 0 {
        let Some(request) = probe.update(cx, |input, _| input.take_request()) else {
            cx.update(|window, app| window.draw(app).clear());
            cx.run_until_parked();
            continue;
        };
        match request {
            RangeTextInputRequest::Page(request)
                if request.key().purpose() == PagePurpose::Clipboard =>
            {
                let page = page_for(source, page_id, request);
                page_id += 1;
                cx.update(|window, app| {
                    probe.update(app, |input, cx| {
                        input.deliver_page(page, window, cx).unwrap()
                    })
                });
            }
            RangeTextInputRequest::ReleasePage(_) | RangeTextInputRequest::ReleaseObjectPage(_) => {
            }
            other => panic!("unexpected remaining clipboard request: {other:?}"),
        }
    }
    let transfer = probe.read_with(cx, |input, _| {
        let counts = input.clipboard_counts();
        let diagnostics = input.realization_diagnostics();
        assert_eq!(counts.staged_bytes, 0);
        assert_eq!(counts.retained_object_facts, 0);
        assert_eq!(counts.retained_provenance_items, 0);
        assert_eq!(diagnostics.current.clipboard_bytes, counts.owned_bytes);
        assert!(counts.owned_bytes > 0);
        assert!(diagnostics.current.request_payload_bytes >= 512 * 1024 + 2);
        counts.owned_bytes
    });
    let write = loop {
        let Some(request) = probe.update(cx, |input, _| input.take_request()) else {
            cx.update(|window, app| window.draw(app).clear());
            cx.run_until_parked();
            continue;
        };
        match request {
            RangeTextInputRequest::ClipboardWrite(write) => break write,
            RangeTextInputRequest::ReleasePage(_) | RangeTextInputRequest::ReleaseObjectPage(_) => {
            }
            other => panic!("unexpected final clipboard request: {other:?}"),
        }
    };
    probe.read_with(cx, |input, _| {
        let diagnostics = input.realization_diagnostics();
        assert_eq!(diagnostics.current.clipboard_bytes, transfer);
        assert_eq!(diagnostics.current.request_payload_bytes, 0);
    });
    probe.update(cx, |input, cx| {
        input
            .settle_clipboard_write(write.key(), ClipboardWriteOutcome::Failed, cx)
            .unwrap();
        assert_eq!(input.clipboard_counts(), Default::default());
        assert_eq!(input.realization_diagnostics().current.clipboard_bytes, 0);
    });
    let exact_peak = probe.read_with(cx, |input, _| {
        input.realization_diagnostics().high_water.owned_bytes
    });

    let (exact, cx) = cx.add_window_view(|window, cx| {
        RangeTextInput::new(configured(exact_peak), window, cx).unwrap()
    });
    assert!(drive_pages(&exact, cx, source).is_empty());
    let mut exact_page_id = 84_000;
    let exact_page =
        begin_clipboard_to_provenance(&exact, cx, source, &facts, &mut exact_page_id).unwrap();
    assert_eq!(
        exact.read_with(cx, |input, _| {
            input.realization_diagnostics().high_water.owned_bytes
        }),
        exact_peak
    );
    let exact_key = exact_page.key().clipboard();
    cx.update(|window, app| {
        exact.update(app, |input, cx| {
            input.rebind(binding(source, 2), None, window, cx).unwrap()
        })
    });
    let cancelled = drive_pages(&exact, cx, source);
    assert!(cancelled.iter().any(|request| matches!(
        request,
        RangeTextInputRequest::CancelClipboardProvenancePage(key)
            if key.clipboard() == exact_key
    )));
    exact.read_with(cx, |input, _| {
        assert_eq!(input.clipboard_counts(), Default::default());
        assert_eq!(input.realization_diagnostics().current.clipboard_bytes, 0);
    });

    let (one_under, cx) = cx.add_window_view(|window, cx| {
        RangeTextInput::new(configured(exact_peak - 1), window, cx).unwrap()
    });
    assert!(drive_pages(&one_under, cx, source).is_empty());
    let mut rejected_page_id = 85_000;
    let rejected =
        begin_clipboard_to_provenance(&one_under, cx, source, &facts, &mut rejected_page_id);
    assert!(
        matches!(
            rejected,
            Err(gpui_text_input::RangeTextInputError::SurfaceCapacity)
        ),
        "unexpected one-under result: {rejected:?}"
    );
    one_under.read_with(cx, |input, _| {
        let counts = input.clipboard_counts();
        assert_eq!(counts.pending_object_pages, 1);
        assert_eq!(counts.retained_object_facts, 2);
        assert!(counts.owned_bytes > 0);
    });
    let before_retry = one_under.read_with(cx, |input, _| input.clipboard_counts());
    let retry = cx.update(|window, app| {
        one_under.update(app, |input, cx| {
            input.rebind(binding(source, 2), None, window, cx)
        })
    });
    assert!(matches!(
        retry,
        Err(gpui_text_input::RangeTextInputError::SurfaceCapacity)
    ));
    one_under.read_with(cx, |input, _| {
        assert_eq!(input.clipboard_counts(), before_retry);
        assert!(input.realization_diagnostics().current.clipboard_bytes > 0);
    });
}

#[gpui::test]
fn shared_large_object_presentation_is_charged_once_through_publication(
    cx: &mut gpui::TestAppContext,
) {
    let source = "ab";
    let display_len = 8 * 1024;
    let mut display = String::with_capacity(64 * 1024);
    display.push_str(&"p".repeat(display_len));
    assert!(display.capacity() > display.len());
    let facts = [InlineObjectFact::new(
        InlineObjectId::new(92_911),
        ByteOffset::new(1),
        InlineObjectOrder::new(1),
        "[large]",
        InlineObjectPresentation::new(92_911, display, px(120.), px(100.), px(80.), None, 0, true)
            .unwrap(),
    )];
    let configured = |max_surface_bytes| {
        let mut configuration = config(source, 1);
        configuration.layout.limits.segment_bytes = 16 * 1024;
        configuration.layout.limits.runs = 16;
        configuration.layout.limits.decorations = 16;
        configuration.layout.limits.glyphs = 16 * 1024;
        configuration.layout.limits.wraps = 256;
        configuration.layout.limits.maps = 513;
        configuration.layout.limits.fragments = 8;
        configuration.layout.limits.retained_items = 16 * 1024;
        configuration.layout.limits.retained_bytes = 512 * 1024;
        configuration.geometry_limits =
            ExactGeometryLimits::new(16 * 1024, 16, 2 * 1024 * 1024, 64 * 1024).unwrap();
        configuration.residency_limits = ResidencyLimits::new(8, 512 * 1024, 8, 32 * 1024).unwrap();
        configuration.object_residency_limits =
            ObjectResidencyLimits::new(8, 64, 512 * 1024, 256 * 1024, 8, 64, 512 * 1024).unwrap();
        configuration.limits =
            RangeTextInputLimits::new(max_surface_bytes, 64 * 1024, 8, px(256.), 32, 32, px(16.))
                .unwrap();
        configuration
    };
    let drive = |input: &gpui::Entity<RangeTextInput>,
                 cx: &mut gpui::VisualTestContext,
                 facts: &[InlineObjectFact]| {
        let mut observed_quiescent = false;
        for _ in 0..512 {
            if !facts.is_empty()
                && input.read_with(cx, |input, _| {
                    input
                        .surface()
                        .is_some_and(|surface| !surface.realized_objects().is_empty())
                })
            {
                return Ok(());
            }
            if input.read_with(cx, |input, _| {
                input
                    .realization_diagnostics()
                    .last_response_rejection
                    .is_some()
            }) {
                return Err(gpui_text_input::RangeTextInputError::SurfaceCapacity);
            }
            let request = input.update(cx, |input, _| input.take_request());
            let had_request = request.is_some();
            match request {
                Some(RangeTextInputRequest::Page(request)) => {
                    observed_quiescent = false;
                    let page = page_for(source, request.key().id().get(), request);
                    cx.update(|window, app| {
                        input.update(app, |input, cx| input.deliver_page(page, window, cx))
                    })?;
                }
                Some(RangeTextInputRequest::ObjectPage(request)) => {
                    observed_quiescent = false;
                    let page = restoration_object_page(request, facts, request.key().id().get());
                    cx.update(|window, app| {
                        input.update(app, |input, cx| {
                            input.deliver_object_page_in_window(page, window, cx)
                        })
                    })?;
                }
                Some(RangeTextInputRequest::ReleasePage(_))
                | Some(RangeTextInputRequest::CancelPage(_))
                | Some(RangeTextInputRequest::ReleaseObjectPage(_))
                | Some(RangeTextInputRequest::CancelObjectPage(_)) => {
                    observed_quiescent = false;
                }
                None => {
                    let quiescent = input.read_with(cx, |input, _| input.is_quiescent());
                    if quiescent && observed_quiescent {
                        return Ok(());
                    }
                    observed_quiescent = quiescent;
                }
                Some(request) => panic!("unexpected large-display request: {request:?}"),
            }
            if !had_request {
                cx.update(|window, app| window.draw(app).clear());
                cx.run_until_parked();
            }
        }
        panic!(
            "large-display drive exhausted: {:?}",
            input.read_with(cx, |input, _| input.realization_diagnostics())
        )
    };

    let (probe, cx) = cx.add_window_view(|window, cx| {
        RangeTextInput::new(configured(16 * 1024 * 1024), window, cx).unwrap()
    });
    drive(&probe, cx, &facts).unwrap_or_else(|error| {
        panic!(
            "large-display probe failed with {error:?}: {:?}",
            probe.read_with(cx, |input, _| input.realization_diagnostics())
        )
    });
    let exact_bytes = probe.read_with(cx, |input, _| {
        let diagnostics = input.realization_diagnostics();
        assert_eq!(
            input.surface().unwrap().object_pages()[0].objects()[0]
                .presentation()
                .display()
                .len(),
            display_len
        );
        assert!(diagnostics.current.resident_object_bytes >= display_len);
        diagnostics.high_water.owned_bytes
    });

    let (exact, cx) = cx.add_window_view(|window, cx| {
        RangeTextInput::new(configured(exact_bytes), window, cx).unwrap()
    });
    drive(&exact, cx, &facts).unwrap();
    exact.read_with(cx, |input, _| {
        assert_eq!(
            input.realization_diagnostics().high_water.owned_bytes,
            exact_bytes
        );
        assert_eq!(
            input.surface().unwrap().object_pages()[0].objects()[0]
                .presentation()
                .display()
                .len(),
            display_len
        );
    });
    cx.update(|window, app| {
        exact.update(app, |input, cx| {
            let _ = input.dispose(window, cx);
        })
    });
    exact.read_with(cx, |input, _| {
        assert_eq!(
            input
                .realization_diagnostics()
                .current
                .resident_object_bytes,
            0
        );
        assert!(input.surface().is_none());
    });

    let (one_under, cx) = cx.add_window_view(|window, cx| {
        RangeTextInput::new(configured(exact_bytes - 1), window, cx).unwrap()
    });
    assert!(matches!(
        drive(&one_under, cx, &facts),
        Err(gpui_text_input::RangeTextInputError::SurfaceCapacity)
    ));
    one_under.read_with(cx, |input, _| {
        let diagnostics = input.realization_diagnostics();
        assert_eq!(diagnostics.current.response_custody_count, 1);
        assert!(diagnostics.surface_high_water.bytes < exact_bytes);
        assert!(
            input
                .surface()
                .is_none_or(|surface| surface.realized_objects().is_empty())
        );
    });
}

#[gpui::test]
fn clipboard_sparse_response_capacity_is_admitted_before_transfer_allocation(
    cx: &mut gpui::TestAppContext,
) {
    let source = "ab";
    let configured = |max_surface_bytes, max_surface_items| {
        let mut configuration = config(source, 1);
        configuration.limits.max_surface_bytes = max_surface_bytes;
        configuration.limits.max_surface_items = max_surface_items;
        configuration.clipboard_limits = ClipboardLimits::new_composite(1024, 4, 32, 1024 * 1024)
            .unwrap()
            .with_provenance(ClipboardProvenancePolicy::Stream(
                ClipboardProvenanceLimits::new(1, 4096).unwrap(),
            ));
        configuration
    };
    let stage_request = |input: &gpui::Entity<RangeTextInput>, cx: &mut gpui::VisualTestContext| {
        let start = ordinary_position(0);
        let end = ordinary_position(2);
        let (text, objects) = admitted_sources(source, 1, &[start, end]);
        input
            .update(cx, |input, cx| {
                input.begin_composite_clipboard(
                    gpui_text_input::ClipboardKind::Copy,
                    SourceRange::new(start, end).unwrap(),
                    MutationPositions::new(end, start, end),
                    &text,
                    &objects,
                    cx,
                )
            })
            .unwrap();
        loop {
            match input.update(cx, |input, _| input.take_request()).unwrap() {
                RangeTextInputRequest::ObjectPage(request)
                    if request.key().purpose() == ObjectPurpose::Clipboard =>
                {
                    break request;
                }
                RangeTextInputRequest::ReleasePage(_)
                | RangeTextInputRequest::ReleaseObjectPage(_) => {}
                other => panic!("unexpected sparse clipboard request: {other:?}"),
            }
        }
    };
    let response = |request: gpui_text_input::ObjectRequest, id| {
        let mut fallback = String::with_capacity(128 * 1024);
        fallback.push('x');
        let mut display = String::with_capacity(64 * 1024);
        display.push('p');
        let presentation =
            InlineObjectPresentation::new(id, display, px(8.0), px(8.0), px(6.0), None, 0, true)
                .unwrap();
        let mut objects = Vec::with_capacity(512);
        objects.push(InlineObjectFact::new(
            InlineObjectId::new(id.into()),
            ByteOffset::new(1),
            InlineObjectOrder::new(1),
            fallback,
            presentation,
        ));
        ObjectPage::new(
            ObjectPageId::new(id),
            request.key(),
            objects,
            ObjectPageEdgeFact::EnvelopeBoundary,
            ObjectPageEdgeFact::EnvelopeBoundary,
            true,
            None,
        )
        .unwrap()
    };

    let (probe, cx) = cx.add_window_view(|window, cx| {
        RangeTextInput::new(configured(2 * 1024 * 1024, 2 * 1024 * 1024), window, cx).unwrap()
    });
    assert!(drive_pages(&probe, cx, source).is_empty());
    let request = stage_request(&probe, cx);
    let page = response(request, 93_001);
    cx.update(|window, app| {
        probe.update(app, |input, cx| {
            input
                .deliver_object_page_in_window(page, window, cx)
                .unwrap()
        })
    });
    let (exact_bytes, exact_items) = probe.read_with(cx, |input, _| {
        let admission = input.realization_diagnostics().surface_high_water;
        (admission.bytes, admission.items)
    });

    let (one_under, cx) = cx.add_window_view(|window, cx| {
        RangeTextInput::new(configured(exact_bytes - 1, 2 * 1024 * 1024), window, cx).unwrap()
    });
    assert!(drive_pages(&one_under, cx, source).is_empty());
    let request = stage_request(&one_under, cx);
    let page = response(request, 93_001);
    let rejected = cx
        .update(|window, app| {
            one_under.update(app, |input, cx| {
                input.deliver_object_page_in_window(page, window, cx)
            })
        })
        .unwrap_err();
    let gpui_text_input::RangeTextInputError::ObjectResponseCapacity(page) = rejected else {
        panic!("unexpected sparse one-under response: {rejected:?}");
    };
    assert_eq!(page.objects().len(), 1);
    assert_eq!(page.objects()[0].fallback_copy(), "x");
    one_under.read_with(cx, |input, _| {
        assert_eq!(input.clipboard_counts().retained_object_facts, 0);
        assert!(input.realization_diagnostics().surface_high_water.bytes < exact_bytes);
    });

    let (item_under, cx) = cx.add_window_view(|window, cx| {
        RangeTextInput::new(configured(2 * 1024 * 1024, exact_items - 1), window, cx).unwrap()
    });
    assert!(drive_pages(&item_under, cx, source).is_empty());
    let request = stage_request(&item_under, cx);
    let page = response(request, 93_001);
    let rejected = cx
        .update(|window, app| {
            item_under.update(app, |input, cx| {
                input.deliver_object_page_in_window(page, window, cx)
            })
        })
        .unwrap_err();
    assert!(matches!(
        rejected,
        gpui_text_input::RangeTextInputError::ObjectResponseCapacity(_)
    ));
    item_under.read_with(cx, |input, _| {
        assert_eq!(input.clipboard_counts().retained_object_facts, 0);
        assert!(input.realization_diagnostics().surface_high_water.items < exact_items);
    });
}

#[gpui::test]
fn clipboard_prepare_exact_fit_and_one_under_cross_split_atom_before_empty_object_provenance(
    cx: &mut gpui::TestAppContext,
) {
    let source = format!("abcdefghij\n{}", "tail\n".repeat(32));
    let selected_end = 10;
    let target_block = px(320.);
    let atom = AtomId::new(711);
    let atom_range = ByteRange::from_u64(0, 8).unwrap();
    let facts = [object_fact_with_fallback(712, 9, 1, String::new())];
    let configured = |max_surface_bytes| {
        let mut configuration = config(&source, 1);
        configuration.limits.max_surface_bytes = max_surface_bytes;
        configuration.limits.max_surface_items = 2 * 1024 * 1024;
        configuration.clipboard_limits = ClipboardLimits::new_composite(64, 8, 4, 64 * 1024)
            .unwrap()
            .with_provenance(ClipboardProvenancePolicy::Stream(
                ClipboardProvenanceLimits::new(32 * 1024, 4 * 1024 * 1024).unwrap(),
            ));
        configuration
    };

    let (probe, cx) = cx.add_window_view(|window, cx| {
        RangeTextInput::new(configured(16 * 1024 * 1024), window, cx).unwrap()
    });
    drive_pages_with_split_atom_to_quiescence(&probe, cx, &source, atom, atom_range, "");
    let baseline = probe.read_with(cx, |input, _| {
        input.realization_diagnostics().high_water.owned_bytes
    });
    let probe_target = hold_same_revision_geometry_target(&probe, cx, target_block);
    let mut page_id = 86_000;
    let SplitAtomClipboardAttempt {
        key: probe_begin_key,
        provenance: probe_provenance,
        delivered_pages: probe_delivered_pages,
        released_pages: probe_released_pages,
        delivered_object_pages: probe_delivered_object_pages,
        released_object_pages: probe_released_object_pages,
    } = begin_split_atom_clipboard_to_provenance(
        &probe,
        cx,
        &source,
        selected_end,
        &facts,
        atom,
        atom_range,
        "",
        &mut page_id,
    )
    .unwrap();
    let page = probe_provenance.unwrap();
    assert_eq!(page.key().clipboard(), probe_begin_key);
    assert_exact_clipboard_text_response_releases(
        probe_begin_key,
        &probe_delivered_pages,
        &probe_released_pages,
    );
    assert_exact_clipboard_object_response_releases(
        probe_begin_key,
        &probe_delivered_object_pages,
        &probe_released_object_pages,
    );
    assert_eq!(probe_target.key().binding(), probe_begin_key.binding());
    assert_eq!(probe_target.key().revision(), probe_begin_key.revision());
    assert_eq!(page.items().len(), 1);
    assert_eq!(page.items()[0].object_id(), facts[0].id());
    assert_eq!(page.items()[0].output_range().start(), ByteOffset::new(1));
    assert_eq!(page.items()[0].output_range().end(), ByteOffset::new(1));
    assert!(!probe_released_pages.is_empty());
    assert!(probe_released_pages
        .iter()
        .all(|key| key.purpose() == PagePurpose::Clipboard));
    let exact_peak = probe.read_with(cx, |input, _| {
        input.realization_diagnostics().high_water.owned_bytes
    });
    assert!(exact_peak > baseline);
    let probe_page_key = page.key();
    cx.update(|window, app| {
        probe.update(app, |input, cx| {
            input.rebind(binding(&source, 2), None, window, cx).unwrap()
        })
    });
    assert_eq!(probe_target.key().purpose(), PagePurpose::GeometryTarget);
    let probe_cleanup = drain_rebound_surface_strict(&probe, cx, &source);
    assert_eq!(
        probe_cleanup
            .iter()
            .filter(|request| matches!(
                request,
                RangeTextInputRequest::CancelClipboardProvenancePage(key)
                    if *key == probe_page_key
            ))
            .count(),
        1
    );
    assert_eq!(
        probe_cleanup
            .iter()
            .filter(|request| matches!(
                request,
                RangeTextInputRequest::CancelPage(key) if *key == probe_target.key()
            ))
            .count(),
        1
    );
    assert!(!probe_cleanup.iter().any(|request| matches!(
        request,
        RangeTextInputRequest::CancelPage(key) if probe_delivered_pages.contains(key)
    )));
    let mut probe_all_released_pages = probe_released_pages.clone();
    probe_all_released_pages.extend(probe_cleanup.iter().filter_map(|request| match request {
        RangeTextInputRequest::ReleasePage(key) if key.purpose() == PagePurpose::Clipboard => {
            Some(*key)
        }
        _ => None,
    }));
    let mut probe_all_released_object_pages = probe_released_object_pages.clone();
    probe_all_released_object_pages.extend(probe_cleanup.iter().filter_map(
        |request| match request {
            RangeTextInputRequest::ReleaseObjectPage(key)
                if key.purpose() == ObjectPurpose::Clipboard =>
            {
                Some(*key)
            }
            _ => None,
        },
    ));
    assert_exact_clipboard_text_response_releases(
        probe_begin_key,
        &probe_delivered_pages,
        &probe_all_released_pages,
    );
    assert_exact_clipboard_object_response_releases(
        probe_begin_key,
        &probe_delivered_object_pages,
        &probe_all_released_object_pages,
    );
    probe.update(cx, |input, cx| {
        assert!(matches!(
            input.acknowledge_clipboard_provenance_page(page, cx),
            Err(gpui_text_input::RangeTextInputError::Stale)
        ));
    });

    let (exact, cx) = cx.add_window_view(|window, cx| {
        RangeTextInput::new(configured(exact_peak), window, cx).unwrap()
    });
    drive_pages_with_split_atom_to_quiescence(&exact, cx, &source, atom, atom_range, "");
    let exact_target = hold_same_revision_geometry_target(&exact, cx, target_block);
    let mut exact_page_id = 87_000;
    let SplitAtomClipboardAttempt {
        key: exact_begin_key,
        provenance: exact_provenance,
        delivered_pages: exact_delivered_pages,
        released_pages: exact_released_pages,
        delivered_object_pages: exact_delivered_object_pages,
        released_object_pages: exact_released_object_pages,
    } = begin_split_atom_clipboard_to_provenance(
        &exact,
        cx,
        &source,
        selected_end,
        &facts,
        atom,
        atom_range,
        "",
        &mut exact_page_id,
    )
    .unwrap();
    let exact_page = exact_provenance.unwrap();
    assert_eq!(exact_page.key().clipboard(), exact_begin_key);
    assert_exact_clipboard_text_response_releases(
        exact_begin_key,
        &exact_delivered_pages,
        &exact_released_pages,
    );
    assert_exact_clipboard_object_response_releases(
        exact_begin_key,
        &exact_delivered_object_pages,
        &exact_released_object_pages,
    );
    assert_eq!(exact_target.key().binding(), exact_begin_key.binding());
    assert_eq!(exact_target.key().revision(), exact_begin_key.revision());
    assert_eq!(exact_page.items().len(), 1);
    assert!(!exact_released_pages.is_empty());
    assert!(exact_released_pages
        .iter()
        .all(|key| key.purpose() == PagePurpose::Clipboard));
    assert_eq!(
        exact.read_with(cx, |input, _| {
            input.realization_diagnostics().high_water.owned_bytes
        }),
        exact_peak
    );
    let stale_exact_page = exact_page.clone();
    exact.update(cx, |input, cx| {
        input
            .acknowledge_clipboard_provenance_page(exact_page, cx)
            .unwrap();
        let after_acknowledgement = input.clipboard_counts();
        assert!(matches!(
            input.acknowledge_clipboard_provenance_page(stale_exact_page, cx),
            Err(gpui_text_input::RangeTextInputError::Stale)
        ));
        assert_eq!(input.clipboard_counts(), after_acknowledgement);
    });
    cx.update(|window, app| {
        exact.update(app, |input, cx| {
            input.rebind(binding(&source, 2), None, window, cx).unwrap()
        })
    });
    assert_eq!(exact_target.key().purpose(), PagePurpose::GeometryTarget);
    let exact_cleanup = drain_rebound_surface_strict(&exact, cx, &source);
    assert_eq!(
        exact_cleanup
            .iter()
            .filter(|request| matches!(
                request,
                RangeTextInputRequest::CancelPage(key) if *key == exact_target.key()
            ))
            .count(),
        1
    );
    assert!(!exact_cleanup.iter().any(|request| matches!(
        request,
        RangeTextInputRequest::CancelPage(key) if exact_delivered_pages.contains(key)
    )));
    let mut exact_all_released_pages = exact_released_pages.clone();
    exact_all_released_pages.extend(exact_cleanup.iter().filter_map(|request| match request {
        RangeTextInputRequest::ReleasePage(key) if key.purpose() == PagePurpose::Clipboard => {
            Some(*key)
        }
        _ => None,
    }));
    let mut exact_all_released_object_pages = exact_released_object_pages.clone();
    exact_all_released_object_pages.extend(exact_cleanup.iter().filter_map(
        |request| match request {
            RangeTextInputRequest::ReleaseObjectPage(key)
                if key.purpose() == ObjectPurpose::Clipboard =>
            {
                Some(*key)
            }
            _ => None,
        },
    ));
    assert_exact_clipboard_text_response_releases(
        exact_begin_key,
        &exact_delivered_pages,
        &exact_all_released_pages,
    );
    assert_exact_clipboard_object_response_releases(
        exact_begin_key,
        &exact_delivered_object_pages,
        &exact_all_released_object_pages,
    );
    exact.read_with(cx, |input, _| {
        assert_eq!(input.clipboard_counts(), Default::default());
        assert!(input.is_quiescent());
    });

    let (one_under, cx) = cx.add_window_view(|window, cx| {
        RangeTextInput::new(configured(exact_peak - 1), window, cx).unwrap()
    });
    drive_pages_with_split_atom_to_quiescence(&one_under, cx, &source, atom, atom_range, "");
    let prior_surface = range_publication_fingerprint(&one_under, cx).surface;
    let target = hold_same_revision_geometry_target(&one_under, cx, target_block);
    assert_eq!(
        range_publication_fingerprint(&one_under, cx).surface,
        prior_surface
    );
    let mut rejected_page_id = 88_000;
    let SplitAtomClipboardAttempt {
        key: rejected_begin_key,
        provenance: rejected,
        delivered_pages: rejected_delivered_pages,
        released_pages: rejected_released_pages,
        delivered_object_pages: rejected_delivered_object_pages,
        released_object_pages: rejected_released_object_pages,
    } = begin_split_atom_clipboard_to_provenance(
        &one_under,
        cx,
        &source,
        selected_end,
        &facts,
        atom,
        atom_range,
        "",
        &mut rejected_page_id,
    )
    .unwrap();
    assert_exact_clipboard_text_response_releases(
        rejected_begin_key,
        &rejected_delivered_pages,
        &rejected_released_pages,
    );
    assert_exact_clipboard_object_response_releases(
        rejected_begin_key,
        &rejected_delivered_object_pages,
        &rejected_released_object_pages,
    );
    assert_eq!(target.key().binding(), rejected_begin_key.binding());
    assert_eq!(target.key().revision(), rejected_begin_key.revision());
    assert!(
        matches!(
            rejected,
            Err(gpui_text_input::RangeTextInputError::SurfaceCapacity)
        ),
        "unexpected one-under result: {rejected:?}"
    );
    let (before_release, before_retry) = one_under.read_with(cx, |input, _| {
        let counts = input.clipboard_counts();
        let diagnostics = input.realization_diagnostics();
        assert_eq!(counts.pending_object_pages, 0);
        assert_eq!(counts.retained_object_facts, 0);
        assert_eq!(counts.retained_provenance_items, 0);
        assert!(counts.retained_provenance_bytes > 1024 * 1024);
        assert_eq!(counts.staged_bytes, 64);
        assert!(
            diagnostics.high_water.owned_bytes < exact_peak,
            "one-under destination allocation crossed the admitted cap"
        );
        assert_eq!(diagnostics.current.active_geometry_jobs, 1);
        assert_eq!(diagnostics.current.dispatched_page_requests, 1);
        assert_eq!(diagnostics.current.pending_object_requests, 0);
        assert_eq!(diagnostics.current.dispatched_object_requests, 0);
        assert_eq!(diagnostics.current.response_custody_count, 0);
        assert_eq!(diagnostics.current.scheduled_continuations, 1);
        assert_eq!(input.surface().unwrap().binding(), binding(&source, 1));
        (diagnostics.current.owned_bytes, counts)
    });
    let target_settlement = one_under.update(cx, |input, cx| {
        input.fail_page(target.key(), PageFailure::Unavailable, cx)
    });
    assert!(
        target_settlement.is_ok(),
        "geometry target host settlement failed: {target_settlement:?}"
    );
    one_under.read_with(cx, |input, _| {
        let diagnostics = input.realization_diagnostics();
        let counts = input.clipboard_counts();
        assert_eq!(counts.owned_bytes, before_retry.owned_bytes + 832);
        assert_eq!(counts.owned_items, before_retry.owned_items + 1);
        assert_eq!(counts.staged_bytes, before_retry.staged_bytes);
        assert_eq!(counts.pending_text_pages, 0);
        assert_eq!(counts.pending_object_pages, 0);
        assert_eq!(counts.retained_object_facts, 0);
        assert_eq!(counts.retained_provenance_items, 1);
        assert!(counts.retained_provenance_bytes > before_retry.retained_provenance_bytes);
        assert!(diagnostics.current.owned_bytes < before_release);
        assert!(before_release - diagnostics.current.owned_bytes >= 832);
        assert_eq!(diagnostics.current.active_geometry_jobs, 0);
        assert_eq!(diagnostics.current.dispatched_page_requests, 0);
        assert_eq!(diagnostics.current.pending_object_requests, 0);
        assert_eq!(diagnostics.current.dispatched_object_requests, 0);
        assert_eq!(diagnostics.current.response_custody_count, 0);
        assert_eq!(
            range_publication_fingerprint_from(input).surface,
            prior_surface
        );
    });

    let mut recovery_released_pages = Vec::new();
    let mut recovery_released_object_pages = Vec::new();
    let mut recovered_page = None;
    for _ in 0..256 {
        match one_under.update(cx, |input, _| input.take_request()) {
            Some(RangeTextInputRequest::ReleasePage(key)) => recovery_released_pages.push(key),
            Some(RangeTextInputRequest::ClipboardProvenancePage(page)) => {
                recovered_page = Some(page);
            }
            Some(RangeTextInputRequest::ReleaseObjectPage(key)) => {
                recovery_released_object_pages.push(key)
            }
            Some(RangeTextInputRequest::Page(request)) => {
                assert!(
                    !rejected_delivered_pages.contains(&request.key()),
                    "prior clipboard response was redispatched: {request:?}"
                );
                panic!("unexpected request during capacity recovery: {request:?}")
            }
            Some(RangeTextInputRequest::ObjectPage(request)) => {
                panic!("rejected clipboard object response was redispatched: {request:?}")
            }
            Some(other) => panic!("unexpected capacity-return request: {other:?}"),
            None => {
                cx.update(|window, app| window.draw(app).clear());
            }
        }
        if recovered_page.is_some() {
            break;
        }
    }
    let recovered_page = recovered_page.unwrap_or_else(|| {
        panic!(
            "capacity-returned clipboard preparation did not commit: {:?}",
            one_under.read_with(cx, |input, _| input.realization_diagnostics())
        )
    });
    assert_eq!(
        recovery_released_pages
            .iter()
            .filter(|key| **key == target.key())
            .count(),
        0
    );
    assert!(recovery_released_pages
        .iter()
        .all(|key| { *key == target.key() || key.purpose() == PagePurpose::Clipboard }));
    let mut rejected_releases_through_recovery = rejected_released_pages.clone();
    rejected_releases_through_recovery.extend(
        recovery_released_pages
            .iter()
            .copied()
            .filter(|key| key.purpose() == PagePurpose::Clipboard),
    );
    assert_exact_clipboard_text_response_releases(
        rejected_begin_key,
        &rejected_delivered_pages,
        &rejected_releases_through_recovery,
    );
    let mut rejected_object_releases_through_recovery = rejected_released_object_pages.clone();
    rejected_object_releases_through_recovery.extend(
        recovery_released_object_pages
            .iter()
            .copied()
            .filter(|key| key.purpose() == ObjectPurpose::Clipboard),
    );
    assert_exact_clipboard_object_response_releases(
        rejected_begin_key,
        &rejected_delivered_object_pages,
        &rejected_object_releases_through_recovery,
    );
    assert!(rejected_released_pages
        .iter()
        .all(|key| key.purpose() == PagePurpose::Clipboard));
    assert_eq!(recovered_page.items().len(), 1);
    assert_eq!(recovered_page.key().clipboard(), rejected_begin_key);
    assert_eq!(recovered_page.items()[0].object_id(), facts[0].id());
    assert_eq!(
        recovered_page.items()[0].output_range().start(),
        ByteOffset::new(1)
    );
    assert_eq!(
        recovered_page.items()[0].output_range().end(),
        ByteOffset::new(1)
    );
    one_under.read_with(cx, |input, _| {
        let diagnostics = input.realization_diagnostics();
        assert_eq!(diagnostics.current.response_custody_count, 0);
        assert_eq!(diagnostics.current.dispatched_page_requests, 0);
        assert_eq!(diagnostics.current.pending_object_requests, 0);
        assert_eq!(diagnostics.current.dispatched_object_requests, 0);
        assert_eq!(input.clipboard_counts().retained_provenance_items, 1);
        assert!(diagnostics.high_water.owned_bytes <= exact_peak - 1);
        assert_eq!(
            range_publication_fingerprint_from(input).surface,
            prior_surface
        );
    });

    let stale_page = recovered_page.clone();
    let clipboard_key = recovered_page.key().clipboard();
    assert_eq!(clipboard_key, rejected_begin_key);
    one_under.update(cx, |input, cx| {
        input
            .acknowledge_clipboard_provenance_page(recovered_page, cx)
            .unwrap();
        let after_acknowledgement = input.clipboard_counts();
        assert!(matches!(
            input.acknowledge_clipboard_provenance_page(stale_page, cx),
            Err(gpui_text_input::RangeTextInputError::Stale)
        ));
        assert_eq!(input.clipboard_counts(), after_acknowledgement);
    });

    let mut successor_delivered_pages = Vec::new();
    let mut completion_released_pages = Vec::new();
    let mut completion_released_object_pages = Vec::new();
    let write = loop {
        let request = take_request_after_scheduled_frames(
            &one_under,
            cx,
            "clipboard completion after prepared retry",
        );
        match request {
            RangeTextInputRequest::Page(request)
                if request.key().purpose() == PagePurpose::Clipboard =>
            {
                assert_eq!(request.key().binding(), rejected_begin_key.binding());
                assert_eq!(request.key().revision(), rejected_begin_key.revision());
                assert!(!rejected_delivered_pages.contains(&request.key()));
                assert!(!successor_delivered_pages.contains(&request.key()));
                successor_delivered_pages.push(request.key());
                let page = page_for_split_atom(
                    &source,
                    request.key().id().get(),
                    request,
                    atom,
                    atom_range,
                    "",
                );
                cx.update(|window, app| {
                    one_under.update(app, |input, cx| {
                        input.deliver_page(page, window, cx).unwrap()
                    })
                });
            }
            RangeTextInputRequest::ReleasePage(key) => {
                completion_released_pages.push(key);
            }
            RangeTextInputRequest::ReleaseObjectPage(key) => {
                completion_released_object_pages.push(key);
            }
            RangeTextInputRequest::ClipboardWrite(write) => break write,
            RangeTextInputRequest::ClipboardProvenancePage(page) => {
                panic!("prepared provenance step committed twice: {page:?}")
            }
            other => panic!("unexpected clipboard completion request: {other:?}"),
        }
    };
    assert_eq!(write.key(), clipboard_key);
    assert_eq!(write.key(), rejected_begin_key);
    assert_eq!(write.text(), "ij");
    let write_key = write.key();
    one_under.update(cx, |input, cx| {
        assert_eq!(
            input
                .settle_clipboard_write(write_key, ClipboardWriteOutcome::Failed, cx)
                .unwrap(),
            gpui_text_input::ClipboardCompletion::WriteFailed
        );
        assert!(matches!(
            input.settle_clipboard_write(write_key, ClipboardWriteOutcome::Failed, cx),
            Err(gpui_text_input::RangeTextInputError::Stale)
        ));
    });
    let terminal_lifecycle = drain_terminal_lifecycle_strict(&one_under, cx);
    assert!(!terminal_lifecycle.iter().any(|request| matches!(
        request,
        RangeTextInputRequest::CancelPage(_)
            | RangeTextInputRequest::CancelObjectPage(_)
            | RangeTextInputRequest::CancelClipboardProvenancePage(_)
            | RangeTextInputRequest::CancelClipboardWrite(_)
    )));
    let mut all_delivered_pages = rejected_delivered_pages.clone();
    all_delivered_pages.extend(successor_delivered_pages.iter().copied());
    let mut all_released_pages = rejected_released_pages.clone();
    all_released_pages.extend(
        recovery_released_pages
            .iter()
            .copied()
            .filter(|key| key.purpose() == PagePurpose::Clipboard),
    );
    all_released_pages.extend(
        completion_released_pages
            .iter()
            .copied()
            .filter(|key| key.purpose() == PagePurpose::Clipboard),
    );
    all_released_pages.extend(
        terminal_lifecycle
            .iter()
            .filter_map(|request| match request {
                RangeTextInputRequest::ReleasePage(key)
                    if key.purpose() == PagePurpose::Clipboard =>
                {
                    Some(*key)
                }
                _ => None,
            }),
    );
    assert_exact_clipboard_text_response_releases(
        rejected_begin_key,
        &all_delivered_pages,
        &all_released_pages,
    );
    let mut all_released_object_pages = rejected_released_object_pages.clone();
    all_released_object_pages.extend(recovery_released_object_pages.iter().copied());
    all_released_object_pages.extend(completion_released_object_pages.iter().copied());
    all_released_object_pages.extend(terminal_lifecycle.iter().filter_map(
        |request| match request {
            RangeTextInputRequest::ReleaseObjectPage(key) => Some(*key),
            _ => None,
        },
    ));
    assert_exact_clipboard_object_response_releases(
        rejected_begin_key,
        &rejected_delivered_object_pages,
        &all_released_object_pages,
    );
    assert!(completion_released_pages
        .iter()
        .chain(
            terminal_lifecycle
                .iter()
                .filter_map(|request| match request {
                    RangeTextInputRequest::ReleasePage(key) => Some(key),
                    _ => None,
                })
        )
        .all(|key| *key == target.key() || key.purpose() == PagePurpose::Clipboard));
    assert_eq!(
        recovery_released_pages
            .iter()
            .chain(completion_released_pages.iter())
            .chain(
                terminal_lifecycle
                    .iter()
                    .filter_map(|request| match request {
                        RangeTextInputRequest::ReleasePage(key) => Some(key),
                        _ => None,
                    })
            )
            .filter(|key| **key == target.key())
            .count(),
        0
    );
    one_under.read_with(cx, |input, _| {
        let diagnostics = input.realization_diagnostics();
        assert_eq!(input.clipboard_counts(), Default::default());
        assert_eq!(diagnostics.current.clipboard_bytes, 0);
        assert_eq!(diagnostics.current.request_payload_bytes, 0);
        assert_eq!(diagnostics.current.request_payload_items, 0);
        assert_eq!(diagnostics.current.response_custody_count, 0);
        assert_eq!(diagnostics.current.dispatched_page_requests, 0);
        assert_eq!(diagnostics.current.pending_object_requests, 0);
        assert_eq!(diagnostics.current.dispatched_object_requests, 0);
        assert_eq!(diagnostics.current.active_geometry_jobs, 0);
        assert!(diagnostics.high_water.owned_bytes <= exact_peak - 1);
        assert_eq!(
            range_publication_fingerprint_from(input).surface,
            prior_surface
        );
        assert!(input.is_quiescent());
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
    let invalid = SourcePosition::new(ByteOffset::new(1), InlineObjectGap::NoObjects);
    let seed = restoration_seed(source, 1, invalid);
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
    input.read_with(cx, |input, _| assert!(input.surface().is_none()));
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
    for page_id in 900..2400 {
        let Some(request) = input.update(cx, |input, _| input.take_request()) else {
            if input.read_with(cx, |input, _| input.is_quiescent()) {
                break;
            }
            cx.update(|window, app| window.draw(app).clear());
            cx.run_until_parked();
            saw_estimate |= input.read_with(cx, |input, _| input.geometry_estimate().is_some());
            continue;
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
            RangeTextInputRequest::ObjectPage(request) => {
                let page = restoration_object_page(request, &[], page_id);
                cx.update(|window, app| {
                    input.update(app, |input, cx| {
                        input
                            .deliver_object_page_in_window(page, window, cx)
                            .unwrap()
                    })
                });
            }
            RangeTextInputRequest::ReleasePage(_) | RangeTextInputRequest::CancelPage(_) => {}
            RangeTextInputRequest::ReleaseObjectPage(_)
            | RangeTextInputRequest::CancelObjectPage(_) => {}
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
fn disabled_render_omits_input_routes_while_prepaint_advances_realization(
    cx: &mut gpui::TestAppContext,
) {
    cx.update(ensure_text_input_bindings);
    let source = (0..100)
        .map(|line| format!("line-{line:03}\n"))
        .collect::<String>();
    let (input, cx) = cx.add_window_view(|window, cx| {
        let input = RangeTextInput::new(config(&source, 1), window, cx).unwrap();
        input.focus(window);
        input
    });
    assert!(drive_pages(&input, cx, &source).is_empty());
    let before_disabled_render = input.read_with(cx, |input, _| {
        input.realization_diagnostics().frame_generation
    });
    input.update(cx, |input, cx| input.set_enabled(false, cx));

    cx.update(|window, app| window.draw_and_present_for_test(app));

    input.read_with(cx, |input, _| {
        assert!(!input.is_enabled());
        assert!(input.realization_diagnostics().frame_generation > before_disabled_render);
    });
    assert!(drive_pages(&input, cx, &source).is_empty());
    let before = range_publication_fingerprint(&input, cx);
    let events = restoration_events(&input, cx);
    cx.simulate_keystrokes("ctrl-a");
    cx.simulate_event(MouseDownEvent {
        position: point(px(1.), px(1.)),
        modifiers: Modifiers::none(),
        button: MouseButton::Left,
        click_count: 1,
        first_mouse: false,
    });
    cx.simulate_event(ScrollWheelEvent {
        position: point(px(1.), px(1.)),
        delta: ScrollDelta::Pixels(point(px(0.), px(-48.))),
        ..Default::default()
    });

    assert!(input.update(cx, |input, _| input.take_request()).is_none());
    assert_eq!(range_publication_fingerprint(&input, cx), before);
    assert!(events.borrow().is_empty());
}

fn begin_normal_clipboard_to_write(
    input: &gpui::Entity<RangeTextInput>,
    cx: &mut gpui::VisualTestContext,
    source: &str,
    facts: &[InlineObjectFact],
    kind: gpui_text_input::ClipboardKind,
) -> gpui_text_input::ClipboardWriteRequest {
    input.update(cx, |input, cx| input.begin_clipboard(kind, cx).unwrap());
    for _ in 0..256 {
        match take_request_after_scheduled_frames(input, cx, "normal clipboard progress") {
            RangeTextInputRequest::Page(request) => {
                let page = page_for(source, request.key().id().get(), request);
                cx.update(|window, app| {
                    input.update(app, |input, cx| {
                        input.deliver_page(page, window, cx).unwrap()
                    })
                });
            }
            RangeTextInputRequest::ObjectPage(request) => {
                let page = restoration_object_page(request, facts, request.key().id().get());
                cx.update(|window, app| {
                    input.update(app, |input, cx| {
                        input
                            .deliver_object_page_in_window(page, window, cx)
                            .unwrap()
                    })
                });
            }
            RangeTextInputRequest::ReleasePage(_) | RangeTextInputRequest::ReleaseObjectPage(_) => {
            }
            RangeTextInputRequest::ClipboardWrite(write) => return write,
            other => panic!("unexpected normal clipboard request: {other:?}"),
        }
    }
    panic!("normal clipboard did not reach its write")
}

fn normal_clipboard_uses_published_selection_and_preserves_pending_target(
    kind: gpui_text_input::ClipboardKind,
    cx: &mut gpui::TestAppContext,
) {
    cx.update(ensure_text_input_bindings);
    let source = format!("{}\n{}", "0".repeat(31), "1".repeat(88));
    let mut configuration = config(&source, 1);
    configuration.limits.max_realization_work_per_frame = 1;
    let (input, cx) = cx.add_window_view(|window, cx| {
        let input = RangeTextInput::new(configuration, window, cx).unwrap();
        input.focus(window);
        input
    });
    assert!(drive_pages(&input, cx, &source).is_empty());
    cx.simulate_keystrokes("shift-end");
    assert!(drive_pages(&input, cx, &source).is_empty());
    let published = RangeSourceSelection {
        anchor: ordinary_position(0),
        head: ordinary_position(32),
    };
    input.read_with(cx, |input, _| {
        assert_eq!(input.surface().unwrap().selection(), published);
    });
    let resident_pages = input.read_with(cx, |input, _| {
        input.realization_diagnostics().current.resident_pages
    });

    let unpublished = RangeSourceSelection::caret(ordinary_position(96));
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input
                .rebind(binding(&source, 1), Some(unpublished), window, cx)
                .unwrap()
        })
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
    let held_target_demand = geometry.key().demand();
    let page = page_for(&source, 700, geometry);
    let held_target_range = page.range();
    let custody_before = input.read_with(cx, |input, _| input.realization_diagnostics());
    assert_eq!(custody_before.current.response_custody_count, 0);
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.begin_clipboard(kind, cx).unwrap();
            assert_ne!(input.surface().unwrap().selection(), unpublished);
            let ownership = input.realization_diagnostics().current;
            assert_eq!(
                ownership.response_custody_count,
                custody_before.current.response_custody_count
            );
            assert_eq!(
                ownership.response_custody_bytes,
                custody_before.current.response_custody_bytes
            );
            assert_eq!(
                ownership.response_custody_items,
                custody_before.current.response_custody_items
            );
            assert_eq!(ownership.dispatched_page_requests, 1);
            assert_eq!(ownership.pending_page_requests, 1);
            assert_eq!(ownership.resident_pages, resident_pages);
            input.deliver_page(page, window, cx).unwrap();
        })
    });
    input.read_with(cx, |input, _| {
        let ownership = input.realization_diagnostics().current;
        assert_eq!(ownership.dispatched_page_requests, 0);
        assert_eq!(ownership.resident_pages, resident_pages + 1);
        assert_eq!(ownership.response_custody_count, 0);
    });
    let mut target_resident_pages = resident_pages + 1;
    let mut clipboard_page_demands = Vec::new();
    let mut observed_resident_clipboard_custody = false;
    let mut write = None;
    for _ in 0..512 {
        let request =
            take_request_after_scheduled_frames(&input, cx, "normal clipboard and target overlap");
        match request {
            RangeTextInputRequest::Page(request) => {
                let purpose = request.key().purpose();
                assert!(matches!(
                    purpose,
                    PagePurpose::Clipboard | PagePurpose::GeometryTarget
                ));
                if purpose == PagePurpose::Clipboard {
                    assert!(!clipboard_page_demands.contains(&request.key().demand()));
                    clipboard_page_demands.push(request.key().demand());
                }
                input.read_with(cx, |input, _| {
                    let ownership = input.realization_diagnostics().current;
                    assert_eq!(ownership.dispatched_page_requests, 1);
                    assert_eq!(ownership.pending_page_requests, 1);
                });
                let page = page_for(&source, request.key().id().get(), request);
                if purpose == PagePurpose::Clipboard {
                    assert_ne!(page.range(), held_target_range);
                    assert_eq!(page.range().intersection(held_target_range), None);
                }
                cx.update(|window, app| {
                    input.update(app, |input, cx| {
                        input.deliver_page(page, window, cx).unwrap()
                    })
                });
                if purpose == PagePurpose::Clipboard {
                    input.read_with(cx, |input, _| {
                        let ownership = input.realization_diagnostics().current;
                        assert_eq!(ownership.dispatched_page_requests, 0);
                        assert_eq!(ownership.resident_pages, resident_pages);
                        assert_eq!(ownership.response_custody_count, 0);
                    });
                } else {
                    target_resident_pages = input.read_with(cx, |input, _| {
                        input.realization_diagnostics().current.resident_pages
                    });
                    assert!(target_resident_pages > resident_pages);
                }
            }
            RangeTextInputRequest::ObjectPage(request) => {
                let purpose = request.key().purpose();
                assert!(matches!(
                    purpose,
                    ObjectPurpose::Clipboard | ObjectPurpose::GeometryTarget
                ));
                let page = restoration_object_page(request, &[], request.key().id().get());
                cx.update(|window, app| {
                    input.update(app, |input, cx| {
                        input
                            .deliver_object_page_in_window(page, window, cx)
                            .unwrap();
                        if purpose == ObjectPurpose::Clipboard {
                            let ownership = input.realization_diagnostics().current;
                            assert_eq!(ownership.dispatched_page_requests, 0);
                            assert_eq!(ownership.resident_pages, target_resident_pages);
                            assert_eq!(ownership.pending_object_requests, 1);
                            assert_eq!(ownership.dispatched_object_requests, 1);
                            assert_eq!(ownership.active_geometry_jobs, 1);
                            assert_eq!(ownership.response_custody_count, 1);
                            assert_eq!(ownership.response_custody_bytes, 5_184);
                            assert_eq!(ownership.response_custody_items, 12);
                            observed_resident_clipboard_custody = true;
                        }
                    })
                });
            }
            RangeTextInputRequest::ReleasePage(_) | RangeTextInputRequest::ReleaseObjectPage(_) => {
            }
            RangeTextInputRequest::ClipboardWrite(request) => {
                input.read_with(cx, |input, _| {
                    let ownership = input.realization_diagnostics().current;
                    assert_eq!(
                        ownership.dispatched_page_requests + ownership.dispatched_object_requests,
                        1
                    );
                    assert_eq!(
                        ownership.pending_page_requests + ownership.pending_object_requests,
                        1
                    );
                    assert_eq!(ownership.resident_pages, target_resident_pages);
                    assert_eq!(ownership.active_geometry_jobs, 1);
                });
                write = Some(request);
                break;
            }
            other => panic!("unexpected overlap request: {other:?}"),
        }
    }
    assert_eq!(clipboard_page_demands.len(), 0);
    assert!(!clipboard_page_demands.contains(&held_target_demand));
    assert!(observed_resident_clipboard_custody);
    let write = write.expect("resident first page advances clipboard to completion");
    assert_eq!(write.text(), &source[..32]);
    input.update(cx, |input, cx| {
        assert_eq!(
            input
                .settle_clipboard_write(write.key(), ClipboardWriteOutcome::Failed, cx)
                .unwrap(),
            gpui_text_input::ClipboardCompletion::WriteFailed
        );
        assert_eq!(input.clipboard_counts(), Default::default());
    });
    assert!(drive_pages(&input, cx, &source).is_empty());
    input.read_with(cx, |input, _| {
        let ownership = input.realization_diagnostics().current;
        assert_eq!(input.surface().unwrap().selection(), unpublished);
        assert_eq!(input.clipboard_counts(), Default::default());
        assert_eq!(ownership.response_custody_count, 0);
        assert_eq!(
            ownership.response_custody_bytes,
            custody_before.current.response_custody_bytes
        );
        assert_eq!(
            ownership.response_custody_items,
            custody_before.current.response_custody_items
        );
        assert!(input.is_quiescent());
    });
}

#[gpui::test]
fn normal_copy_and_cut_use_published_selection_and_preserve_pending_target(
    cx: &mut gpui::TestAppContext,
) {
    for kind in [
        gpui_text_input::ClipboardKind::Copy,
        gpui_text_input::ClipboardKind::Cut,
    ] {
        normal_clipboard_uses_published_selection_and_preserves_pending_target(kind, cx);
    }
}

#[gpui::test]
fn normal_cut_defers_deletion_until_written_and_cleans_up_failure(cx: &mut gpui::TestAppContext) {
    cx.update(ensure_text_input_bindings);
    let source = "0123456789".repeat(12);
    let (failed, cx) = cx.add_window_view(|window, cx| {
        let input = RangeTextInput::new(config(&source, 1), window, cx).unwrap();
        input.focus(window);
        input
    });
    assert!(drive_pages(&failed, cx, &source).is_empty());
    cx.simulate_keystrokes("ctrl-a");
    assert!(drive_pages(&failed, cx, &source).is_empty());
    let failed_write = begin_normal_clipboard_to_write(
        &failed,
        cx,
        &source,
        &[],
        gpui_text_input::ClipboardKind::Cut,
    );
    assert_eq!(failed_write.text(), source);
    assert!(failed.update(cx, |input, _| input.take_request()).is_none());
    failed.update(cx, |input, cx| {
        assert_eq!(
            input
                .settle_clipboard_write(failed_write.key(), ClipboardWriteOutcome::Failed, cx,)
                .unwrap(),
            gpui_text_input::ClipboardCompletion::WriteFailed
        );
        assert_eq!(input.clipboard_counts(), Default::default());
    });
    assert!(drive_pages(&failed, cx, &source).is_empty());

    let (written, cx) = cx.add_window_view(|window, cx| {
        let input = RangeTextInput::new(config(&source, 1), window, cx).unwrap();
        input.focus(window);
        input
    });
    assert!(drive_pages(&written, cx, &source).is_empty());
    let selected = RangeSourceSelection {
        anchor: ordinary_position(96),
        head: ordinary_position(106),
    };
    cx.update(|window, app| {
        written.update(app, |input, cx| {
            input
                .rebind(binding(&source, 1), Some(selected), window, cx)
                .unwrap()
        })
    });
    assert!(drive_pages(&written, cx, &source).is_empty());
    let written_request = begin_normal_clipboard_to_write(
        &written,
        cx,
        &source,
        &[],
        gpui_text_input::ClipboardKind::Cut,
    );
    assert_eq!(written_request.text(), &source[96..106]);
    assert!(
        written
            .update(cx, |input, _| input.take_request())
            .is_none()
    );
    written.update(cx, |input, cx| {
        assert!(matches!(
            input
                .settle_clipboard_write(written_request.key(), ClipboardWriteOutcome::Written, cx,)
                .unwrap(),
            gpui_text_input::ClipboardCompletion::Delete(_)
        ));
        assert_eq!(input.clipboard_counts(), Default::default());
    });
    assert!(matches!(
        written.update(cx, |input, _| input.take_request()),
        Some(RangeTextInputRequest::MutationBegin(_))
    ));
    cx.update(|window, app| written.update(app, |input, cx| input.dispose(window, cx)));
}

#[gpui::test]
fn normal_cut_post_write_proof_failure_deletes_nothing_and_releases_clipboard(
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
    let write = begin_normal_clipboard_to_write(
        &input,
        cx,
        &source,
        &[],
        gpui_text_input::ClipboardKind::Cut,
    );
    assert_eq!(write.text(), source);
    assert!(input.update(cx, |input, _| input.take_request()).is_none());
    input.update(cx, |input, cx| {
        let result = input.settle_clipboard_write(write.key(), ClipboardWriteOutcome::Written, cx);
        assert!(
            matches!(
                result,
                Err(gpui_text_input::RangeTextInputError::Mutation(_))
            ),
            "unexpected proof-failure settlement: {result:?}"
        );
        assert_eq!(input.clipboard_counts(), Default::default());
        assert!(input.take_request().is_none());
    });
    assert!(drive_pages(&input, cx, &source).is_empty());
}

#[gpui::test]
fn normal_cut_post_write_edit_admission_failure_does_not_queue_deletion(
    cx: &mut gpui::TestAppContext,
) {
    cx.update(ensure_text_input_bindings);
    let source = "0123456789".repeat(12);
    let selected = RangeSourceSelection {
        anchor: ordinary_position(96),
        head: ordinary_position(106),
    };
    let (input, cx) = cx.add_window_view(|window, cx| {
        let input = RangeTextInput::new(config(&source, 1), window, cx).unwrap();
        input.focus(window);
        input
    });
    assert!(drive_pages(&input, cx, &source).is_empty());
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input
                .rebind(binding(&source, 1), Some(selected), window, cx)
                .unwrap()
        })
    });
    assert!(drive_pages(&input, cx, &source).is_empty());
    let write = begin_normal_clipboard_to_write(
        &input,
        cx,
        &source,
        &[],
        gpui_text_input::ClipboardKind::Cut,
    );
    let base_positions = [selected.anchor, selected.head];
    let (text, objects) = admitted_sources(&source, 1, &base_positions);
    input.update(cx, |input, cx| {
        let operation = input.lease_host_operation().unwrap();
        let current = selected.head;
        let begin = MutationBeginRequest::new(
            MutationProposal::new(
                MutationKey::new(
                    binding(&source, 1).binding(),
                    binding(&source, 1).revision(),
                    operation.operation(),
                ),
                MutationKind::Edit,
                MutationPositions::collapsed(current),
                SourceRange::new(current, current).unwrap(),
                0,
            ),
            MutationCursor::new(0),
            MutationCursor::new(0),
        );
        input
            .begin_host_mutation(operation, begin, &base_positions, &text, &objects, cx)
            .unwrap();
        assert!(matches!(
            input.take_request(),
            Some(RangeTextInputRequest::MutationBegin(request)) if request == begin
        ));
    });
    input.update(cx, |input, cx| {
        let result = input.settle_clipboard_write(write.key(), ClipboardWriteOutcome::Written, cx);
        assert!(
            matches!(
                result,
                Err(gpui_text_input::RangeTextInputError::Mutation(_))
            ),
            "unexpected edit-admission settlement: {result:?}"
        );
        assert_eq!(input.clipboard_counts(), Default::default());
        assert!(input.take_request().is_none());
    });
    cx.update(|window, app| input.update(app, |input, cx| input.dispose(window, cx)));
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
fn queued_semantic_cleanup_prevents_clean_release_until_host_dispatch(
    cx: &mut gpui::TestAppContext,
) {
    let source = "semantic cleanup";
    let (input, cx) = cx
        .add_window_view(|window, cx| RangeTextInput::new(config(source, 1), window, cx).unwrap());
    assert!(drive_pages(&input, cx, source).is_empty());
    let current = input.read_with(cx, |input, _| input.surface().unwrap().selection().head);
    let (text, objects) = admitted_sources(source, 1, &[current]);
    let base = binding(source, 1);
    let operation = input.read_with(cx, |input, _| input.lease_host_operation().unwrap());
    let begin = MutationBeginRequest::new(
        MutationProposal::new(
            MutationKey::new(base.binding(), base.revision(), operation.operation()),
            MutationKind::Edit,
            MutationPositions::collapsed(current),
            SourceRange::new(current, current).unwrap(),
            0,
        ),
        MutationCursor::new(0),
        MutationCursor::new(0),
    );
    let key = begin.proposal().key();

    input.update(cx, |input, cx| {
        assert!(input.is_semantically_quiescent());
        input
            .begin_host_mutation(operation, begin, &[current], &text, &objects, cx)
            .unwrap();
        assert!(!input.is_semantically_quiescent());
        assert!(matches!(
            input.take_request(),
            Some(RangeTextInputRequest::MutationBegin(request)) if request.proposal().key() == key
        ));
        assert!(matches!(
            input.cancel_mutation(key, cx),
            Ok(gpui_text_input::MutationCancellation::Cancelled)
        ));
        assert!(!input.is_semantically_quiescent());
        assert!(matches!(
            input.take_request(),
            Some(RangeTextInputRequest::CancelMutation(request)) if request.key() == key
        ));
        assert!(input.is_semantically_quiescent());
    });
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
    input.update(cx, |input, _| {
        admit_ordinary_edit_positions(input, source, 1, &[0])
    });
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
fn presentation_only_generation_preserves_epoch_and_layout_replacement_advances_it(
    cx: &mut gpui::TestAppContext,
) {
    let source = "presentation lifecycle";
    let configuration = config(source, 1);
    let layout = configuration.layout.clone();
    let style = configuration.style.clone();
    let (input, cx) =
        cx.add_window_view(|window, cx| RangeTextInput::new(configuration, window, cx).unwrap());
    assert!(drive_pages(&input, cx, source).is_empty());
    let initial = input.read_with(cx, |input, _| input.surface().unwrap().geometry_key());

    input.update(cx, |input, cx| {
        input
            .set_presentation_generation(PresentationGeneration::new(2), cx)
            .unwrap();
        assert_eq!(input.surface().unwrap().geometry_key(), initial);
    });
    input.read_with(cx, |input, _| {
        assert_eq!(input.realization_diagnostics().current.candidates, 1);
    });
    assert_normal_clipboard_blocked_without_custody(&input, cx);
    assert!(drive_pages(&input, cx, source).is_empty());
    let presentation = input.read_with(cx, |input, _| {
        let key = input.surface().unwrap().geometry_key();
        assert_eq!(key.epoch(), initial.epoch());
        assert_eq!(
            key.presentation_generation(),
            PresentationGeneration::new(2)
        );
        key
    });

    input.update(cx, |input, cx| {
        input.set_layout(layout, style, cx).unwrap();
        assert_eq!(input.surface().unwrap().geometry_key(), presentation);
    });
    input.read_with(cx, |input, _| {
        assert_eq!(input.realization_diagnostics().current.candidates, 1);
    });
    assert_normal_clipboard_blocked_without_custody(&input, cx);
    assert!(drive_pages(&input, cx, source).is_empty());
    input.read_with(cx, |input, _| {
        let key = input.surface().unwrap().geometry_key();
        assert!(key.epoch() > presentation.epoch());
        assert_eq!(
            key.presentation_generation(),
            PresentationGeneration::new(2)
        );
        assert!(input.is_quiescent());
    });
}

#[gpui::test]
fn geometry_object_demand_caps_accept_exact_fit_and_reject_one_under(
    cx: &mut gpui::TestAppContext,
) {
    let source = "object demand caps";
    let exact = config(source, 1);
    let mut count_under = exact.clone();
    count_under.object_residency_limits =
        ObjectResidencyLimits::new(4, 32, 128 * 1024, 64 * 1024, 4, 31, 128 * 1024).unwrap();
    let mut bytes_under = exact.clone();
    bytes_under.object_residency_limits =
        ObjectResidencyLimits::new(4, 32, 128 * 1024, 64 * 1024, 4, 32, 128 * 1024 - 1).unwrap();
    let failures = Rc::new(RefCell::new(Vec::new()));
    let captured = failures.clone();
    let (input, cx) = cx.add_window_view(move |window, cx| {
        for invalid in [count_under, bytes_under] {
            match RangeTextInput::new(invalid, window, cx) {
                Err(error) => captured.borrow_mut().push(error),
                Ok(_) => panic!("one-under object demand cap must be rejected"),
            }
        }
        RangeTextInput::new(exact, window, cx).unwrap()
    });
    assert_eq!(failures.borrow().len(), 2);
    assert!(
        failures
            .borrow()
            .iter()
            .all(|error| matches!(error, gpui_text_input::RangeTextInputError::InvalidLimits))
    );
    assert!(drive_pages(&input, cx, source).is_empty());
    input.read_with(cx, |input, _| {
        assert!(input.surface().is_some());
        assert!(input.is_quiescent());
    });
}

#[gpui::test]
fn malformed_exact_geometry_index_text_page_terminates_and_can_restart(
    cx: &mut gpui::TestAppContext,
) {
    let source = "malformed geometry index text";
    let configuration = config(source, 1);
    let layout = configuration.layout.clone();
    let style = configuration.style.clone();
    let (input, cx) =
        cx.add_window_view(|window, cx| RangeTextInput::new(configuration, window, cx).unwrap());
    let target = loop {
        match take_request_after_scheduled_frames(&input, cx, "initial geometry-target text") {
            RangeTextInputRequest::Page(request) => break request,
            other => panic!("unexpected initial geometry-target text request: {other:?}"),
        }
    };
    assert_eq!(target.key().purpose(), PagePurpose::GeometryTarget);
    let page = page_for(source, 94_000, target);
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.deliver_page(page, window, cx).unwrap()
        })
    });
    let (request, prior) = loop {
        match take_request_after_scheduled_frames(&input, cx, "geometry-index text after target") {
            RangeTextInputRequest::ObjectPage(request)
                if request.key().purpose() == ObjectPurpose::GeometryTarget =>
            {
                let page = restoration_object_page(request, &[], 94_001);
                cx.update(|window, app| {
                    input.update(app, |input, cx| {
                        input
                            .deliver_object_page_in_window(page, window, cx)
                            .unwrap()
                    })
                });
            }
            RangeTextInputRequest::Page(request)
                if request.key().purpose() == PagePurpose::GeometryIndex =>
            {
                break (
                    request,
                    input.read_with(cx, |input, _| input.surface().unwrap().geometry_key()),
                );
            }
            RangeTextInputRequest::ReleasePage(_) | RangeTextInputRequest::ReleaseObjectPage(_) => {
            }
            other => panic!("unexpected geometry-index text request after target: {other:?}"),
        }
    };
    assert_eq!(request.key().purpose(), PagePurpose::GeometryIndex);
    let malformed = malformed_geometry_text_page(94_000, request, source.len());
    let late = malformed.clone();
    let error = cx.update(|window, app| {
        input.update(app, |input, cx| input.deliver_page(malformed, window, cx))
    });
    assert!(
        matches!(
            error,
            Err(gpui_text_input::RangeTextInputError::Geometry(
                gpui_text_input::ExactGeometryError::SourceContract
            ))
        ),
        "{error:?}"
    );
    let mut releases = 0;
    while let Some(released) = input.update(cx, |input, _| input.take_request()) {
        match released {
            RangeTextInputRequest::ReleasePage(key) if key == request.key() => releases += 1,
            other => panic!("unexpected index-text settlement request: {other:?}"),
        }
    }
    assert_eq!(releases, 1);
    input.read_with(cx, |input, _| {
        assert_eq!(input.surface().unwrap().geometry_key(), prior);
        assert!(input.is_quiescent());
    });

    assert!(matches!(
        cx.update(|window, app| {
            input.update(app, |input, cx| input.deliver_page(late, window, cx))
        }),
        Err(gpui_text_input::RangeTextInputError::PageResponseRejected(
            _
        ))
    ));
    assert!(input.update(cx, |input, _| input.take_request()).is_none());
    input.update(cx, |input, cx| input.set_layout(layout, style, cx).unwrap());
    cx.update(|window, app| window.draw(app).clear());
    assert!(drive_pages(&input, cx, source).is_empty());
    input.read_with(cx, |input, _| {
        assert!(input.surface().is_some());
        assert!(input.is_quiescent());
    });
}

#[gpui::test]
fn malformed_exact_geometry_target_text_page_retains_prior_surface_and_can_restart(
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
    let prior = input.read_with(cx, |input, _| input.surface().unwrap().geometry_key());
    input.update(cx, |input, cx| {
        input.request_absolute_scroll(px(0.), cx).unwrap()
    });

    let target = loop {
        match input.update(cx, |input, _| input.take_request()).unwrap() {
            RangeTextInputRequest::Page(request)
                if request.key().purpose() == PagePurpose::GeometryTarget =>
            {
                break request;
            }
            RangeTextInputRequest::Page(request) => {
                let page = page_for(&source, 94_100, request);
                cx.update(|window, app| {
                    input.update(app, |input, cx| {
                        input.deliver_page(page, window, cx).unwrap()
                    })
                });
            }
            RangeTextInputRequest::ObjectPage(request) => {
                let page = restoration_object_page(request, &[], 94_101);
                cx.update(|window, app| {
                    input.update(app, |input, cx| {
                        input
                            .deliver_object_page_in_window(page, window, cx)
                            .unwrap()
                    })
                });
            }
            RangeTextInputRequest::ReleasePage(_) | RangeTextInputRequest::ReleaseObjectPage(_) => {
            }
            other => panic!("unexpected target-text request: {other:?}"),
        }
    };
    let malformed = malformed_geometry_text_page(94_102, target, source.len());
    let late = malformed.clone();
    let error = cx.update(|window, app| {
        input.update(app, |input, cx| input.deliver_page(malformed, window, cx))
    });
    assert!(matches!(
        error,
        Err(gpui_text_input::RangeTextInputError::Geometry(
            gpui_text_input::ExactGeometryError::SourceContract
        ))
    ));
    input.read_with(cx, |input, _| {
        assert_eq!(input.surface().unwrap().geometry_key(), prior);
    });
    let mut releases = 0;
    while let Some(released) = input.update(cx, |input, _| input.take_request()) {
        match released {
            RangeTextInputRequest::ReleasePage(key) if key == target.key() => releases += 1,
            RangeTextInputRequest::ReleasePage(_) | RangeTextInputRequest::ReleaseObjectPage(_) => {
            }
            other => panic!("unexpected target-text settlement request: {other:?}"),
        }
    }
    assert_eq!(releases, 1);
    input.read_with(cx, |input, _| assert!(input.is_quiescent()));

    assert!(matches!(
        cx.update(|window, app| {
            input.update(app, |input, cx| input.deliver_page(late, window, cx))
        }),
        Err(gpui_text_input::RangeTextInputError::PageResponseRejected(
            _
        ))
    ));
    assert!(input.update(cx, |input, _| input.take_request()).is_none());
    input.update(cx, |input, cx| {
        input.request_absolute_scroll(px(0.), cx).unwrap()
    });
    let RangeTextInputRequest::Page(restarted) =
        input.update(cx, |input, _| input.take_request()).unwrap()
    else {
        panic!("fresh target-text request")
    };
    assert_eq!(restarted.key().purpose(), PagePurpose::GeometryTarget);
    let page = page_for(&source, 94_103, restarted);
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.deliver_page(page, window, cx).unwrap()
        })
    });
    cx.update(|window, app| window.draw(app).clear());
    assert!(drive_pages(&input, cx, &source).is_empty());
    input.read_with(cx, |input, _| {
        assert!(input.surface().is_some());
        assert!(input.is_quiescent());
    });
}

#[gpui::test]
fn malformed_geometry_index_text_residency_conflict_terminates_and_can_restart(
    cx: &mut gpui::TestAppContext,
) {
    let source = "x".repeat(120);
    let configuration = config(&source, 1);
    let layout = configuration.layout.clone();
    let style = configuration.style.clone();
    let (input, cx) =
        cx.add_window_view(|window, cx| RangeTextInput::new(configuration, window, cx).unwrap());
    let resident_atom = AtomId::new(950);
    assert!(drive_pages(&input, cx, &source).is_empty());
    input.update(cx, |input, cx| {
        input.set_layout(layout.clone(), style.clone(), cx).unwrap()
    });
    let (request, prior) = loop {
        match take_request_after_scheduled_frames(&input, cx, "geometry-index text conflict") {
            RangeTextInputRequest::Page(request)
                if request.key().purpose() == PagePurpose::GeometryTarget =>
            {
                let page = page_for(&source, 98_000, request);
                cx.update(|window, app| {
                    input.update(app, |input, cx| {
                        input.deliver_page(page, window, cx).unwrap()
                    })
                });
            }
            RangeTextInputRequest::ObjectPage(request)
                if request.key().purpose() == ObjectPurpose::GeometryTarget =>
            {
                let page = restoration_object_page(request, &[], 98_001);
                cx.update(|window, app| {
                    input.update(app, |input, cx| {
                        input
                            .deliver_object_page_in_window(page, window, cx)
                            .unwrap()
                    })
                });
            }
            RangeTextInputRequest::Page(request)
                if request.key().purpose() == PagePurpose::GeometryIndex =>
            {
                break (
                    request,
                    input.read_with(cx, |input, _| input.surface().unwrap().geometry_key()),
                );
            }
            RangeTextInputRequest::ReleasePage(_) | RangeTextInputRequest::ReleaseObjectPage(_) => {
            }
            other => panic!("unexpected index text-conflict request: {other:?}"),
        }
    };
    assert_eq!(request.key().purpose(), PagePurpose::GeometryIndex);
    let first_page = page_for_with_local_atom(&source, 98_000, request, resident_atom, "resident");
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.deliver_page(first_page, window, cx).unwrap()
        })
    });
    let object_request = loop {
        match input.update(cx, |input, _| input.take_request()).unwrap() {
            RangeTextInputRequest::ObjectPage(request) => break request,
            RangeTextInputRequest::ReleasePage(_) | RangeTextInputRequest::ReleaseObjectPage(_) => {
            }
            other => panic!("unexpected index text-conflict object request: {other:?}"),
        }
    };
    assert_eq!(object_request.key().purpose(), ObjectPurpose::GeometryIndex);
    let object_page = restoration_object_page(object_request, &[], 98_001);
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input
                .deliver_object_page_in_window(object_page, window, cx)
                .unwrap()
        })
    });
    let conflict_request = loop {
        match input.update(cx, |input, _| input.take_request()).unwrap() {
            RangeTextInputRequest::Page(request) => break request,
            RangeTextInputRequest::ReleasePage(_) | RangeTextInputRequest::ReleaseObjectPage(_) => {
            }
            other => panic!("unexpected index text-conflict successor request: {other:?}"),
        }
    };
    assert_eq!(conflict_request.key().purpose(), PagePurpose::GeometryIndex);
    let malformed = page_for_with_local_atom(
        &source,
        98_002,
        conflict_request,
        resident_atom,
        "conflicting",
    );
    let late = malformed.clone();
    let error = cx.update(|window, app| {
        input.update(app, |input, cx| input.deliver_page(malformed, window, cx))
    });
    assert!(matches!(
        error,
        Err(gpui_text_input::RangeTextInputError::Geometry(
            gpui_text_input::ExactGeometryError::SourceContract
        ))
    ));
    input.read_with(cx, |input, _| {
        assert_eq!(input.surface().unwrap().geometry_key(), prior);
    });
    let mut releases = 0;
    while let Some(released) = input.update(cx, |input, _| input.take_request()) {
        match released {
            RangeTextInputRequest::ReleasePage(key) if key == conflict_request.key() => {
                releases += 1
            }
            RangeTextInputRequest::ReleasePage(_) | RangeTextInputRequest::ReleaseObjectPage(_) => {
            }
            other => panic!("unexpected index text-conflict settlement request: {other:?}"),
        }
    }
    assert_eq!(releases, 1);
    input.read_with(cx, |input, _| assert!(input.is_quiescent()));
    assert!(matches!(
        cx.update(|window, app| {
            input.update(app, |input, cx| input.deliver_page(late, window, cx))
        }),
        Err(gpui_text_input::RangeTextInputError::PageResponseRejected(
            _
        ))
    ));
    assert!(input.update(cx, |input, _| input.take_request()).is_none());
    input.update(cx, |input, cx| input.set_layout(layout, style, cx).unwrap());
    assert!(drive_pages(&input, cx, &source).is_empty());
    input.read_with(cx, |input, _| {
        assert!(input.surface().is_some());
        assert!(input.is_quiescent());
    });
}

#[gpui::test]
fn malformed_geometry_target_text_residency_conflict_retains_surface_and_restarts(
    cx: &mut gpui::TestAppContext,
) {
    cx.update(ensure_text_input_bindings);
    let source = "x".repeat(120);
    let (input, cx) = cx.add_window_view(|window, cx| {
        let input = RangeTextInput::new(config(&source, 1), window, cx).unwrap();
        input.focus(window);
        input
    });
    assert!(drive_pages(&input, cx, &source).is_empty());
    cx.simulate_keystrokes("ctrl-a");
    assert!(drive_pages(&input, cx, &source).is_empty());
    let prior = input.read_with(cx, |input, _| input.surface().unwrap().geometry_key());
    input.update(cx, |input, cx| {
        input.request_absolute_scroll(px(0.), cx).unwrap()
    });
    let first_request = loop {
        match input.update(cx, |input, _| input.take_request()).unwrap() {
            RangeTextInputRequest::Page(request) => break request,
            RangeTextInputRequest::ReleasePage(_) | RangeTextInputRequest::ReleaseObjectPage(_) => {
            }
            other => panic!("unexpected target text-conflict request: {other:?}"),
        }
    };
    assert_eq!(first_request.key().purpose(), PagePurpose::GeometryTarget);
    let resident_atom = AtomId::new(952);
    let first_page =
        page_for_with_local_atom(&source, 98_100, first_request, resident_atom, "resident");
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.deliver_page(first_page, window, cx).unwrap()
        })
    });
    let object_request = loop {
        match input.update(cx, |input, _| input.take_request()).unwrap() {
            RangeTextInputRequest::ObjectPage(request) => break request,
            RangeTextInputRequest::ReleasePage(_) | RangeTextInputRequest::ReleaseObjectPage(_) => {
            }
            other => panic!("unexpected target text-conflict object request: {other:?}"),
        }
    };
    assert_eq!(
        object_request.key().purpose(),
        ObjectPurpose::GeometryTarget
    );
    let object_page = restoration_object_page(object_request, &[], 98_101);
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input
                .deliver_object_page_in_window(object_page, window, cx)
                .unwrap()
        })
    });
    let conflict_request = loop {
        match input.update(cx, |input, _| input.take_request()).unwrap() {
            RangeTextInputRequest::Page(request) => break request,
            RangeTextInputRequest::ReleasePage(_) | RangeTextInputRequest::ReleaseObjectPage(_) => {
            }
            other => panic!("unexpected target text-conflict successor request: {other:?}"),
        }
    };
    assert_eq!(
        conflict_request.key().purpose(),
        PagePurpose::GeometryTarget
    );
    let malformed = page_for_with_local_atom(
        &source,
        98_102,
        conflict_request,
        resident_atom,
        "conflicting",
    );
    let late = malformed.clone();
    let error = cx.update(|window, app| {
        input.update(app, |input, cx| input.deliver_page(malformed, window, cx))
    });
    assert!(matches!(
        error,
        Err(gpui_text_input::RangeTextInputError::Geometry(
            gpui_text_input::ExactGeometryError::SourceContract
        ))
    ));
    input.read_with(cx, |input, _| {
        assert_eq!(input.surface().unwrap().geometry_key(), prior);
    });
    let mut releases = 0;
    while let Some(released) = input.update(cx, |input, _| input.take_request()) {
        match released {
            RangeTextInputRequest::ReleasePage(key) if key == conflict_request.key() => {
                releases += 1
            }
            RangeTextInputRequest::ReleasePage(_) | RangeTextInputRequest::ReleaseObjectPage(_) => {
            }
            other => panic!("unexpected target text-conflict settlement request: {other:?}"),
        }
    }
    assert_eq!(releases, 1);
    input.read_with(cx, |input, _| assert!(input.is_quiescent()));
    assert!(matches!(
        cx.update(|window, app| {
            input.update(app, |input, cx| input.deliver_page(late, window, cx))
        }),
        Err(gpui_text_input::RangeTextInputError::PageResponseRejected(
            _
        ))
    ));
    assert!(input.update(cx, |input, _| input.take_request()).is_none());
    input.update(cx, |input, cx| {
        input.request_absolute_scroll(px(0.), cx).unwrap()
    });
    assert!(drive_pages(&input, cx, &source).is_empty());
    input.read_with(cx, |input, _| {
        assert!(input.surface().is_some());
        assert!(input.is_quiescent());
    });
}

#[gpui::test]
fn malformed_exact_geometry_index_object_page_terminates_and_can_restart(
    cx: &mut gpui::TestAppContext,
) {
    let source = "é";
    let configuration = config(source, 1);
    let layout = configuration.layout.clone();
    let style = configuration.style.clone();
    let (input, cx) =
        cx.add_window_view(|window, cx| RangeTextInput::new(configuration, window, cx).unwrap());

    let request = loop {
        match take_request_after_scheduled_frames(&input, cx, "geometry-index object") {
            RangeTextInputRequest::Page(request)
                if request.key().purpose() == PagePurpose::GeometryTarget =>
            {
                let page = page_for(source, 95_000, request);
                cx.update(|window, app| {
                    input.update(app, |input, cx| {
                        input.deliver_page(page, window, cx).unwrap()
                    })
                });
            }
            RangeTextInputRequest::ObjectPage(request)
                if request.key().purpose() == ObjectPurpose::GeometryTarget =>
            {
                let page = restoration_object_page(request, &[], 95_001);
                cx.update(|window, app| {
                    input.update(app, |input, cx| {
                        input
                            .deliver_object_page_in_window(page, window, cx)
                            .unwrap()
                    })
                });
            }
            RangeTextInputRequest::Page(request)
                if request.key().purpose() == PagePurpose::GeometryIndex =>
            {
                let page = page_for(source, 95_002, request);
                cx.update(|window, app| {
                    input.update(app, |input, cx| {
                        input.deliver_page(page, window, cx).unwrap()
                    })
                });
            }
            RangeTextInputRequest::ObjectPage(request)
                if request.key().purpose() == ObjectPurpose::GeometryIndex =>
            {
                break request;
            }
            RangeTextInputRequest::ReleasePage(_) | RangeTextInputRequest::ReleaseObjectPage(_) => {
            }
            other => panic!("unexpected index request: {other:?}"),
        }
    };
    assert_eq!(request.key().purpose(), ObjectPurpose::GeometryIndex);
    let malformed = restoration_object_page(request, &[object_fact(901, 1, 10)], 95_001);
    let late = malformed.clone();
    let error = cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.deliver_object_page_in_window(malformed, window, cx)
        })
    });
    assert!(matches!(
        error,
        Err(gpui_text_input::RangeTextInputError::Geometry(
            gpui_text_input::ExactGeometryError::SourceContract
        ))
    ));
    let mut releases = 0;
    while let Some(released) = input.update(cx, |input, _| input.take_request()) {
        match released {
            RangeTextInputRequest::ReleaseObjectPage(key) if key == request.key() => releases += 1,
            RangeTextInputRequest::ReleasePage(_) => {}
            other => panic!("unexpected settlement request: {other:?}"),
        }
    }
    assert_eq!(releases, 1);
    input.read_with(cx, |input, _| assert!(input.is_quiescent()));

    assert!(matches!(
        input.update(cx, |input, cx| input.deliver_object_page(late, cx)),
        Err(gpui_text_input::RangeTextInputError::ObjectResponseRejected(_))
    ));
    assert!(input.update(cx, |input, _| input.take_request()).is_none());
    input.update(cx, |input, cx| input.set_layout(layout, style, cx).unwrap());
    cx.update(|window, app| window.draw(app).clear());
    assert!(drive_pages(&input, cx, source).is_empty());
    input.read_with(cx, |input, _| {
        assert!(input.surface().is_some());
        assert!(input.is_quiescent());
    });
}

#[gpui::test]
fn malformed_exact_geometry_target_object_page_retains_prior_surface_and_can_restart(
    cx: &mut gpui::TestAppContext,
) {
    let source = "é";
    let configuration = config(source, 1);
    let layout = configuration.layout.clone();
    let style = configuration.style.clone();
    let (input, cx) =
        cx.add_window_view(|window, cx| RangeTextInput::new(configuration, window, cx).unwrap());
    assert!(drive_pages(&input, cx, source).is_empty());
    let prior = input.read_with(cx, |input, _| input.surface().unwrap().geometry_key());
    input.update(cx, |input, cx| {
        input.set_layout(layout.clone(), style.clone(), cx).unwrap()
    });

    let target = loop {
        match input.update(cx, |input, _| input.take_request()).unwrap() {
            RangeTextInputRequest::Page(request) => {
                let page = page_for(source, 96_000, request);
                cx.update(|window, app| {
                    input.update(app, |input, cx| {
                        input.deliver_page(page, window, cx).unwrap()
                    })
                });
            }
            RangeTextInputRequest::ObjectPage(request)
                if request.key().purpose() == ObjectPurpose::GeometryIndex =>
            {
                let page = restoration_object_page(request, &[], 96_001);
                cx.update(|window, app| {
                    input.update(app, |input, cx| {
                        input
                            .deliver_object_page_in_window(page, window, cx)
                            .unwrap()
                    })
                });
            }
            RangeTextInputRequest::ObjectPage(request) => break request,
            RangeTextInputRequest::ReleasePage(_) | RangeTextInputRequest::ReleaseObjectPage(_) => {
            }
            other => panic!("unexpected target request: {other:?}"),
        }
    };
    assert_eq!(target.key().purpose(), ObjectPurpose::GeometryTarget);
    let malformed = restoration_object_page(target, &[object_fact(902, 1, 10)], 96_002);
    let error = cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.deliver_object_page_in_window(malformed, window, cx)
        })
    });
    assert!(matches!(
        error,
        Err(gpui_text_input::RangeTextInputError::Geometry(
            gpui_text_input::ExactGeometryError::SourceContract
        ))
    ));
    input.read_with(cx, |input, _| {
        assert_eq!(input.surface().unwrap().geometry_key(), prior);
    });
    let mut releases = 0;
    while let Some(released) = input.update(cx, |input, _| input.take_request()) {
        match released {
            RangeTextInputRequest::ReleaseObjectPage(key) if key == target.key() => releases += 1,
            RangeTextInputRequest::ReleasePage(_) => {}
            other => panic!("unexpected target settlement request: {other:?}"),
        }
    }
    assert_eq!(releases, 1);
    input.read_with(cx, |input, _| assert!(input.is_quiescent()));

    input.update(cx, |input, cx| input.set_layout(layout, style, cx).unwrap());
    cx.update(|window, app| window.draw(app).clear());
    assert!(drive_pages(&input, cx, source).is_empty());
    input.read_with(cx, |input, _| {
        assert_ne!(input.surface().unwrap().geometry_key(), prior);
        assert!(input.is_quiescent());
    });
}

fn marker_object_configuration(source: &str, work_per_frame: usize) -> RangeTextInputConfig {
    let first = object_neighbor(93_001, 10);
    let mut configuration = config(source, 1);
    configuration.layout.start_position =
        SourcePosition::new(ByteOffset::new(0), InlineObjectGap::before(first)).into();
    configuration.layout.limits.segment_bytes = 4096;
    configuration.layout.limits.glyphs = 4096;
    configuration.layout.limits.maps = 4097;
    configuration.layout.limits.retained_items = 32_768;
    configuration.layout.limits.retained_bytes = 2 * 1024 * 1024;
    configuration.geometry_limits =
        ExactGeometryLimits::new(49_152, 16, 4 * 1024 * 1024, 65_536).unwrap();
    configuration.residency_limits = ResidencyLimits::new(6, 384 * 1024, 6, 384 * 1024).unwrap();
    configuration.object_residency_limits =
        ObjectResidencyLimits::new(6, 48, 65_536, 65_536, 6, 48, 65_536).unwrap();
    configuration.limits = RangeTextInputLimits::new(
        8 * 1024 * 1024,
        131_072,
        work_per_frame,
        px(80.),
        49_152,
        49_152,
        px(16.),
    )
    .unwrap();
    configuration
}

#[gpui::test]
fn selection_retarget_settles_superseded_object_response_and_nonterminal_index_accounting(
    cx: &mut gpui::TestAppContext,
) {
    let source = "AB";
    let facts = [
        object_fact(93_001, 0, 10),
        object_fact(93_002, 0, 20),
        object_fact(93_003, 1, 10),
        object_fact(93_004, 1, 20),
    ];
    let (input, cx) = cx.add_window_view(|window, cx| {
        RangeTextInput::new(marker_object_configuration(source, 32), window, cx).unwrap()
    });
    for _ in 0..256 {
        if input.read_with(cx, |input, _| input.is_surface_current_and_interactive()) {
            break;
        }
        match input.update(cx, |input, _| input.take_request()) {
            Some(RangeTextInputRequest::Page(request)) => {
                let page = page_for(source, request.key().id().get(), request);
                cx.update(|window, app| {
                    input.update(app, |input, cx| {
                        input.deliver_page(page, window, cx).unwrap()
                    })
                });
            }
            Some(RangeTextInputRequest::ObjectPage(request)) => {
                let page = restoration_object_page(request, &facts, request.key().id().get());
                cx.update(|window, app| {
                    input.update(app, |input, cx| {
                        input
                            .deliver_object_page_in_window(page, window, cx)
                            .unwrap()
                    })
                });
            }
            Some(RangeTextInputRequest::ReleasePage(_))
            | Some(RangeTextInputRequest::CancelPage(_))
            | Some(RangeTextInputRequest::ReleaseObjectPage(_))
            | Some(RangeTextInputRequest::CancelObjectPage(_)) => {}
            Some(other) => panic!("unexpected initial marker request: {other:?}"),
            None => {
                cx.update(|window, app| window.draw(app).clear());
                cx.run_until_parked();
            }
        }
    }
    input.read_with(cx, |input, _| {
        assert!(input.is_surface_current_and_interactive());
        assert!(!input.is_quiescent());
    });
    let mut queued_index_object = false;
    for _ in 0..256 {
        match input.update(cx, |input, _| input.take_request()) {
            Some(RangeTextInputRequest::Page(request)) => {
                let page = page_for(source, request.key().id().get(), request);
                cx.update(|window, app| {
                    input.update(app, |input, cx| {
                        input.deliver_page(page, window, cx).unwrap()
                    })
                });
                queued_index_object = input.read_with(cx, |input, _| {
                    let current = input.realization_diagnostics().current;
                    current.pending_geometry_objects == 1 && current.queued_requests > 0
                });
            }
            Some(RangeTextInputRequest::ObjectPage(request)) => {
                let page = restoration_object_page(request, &facts, request.key().id().get());
                cx.update(|window, app| {
                    input.update(app, |input, cx| {
                        input
                            .deliver_object_page_in_window(page, window, cx)
                            .unwrap()
                    })
                });
            }
            Some(RangeTextInputRequest::ReleasePage(_))
            | Some(RangeTextInputRequest::CancelPage(_))
            | Some(RangeTextInputRequest::ReleaseObjectPage(_))
            | Some(RangeTextInputRequest::CancelObjectPage(_)) => {}
            Some(other) => panic!("unexpected retarget request: {other:?}"),
            None => {
                cx.update(|window, app| window.draw(app).clear());
                cx.run_until_parked();
            }
        }
        if queued_index_object {
            break;
        }
    }
    assert!(queued_index_object);
    let selection = RangeSourceSelection {
        anchor: SourcePosition::new(
            ByteOffset::new(0),
            InlineObjectGap::before(object_neighbor(93_001, 10)),
        ),
        head: ordinary_position(source.len() as u64),
    };
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input
                .rebind(binding(source, 1), Some(selection), window, cx)
                .unwrap()
        })
    });
    drive_pages_with_objects(&input, cx, source, &facts);
    input.read_with(cx, |input, _| {
        let diagnostics = input.realization_diagnostics();
        assert_eq!(diagnostics.response_rejection_count, 0);
        assert!(diagnostics.superseded_geometry_object_responses_settled > 0);
        assert_eq!(diagnostics.current.response_custody_count, 0);
        assert!(input.is_surface_current_and_interactive());
        assert!(input.is_quiescent());
    });
}

fn malformed_geometry_object_residency_conflict(cx: &mut gpui::TestAppContext, target: bool) {
    cx.update(ensure_text_input_bindings);
    let source = "x".repeat(120);
    let configuration = config(&source, 1);
    let layout = configuration.layout.clone();
    let style = configuration.style.clone();
    let (input, cx) = cx.add_window_view(|window, cx| {
        let input = RangeTextInput::new(configuration, window, cx).unwrap();
        input.focus(window);
        input
    });
    assert!(drive_pages(&input, cx, &source).is_empty());
    if target {
        cx.simulate_keystrokes("ctrl-a");
        assert!(drive_pages(&input, cx, &source).is_empty());
        input.update(cx, |input, cx| {
            input.request_absolute_scroll(px(0.), cx).unwrap()
        });
    } else {
        input.update(cx, |input, cx| {
            input.set_layout(layout.clone(), style.clone(), cx).unwrap()
        });
    }
    let mut prior = input.read_with(cx, |input, _| input.surface().unwrap().geometry_key());
    let text_purpose = if target {
        PagePurpose::GeometryTarget
    } else {
        PagePurpose::GeometryIndex
    };
    let object_purpose = if target {
        ObjectPurpose::GeometryTarget
    } else {
        ObjectPurpose::GeometryIndex
    };
    let first_text = loop {
        let request = if target {
            input.update(cx, |input, _| input.take_request()).unwrap()
        } else {
            take_request_after_scheduled_frames(&input, cx, "first object-conflict text")
        };
        match request {
            RangeTextInputRequest::Page(request)
                if !target && request.key().purpose() == PagePurpose::GeometryTarget =>
            {
                let page = page_for(&source, 98_200, request);
                cx.update(|window, app| {
                    input.update(app, |input, cx| {
                        input.deliver_page(page, window, cx).unwrap()
                    })
                });
            }
            RangeTextInputRequest::ObjectPage(request)
                if !target && request.key().purpose() == ObjectPurpose::GeometryTarget =>
            {
                let page = restoration_object_page(request, &[], 98_201);
                cx.update(|window, app| {
                    input.update(app, |input, cx| {
                        input
                            .deliver_object_page_in_window(page, window, cx)
                            .unwrap()
                    })
                });
            }
            RangeTextInputRequest::Page(request) => {
                if !target {
                    prior = input.read_with(cx, |input, _| input.surface().unwrap().geometry_key());
                }
                break request;
            }
            RangeTextInputRequest::ReleasePage(_) | RangeTextInputRequest::ReleaseObjectPage(_) => {
            }
            other => panic!("unexpected first object-conflict text request: {other:?}"),
        }
    };
    assert_eq!(first_text.key().purpose(), text_purpose);
    let page = page_for(&source, 98_200, first_text);
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.deliver_page(page, window, cx).unwrap()
        })
    });
    let first_object = loop {
        match input.update(cx, |input, _| input.take_request()).unwrap() {
            RangeTextInputRequest::ObjectPage(request) => break request,
            RangeTextInputRequest::ReleasePage(_) | RangeTextInputRequest::ReleaseObjectPage(_) => {
            }
            other => panic!("unexpected first object-conflict request: {other:?}"),
        }
    };
    assert_eq!(first_object.key().purpose(), object_purpose);
    let page = restoration_object_page(first_object, &[], 98_201);
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input
                .deliver_object_page_in_window(page, window, cx)
                .unwrap()
        })
    });
    let second_text = loop {
        match input.update(cx, |input, _| input.take_request()).unwrap() {
            RangeTextInputRequest::Page(request) => break request,
            RangeTextInputRequest::ReleasePage(_) | RangeTextInputRequest::ReleaseObjectPage(_) => {
            }
            other => panic!("unexpected second object-conflict text request: {other:?}"),
        }
    };
    assert_eq!(second_text.key().purpose(), text_purpose);
    let page = page_for(&source, 98_202, second_text);
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.deliver_page(page, window, cx).unwrap()
        })
    });
    let conflict_request = loop {
        match input.update(cx, |input, _| input.take_request()).unwrap() {
            RangeTextInputRequest::ObjectPage(request) => break request,
            RangeTextInputRequest::ReleasePage(_) | RangeTextInputRequest::ReleaseObjectPage(_) => {
            }
            other => panic!("unexpected conflicting object request: {other:?}"),
        }
    };
    assert_eq!(conflict_request.key().purpose(), object_purpose);
    let conflict_anchor = match conflict_request.key().demand() {
        ObjectDemandEnvelope::Range { range, .. } => range.start().get(),
        ObjectDemandEnvelope::Anchor { anchor, .. } => anchor.get(),
    };
    let malformed = restoration_object_page(
        conflict_request,
        &[object_fact(998, conflict_anchor, 10)],
        98_201,
    );
    let late = malformed.clone();
    let error = cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.deliver_object_page_in_window(malformed, window, cx)
        })
    });
    assert!(matches!(
        error,
        Err(gpui_text_input::RangeTextInputError::Geometry(
            gpui_text_input::ExactGeometryError::SourceContract
        ))
    ));
    input.read_with(cx, |input, _| {
        assert_eq!(input.surface().unwrap().geometry_key(), prior);
    });
    let mut releases = 0;
    while let Some(released) = input.update(cx, |input, _| input.take_request()) {
        match released {
            RangeTextInputRequest::ReleaseObjectPage(key) if key == conflict_request.key() => {
                releases += 1
            }
            RangeTextInputRequest::ReleasePage(_) | RangeTextInputRequest::ReleaseObjectPage(_) => {
            }
            other => panic!("unexpected object-conflict settlement request: {other:?}"),
        }
    }
    assert_eq!(releases, 1);
    input.read_with(cx, |input, _| assert!(input.is_quiescent()));
    assert!(matches!(
        input.update(cx, |input, cx| input.deliver_object_page(late, cx)),
        Err(gpui_text_input::RangeTextInputError::ObjectResponseRejected(_))
    ));
    assert!(input.update(cx, |input, _| input.take_request()).is_none());
    if target {
        input.update(cx, |input, cx| {
            input.request_absolute_scroll(px(0.), cx).unwrap()
        });
    } else {
        input.update(cx, |input, cx| input.set_layout(layout, style, cx).unwrap());
    }
    assert!(drive_pages(&input, cx, &source).is_empty());
    input.read_with(cx, |input, _| {
        assert!(input.surface().is_some());
        assert!(input.is_quiescent());
    });
}

#[gpui::test]
fn malformed_geometry_index_object_residency_conflict_terminates_and_can_restart(
    cx: &mut gpui::TestAppContext,
) {
    malformed_geometry_object_residency_conflict(cx, false);
}

#[gpui::test]
fn malformed_geometry_target_object_residency_conflict_retains_surface_and_restarts(
    cx: &mut gpui::TestAppContext,
) {
    malformed_geometry_object_residency_conflict(cx, true);
}

#[gpui::test]
fn failed_successor_geometry_retains_prior_surface_and_returns_late_page(
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
            assert!(matches!(
                input.deliver_page(late, window, cx),
                Err(gpui_text_input::RangeTextInputError::PageResponseRejected(
                    _
                ))
            ));
        })
    });
    let lifecycle = (0..8)
        .filter_map(|_| input.update(cx, |input, _| input.take_request()))
        .collect::<Vec<_>>();
    assert!(lifecycle.is_empty());
    assert_eq!(
        input.read_with(cx, |input, _| input.surface().unwrap().binding()),
        prior
    );
}

#[gpui::test]
fn boundary_overlapping_object_pages_realize_wrapped_adjacent_objects_once(
    cx: &mut gpui::TestAppContext,
) {
    cx.update(ensure_text_input_bindings);
    let source = "x".repeat(40);
    let mut configuration = config(&source, 1);
    configuration.layout.wrap_width = px(240.);
    configuration.viewport_extent = px(480.);
    let (input, cx) = cx.add_window_view(|window, cx| {
        let input = RangeTextInput::new(configuration, window, cx).unwrap();
        input.focus(window);
        input
    });
    let object_width = cx.update(|window, _| window.viewport_size().width * 0.6);
    let facts = [
        object_fact_with_width_and_activation(221, 32, 10, object_width, true),
        object_fact_with_width_and_activation(222, 32, 20, object_width, true),
    ];
    drive_pages_with_objects(&input, cx, &source, &facts);
    cx.simulate_keystrokes("end");
    drive_pages_with_objects(&input, cx, &source, &facts);

    input.read_with(cx, |input, _| {
        let surface = input.surface().unwrap();
        let realized = surface.realized_objects();
        assert_eq!(
            realized.iter().map(|object| object.id()).collect::<Vec<_>>(),
            vec![InlineObjectId::new(221), InlineObjectId::new(222)]
        );
        let retained_occurrences = surface
            .object_pages()
            .iter()
            .flat_map(|page| page.objects())
            .filter(|object| {
                matches!(object.id(), id if id == InlineObjectId::new(221) || id == InlineObjectId::new(222))
            })
            .count();
        let retained = surface.object_pages();
        assert_eq!(retained.len(), 2);
        let first = &retained[0];
        let second = &retained[1];
        let ObjectDemandEnvelope::Range {
            range: first_range,
            cursor: first_cursor,
            ..
        } = first.key().demand()
        else {
            panic!("first retained object page uses a range envelope")
        };
        assert_eq!(
            first_range,
            ByteRange::from_u64(0, 32).unwrap()
        );
        assert_eq!(first_cursor, None);
        assert_eq!(
            first
                .objects()
                .iter()
                .map(|object| object.id())
                .collect::<Vec<_>>(),
            vec![InlineObjectId::new(221), InlineObjectId::new(222)]
        );
        let ObjectDemandEnvelope::Range {
            range: second_range,
            cursor: second_cursor,
            ..
        } = second.key().demand()
        else {
            panic!("second retained object page uses a range envelope")
        };
        assert_eq!(second_range, ByteRange::from_u64(32, 40).unwrap());
        let cursor = second_cursor.unwrap();
        assert_eq!(cursor.anchor(), ByteOffset::new(32));
        assert_eq!(cursor.order(), InlineObjectOrder::new(20));
        assert_eq!(cursor.id(), InlineObjectId::new(222));
        assert!(second.objects().is_empty());
        assert_eq!(first_range.end(), second_range.start());
        assert_eq!(retained_occurrences, 2);

        let shared = SourcePosition::new(
            ByteOffset::new(32),
            InlineObjectGap::between(object_neighbor(221, 10), object_neighbor(222, 20)).unwrap(),
        );
        assert_eq!(realized[0].trailing(), shared);
        assert_eq!(realized[1].leading(), shared);
        assert!(realized[1].bounds().origin.y > realized[0].bounds().origin.y);
        let gaps = surface
            .realized_object_gaps()
            .iter()
            .filter(|gap| gap.position() == shared)
            .copied()
            .collect::<Vec<_>>();
        assert_eq!(gaps.len(), 1);
        assert_eq!(gaps[0].caret_bounds(), realized[1].leading_caret_bounds());
    });
}

#[gpui::test]
fn same_anchor_objects_cross_pages_keep_order_gaps_hits_and_bounded_residency(
    cx: &mut gpui::TestAppContext,
) {
    cx.update(ensure_text_input_bindings);
    let source = "ab";
    let facts = (0..96)
        .map(|index| object_fact(300 + index, 1, index + 1))
        .collect::<Vec<_>>();
    let (input, cx) = cx.add_window_view(|window, cx| {
        let mut configuration = config(source, 1);
        configuration.object_residency_limits =
            ObjectResidencyLimits::new(4, 48, 512 * 1024, 64 * 1024, 4, 48, 512 * 1024).unwrap();
        let input = RangeTextInput::new(configuration, window, cx).unwrap();
        input.focus(window);
        input
    });
    let mut page_id = 96_000;
    let mut delivered_object_pages = 0usize;
    for _ in 0..128 {
        match input.update(cx, |input, _| input.take_request()) {
            Some(RangeTextInputRequest::Page(request)) => {
                let page = page_for(source, page_id, request);
                page_id += 1;
                cx.update(|window, app| {
                    input.update(app, |input, cx| {
                        input.deliver_page(page, window, cx).unwrap()
                    })
                });
            }
            Some(RangeTextInputRequest::ObjectPage(request)) => {
                let page = forward_object_page_with_limit(request, &facts, page_id, 8);
                page_id += 1;
                delivered_object_pages += 1;
                cx.update(|window, app| {
                    input.update(app, |input, cx| {
                        input
                            .deliver_object_page_in_window(page, window, cx)
                            .unwrap()
                    })
                });
            }
            Some(RangeTextInputRequest::ReleasePage(_))
            | Some(RangeTextInputRequest::CancelPage(_))
            | Some(RangeTextInputRequest::ReleaseObjectPage(_))
            | Some(RangeTextInputRequest::CancelObjectPage(_)) => {}
            None => break,
            Some(request) => panic!("unexpected same-anchor request: {request:?}"),
        }
    }

    input.read_with(cx, |input, _| {
        let surface = input.surface().unwrap();
        assert!(delivered_object_pages > 1);
        assert!(surface.object_pages().len() <= 4);
        assert!(
            surface
                .object_pages()
                .iter()
                .all(|page| page.objects().len() <= 8)
        );
        assert_eq!(
            surface
                .realized_objects()
                .iter()
                .map(|object| object.id())
                .collect::<Vec<_>>(),
            facts.iter().map(InlineObjectFact::id).collect::<Vec<_>>()
        );
        for pair in facts.windows(2) {
            let position = SourcePosition::new(
                ByteOffset::new(1),
                InlineObjectGap::between(
                    object_neighbor(pair[0].id().get(), pair[0].order().get()),
                    object_neighbor(pair[1].id().get(), pair[1].order().get()),
                )
                .unwrap(),
            );
            assert_eq!(
                surface
                    .realized_object_gaps()
                    .iter()
                    .filter(|gap| gap.position() == position)
                    .count(),
                1
            );
        }
        for object in surface.realized_objects() {
            let bounds = object.bounds();
            let hit = surface
                .hit_test_composite(point(bounds.origin.x + px(1.), bounds.origin.y + px(1.)));
            assert!(matches!(hit, Some(RangeSurfaceHit::Object(hit)) if hit.id() == object.id()));
        }
        let diagnostics = input.realization_diagnostics();
        assert!(diagnostics.current.resident_objects <= 48);
        assert!(surface.object_pages().len() <= 4);
    });
    let (click, selected) = input.read_with(cx, |input, _| {
        let surface = input.surface().unwrap();
        let scroll = surface.scroll_block();
        (0..20)
            .find_map(|row| {
                (0..26).find_map(|column| {
                    let viewport = point(px(column as f32 * 4.), px(row as f32 * 4.));
                    let logical = viewport + point(gpui::Pixels::ZERO, scroll);
                    let RangeSurfaceHit::Object(object) = surface.hit_test_composite(logical)?
                    else {
                        return None;
                    };
                    let resident = surface.object_pages().iter().any(|page| {
                        page.objects()
                            .iter()
                            .any(|fact| fact.id() == object.id() && fact.order() == object.order())
                    });
                    (!resident).then_some((viewport, object))
                })
            })
            .expect("no visible geometry-owned object was outside generic residency")
    });
    cx.simulate_event(MouseDownEvent {
        position: click,
        modifiers: Modifiers::none(),
        button: MouseButton::Left,
        click_count: 1,
        first_mouse: false,
    });
    drive_pages_with_limited_objects(&input, cx, source, &facts, 8);
    let active = input.read_with(cx, |input, _| input.active_inline_object().unwrap());
    assert_eq!(active.object_id, selected.id());
    assert_eq!(active.order, selected.order());
}

#[gpui::test]
fn off_origin_document_commands_use_exact_before_all_and_after_all_gaps(
    cx: &mut gpui::TestAppContext,
) {
    cx.update(ensure_text_input_bindings);
    let source = (0..100)
        .map(|line| format!("line-{line:03}\n"))
        .collect::<String>();
    let end = source.len() as u64;
    let facts = [object_fact(231, 0, 10), object_fact(232, end, 10)];
    let before_all = SourcePosition::new(
        ByteOffset::new(0),
        InlineObjectGap::before(object_neighbor(231, 10)),
    );
    let after_all = SourcePosition::new(
        ByteOffset::new(end),
        InlineObjectGap::after(object_neighbor(232, 10)),
    );
    let (input, cx) = cx.add_window_view(|window, cx| {
        let input = RangeTextInput::new(config(&source, 1), window, cx).unwrap();
        input.focus(window);
        input
    });
    drive_pages_with_objects(&input, cx, &source, &facts);
    input.update(cx, |input, cx| {
        input.request_absolute_scroll(px(10_000.), cx).unwrap()
    });
    drive_pages_with_objects(&input, cx, &source, &facts);
    assert!(input.read_with(cx, |input, _| {
        input.surface().unwrap().viewport().start().get() > 0
    }));

    cx.simulate_keystrokes("ctrl-a");
    drive_pages_with_objects(&input, cx, &source, &facts);
    assert_eq!(
        input.read_with(cx, |input, _| input.surface().unwrap().selection()),
        RangeSourceSelection {
            anchor: before_all,
            head: after_all,
        }
    );

    cx.simulate_keystrokes("ctrl-end");
    drive_pages_with_objects(&input, cx, &source, &facts);
    assert_eq!(
        input.read_with(cx, |input, _| input.surface().unwrap().selection()),
        RangeSourceSelection::caret(after_all)
    );
    cx.simulate_keystrokes("ctrl-shift-home");
    drive_pages_with_objects(&input, cx, &source, &facts);
    assert_eq!(
        input.read_with(cx, |input, _| input.surface().unwrap().selection()),
        RangeSourceSelection {
            anchor: after_all,
            head: before_all,
        }
    );

    cx.simulate_keystrokes("ctrl-home");
    drive_pages_with_objects(&input, cx, &source, &facts);
    assert_eq!(
        input.read_with(cx, |input, _| input.surface().unwrap().selection()),
        RangeSourceSelection::caret(before_all)
    );
    cx.simulate_keystrokes("ctrl-shift-end");
    drive_pages_with_objects(&input, cx, &source, &facts);
    assert_eq!(
        input.read_with(cx, |input, _| input.surface().unwrap().selection()),
        RangeSourceSelection {
            anchor: before_all,
            head: after_all,
        }
    );
}

#[gpui::test]
fn resident_utf16_mapping_rejects_off_origin_and_incomplete_origin_prefix_then_accepts_complete_prefix(
    cx: &mut gpui::TestAppContext,
) {
    cx.update(ensure_text_input_bindings);
    let source = (0..160)
        .map(|line| format!("🙂-{line:03}\n"))
        .collect::<String>();
    let (input, cx) = cx.add_window_view(|window, cx| {
        let input = RangeTextInput::new(config(&source, 1), window, cx).unwrap();
        input.focus(window);
        input
    });
    drive_pages(&input, cx, &source);
    input.update(cx, |input, cx| {
        input.request_absolute_scroll(px(10_000.), cx).unwrap()
    });
    drive_pages(&input, cx, &source);
    cx.simulate_keystrokes("ctrl-end");
    drive_pages(&input, cx, &source);
    assert!(input.read_with(cx, |input, _| {
        input
            .surface()
            .unwrap()
            .pages()
            .iter()
            .all(|page| page.range().start().get() > 0)
    }));
    assert!(
        cx.update(|window, app| {
            input.update(app, |input, cx| {
                input.selected_text_range(false, window, cx)
            })
        })
        .is_none()
    );

    cx.simulate_keystrokes("ctrl-shift-home");
    drive_pages(&input, cx, &source);
    input.read_with(cx, |input, _| {
        let surface = input.surface().unwrap();
        let selection = surface.selection().range().unwrap();
        let mut ranges = input
            .surface()
            .unwrap()
            .pages()
            .iter()
            .map(|page| page.range())
            .collect::<Vec<_>>();
        ranges.sort_by_key(|range| range.start());
        assert_eq!(selection.start().byte_offset.get(), 0);
        assert_eq!(selection.end().byte_offset.get(), source.len() as u64);
        assert_eq!(ranges.first().unwrap().start().get(), 0);
        assert!(ranges.last().unwrap().end() < selection.end().byte_offset);
    });
    assert!(
        cx.update(|window, app| {
            input.update(app, |input, cx| {
                input.selected_text_range(false, window, cx)
            })
        })
        .is_none()
    );

    cx.simulate_keystrokes("ctrl-home");
    drive_pages(&input, cx, &source);
    cx.simulate_keystrokes("right");
    drive_pages(&input, cx, &source);
    input.read_with(cx, |input, _| {
        assert!(input.surface().unwrap().pages().iter().any(|page| {
            page.range().start().get() == 0 && page.range().end().get() >= "🙂".len() as u64
        }));
    });
    let selection = cx
        .update(|window, app| {
            input.update(app, |input, cx| {
                input.selected_text_range(false, window, cx)
            })
        })
        .unwrap();
    assert_eq!(selection.range, 2..2);
    assert!(!selection.reversed);
}

#[gpui::test]
fn pointer_activation_and_realization_loss_use_only_the_current_exact_surface(
    cx: &mut gpui::TestAppContext,
) {
    cx.update(ensure_text_input_bindings);
    let source = (0..80)
        .map(|index| format!("line {index}\n"))
        .collect::<String>();
    let facts = vec![object_fact(301, 1, 10)];
    let (input, cx) = cx.add_window_view(|window, cx| {
        let input = RangeTextInput::new(config(&source, 1), window, cx).unwrap();
        input.focus(window);
        input
    });
    drive_pages_with_objects(&input, cx, &source, &facts);
    let events = restoration_events(&input, cx);
    let object = input.read_with(cx, |input, _| {
        input.surface().unwrap().realized_objects()[0]
    });
    let click = object.hit_bounds().origin + gpui::point(px(1.), px(1.));
    let initial_selection = input.read_with(cx, |input, _| input.surface().unwrap().selection());
    for button in [MouseButton::Right, MouseButton::Middle] {
        cx.simulate_event(MouseDownEvent {
            position: click,
            modifiers: Modifiers::none(),
            button,
            click_count: 1,
            first_mouse: false,
        });
        assert!(
            events
                .borrow()
                .iter()
                .all(|event| !matches!(event, RangeTextInputEvent::InlineObjectActivated(_)))
        );
        input.read_with(cx, |input, _| {
            assert!(input.active_inline_object().is_none());
            assert_eq!(input.surface().unwrap().selection(), initial_selection);
        });
    }
    cx.simulate_event(MouseDownEvent {
        position: click,
        modifiers: Modifiers::none(),
        button: MouseButton::Left,
        click_count: 1,
        first_mouse: false,
    });
    drive_pages_with_objects(&input, cx, &source, &facts);
    let activation = events
        .borrow()
        .iter()
        .find_map(|event| match event {
            RangeTextInputEvent::InlineObjectActivated(activation) => Some(*activation),
            _ => None,
        })
        .expect("pointer activation");
    assert_eq!(activation.anchor.binding, binding(&source, 1));
    assert_eq!(activation.anchor.object_id, InlineObjectId::new(301));
    assert_eq!(activation.anchor.order, InlineObjectOrder::new(10));
    assert_eq!(activation.anchor.bounds, object.bounds());
    let initial_geometry = input.read_with(cx, |input, _| input.surface().unwrap().geometry_key());
    assert_eq!(
        activation.anchor.presentation_generation,
        initial_geometry.presentation_generation()
    );
    assert_eq!(activation.anchor.layout_epoch, initial_geometry.epoch());
    assert!(matches!(
        activation.origin,
        gpui_text_input::InlineObjectInputOrigin::Pointer { point } if point == click
    ));

    input.update(cx, |input, cx| {
        input
            .set_presentation_generation(PresentationGeneration::new(2), cx)
            .unwrap()
    });
    assert!(input.read_with(cx, |input, _| input.active_inline_object().is_none()));
    assert_eq!(
        events
            .borrow()
            .iter()
            .filter(|event| matches!(
                event,
                RangeTextInputEvent::InlineObjectRealizationLost(loss)
                    if loss.reason
                        == gpui_text_input::InlineObjectRealizationLossReason::Superseded
            ))
            .count(),
        1
    );
    drive_pages_with_objects(&input, cx, &source, &facts);
    let current_object = input.read_with(cx, |input, _| {
        input.surface().unwrap().realized_objects()[0]
    });
    cx.simulate_event(MouseDownEvent {
        position: current_object.hit_bounds().origin + gpui::point(px(1.), px(1.)),
        modifiers: Modifiers::none(),
        button: MouseButton::Left,
        click_count: 1,
        first_mouse: false,
    });
    drive_pages_with_objects(&input, cx, &source, &facts);
    assert!(input.read_with(cx, |input, _| input.active_inline_object().is_some()));
    let attached = input
        .update(cx, |input, _| {
            input.attach_active_inline_object_surface(input.active_inline_object().unwrap())
        })
        .unwrap();

    input.update(cx, |input, cx| {
        input.request_absolute_scroll(px(800.), cx).unwrap()
    });
    drive_pages_with_objects(&input, cx, &source, &facts);
    input.read_with(cx, |input, _| {
        assert!(input.active_inline_object().is_none());
        assert!(
            input
                .surface()
                .unwrap()
                .realized_objects()
                .iter()
                .all(|object| object.id() != InlineObjectId::new(301))
        );
    });
    assert!(events.borrow().iter().any(|event| matches!(
        event,
        RangeTextInputEvent::InlineObjectRealizationLost(loss)
            if loss.anchor.object_id == InlineObjectId::new(301)
                && loss.reason == gpui_text_input::InlineObjectRealizationLossReason::Unrealized
    )));
    assert!(matches!(
        cx.update(|window, app| input.update(app, |input, cx| input
            .dismiss_active_inline_object_surface(
                attached,
                InlineObjectSurfaceDismissal::RefocusObject,
                window,
                cx,
            ))),
        Err(gpui_text_input::RangeTextInputError::Stale)
    ));

    input.update(cx, |input, cx| {
        input
            .set_presentation_generation(PresentationGeneration::new(3), cx)
            .unwrap()
    });
    drive_pages_with_objects(&input, cx, &source, &facts);
    input.read_with(cx, |input, _| {
        let surface = input.surface().unwrap();
        assert_eq!(
            surface.geometry_key().presentation_generation(),
            PresentationGeneration::new(3)
        );
        assert!(
            surface
                .realized_presentations(surface.publication_key())
                .unwrap()
                .all(|fact| {
                    fact.geometry().id() != InlineObjectId::new(301)
                        || (fact.presentation().semantic_state() == 0
                            && fact.presentation().activation_eligible())
                })
        );
    });
}

#[gpui::test]
fn exact_attached_inline_object_surface_owns_focus_loss_until_one_explicit_dismissal(
    cx: &mut gpui::TestAppContext,
) {
    cx.update(ensure_text_input_bindings);
    let source = "ab";
    let facts = [object_fact(307, 1, 10)];
    let (input, cx) = cx.add_window_view(|window, cx| {
        let input = RangeTextInput::new(config(source, 1), window, cx).unwrap();
        input.focus(window);
        input
    });
    drive_pages_with_objects(&input, cx, source, &facts);
    let events = restoration_events(&input, cx);
    let object = input.read_with(cx, |input, _| {
        input.surface().unwrap().realized_objects()[0]
    });
    cx.simulate_event(MouseDownEvent {
        position: object.hit_bounds().origin + point(px(1.), px(1.)),
        modifiers: Modifiers::none(),
        button: MouseButton::Left,
        click_count: 1,
        first_mouse: false,
    });
    drive_pages_with_objects(&input, cx, source, &facts);
    let active = input.read_with(cx, |input, _| input.active_inline_object().unwrap());
    let mut stale = active;
    stale.layout_epoch = gpui_text_input::LayoutEpoch::new(active.layout_epoch.get() + 1);
    input.update(cx, |input, _| {
        assert!(matches!(
            input.attach_active_inline_object_surface(stale),
            Err(gpui_text_input::RangeTextInputError::Stale)
        ));
    });
    let menu_attachment = input
        .update(cx, |input, _| {
            input.attach_active_inline_object_surface(active)
        })
        .unwrap();
    assert_eq!(menu_attachment.anchor(), active);
    input.read_with(cx, |input, _| assert!(!input.is_quiescent()));

    cx.update(|window, _| window.blur());
    drive_attached_inline_object_surface_requests(&input, cx, source, &facts);
    input.read_with(cx, |input, _| {
        assert_eq!(input.active_inline_object(), Some(active));
        assert!(!input.is_quiescent());
    });
    assert!(events.borrow().iter().all(|event| !matches!(
        event,
        RangeTextInputEvent::InlineObjectRealizationLost(loss)
            if loss.anchor == active
                && loss.reason
                    == gpui_text_input::InlineObjectRealizationLossReason::FocusLost
    )));

    let preview_attachment = menu_attachment;
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input
                .dismiss_active_inline_object_surface(
                    preview_attachment,
                    InlineObjectSurfaceDismissal::RefocusObject,
                    window,
                    cx,
                )
                .unwrap();
        })
    });
    drive_pages_with_objects(&input, cx, source, &facts);
    input.read_with(cx, |input, _| {
        assert_eq!(input.active_inline_object(), Some(active));
        assert!(input.is_quiescent());
    });

    assert!(events.borrow().iter().all(|event| !matches!(
        event,
        RangeTextInputEvent::InlineObjectRealizationLost(loss)
            if loss.anchor == active
                && loss.reason
                    == gpui_text_input::InlineObjectRealizationLossReason::FocusLost
    )));

    let attachment = input
        .update(cx, |input, _| {
            input.attach_active_inline_object_surface(active)
        })
        .unwrap();
    cx.update(|window, _| window.blur());
    drive_attached_inline_object_surface_requests(&input, cx, source, &facts);
    assert!(events.borrow().iter().all(|event| !matches!(
        event,
        RangeTextInputEvent::InlineObjectRealizationLost(loss)
            if loss.anchor == active
                && loss.reason
                    == gpui_text_input::InlineObjectRealizationLossReason::FocusLost
    )));
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input
                .dismiss_active_inline_object_surface(
                    attachment,
                    InlineObjectSurfaceDismissal::ClearObject,
                    window,
                    cx,
                )
                .unwrap();
        })
    });
    drive_pages_with_objects(&input, cx, source, &facts);
    input.read_with(cx, |input, _| {
        assert!(input.active_inline_object().is_none());
        assert!(input.is_quiescent());
    });
    assert_eq!(
        events
            .borrow()
            .iter()
            .filter(|event| matches!(
                event,
                RangeTextInputEvent::InlineObjectRealizationLost(loss)
                    if loss.anchor == active
                        && loss.reason
                            == gpui_text_input::InlineObjectRealizationLossReason::FocusLost
            ))
            .count(),
        1
    );
}

#[gpui::test]
fn active_inline_object_remove_requires_the_exact_realization_anchor(
    cx: &mut gpui::TestAppContext,
) {
    cx.update(ensure_text_input_bindings);
    let source = "ab";
    let facts = [object_fact(302, 1, 10)];
    let (input, cx) = cx.add_window_view(|window, cx| {
        let input = RangeTextInput::new(config(source, 1), window, cx).unwrap();
        input.focus(window);
        input
    });
    drive_pages_with_objects(&input, cx, source, &facts);
    let object = input.read_with(cx, |input, _| {
        input.surface().unwrap().realized_objects()[0]
    });
    cx.simulate_event(MouseDownEvent {
        position: object.hit_bounds().origin + point(px(1.), px(1.)),
        modifiers: Modifiers::none(),
        button: MouseButton::Left,
        click_count: 1,
        first_mouse: false,
    });
    drive_pages_with_objects(&input, cx, source, &facts);
    let active = input.read_with(cx, |input, _| input.active_inline_object().unwrap());
    let mut stale = active;
    stale.presentation_generation = PresentationGeneration::new(2);

    input.update(cx, |input, cx| {
        assert!(matches!(
            input.remove_active_inline_object(stale, cx),
            Err(gpui_text_input::RangeTextInputError::Stale)
        ));
        assert_eq!(input.active_inline_object(), Some(active));
        assert!(input.take_request().is_none());
    });
}

#[gpui::test]
fn active_inline_object_remove_stages_its_exact_composite_range(cx: &mut gpui::TestAppContext) {
    cx.update(ensure_text_input_bindings);
    let source = "ab";
    let facts = [object_fact(303, 1, 10)];
    let (input, cx) = cx.add_window_view(|window, cx| {
        let input = RangeTextInput::new(config(source, 1), window, cx).unwrap();
        input.focus(window);
        input
    });
    drive_pages_with_objects(&input, cx, source, &facts);
    let object = input.read_with(cx, |input, _| {
        input.surface().unwrap().realized_objects()[0]
    });
    cx.simulate_event(MouseDownEvent {
        position: object.hit_bounds().origin + point(px(1.), px(1.)),
        modifiers: Modifiers::none(),
        button: MouseButton::Left,
        click_count: 1,
        first_mouse: false,
    });
    drive_pages_with_objects(&input, cx, source, &facts);
    let (active, selected) = input.read_with(cx, |input, _| {
        (
            input.active_inline_object().unwrap(),
            input.surface().unwrap().selection().range().unwrap(),
        )
    });

    input.update(cx, |input, cx| {
        let key = input.remove_active_inline_object(active, cx).unwrap();
        let Some(RangeTextInputRequest::MutationBegin(request)) = input.take_request() else {
            panic!("exact remove must begin one staged mutation")
        };
        assert_eq!(request.proposal().key(), key);
        assert_eq!(request.proposal().kind(), MutationKind::Edit);
        assert_eq!(request.proposal().replacement(), selected);
    });
}

#[gpui::test]
fn committed_inline_object_replacement_invalidates_an_exact_surface_attachment(
    cx: &mut gpui::TestAppContext,
) {
    cx.update(ensure_text_input_bindings);
    let source = "ab";
    let facts = [object_fact(308, 1, 10)];
    let (input, cx) = cx.add_window_view(|window, cx| {
        let input = RangeTextInput::new(config(source, 1), window, cx).unwrap();
        input.focus(window);
        input
    });
    drive_pages_with_objects(&input, cx, source, &facts);
    let events = restoration_events(&input, cx);
    let object = input.read_with(cx, |input, _| {
        input.surface().unwrap().realized_objects()[0]
    });
    cx.simulate_event(MouseDownEvent {
        position: object.hit_bounds().origin + point(px(1.), px(1.)),
        modifiers: Modifiers::none(),
        button: MouseButton::Left,
        click_count: 1,
        first_mouse: false,
    });
    drive_pages_with_objects(&input, cx, source, &facts);
    let (active, predecessor) = input.read_with(cx, |input, _| {
        let surface = input.surface().unwrap();
        let selection = surface.selection();
        (
            input.active_inline_object().unwrap(),
            MutationPositions::new(selection.head, selection.anchor, selection.head),
        )
    });
    let attached = input
        .update(cx, |input, _| {
            input.attach_active_inline_object_surface(active)
        })
        .unwrap();
    let replacement = SourceRange::new(object.leading(), object.trailing()).unwrap();
    let operation = input.read_with(cx, |input, _| input.lease_host_operation().unwrap());
    let key = MutationKey::new(
        binding(source, 1).binding(),
        binding(source, 1).revision(),
        operation.operation(),
    );
    let proposal = MutationProposal::new(key, MutationKind::Edit, predecessor, replacement, 0);
    let begin = MutationBeginRequest::new(proposal, MutationCursor::new(0), MutationCursor::new(0));
    let mut base_positions = Vec::new();
    for position in [
        predecessor.caret(),
        predecessor.selection_anchor(),
        predecessor.selection_head(),
        replacement.start(),
        replacement.end(),
    ] {
        if !base_positions.contains(&position) {
            base_positions.push(position);
        }
    }
    let (base_text, base_objects) = admitted_sources_with_facts(source, 1, &base_positions, &facts);
    input.update(cx, |input, cx| {
        input
            .begin_host_mutation(
                operation,
                begin,
                &base_positions,
                &base_text,
                &base_objects,
                cx,
            )
            .unwrap();
        assert!(matches!(
            input.take_request(),
            Some(RangeTextInputRequest::MutationBegin(request)) if request == begin
        ));
        input.accept_mutation_preflight(key, cx).unwrap();
    });

    let successor_id = InlineObjectId::new(309);
    let successor_order = InlineObjectOrder::new(20);
    let page = gpui_text_input::MutationPage::new(
        gpui_text_input::MutationPageKey::new(
            key,
            gpui_text_input::MutationLane::Proposal,
            MutationCursor::new(0),
            0,
            gpui_text_input::MutationIdentity::ROOT,
        ),
        MutationCursor::new(1),
        vec![gpui_text_input::MutationPageItem::Object(
            gpui_text_input::ObjectChange::Replace {
                target: gpui_text_input::ObjectTarget::new(
                    replacement,
                    active.object_id,
                    active.order,
                )
                .unwrap(),
                object: gpui_text_input::SuccessorObject::new(
                    successor_id,
                    object.leading().byte_offset,
                    successor_order,
                    1,
                    1,
                ),
            },
        )],
    )
    .unwrap();
    let intended_position = SourcePosition::new(
        object.leading().byte_offset,
        InlineObjectGap::After(InlineObjectNeighbor::new(successor_id, successor_order)),
    );
    let intended = MutationPositions::collapsed(intended_position);
    let finish = gpui_text_input::MutationFinishInput::new(
        key,
        gpui_text_input::MutationStreamFinish {
            next_cursor: begin.source_cursor(),
            next_ordinal: 0,
            cumulative_identity: gpui_text_input::MutationIdentity::ROOT,
            totals: gpui_text_input::MutationTotals::default(),
        },
        gpui_text_input::MutationStreamFinish {
            next_cursor: page.next_cursor(),
            next_ordinal: 1,
            cumulative_identity: page.cumulative_identity(),
            totals: page.totals(),
        },
        binding(source, 1).extent(),
        intended,
    );
    input.update(cx, |input, cx| {
        input.submit_mutation_page(page, cx).unwrap();
        assert!(matches!(
            input.take_request(),
            Some(RangeTextInputRequest::MutationProposalPage(_))
        ));
        input.submit_mutation_finish(finish, cx).unwrap();
        assert!(matches!(
            input.take_request(),
            Some(RangeTextInputRequest::MutationFinishInput(request)) if request == finish
        ));
        input.accept_mutation_finish(key, cx).unwrap();
        assert!(matches!(
            input.take_request(),
            Some(RangeTextInputRequest::MutationCommit(request)) if request.key() == key
        ));
    });
    let successor_facts = [object_fact(309, 1, 20)];
    let (successor_text, successor_objects) =
        admitted_sources_with_facts(source, 2, &[intended_position], &successor_facts);
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input
                .settle_committed_mutation(
                    key,
                    binding(source, 2),
                    intended,
                    &successor_text,
                    &successor_objects,
                    window,
                    cx,
                )
                .unwrap();
        })
    });
    drive_pages_with_objects(&input, cx, source, &successor_facts);
    assert!(input.read_with(cx, |input, _| input.active_inline_object().is_none()));
    assert_eq!(
        events
            .borrow()
            .iter()
            .filter(|event| matches!(
                event,
                RangeTextInputEvent::InlineObjectRealizationLost(loss)
                    if loss.anchor == active
                        && loss.reason
                            == gpui_text_input::InlineObjectRealizationLossReason::Replaced
            ))
            .count(),
        1
    );
    assert!(matches!(
        cx.update(|window, app| input.update(app, |input, cx| input
            .dismiss_active_inline_object_surface(
                attached,
                InlineObjectSurfaceDismissal::RefocusObject,
                window,
                cx,
            ))),
        Err(gpui_text_input::RangeTextInputError::Stale)
    ));
}

#[gpui::test]
fn object_gap_platform_composition_is_not_collapsed_and_lifecycle_loss_is_once(
    cx: &mut gpui::TestAppContext,
) {
    cx.update(ensure_text_input_bindings);
    let source = "ab";
    let facts = vec![object_fact(501, 1, 10)];
    let configuration = config(source, 1);
    let settlement_coordinator = configuration.settlement_coordinator.clone();
    let (input, cx) = cx.add_window_view(move |window, cx| {
        let input = RangeTextInput::new(configuration, window, cx).unwrap();
        input.focus(window);
        input
    });
    drive_pages_with_objects(&input, cx, source, &facts);
    let events = restoration_events(&input, cx);
    cx.simulate_keystrokes("right");
    drive_pages_with_objects(&input, cx, source, &facts);
    cx.simulate_keystrokes("right");
    drive_pages_with_objects(&input, cx, source, &facts);
    let object = InlineObjectNeighbor::new(InlineObjectId::new(501), InlineObjectOrder::new(10));
    let expected_selection = RangeSourceSelection {
        anchor: SourcePosition::new(ByteOffset::new(1), InlineObjectGap::Before(object)),
        head: SourcePosition::new(ByteOffset::new(1), InlineObjectGap::After(object)),
    };
    let before_mark = input.read_with(cx, |input, _| {
        let surface = input.surface().unwrap();
        (
            surface.binding(),
            surface.selection(),
            surface.platform_selection(),
            surface.composition(),
            input.active_inline_object(),
            input.realization_diagnostics().current,
        )
    });
    assert_eq!(before_mark.0, binding(source, 1));
    assert_eq!(before_mark.1, expected_selection);
    assert!(before_mark.2.is_none());
    assert!(before_mark.3.is_none());
    assert_eq!(before_mark.4.unwrap().object_id, InlineObjectId::new(501));
    assert!(input.read_with(cx, |input, _| input.is_quiescent()));
    assert!(input.read_with(cx, |input, _| input.is_semantically_quiescent()));
    assert_eq!(settlement_coordinator.retained_count(), 0);
    let settled_before = events
        .borrow()
        .iter()
        .filter(|event| matches!(event, RangeTextInputEvent::MutationSettled { .. }))
        .count();
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            assert!(input.selected_text_range(false, window, cx).is_none());
            input.replace_and_mark_text_in_range(None, "marked", None, window, cx);
        })
    });
    let after_mark = input.read_with(cx, |input, _| {
        let surface = input.surface().unwrap();
        (
            surface.binding(),
            surface.selection(),
            surface.platform_selection(),
            surface.composition(),
            input.active_inline_object(),
            input.realization_diagnostics().current,
        )
    });
    assert_eq!(after_mark, before_mark);
    assert!(input.update(cx, |input, _| input.take_request()).is_none());
    assert!(input.read_with(cx, |input, _| input.is_quiescent()));
    assert!(input.read_with(cx, |input, _| input.is_semantically_quiescent()));
    assert_eq!(settlement_coordinator.retained_count(), 0);
    assert_eq!(
        input.read_with(cx, |input, _| input.lease_host_operation().unwrap().operation()),
        gpui_text_input::OperationId::new(1)
    );
    assert_eq!(settlement_coordinator.retained_count(), 0);
    assert_eq!(
        events
            .borrow()
            .iter()
            .filter(|event| matches!(event, RangeTextInputEvent::MutationSettled { .. }))
            .count(),
        settled_before
    );
    input.read_with(cx, |input, _| {
        let diagnostics = input.realization_diagnostics();
        assert!(input.surface().unwrap().composition().is_none());
        assert!(input.adopted_mutation_positions().is_none());
        assert!(input.active_inline_object().is_some());
        assert_eq!(diagnostics.current.pending_rebind_intents, 0);
        assert_eq!(diagnostics.current.pending_configuration_bytes, 0);
        assert_eq!(diagnostics.current.pending_configuration_items, 0);
        assert_eq!(diagnostics.current.request_payload_bytes, 0);
        assert_eq!(diagnostics.current.request_payload_items, 0);
        assert_eq!(diagnostics.current.response_custody_count, 0);
    });

    // This independent direct rebind is only a downstream control; no mutation settled it.
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.rebind(binding(source, 2), None, window, cx).unwrap();
        })
    });
    let after_direct_rebind = input.read_with(cx, |input, _| {
        (
            input.surface().map(|surface| surface.binding()),
            input.surface().and_then(|surface| surface.composition()),
            input.active_inline_object(),
            input.realization_diagnostics().current,
        )
    });
    assert_eq!(after_direct_rebind.0, Some(binding(source, 1)));
    assert!(after_direct_rebind.1.is_none());
    assert!(after_direct_rebind.2.is_none());
    assert_eq!(after_direct_rebind.3.pending_page_requests, 1);
    assert_eq!(after_direct_rebind.3.pending_index_intents, 1);
    assert_eq!(after_direct_rebind.3.pending_target_intents, 0);
    assert_eq!(after_direct_rebind.3.pending_rebind_intents, 0);
    assert_eq!(after_direct_rebind.3.candidates, 1);
    assert_eq!(after_direct_rebind.3.queued_requests, 1);

    let mut target_step = None;
    let mut terminal_publication_step = None;
    let mut index_step = None;
    let mut observed_quiescent = false;
    for step in 0..512 {
        let request = input.update(cx, |input, _| input.take_request());
        let had_request = request.is_some();
        match request {
            Some(RangeTextInputRequest::Page(request)) => {
                observed_quiescent = false;
                assert_eq!(request.key().binding(), BindingId::new(17));
                assert_eq!(request.key().revision(), SourceRevision::new(2));
                let ownership = input.read_with(cx, |input, _| {
                    input.realization_diagnostics().current
                });
                assert_eq!(ownership.active_geometry_jobs, 1);
                if request.key().purpose() == PagePurpose::GeometryTarget {
                    target_step.get_or_insert(step);
                } else if request.key().purpose() == PagePurpose::GeometryIndex {
                    index_step.get_or_insert(step);
                } else {
                    panic!("unexpected direct-rebind page purpose: {:?}", request.key().purpose());
                }
                let page = page_for(source, request.key().id().get(), request);
                cx.update(|window, app| {
                    input.update(app, |input, cx| input.deliver_page(page, window, cx).unwrap())
                });
            }
            Some(RangeTextInputRequest::ObjectPage(request)) => {
                observed_quiescent = false;
                assert_eq!(request.key().binding(), BindingId::new(17));
                assert_eq!(request.key().revision(), SourceRevision::new(2));
                let ownership = input.read_with(cx, |input, _| {
                    input.realization_diagnostics().current
                });
                assert_eq!(ownership.active_geometry_jobs, 1);
                if request.key().purpose() == ObjectPurpose::GeometryTarget {
                    target_step.get_or_insert(step);
                } else if request.key().purpose() == ObjectPurpose::GeometryIndex {
                    index_step.get_or_insert(step);
                } else {
                    panic!(
                        "unexpected direct-rebind object purpose: {:?}",
                        request.key().purpose()
                    );
                }
                let page = restoration_object_page(request, &facts, request.key().id().get());
                cx.update(|window, app| {
                    input.update(app, |input, cx| {
                        input.deliver_object_page_in_window(page, window, cx).unwrap()
                    })
                });
            }
            Some(RangeTextInputRequest::ReleasePage(_))
            | Some(RangeTextInputRequest::CancelPage(_))
            | Some(RangeTextInputRequest::ReleaseObjectPage(_))
            | Some(RangeTextInputRequest::CancelObjectPage(_)) => {
                observed_quiescent = false;
            }
            None => {
                let quiescent = input.read_with(cx, |input, _| input.is_quiescent());
                if quiescent && observed_quiescent {
                    break;
                }
                observed_quiescent = quiescent;
            }
            Some(request) => panic!("unexpected direct-rebind request: {request:?}"),
        }
        let successor_published = input.read_with(cx, |input, _| {
            input
                .surface()
                .is_some_and(|surface| surface.binding() == binding(source, 2))
        });
        if successor_published && terminal_publication_step.is_none() {
            terminal_publication_step = Some(step);
            input.read_with(cx, |input, _| {
                let surface = input.surface().unwrap();
                assert_eq!(
                    surface.selection(),
                    RangeSourceSelection::caret(SourcePosition::new(
                        ByteOffset::new(0),
                        InlineObjectGap::NoObjects,
                    ))
                );
                assert_eq!(
                    surface.platform_selection(),
                    Some(RangeSelection::caret(ByteOffset::new(0)))
                );
                assert!(surface.composition().is_none());
            });
        }
        if !had_request {
            cx.update(|window, app| window.draw(app).clear());
            cx.run_until_parked();
        }
    }
    assert!(target_step.is_some());
    assert!(terminal_publication_step.is_some());
    assert!(index_step.is_some());
    assert!(target_step <= terminal_publication_step);
    assert!(terminal_publication_step <= index_step);
    input.read_with(cx, |input, _| {
        let surface = input.surface().unwrap();
        let ownership = input.realization_diagnostics().current;
        assert_eq!(surface.binding(), binding(source, 2));
        assert_eq!(
            surface.selection(),
            RangeSourceSelection::caret(SourcePosition::new(
                ByteOffset::new(0),
                InlineObjectGap::NoObjects,
            ))
        );
        assert_eq!(
            surface.platform_selection(),
            Some(RangeSelection::caret(ByteOffset::new(0)))
        );
        assert!(surface.composition().is_none());
        assert!(input.is_quiescent());
        assert_eq!(ownership.pending_index_intents, 0);
        assert_eq!(ownership.pending_target_intents, 0);
        assert_eq!(ownership.pending_presentation_intents, 0);
        assert_eq!(ownership.queued_requests, 0);
    });
    assert_eq!(
        events
            .borrow()
            .iter()
            .filter(|event| matches!(event, RangeTextInputEvent::MutationSettled { .. }))
            .count(),
        settled_before
    );
    assert_eq!(
        events
            .borrow()
            .iter()
            .filter(|event| matches!(
                event,
                RangeTextInputEvent::InlineObjectRealizationLost(loss)
                    if loss.anchor.object_id == InlineObjectId::new(501)
                        && loss.reason
                            == gpui_text_input::InlineObjectRealizationLossReason::Superseded
            ))
            .count(),
        1
    );
    input.read_with(cx, |input, _| {
        assert!(input.active_inline_object().is_none())
    });

    cx.simulate_keystrokes("right");
    drive_pages_with_objects(&input, cx, source, &facts);
    cx.simulate_keystrokes("right");
    drive_pages_with_objects(&input, cx, source, &facts);
    let attached = input
        .update(cx, |input, _| {
            input.attach_active_inline_object_surface(input.active_inline_object().unwrap())
        })
        .unwrap();
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            let disposal_requests = input.dispose(window, cx);
            assert!(disposal_requests.is_empty());
            assert!(input.dispose(window, cx).is_empty());
        })
    });
    input.read_with(cx, |input, _| {
        let diagnostics = input.realization_diagnostics();
        let ownership = diagnostics.current;
        assert!(input.surface().is_none());
        assert!(input.active_inline_object().is_none());
        assert!(input.is_quiescent());
        assert!(input.is_semantically_quiescent());
        assert!(!diagnostics.continuation_scheduled);
        assert_eq!(diagnostics.adopted_custody_bytes, 0);
        assert_eq!(diagnostics.adopted_custody_items, 0);
        assert_eq!(diagnostics.filler_count, 0);
        assert_eq!(diagnostics.surface_charge.bytes, 0);
        assert_eq!(diagnostics.surface_charge.items, 0);
        assert_eq!(ownership.pending_configuration_bytes, 0);
        assert_eq!(ownership.pending_configuration_items, 0);
        assert_eq!(ownership.pending_rebind_intents, 0);
        assert_eq!(ownership.pending_index_intents, 0);
        assert_eq!(ownership.pending_target_intents, 0);
        assert_eq!(ownership.pending_layout_intents, 0);
        assert_eq!(ownership.pending_presentation_intents, 0);
        assert_eq!(ownership.scheduled_continuations, 0);
        assert_eq!(ownership.request_storage_bytes, 0);
        assert_eq!(ownership.request_storage_items, 0);
        assert_eq!(ownership.request_payload_bytes, 0);
        assert_eq!(ownership.request_payload_items, 0);
        assert_eq!(ownership.response_custody_count, 0);
        assert_eq!(ownership.response_custody_bytes, 0);
        assert_eq!(ownership.response_custody_items, 0);
        assert_eq!(ownership.response_processing_bytes, 0);
        assert_eq!(ownership.response_processing_items, 0);
        assert_eq!(ownership.deferred_response_bytes, 0);
        assert_eq!(ownership.deferred_response_items, 0);
        assert_eq!(ownership.page_alias_storage_bytes, 0);
        assert_eq!(ownership.page_alias_storage_items, 0);
        assert_eq!(ownership.page_alias_waits, 0);
        assert_eq!(ownership.resident_page_bytes, 0);
        assert_eq!(ownership.resident_object_bytes, 0);
        assert_eq!(ownership.pending_page_bytes, 0);
        assert_eq!(ownership.pending_object_bytes, 0);
        assert_eq!(ownership.clipboard_bytes, 0);
        assert_eq!(ownership.clipboard_items, 0);
        assert_eq!(ownership.resident_pages, 0);
        assert_eq!(ownership.resident_objects, 0);
        assert_eq!(ownership.pending_page_requests, 0);
        assert_eq!(ownership.pending_object_requests, 0);
        assert_eq!(ownership.dispatched_page_requests, 0);
        assert_eq!(ownership.dispatched_object_requests, 0);
        assert_eq!(ownership.active_geometry_jobs, 0);
        assert_eq!(ownership.pending_geometry_pages, 0);
        assert_eq!(ownership.pending_geometry_objects, 0);
        assert_eq!(ownership.resident_geometry_page_waits, 0);
        assert_eq!(ownership.coalesced_geometry_page_waits, 0);
        assert_eq!(ownership.index_geometry_page_waits, 0);
        assert_eq!(ownership.target_geometry_page_waits, 0);
        assert_eq!(ownership.deferred_geometry_responses, 0);
        assert_eq!(ownership.candidates, 0);
        assert_eq!(ownership.candidate_bytes, 0);
        assert_eq!(ownership.candidate_items, 0);
        assert_eq!(ownership.pending_geometry_record_bytes, 0);
        assert_eq!(ownership.pending_geometry_record_items, 0);
        assert_eq!(ownership.dispatched_record_bytes, 0);
        assert_eq!(ownership.dispatched_record_items, 0);
        assert_eq!(ownership.queued_requests, 0);
        assert_eq!(ownership.checkpoints, 0);
    });
    assert_eq!(settlement_coordinator.retained_count(), 0);
    assert_eq!(
        events
            .borrow()
            .iter()
            .filter(|event| matches!(
                event,
                RangeTextInputEvent::InlineObjectRealizationLost(loss)
                    if loss.anchor.object_id == InlineObjectId::new(501)
                        && loss.reason
                            == gpui_text_input::InlineObjectRealizationLossReason::Disposed
            ))
            .count(),
        1
    );
    assert!(matches!(
        cx.update(|window, app| input.update(app, |input, cx| input
            .dismiss_active_inline_object_surface(
                attached,
                InlineObjectSurfaceDismissal::RefocusObject,
                window,
                cx,
            ))),
        Err(gpui_text_input::RangeTextInputError::Stale)
    ));
}

#[gpui::test]
fn no_op_rebind_preserves_active_object_until_true_rebind(cx: &mut gpui::TestAppContext) {
    cx.update(ensure_text_input_bindings);
    let source = "ab";
    let facts = [object_fact(901, 1, 10)];
    let (input, cx) = cx.add_window_view(|window, cx| {
        let input = RangeTextInput::new(config(source, 1), window, cx).unwrap();
        input.focus(window);
        input
    });
    drive_pages_with_objects(&input, cx, source, &facts);
    let events = restoration_events(&input, cx);
    let object = input.read_with(cx, |input, _| {
        input.surface().unwrap().realized_objects()[0]
    });
    cx.simulate_event(MouseDownEvent {
        position: object.hit_bounds().origin + gpui::point(px(1.), px(1.)),
        modifiers: Modifiers::none(),
        button: MouseButton::Left,
        click_count: 1,
        first_mouse: false,
    });
    drive_pages_with_objects(&input, cx, source, &facts);
    let (active, selection) = input.read_with(cx, |input, _| {
        (
            input.active_inline_object().unwrap(),
            input.surface().unwrap().selection(),
        )
    });

    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input
                .rebind(binding(source, 1), Some(selection), window, cx)
                .unwrap();
        })
    });
    input.read_with(cx, |input, _| {
        assert_eq!(input.active_inline_object(), Some(active));
        assert_eq!(input.surface().unwrap().binding(), binding(source, 1));
        assert!(input.is_quiescent());
    });
    assert!(
        events
            .borrow()
            .iter()
            .all(|event| !matches!(event, RangeTextInputEvent::InlineObjectRealizationLost(_)))
    );

    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.rebind(binding(source, 2), None, window, cx).unwrap();
        })
    });
    drive_pages_with_objects(&input, cx, source, &facts);
    assert!(input.read_with(cx, |input, _| input.active_inline_object().is_none()));
    assert_eq!(
        events
            .borrow()
            .iter()
            .filter(|event| matches!(
                event,
                RangeTextInputEvent::InlineObjectRealizationLost(loss)
                    if loss.anchor == active
                        && loss.reason
                            == gpui_text_input::InlineObjectRealizationLossReason::Superseded
            ))
            .count(),
        1
    );
}

#[gpui::test]
fn same_binding_selection_change_uses_ordinary_target_and_exact_loss(
    cx: &mut gpui::TestAppContext,
) {
    cx.update(ensure_text_input_bindings);
    let source = "ab";
    let facts = [object_fact(906, 1, 10)];
    let (input, cx) = cx.add_window_view(|window, cx| {
        let input = RangeTextInput::new(config(source, 1), window, cx).unwrap();
        input.focus(window);
        input
    });
    drive_pages_with_objects(&input, cx, source, &facts);
    let events = restoration_events(&input, cx);
    let object = input.read_with(cx, |input, _| {
        input.surface().unwrap().realized_objects()[0]
    });
    cx.simulate_event(MouseDownEvent {
        position: object.hit_bounds().origin + point(px(1.), px(1.)),
        modifiers: Modifiers::none(),
        button: MouseButton::Left,
        click_count: 1,
        first_mouse: false,
    });
    drive_pages_with_objects(&input, cx, source, &facts);
    let active = input.read_with(cx, |input, _| input.active_inline_object().unwrap());
    let origin = RangeSourceSelection::caret(ordinary_position(0));

    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input
                .rebind(binding(source, 1), Some(origin), window, cx)
                .unwrap();
        })
    });
    drive_pages_with_objects(&input, cx, source, &facts);
    input.read_with(cx, |input, _| {
        assert_eq!(input.surface().unwrap().binding(), binding(source, 1));
        assert_eq!(input.surface().unwrap().selection(), origin);
        assert!(input.active_inline_object().is_none());
        assert!(input.is_quiescent());
    });
    assert_eq!(
        events
            .borrow()
            .iter()
            .filter(|event| matches!(
                event,
                RangeTextInputEvent::InlineObjectRealizationLost(loss)
                    if loss.anchor == active
                        && loss.reason
                            == gpui_text_input::InlineObjectRealizationLossReason::SelectionChanged
            ))
            .count(),
        1
    );
    assert!(events.borrow().iter().all(|event| !matches!(
        event,
        RangeTextInputEvent::InlineObjectRealizationLost(loss)
            if loss.anchor == active
                && loss.reason == gpui_text_input::InlineObjectRealizationLossReason::Superseded
    )));
}

#[gpui::test]
fn rejected_layout_replacements_preserve_active_coherent_surface_without_loss(
    cx: &mut gpui::TestAppContext,
) {
    cx.update(ensure_text_input_bindings);
    let source = "ab";
    let facts = [object_fact(902, 1, 10)];
    let base = config(source, 1);
    let mut replacement_layout = base.layout.clone();
    replacement_layout.wrap_width = px(96.);
    replacement_layout.limits.segment_bytes = 64 * 1024;
    let replacement_style = replacement_geometry_style();
    let mut probe = ExactGeometryOwner::new(
        base.binding,
        base.presentation_generation,
        base.layout,
        base.style,
        base.geometry_limits,
    )
    .unwrap();
    probe
        .start_index(gpui_text_input::GeometryJobId::new(1))
        .unwrap();
    let replacement_peak = probe
        .set_layout_required_bytes(&replacement_layout, &replacement_style)
        .unwrap();

    let mut constrained = config(source, 1);
    let limits = constrained.geometry_limits;
    constrained.geometry_limits = ExactGeometryLimits::new(
        limits.max_page_bytes(),
        limits.max_checkpoints(),
        replacement_peak - 1,
        limits.max_retained_items(),
    )
    .unwrap();
    let (input, cx) = cx.add_window_view(|window, cx| {
        let input = RangeTextInput::new(constrained, window, cx).unwrap();
        input.focus(window);
        input
    });
    drive_pages_with_objects(&input, cx, source, &facts);
    let events = restoration_events(&input, cx);
    let object = input.read_with(cx, |input, _| {
        input.surface().unwrap().realized_objects()[0]
    });
    cx.simulate_event(MouseDownEvent {
        position: object.hit_bounds().origin + point(px(1.), px(1.)),
        modifiers: Modifiers::none(),
        button: MouseButton::Left,
        click_count: 1,
        first_mouse: false,
    });
    drive_pages_with_objects(&input, cx, source, &facts);
    let (active, geometry, selection, charge, admission) = input.read_with(cx, |input, _| {
        (
            input.active_inline_object().unwrap(),
            input.surface().unwrap().geometry_key(),
            input.surface().unwrap().selection(),
            input.surface().unwrap().charge(),
            input.last_surface_admission_charge(),
        )
    });
    let event_count = events.borrow().len();

    let mut invalid_layout = replacement_layout.clone();
    invalid_layout.limits.fragments = 0;
    assert!(matches!(
        input.update(cx, |input, cx| input.set_layout(
            invalid_layout,
            replacement_style.clone(),
            cx
        )),
        Err(gpui_text_input::RangeTextInputError::Geometry(
            gpui_text_input::ExactGeometryError::InvalidLimits
        ))
    ));
    assert!(matches!(
        input.update(cx, |input, cx| input.set_layout(
            replacement_layout,
            replacement_style,
            cx
        )),
        Err(gpui_text_input::RangeTextInputError::Geometry(
            gpui_text_input::ExactGeometryError::CapacityExceeded
        ))
    ));

    input.read_with(cx, |input, _| {
        let surface = input.surface().unwrap();
        assert_eq!(input.active_inline_object(), Some(active));
        assert_eq!(surface.geometry_key(), geometry);
        assert_eq!(surface.selection(), selection);
        assert_eq!(surface.charge(), charge);
        assert_eq!(input.last_surface_admission_charge(), admission);
        assert_eq!(surface.realized_objects()[0].id(), InlineObjectId::new(902));
        assert_eq!(
            input
                .realization_diagnostics()
                .current
                .pending_layout_intents,
            1
        );
    });
    assert_normal_clipboard_blocked_without_custody(&input, cx);
    assert_eq!(events.borrow().len(), event_count);
    assert!(
        events
            .borrow()
            .iter()
            .all(|event| !matches!(event, RangeTextInputEvent::InlineObjectRealizationLost(_)))
    );
}

#[gpui::test]
fn successful_layout_replacement_loses_active_object_exactly_once(cx: &mut gpui::TestAppContext) {
    cx.update(ensure_text_input_bindings);
    let source = "ab";
    let facts = [object_fact(903, 1, 10)];
    let initial = config(source, 1);
    let mut replacement_layout = initial.layout.clone();
    replacement_layout.wrap_width = px(96.);
    let replacement_style = initial.style.clone();
    let (input, cx) = cx.add_window_view(|window, cx| {
        let input = RangeTextInput::new(initial, window, cx).unwrap();
        input.focus(window);
        input
    });
    drive_pages_with_objects(&input, cx, source, &facts);
    let events = restoration_events(&input, cx);
    let object = input.read_with(cx, |input, _| {
        input.surface().unwrap().realized_objects()[0]
    });
    cx.simulate_event(MouseDownEvent {
        position: object.hit_bounds().origin + point(px(1.), px(1.)),
        modifiers: Modifiers::none(),
        button: MouseButton::Left,
        click_count: 1,
        first_mouse: false,
    });
    drive_pages_with_objects(&input, cx, source, &facts);
    let (active, old_geometry) = input.read_with(cx, |input, _| {
        (
            input.active_inline_object().unwrap(),
            input.surface().unwrap().geometry_key(),
        )
    });
    let attached = input
        .update(cx, |input, _| {
            input.attach_active_inline_object_surface(active)
        })
        .unwrap();

    input
        .update(cx, |input, cx| {
            input.set_layout(replacement_layout, replacement_style, cx)
        })
        .unwrap();
    input.read_with(cx, |input, _| {
        assert!(input.active_inline_object().is_none());
        assert_eq!(input.surface().unwrap().geometry_key(), old_geometry);
    });
    assert_eq!(
        events
            .borrow()
            .iter()
            .filter(|event| matches!(
                event,
                RangeTextInputEvent::InlineObjectRealizationLost(loss)
                    if loss.anchor == active
                        && loss.reason
                            == gpui_text_input::InlineObjectRealizationLossReason::Superseded
            ))
            .count(),
        1
    );
    assert!(matches!(
        cx.update(|window, app| input.update(app, |input, cx| input
            .dismiss_active_inline_object_surface(
                attached,
                InlineObjectSurfaceDismissal::RefocusObject,
                window,
                cx,
            ))),
        Err(gpui_text_input::RangeTextInputError::Stale)
    ));

    drive_pages_with_objects(&input, cx, source, &facts);
    input.read_with(cx, |input, _| {
        assert_ne!(input.surface().unwrap().geometry_key(), old_geometry);
        assert!(input.active_inline_object().is_none());
        assert!(input.is_quiescent());
    });
    assert_eq!(
        events
            .borrow()
            .iter()
            .filter(|event| matches!(
                event,
                RangeTextInputEvent::InlineObjectRealizationLost(loss)
                    if loss.anchor == active
                        && loss.reason
                            == gpui_text_input::InlineObjectRealizationLossReason::Superseded
            ))
            .count(),
        1
    );
}

#[gpui::test]
fn rejected_presentation_replacement_preserves_active_coherent_surface_without_loss(
    cx: &mut gpui::TestAppContext,
) {
    cx.update(ensure_text_input_bindings);
    let source = "ab";
    let facts = [object_fact(904, 1, 10)];
    let (input, cx) = cx.add_window_view(|window, cx| {
        let input =
            RangeTextInput::new(one_under_geometry_replacement_config(source), window, cx).unwrap();
        input.focus(window);
        input
    });
    drive_pages_with_objects(&input, cx, source, &facts);
    let events = restoration_events(&input, cx);
    let object = input.read_with(cx, |input, _| {
        input.surface().unwrap().realized_objects()[0]
    });
    cx.simulate_event(MouseDownEvent {
        position: object.hit_bounds().origin + point(px(1.), px(1.)),
        modifiers: Modifiers::none(),
        button: MouseButton::Left,
        click_count: 1,
        first_mouse: false,
    });
    drive_pages_with_objects(&input, cx, source, &facts);
    let (active, geometry, selection) = input.read_with(cx, |input, _| {
        (
            input.active_inline_object().unwrap(),
            input.surface().unwrap().geometry_key(),
            input.surface().unwrap().selection(),
        )
    });

    assert!(matches!(
        input.update(cx, |input, cx| input
            .set_presentation_generation(PresentationGeneration::new(2), cx)),
        Err(gpui_text_input::RangeTextInputError::Geometry(
            gpui_text_input::ExactGeometryError::CapacityExceeded
        ))
    ));
    input.read_with(cx, |input, _| {
        let surface = input.surface().unwrap();
        assert_eq!(input.active_inline_object(), Some(active));
        assert_eq!(surface.geometry_key(), geometry);
        assert_eq!(surface.selection(), selection);
        assert_eq!(surface.realized_objects()[0].id(), InlineObjectId::new(904));
        assert_eq!(
            input
                .realization_diagnostics()
                .current
                .pending_presentation_intents,
            1
        );
    });
    assert_normal_clipboard_blocked_without_custody(&input, cx);
    assert!(
        events
            .borrow()
            .iter()
            .all(|event| !matches!(event, RangeTextInputEvent::InlineObjectRealizationLost(_)))
    );
}

#[gpui::test]
fn deferred_true_rebind_preserves_surface_and_blocks_normal_clipboard(
    cx: &mut gpui::TestAppContext,
) {
    cx.update(ensure_text_input_bindings);
    let source = "ab";
    let facts = [object_fact(905, 1, 10)];
    let (input, cx) = cx.add_window_view(|window, cx| {
        let input =
            RangeTextInput::new(one_under_geometry_replacement_config(source), window, cx).unwrap();
        input.focus(window);
        input
    });
    drive_pages_with_objects(&input, cx, source, &facts);
    let events = restoration_events(&input, cx);
    let object = input.read_with(cx, |input, _| {
        input.surface().unwrap().realized_objects()[0]
    });
    cx.simulate_event(MouseDownEvent {
        position: object.hit_bounds().origin + point(px(1.), px(1.)),
        modifiers: Modifiers::none(),
        button: MouseButton::Left,
        click_count: 1,
        first_mouse: false,
    });
    drive_pages_with_objects(&input, cx, source, &facts);
    let (active, geometry, selection) = input.read_with(cx, |input, _| {
        (
            input.active_inline_object().unwrap(),
            input.surface().unwrap().geometry_key(),
            input.surface().unwrap().selection(),
        )
    });

    let rebind = cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.rebind(binding(source, 2), None, window, cx)
        })
    });
    assert!(
        matches!(
            rebind,
            Err(gpui_text_input::RangeTextInputError::Geometry(
                gpui_text_input::ExactGeometryError::CapacityExceeded
            ))
        ),
        "unexpected deferred rebind result: {rebind:?}"
    );
    input.read_with(cx, |input, _| {
        let surface = input.surface().unwrap();
        assert_eq!(input.active_inline_object(), Some(active));
        assert_eq!(surface.geometry_key(), geometry);
        assert_eq!(surface.binding(), binding(source, 1));
        assert_eq!(surface.selection(), selection);
        assert_eq!(surface.realized_objects()[0].id(), InlineObjectId::new(905));
        assert_eq!(
            input
                .realization_diagnostics()
                .current
                .pending_rebind_intents,
            1
        );
    });
    input.update(cx, |input, cx| {
        for kind in [
            gpui_text_input::ClipboardKind::Copy,
            gpui_text_input::ClipboardKind::Cut,
        ] {
            assert!(matches!(
                input.begin_clipboard(kind, cx),
                Err(gpui_text_input::RangeTextInputError::Busy)
            ));
        }
        assert_eq!(input.clipboard_counts(), Default::default());
        assert!(input.take_request().is_none());
    });
    assert!(
        events
            .borrow()
            .iter()
            .all(|event| !matches!(event, RangeTextInputEvent::InlineObjectRealizationLost(_)))
    );
}

#[cfg(feature = "test-support")]
#[gpui::test]
fn repeated_wheel_retarget_rejection_preserves_full_publication_fingerprint(
    cx: &mut gpui::TestAppContext,
) {
    let source = (0..100)
        .map(|line| format!("line-{line:03}\n"))
        .collect::<String>();
    let mut configuration = config(&source, 1);
    configuration.limits.max_surface_bytes = 2 * 1024 * 1024;
    configuration.limits.max_surface_items = 2 * 1024 * 1024;
    configuration.limits.max_realization_work_per_frame = 1_024;
    let (input, cx) =
        cx.add_window_view(|window, cx| RangeTextInput::new(configuration, window, cx).unwrap());
    drive_pages(&input, cx, &source);
    let events = restoration_events(&input, cx);
    let initial = range_publication_fingerprint(&input, cx);
    let initial_ownership = input.read_with(cx, |input, _| input.realization_diagnostics().current);

    cx.simulate_event(ScrollWheelEvent {
        position: point(px(1.), px(1.)),
        delta: ScrollDelta::Pixels(point(px(0.), px(-48.))),
        ..Default::default()
    });
    let Some(RangeTextInputRequest::Page(retarget)) =
        input.update(cx, |input, _| input.take_request())
    else {
        panic!("wheel retarget request")
    };
    assert_eq!(retarget.key().purpose(), PagePurpose::GeometryTarget);
    assert!(matches!(
        retarget.key().demand(),
        PageDemandEnvelope::Adjacent {
            direction: PageDirection::Forward,
            ..
        }
    ));
    let admitted = range_publication_fingerprint(&input, cx);
    let admission = input
        .update(cx, |input, _| input.last_surface_admission_charge())
        .expect("wheel retarget admission charge");
    let ownership = input.read_with(cx, |input, _| input.realization_diagnostics().current);
    assert_ne!(admitted.admission, initial.admission);
    assert_eq!(admitted.admission, Some(admission));
    assert_eq!(
        ownership.dispatched_page_requests,
        initial_ownership.dispatched_page_requests + 1
    );
    assert_eq!(
        ownership.candidate_items,
        initial_ownership.candidate_items + 1
    );
    let exact_items =
        admission.items + (ownership.candidate_items - initial_ownership.candidate_items);
    assert_eq!(exact_items, admission.items + 1);

    input.update(cx, |input, cx| {
        input
            .fail_page(retarget.key(), PageFailure::Cancelled, cx)
            .unwrap();
    });
    assert_eq!(
        range_publication_fingerprint(&input, cx).surface,
        initial.surface
    );

    input.update(cx, |input, _| {
        input
            .lower_max_surface_items_for_test(NonZeroUsize::new(exact_items).unwrap())
            .unwrap();
    });
    let exact_before_ownership =
        input.read_with(cx, |input, _| input.realization_diagnostics().current);
    let events_after_exact_lowering = events.borrow().len();
    cx.simulate_event(ScrollWheelEvent {
        position: point(px(1.), px(1.)),
        delta: ScrollDelta::Pixels(point(px(0.), px(-48.))),
        ..Default::default()
    });
    let Some(RangeTextInputRequest::Page(exact_retarget)) =
        input.update(cx, |input, _| input.take_request())
    else {
        panic!("exact-fit wheel retarget request")
    };
    assert_eq!(exact_retarget.key().purpose(), PagePurpose::GeometryTarget);
    assert!(matches!(
        exact_retarget.key().demand(),
        PageDemandEnvelope::Adjacent {
            direction: PageDirection::Forward,
            ..
        }
    ));
    let exact_admitted = range_publication_fingerprint(&input, cx);
    let exact_admission = input
        .update(cx, |input, _| input.last_surface_admission_charge())
        .expect("exact-fit wheel retarget admission charge");
    let exact_ownership = input.read_with(cx, |input, _| input.realization_diagnostics().current);
    assert_eq!(exact_admitted.admission, Some(exact_admission));
    assert_eq!(exact_admission, admission);
    assert_eq!(
        exact_ownership.candidate_items,
        exact_before_ownership.candidate_items + 1
    );
    assert_eq!(
        exact_admission.items
            + (exact_ownership.candidate_items - exact_before_ownership.candidate_items),
        exact_items
    );
    assert_eq!(events.borrow().len(), events_after_exact_lowering);
    input.update(cx, |input, _| {
        assert!(matches!(
            input.lower_max_surface_items_for_test(NonZeroUsize::new(exact_items + 1).unwrap()),
            Err(gpui_text_input::RangeTextInputError::InvalidLimits)
        ));
    });
    assert_eq!(range_publication_fingerprint(&input, cx), exact_admitted);
    assert_eq!(events.borrow().len(), events_after_exact_lowering);

    let one_under = exact_items.checked_sub(1).expect("one-under capacity");
    input.update(cx, |input, _| {
        input
            .lower_max_surface_items_for_test(NonZeroUsize::new(one_under).unwrap())
            .unwrap();
    });
    let before = range_publication_fingerprint(&input, cx);
    let event_count = events.borrow().len();
    for delta in [-48., -96.] {
        cx.simulate_event(ScrollWheelEvent {
            position: point(px(1.), px(1.)),
            delta: ScrollDelta::Pixels(point(px(0.), px(delta))),
            ..Default::default()
        });
        assert!(input.update(cx, |input, _| input.take_request()).is_none());
        assert_eq!(range_publication_fingerprint(&input, cx), before);
        assert_eq!(events.borrow().len(), event_count);
    }
}

#[cfg(feature = "test-support")]
#[gpui::test]
fn repeated_rendered_scrollbar_retarget_rejection_preserves_full_publication_fingerprint(
    cx: &mut gpui::TestAppContext,
) {
    let source = (0..100)
        .map(|line| format!("line-{line:03}\n"))
        .collect::<String>();
    let mut configuration = config(&source, 1);
    configuration.limits.max_surface_bytes = 2 * 1024 * 1024;
    configuration.limits.max_surface_items = 2 * 1024 * 1024;
    configuration.limits.max_realization_work_per_frame = 1_024;
    let (input, cx) =
        cx.add_window_view(|window, cx| RangeTextInput::new(configuration, window, cx).unwrap());
    drive_pages(&input, cx, &source);
    let events = restoration_events(&input, cx);
    cx.update(|window, app| window.draw(app).clear());
    cx.run_until_parked();
    let viewport = cx.update(|window, _| window.viewport_size());
    let initial = range_publication_fingerprint(&input, cx);
    let initial_ownership = input.read_with(cx, |input, _| input.realization_diagnostics().current);
    cx.simulate_event(MouseDownEvent {
        position: point(viewport.width - px(1.), viewport.height * 0.9),
        modifiers: Modifiers::none(),
        button: MouseButton::Left,
        click_count: 1,
        first_mouse: false,
    });
    let Some(RangeTextInputRequest::Page(retarget)) =
        input.update(cx, |input, _| input.take_request())
    else {
        panic!("rendered scrollbar retarget request")
    };
    assert_eq!(retarget.key().purpose(), PagePurpose::GeometryTarget);
    assert!(matches!(
        retarget.key().demand(),
        PageDemandEnvelope::Adjacent {
            direction: PageDirection::Forward,
            ..
        }
    ));
    let admitted = range_publication_fingerprint(&input, cx);
    let admission = input
        .update(cx, |input, _| input.last_surface_admission_charge())
        .expect("rendered scrollbar retarget admission charge");
    let ownership = input.read_with(cx, |input, _| input.realization_diagnostics().current);
    assert_ne!(admitted.admission, initial.admission);
    assert_eq!(admitted.admission, Some(admission));
    assert_eq!(
        ownership.dispatched_page_requests,
        initial_ownership.dispatched_page_requests + 1
    );
    assert_eq!(
        ownership.candidate_items,
        initial_ownership.candidate_items + 1
    );
    let exact_items =
        admission.items + (ownership.candidate_items - initial_ownership.candidate_items);
    assert_eq!(exact_items, admission.items + 1);

    input.update(cx, |input, cx| {
        input
            .fail_page(retarget.key(), PageFailure::Cancelled, cx)
            .unwrap();
    });
    assert_eq!(
        range_publication_fingerprint(&input, cx).surface,
        initial.surface
    );

    input.update(cx, |input, _| {
        input
            .lower_max_surface_items_for_test(NonZeroUsize::new(exact_items).unwrap())
            .unwrap();
    });
    cx.update(|window, app| window.draw(app).clear());
    cx.run_until_parked();
    let viewport = cx.update(|window, _| window.viewport_size());
    let exact_before_ownership =
        input.read_with(cx, |input, _| input.realization_diagnostics().current);
    let events_after_exact_lowering = events.borrow().len();
    cx.simulate_event(MouseDownEvent {
        position: point(viewport.width - px(1.), viewport.height * 0.9),
        modifiers: Modifiers::none(),
        button: MouseButton::Left,
        click_count: 1,
        first_mouse: false,
    });
    let Some(RangeTextInputRequest::Page(exact_retarget)) =
        input.update(cx, |input, _| input.take_request())
    else {
        panic!("exact-fit rendered scrollbar retarget request")
    };
    assert_eq!(exact_retarget.key().purpose(), PagePurpose::GeometryTarget);
    assert!(matches!(
        exact_retarget.key().demand(),
        PageDemandEnvelope::Adjacent {
            direction: PageDirection::Forward,
            ..
        }
    ));
    let exact_admitted = range_publication_fingerprint(&input, cx);
    let exact_admission = input
        .update(cx, |input, _| input.last_surface_admission_charge())
        .expect("exact-fit rendered scrollbar retarget admission charge");
    let exact_ownership = input.read_with(cx, |input, _| input.realization_diagnostics().current);
    assert_eq!(exact_admitted.admission, Some(exact_admission));
    assert_eq!(exact_admission, admission);
    assert_eq!(
        exact_ownership.candidate_items,
        exact_before_ownership.candidate_items + 1
    );
    assert_eq!(
        exact_admission.items
            + (exact_ownership.candidate_items - exact_before_ownership.candidate_items),
        exact_items
    );
    assert_eq!(events.borrow().len(), events_after_exact_lowering);
    input.update(cx, |input, _| {
        assert!(matches!(
            input.lower_max_surface_items_for_test(NonZeroUsize::new(exact_items + 1).unwrap()),
            Err(gpui_text_input::RangeTextInputError::InvalidLimits)
        ));
    });
    assert_eq!(range_publication_fingerprint(&input, cx), exact_admitted);
    assert_eq!(events.borrow().len(), events_after_exact_lowering);

    let one_under = exact_items.checked_sub(1).expect("one-under capacity");
    input.update(cx, |input, _| {
        input
            .lower_max_surface_items_for_test(NonZeroUsize::new(one_under).unwrap())
            .unwrap();
    });
    cx.update(|window, app| window.draw(app).clear());
    cx.run_until_parked();
    let viewport = cx.update(|window, _| window.viewport_size());
    let before = range_publication_fingerprint(&input, cx);
    let event_count = events.borrow().len();
    cx.simulate_event(MouseDownEvent {
        position: point(viewport.width - px(1.), viewport.height * 0.9),
        modifiers: Modifiers::none(),
        button: MouseButton::Left,
        click_count: 1,
        first_mouse: false,
    });
    assert!(input.update(cx, |input, _| input.take_request()).is_none());
    assert_eq!(range_publication_fingerprint(&input, cx), before);
    assert_eq!(events.borrow().len(), event_count);
    for fraction in [0.75, 0.6] {
        cx.simulate_event(MouseDownEvent {
            position: point(viewport.width - px(1.), viewport.height * fraction),
            modifiers: Modifiers::none(),
            button: MouseButton::Left,
            click_count: 1,
            first_mouse: false,
        });
        assert!(input.update(cx, |input, _| input.take_request()).is_none());
        assert_eq!(range_publication_fingerprint(&input, cx), before);
        assert_eq!(events.borrow().len(), event_count);
    }
}
