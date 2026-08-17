use std::{cell::RefCell, rc::Rc, sync::Arc};

use gpui::{
    EntityInputHandler, Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, ScrollDelta,
    ScrollWheelEvent, SharedString, StreamingLayoutBinding, StreamingLayoutFragment,
    StreamingLayoutLimits, StreamingLayoutPosition, TextRun, black, font, point, px,
};
use gpui_scrollbar::ScrollbarStyle;
use gpui_text_input::{
    AtomFact, AtomId, BindingId, ByteOffset, ByteRange, ClipboardLimits, ClipboardWriteOutcome,
    ExactGeometryLimits, ExactGeometryOwner, InlineObjectFact, InlineObjectGap, InlineObjectId,
    InlineObjectNeighbor, InlineObjectOrder, InlineObjectPresentation, LogicalExtent,
    MutationFragment, MutationFragmentPayload, MutationKind, MutationLimits, MutationOutcome,
    MutationPositions, MutationProposal, ObjectDemand, ObjectDemandEnvelope, ObjectDirection,
    ObjectPage, ObjectPageEdgeFact, ObjectPageId, ObjectPurpose, ObjectRequestId, ObjectResidency,
    ObjectResidencyLimits, PageDemand, PageDemandEnvelope, PageDirection, PageEdgeFact,
    PageFailure, PageId, PagePurpose, PageRequestId, PlatformRangeResult, PresentationGeneration,
    RangeBinding, RangeHistoryPlan, RangePage, RangeResidency, RangeRestorationScrollAnchor,
    RangeRestorationSeed, RangeSelection, RangeSourceSelection, RangeTextInput,
    RangeTextInputConfig, RangeTextInputEvent, RangeTextInputLimits, RangeTextInputRequest,
    ResidencyLimits, SegmentationLimits, SourcePosition, SourceRange, SourceRevision,
    StreamingGeometryStyle, StreamingOversizePresentation, TextInputTheme,
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

fn ordinary_range(range: ByteRange) -> SourceRange {
    SourceRange::new(
        ordinary_position(range.start().get()),
        ordinary_position(range.end().get()),
    )
    .unwrap()
}

fn terminal(offset: u64) -> MutationFragmentPayload {
    MutationFragmentPayload::Terminal {
        intended: MutationPositions::collapsed(ordinary_position(offset)),
    }
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

fn bounded_mutation_stream(
    source: &str,
    fragment_count: usize,
) -> (
    MutationProposal,
    Vec<MutationFragment>,
    RangeResidency,
    ObjectResidency,
    usize,
) {
    assert!(fragment_count >= 2);
    let current = binding(source, 1);
    let key = gpui_text_input::MutationKey::new(
        current.binding(),
        current.revision(),
        gpui_text_input::OperationId::new(98_000),
    );
    let proposal = MutationProposal::new(
        key,
        MutationKind::Edit,
        ordinary_range(ByteRange::from_u64(0, 0).unwrap()),
        0,
    );
    let mut fragments = Vec::with_capacity(fragment_count);
    for ordinal in 0..fragment_count - 1 {
        fragments.push(MutationFragment::new(
            key,
            ordinal,
            MutationFragmentPayload::Utf8 {
                inserted_offset: ordinal as u64,
                text: "x".into(),
            },
        ));
    }
    fragments.push(MutationFragment::new(
        key,
        fragment_count - 1,
        terminal((fragment_count - 1) as u64),
    ));
    let (text, objects) = admitted_sources(source, 1, &[ordinary_position(0)]);
    (proposal, fragments, text, objects, fragment_count + 2)
}

fn retain_mutation_request_queue_capacity(
    input: &gpui::Entity<RangeTextInput>,
    cx: &mut gpui::VisualTestContext,
    source: &str,
) {
    let (proposal, fragments, text, objects, request_count) = bounded_mutation_stream(source, 128);
    input.update(cx, |input, cx| {
        input
            .propose_host_mutation(proposal, fragments, &text, &objects, cx)
            .unwrap();
    });
    let key = proposal.key();
    for _ in 0..request_count {
        input
            .update(cx, |input, _| input.take_request())
            .expect("bounded host mutation request");
    }
    assert!(input.update(cx, |input, _| input.take_request()).is_none());
    input.update(cx, |input, cx| {
        input.reject_mutation_staging(key, cx).unwrap();
    });
}

fn settle_ordinary_commit(
    input: &mut RangeTextInput,
    key: gpui_text_input::MutationKey,
    source: &str,
    revision: u64,
    offset: u64,
    window: &mut gpui::Window,
    cx: &mut gpui::Context<RangeTextInput>,
) -> Result<gpui_text_input::MutationSettlement, gpui_text_input::RangeTextInputError> {
    let positions = MutationPositions::collapsed(ordinary_position(offset));
    let (text, objects) = admitted_sources(source, revision, &[positions.caret()]);
    input.settle_committed_mutation(
        key,
        binding(source, revision),
        positions,
        &text,
        &objects,
        window,
        cx,
    )
}

fn submit_admitted_history_plan(
    input: &mut RangeTextInput,
    plan: RangeHistoryPlan,
    source: &str,
    revision: u64,
    cx: &mut gpui::Context<RangeTextInput>,
) -> Result<gpui_text_input::MutationKey, gpui_text_input::RangeTextInputError> {
    let replacement = plan.proposal().replacement();
    let positions = if replacement.is_empty() {
        vec![replacement.start()]
    } else {
        vec![replacement.start(), replacement.end()]
    };
    let (text, objects) = admitted_sources(source, revision, &positions);
    input.submit_history_plan(plan, &positions, &text, &objects, cx)
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
        ordinary_range(ByteRange::from_u64(0, original.len() as u64).unwrap()),
        0,
    );
    input.update(cx, |input, cx| {
        submit_admitted_history_plan(
            input,
            RangeHistoryPlan::new(
                intent,
                proposal,
                MutationPositions::collapsed(ordinary_position(undone.len() as u64)),
            ),
            original,
            1,
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
                MutationFragment::new(intent.key(), 1, terminal(undone.len() as u64)),
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
    input.update(cx, |input, cx| {
        assert!(matches!(
            input.stage_history_fragment(
                MutationFragment::new(intent.key(), 2, terminal(undone.len() as u64)),
                cx,
            ),
            Err(gpui_text_input::RangeTextInputError::Mutation(
                gpui_text_input::MutationError::WrongState {
                    expected: gpui_text_input::MutationState::Staging,
                    actual: gpui_text_input::MutationState::CommitPending,
                }
            ))
        ));
    });
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            settle_ordinary_commit(
                input,
                intent.key(),
                undone,
                2,
                undone.len() as u64,
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
            input.surface().unwrap().platform_selection().unwrap(),
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
        ordinary_range(ByteRange::from_u64(0, undone.len() as u64).unwrap()),
        0,
    );
    input.update(cx, |input, cx| {
        submit_admitted_history_plan(
            input,
            RangeHistoryPlan::new(
                redo,
                redo_proposal,
                MutationPositions::collapsed(ordinary_position(original.len() as u64)),
            ),
            undone,
            2,
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
                MutationFragment::new(redo.key(), 1, terminal(original.len() as u64)),
                cx,
            )
            .unwrap();
        input.admit_mutation_commit(redo.key()).unwrap();
    });
    let _ = drive_pages(&input, cx, undone);
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            settle_ordinary_commit(
                input,
                redo.key(),
                original,
                3,
                original.len() as u64,
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
    input.update(cx, |input, _| {
        admit_ordinary_edit_positions(input, &source, 1, &[150, 160]);
    });

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
        ordinary_range(ByteRange::from_u64(150, 160).unwrap())
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
            settle_ordinary_commit(input, proposal.key(), &successor, 2, 156, window, cx).unwrap();
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
            surface.platform_selection().unwrap(),
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
        admit_ordinary_edit_positions(input, source, 1, &[0]);
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
    input.update(cx, |input, _| {
        admit_ordinary_edit_positions(input, source, 1, &[0]);
    });
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
            let settlement =
                settle_ordinary_commit(input, proposal.key(), "!base", 2, 1, window, cx).unwrap();
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
    input.update(cx, |input, _| {
        admit_ordinary_edit_positions(input, source, 1, &[0]);
    });

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

    input.update(cx, |input, _| {
        admit_ordinary_edit_positions(input, rebound, 2, &[0]);
    });
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
        assert!(input.surface().unwrap().scroll_block() >= px(224.));
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
        limits: RangeTextInputLimits::new(2 * 1024 * 1024, 32768, 32, 32, px(16.), 4).unwrap(),
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
    let objects = facts
        .iter()
        .filter(|fact| demand.contains_anchor(fact.anchor()))
        .filter(|fact| demand.cursor().is_none_or(|cursor| fact.cursor() > cursor))
        .take(demand.max_objects())
        .cloned()
        .collect::<Vec<_>>();
    ObjectPage::new(
        ObjectPageId::new(id),
        request.key(),
        objects,
        demand.cursor().map_or(
            ObjectPageEdgeFact::EnvelopeBoundary,
            ObjectPageEdgeFact::Continues,
        ),
        ObjectPageEdgeFact::EnvelopeBoundary,
        true,
        None,
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
                            .unwrap()
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
            Some(RangeTextInputRequest::ReleasePage(_))
            | Some(RangeTextInputRequest::CancelPage(_))
            | Some(RangeTextInputRequest::ReleaseObjectPage(_))
            | Some(RangeTextInputRequest::CancelObjectPage(_)) => {}
            Some(request) => other.push(request),
            None => break,
        }
    }
    other
}

fn drive_pages_with_objects(
    input: &gpui::Entity<RangeTextInput>,
    cx: &mut gpui::VisualTestContext,
    source: &str,
    facts: &[InlineObjectFact],
) {
    let mut page_id = 91_000;
    for _ in 0..512 {
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
                let page = restoration_object_page(request, facts, page_id);
                page_id += 1;
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
            Some(request) => panic!("unexpected object geometry request: {request:?}"),
        }
    }
}

fn accept_and_collect_mutation(
    input: &gpui::Entity<RangeTextInput>,
    cx: &mut gpui::VisualTestContext,
    key: gpui_text_input::MutationKey,
) -> Vec<RangeTextInputRequest> {
    input.update(cx, |input, cx| {
        input.accept_mutation_preflight(key, cx).unwrap()
    });
    let mut requests = Vec::new();
    while let Some(request) = input.update(cx, |input, _| input.take_request()) {
        requests.push(request);
    }
    requests
}

fn drive_clipboard_write_with_objects(
    input: &gpui::Entity<RangeTextInput>,
    cx: &mut gpui::VisualTestContext,
    source: &str,
    facts: &[InlineObjectFact],
) -> gpui_text_input::ClipboardWriteRequest {
    let mut page_id = 93_000;
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
                let page = restoration_object_page(request, facts, page_id);
                page_id += 1;
                input.update(cx, |input, cx| input.deliver_object_page(page, cx).unwrap());
            }
            Some(RangeTextInputRequest::ClipboardWrite(write)) => return write,
            Some(RangeTextInputRequest::ReleasePage(_))
            | Some(RangeTextInputRequest::CancelPage(_))
            | Some(RangeTextInputRequest::ReleaseObjectPage(_))
            | Some(RangeTextInputRequest::CancelObjectPage(_)) => {}
            Some(request) => panic!("unexpected clipboard request: {request:?}"),
            None => break,
        }
    }
    panic!("clipboard write")
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
        assert_eq!(surface.caret().byte_offset.get(), 0);
        assert!(!surface.fragments().is_empty());
    });

    input.update(cx, |input, _| {
        admit_ordinary_edit_positions(input, source, 1, &[0]);
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
    input.update(cx, |input, _| {
        admit_ordinary_edit_positions(input, source, 1, &[0]);
    });
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
            input
                .surface()
                .unwrap()
                .platform_selection()
                .unwrap()
                .range(),
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
    limited.update(cx, |input, _| {
        admit_ordinary_edit_positions(input, source, 2, &[0]);
    });
    cx.simulate_input("!");
    assert!(drive_pages(&limited, cx, source).is_empty());
    limited.read_with(cx, |input, _| {
        assert!(input.is_quiescent());
        assert_eq!(input.surface().unwrap().binding(), binding(source, 2));
    });

    let mut scalar_limited = config(source, 3);
    scalar_limited.mutation_limits = MutationLimits::new(2, 1).unwrap();
    let (scalar_limited, cx) = cx.add_window_view(|window, cx| {
        let input = RangeTextInput::new(scalar_limited, window, cx).unwrap();
        input.focus(window);
        input
    });
    assert!(drive_pages(&scalar_limited, cx, source).is_empty());
    scalar_limited.update(cx, |input, _| {
        admit_ordinary_edit_positions(input, source, 3, &[0]);
    });
    cx.simulate_input("é");
    let preflight = drive_pages(&scalar_limited, cx, source)
        .into_iter()
        .find_map(|request| match request {
            RangeTextInputRequest::MutationPreflight(proposal) => Some(proposal),
            _ => None,
        })
        .unwrap();
    scalar_limited.update(cx, |input, cx| {
        assert!(matches!(
            input.accept_mutation_preflight(preflight.key(), cx),
            Err(gpui_text_input::RangeTextInputError::SurfaceCapacity)
        ));
        assert!(matches!(
            input.admit_mutation_commit(preflight.key()),
            Err(gpui_text_input::RangeTextInputError::Mutation(
                gpui_text_input::MutationError::ObsoleteOperation(key)
            )) if key == preflight.key()
        ));
        assert!(!input.is_quiescent());
        assert!(matches!(
            input.take_request(),
            Some(RangeTextInputRequest::CancelMutation(key)) if key == preflight.key()
        ));
        assert!(input.take_request().is_none());
        assert!(input.is_quiescent());
        assert_eq!(input.surface().unwrap().binding(), binding(source, 3));
    });
    let drained =
        cx.update(|window, app| scalar_limited.update(app, |input, cx| input.dispose(window, cx)));
    assert!(!drained.iter().any(
        |request| matches!(request, RangeTextInputRequest::CancelMutation(key) if *key == preflight.key())
    ));
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
        caret: after,
        selection: RangeSourceSelection {
            anchor: before,
            head: after,
        },
        scroll: RangeRestorationScrollAnchor {
            position: between,
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

    cx.simulate_keystrokes("ctrl-a");
    let (requests, cancellations) =
        drive_pages_observing_cancel(&input, cx, source, restoration_geometry.key());
    assert!(requests.is_empty());
    assert_eq!(cancellations, 1);
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
fn post_validation_host_scroll_rejects_restoration_once_and_can_retry(
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

    input.update(cx, |input, cx| {
        input.request_absolute_scroll(px(96.), cx).unwrap()
    });
    let (requests, cancellations) =
        drive_pages_observing_cancel(&input, cx, source, restoration_geometry.key());
    assert!(requests.is_empty());
    assert_eq!(cancellations, 1);
    input.read_with(cx, |input, _| {
        assert!(input.surface().unwrap().scroll_block() >= px(96.));
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
fn pre_validation_queued_scroll_retargets_without_host_cancellation_and_can_retry(
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
    input.update(cx, |input, cx| {
        input.request_absolute_scroll(px(96.), cx).unwrap()
    });

    let first = input.update(cx, |input, _| input.take_request()).unwrap();
    let RangeTextInputRequest::Page(first) = first else {
        panic!("queued validation is removed without host cancellation")
    };
    assert!(
        matches!(
            first.key().purpose(),
            PagePurpose::GeometryIndex | PagePurpose::GeometryTarget
        ),
        "unexpected first rebind page purpose: {:?}",
        first.key().purpose()
    );
    let page = page_for(source, 84_000, first);
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.deliver_page(page, window, cx).unwrap()
        })
    });
    assert!(drive_pages(&input, cx, source).is_empty());
    input.read_with(cx, |input, _| {
        assert!(input.surface().unwrap().scroll_block() >= px(96.));
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
}

#[gpui::test]
fn pre_validation_dispatched_text_and_object_scroll_cancel_exactly_and_can_retry(
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
    text_input.update(cx, |input, cx| {
        input.request_absolute_scroll(px(96.), cx).unwrap()
    });
    assert!(matches!(
        text_input.update(cx, |input, _| input.take_request()),
        Some(RangeTextInputRequest::CancelPage(key)) if key == text.key()
    ));
    let late = page_for(source, 84_100, text);
    assert!(matches!(
        cx.update(|window, app| {
            text_input.update(app, |input, cx| input.deliver_page(late, window, cx))
        }),
        Err(gpui_text_input::RangeTextInputError::Stale)
    ));
    assert!(drive_pages(&text_input, cx, source).is_empty());
    text_input.read_with(cx, |input, _| {
        assert!(input.surface().unwrap().scroll_block() >= px(96.));
        assert!(input.is_quiescent());
    });
    assert_eq!(
        text_events
            .borrow()
            .iter()
            .filter(|event| matches!(event, RangeTextInputEvent::RestorationRejected))
            .count(),
        1
    );
    text_input.update(cx, |input, cx| input.import_restoration(seed, cx).unwrap());
    assert!(drive_pages(&text_input, cx, source).is_empty());
    text_input.read_with(cx, |input, _| {
        assert_eq!(input.export_restoration(None).unwrap(), seed)
    });

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
    object_input.update(cx, |input, cx| {
        input.request_absolute_scroll(px(96.), cx).unwrap()
    });
    let mut exact_object_cancellations = 0;
    for _ in 0..4 {
        match object_input.update(cx, |input, _| input.take_request()) {
            Some(RangeTextInputRequest::CancelObjectPage(key)) if key == object.key() => {
                exact_object_cancellations += 1;
                break;
            }
            Some(RangeTextInputRequest::ReleasePage(_)) => {}
            other => panic!("unexpected object-validation cancellation ordering: {other:?}"),
        }
    }
    assert_eq!(exact_object_cancellations, 1);
    let late = restoration_object_page(object, &[], 84_101);
    assert!(matches!(
        object_input.update(cx, |input, cx| input.deliver_object_page(late, cx)),
        Err(gpui_text_input::RangeTextInputError::Stale)
    ));
    assert!(drive_pages(&object_input, cx, source).is_empty());
    object_input.read_with(cx, |input, _| {
        assert!(input.surface().unwrap().scroll_block() >= px(96.));
        assert!(input.is_quiescent());
    });
    assert_eq!(
        object_events
            .borrow()
            .iter()
            .filter(|event| matches!(event, RangeTextInputEvent::RestorationRejected))
            .count(),
        1
    );
    object_input.update(cx, |input, cx| input.import_restoration(seed, cx).unwrap());
    assert!(drive_pages(&object_input, cx, source).is_empty());
    object_input.read_with(cx, |input, _| {
        assert_eq!(input.export_restoration(None).unwrap(), seed)
    });
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
    assert!(matches!(
        cx.update(|window, app| {
            dispatched_rebind.update(app, |input, cx| input.deliver_page(late, window, cx))
        }),
        Err(gpui_text_input::RangeTextInputError::Stale)
    ));
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
        Err(gpui_text_input::RangeTextInputError::Stale)
    ));
    assert!(matches!(
        dispatched_dispose.update(cx, |input, _| input.take_request()),
        Some(RangeTextInputRequest::ReleasePage(key)) if key == validation.key()
    ));
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
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.rebind(binding(source, 2), None, window, cx).unwrap()
        })
    });
    let mut exact_cancellations = 0;
    let mut page_id = 73_000;
    for _ in 0..64 {
        match input.update(cx, |input, _| input.take_request()) {
            Some(RangeTextInputRequest::CancelPage(key)) if key == geometry.key() => {
                exact_cancellations += 1;
            }
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
                cx.update(|window, app| {
                    input.update(app, |input, cx| {
                        input
                            .deliver_object_page_in_window(page, window, cx)
                            .unwrap()
                    })
                });
            }
            Some(RangeTextInputRequest::ReleasePage(_))
            | Some(RangeTextInputRequest::ReleaseObjectPage(_))
            | Some(RangeTextInputRequest::CancelObjectPage(_)) => {}
            Some(other) => panic!("unexpected rebind request: {other:?}"),
            None => break,
        }
    }
    assert_eq!(exact_cancellations, 1);
    input.read_with(cx, |input, _| assert!(input.is_quiescent()));
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
    let drained =
        cx.update(|window, app| disposed.update(app, |input, cx| input.dispose(window, cx)));
    assert_eq!(
        drained
            .iter()
            .filter(|request| matches!(
                request,
                RangeTextInputRequest::CancelPage(key) if *key == geometry.key()
            ))
            .count(),
        1
    );
    disposed.read_with(cx, |input, _| {
        assert!(input.surface().is_none());
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
fn mounted_composite_cut_uses_exact_gap_proofs_before_staged_deletion(
    cx: &mut gpui::TestAppContext,
) {
    let source = "ab";
    let (input, cx) = cx
        .add_window_view(|window, cx| RangeTextInput::new(config(source, 1), window, cx).unwrap());
    assert!(drive_pages(&input, cx, source).is_empty());
    let object = object_neighbor(91, 10);
    let start = SourcePosition::new(ByteOffset::new(1), InlineObjectGap::before(object));
    let end = SourcePosition::new(ByteOffset::new(1), InlineObjectGap::after(object));
    let selection = SourceRange::new(start, end).unwrap();
    let facts = [object_fact(91, 1, 10)];
    let (text, objects) = admitted_sources_with_facts(source, 1, &[start, end], &facts);
    input.update(cx, |input, cx| {
        input
            .begin_composite_clipboard(
                gpui_text_input::ClipboardKind::Cut,
                selection,
                &text,
                &objects,
                cx,
            )
            .unwrap();
    });
    let request = input.update(cx, |input, _| input.take_request()).unwrap();
    let RangeTextInputRequest::ObjectPage(request) = request else {
        panic!("object page")
    };
    let page = restoration_object_page(request, &facts, 980);
    input.update(cx, |input, cx| input.deliver_object_page(page, cx).unwrap());
    let write = (0..3)
        .find_map(
            |_| match input.update(cx, |input, _| input.take_request()) {
                Some(RangeTextInputRequest::ClipboardWrite(write)) => Some(write),
                _ => None,
            },
        )
        .expect("complete exact value is written before deletion");
    assert_eq!(write.text(), "[91]");
    input.update(cx, |input, cx| {
        assert!(matches!(
            input
                .settle_clipboard_write(write.key(), ClipboardWriteOutcome::Written, cx)
                .unwrap(),
            gpui_text_input::ClipboardCompletion::Delete(deletion)
                if deletion.selection() == selection
        ));
    });
    let preflight = (0..3)
        .find_map(|_| {
            input
                .update(cx, |input, _| input.take_request())
                .filter(|request| matches!(request, RangeTextInputRequest::MutationPreflight(_)))
        })
        .unwrap();
    let RangeTextInputRequest::MutationPreflight(proposal) = preflight else {
        panic!("staged deletion preflight")
    };
    assert_eq!(proposal.replacement(), selection);
    let staged = accept_and_collect_mutation(&input, cx, proposal.key());
    let object_fragment = staged
        .iter()
        .position(|request| matches!(
            request,
            RangeTextInputRequest::MutationFragment { fragment, .. }
                if matches!(
                    fragment.payload(),
                    MutationFragmentPayload::Object(gpui_text_input::ObjectChange::Remove { target })
                        if target.range() == selection
                            && target.id() == InlineObjectId::new(91)
                            && target.order() == InlineObjectOrder::new(10)
                )
        ))
        .expect("cut stages the exact object removal");
    let terminal_fragment = staged
        .iter()
        .position(|request| {
            matches!(
                request,
                RangeTextInputRequest::MutationFragment { fragment, .. }
                    if matches!(fragment.payload(), MutationFragmentPayload::Terminal { .. })
            )
        })
        .expect("cut stages terminal positions");
    assert!(object_fragment < terminal_fragment);
    input.update(cx, |input, cx| {
        input.reject_mutation_staging(proposal.key(), cx).unwrap();
        assert_eq!(input.surface().unwrap().binding(), binding(source, 1));
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
    input.update(cx, |input, _| {
        admit_ordinary_edit_positions(input, &source, 1, &[150, 160]);
    });
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
        ordinary_range(ByteRange::from_u64(150, 160).unwrap())
    );
    input.read_with(cx, |input, _| {
        assert_eq!(input.surface().unwrap().binding(), binding(&source, 1))
    });
}

#[gpui::test]
fn queued_clipboard_write_is_dropped_locally_on_rebind_and_dispose(cx: &mut gpui::TestAppContext) {
    let (rebound, cx) =
        cx.add_window_view(|window, cx| RangeTextInput::new(config("", 1), window, cx).unwrap());
    assert!(drive_pages(&rebound, cx, "").is_empty());
    let key = queue_empty_clipboard_cut(&rebound, cx);
    cx.update(|window, app| {
        rebound.update(app, |input, cx| {
            input.rebind(binding("", 2), None, window, cx).unwrap()
        })
    });
    let requests = drive_pages(&rebound, cx, "");
    assert!(!requests.iter().any(|request| matches!(
        request,
        RangeTextInputRequest::ClipboardWrite(_)
            | RangeTextInputRequest::CancelClipboardWrite(_)
            | RangeTextInputRequest::MutationPreflight(_)
    )));
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
    let drained =
        cx.update(|window, app| disposed.update(app, |input, cx| input.dispose(window, cx)));
    assert!(!drained.iter().any(|request| matches!(
        request,
        RangeTextInputRequest::ClipboardWrite(_)
            | RangeTextInputRequest::CancelClipboardWrite(_)
            | RangeTextInputRequest::MutationPreflight(_)
    )));
    disposed.update(cx, |input, cx| {
        assert!(matches!(
            input.settle_clipboard_write(key, ClipboardWriteOutcome::Written, cx),
            Err(gpui_text_input::RangeTextInputError::Stale)
        ));
        assert!(input.is_quiescent());
    });
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
    input.update(cx, |input, _| {
        admit_ordinary_edit_positions(input, source, 1, &[0, source.len() as u64])
    });

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
            admit_ordinary_edit_positions(input, &source, 1, &[0, source.len() as u64]);
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
fn saturated_clipboard_text_demand_unwinds_exactly_and_immediate_retry_succeeds(
    cx: &mut gpui::TestAppContext,
) {
    let source = "abc";
    let mut configuration = config(source, 1);
    configuration.residency_limits = ResidencyLimits::new(8, 128 * 1024, 1, 256).unwrap();
    let (input, cx) =
        cx.add_window_view(|window, cx| RangeTextInput::new(configuration, window, cx).unwrap());
    let geometry = input
        .update(cx, |input, _| input.take_request())
        .and_then(|request| match request {
            RangeTextInputRequest::Page(page) => Some(page),
            _ => None,
        })
        .expect("initial geometry occupies the only pending residency slot");
    let start = ordinary_position(0);
    let end = ordinary_position(source.len() as u64);
    let selection = SourceRange::new(start, end).unwrap();
    let (text, objects) = admitted_sources(source, 1, &[start, end]);

    input.update(cx, |input, cx| {
        input
            .begin_composite_clipboard(
                gpui_text_input::ClipboardKind::Cut,
                selection,
                &text,
                &objects,
                cx,
            )
            .unwrap();
    });
    let object = input
        .update(cx, |input, _| input.take_request())
        .and_then(|request| match request {
            RangeTextInputRequest::ObjectPage(page) => Some(page),
            _ => None,
        })
        .expect("clipboard object phase starts before text demand");
    let page = restoration_object_page(object, &[], 82_000);
    assert!(matches!(
        input.update(cx, |input, cx| input.deliver_object_page(page, cx)),
        Err(gpui_text_input::RangeTextInputError::Busy)
    ));
    let failed_requests = (0..8)
        .filter_map(|_| input.update(cx, |input, _| input.take_request()))
        .collect::<Vec<_>>();
    assert!(
        failed_requests
            .iter()
            .all(|request| matches!(request, RangeTextInputRequest::ReleaseObjectPage(_)))
    );
    assert!(!failed_requests.iter().any(|request| matches!(
        request,
        RangeTextInputRequest::Page(_)
            | RangeTextInputRequest::CancelPage(_)
            | RangeTextInputRequest::ClipboardWrite(_)
            | RangeTextInputRequest::MutationPreflight(_)
    )));

    input.update(cx, |input, cx| {
        input
            .fail_page(geometry.key(), PageFailure::Unavailable, cx)
            .unwrap();
        assert!(input.is_quiescent());
        input
            .begin_composite_clipboard(
                gpui_text_input::ClipboardKind::Copy,
                selection,
                &text,
                &objects,
                cx,
            )
            .unwrap();
    });
    let object = input
        .update(cx, |input, _| input.take_request())
        .and_then(|request| match request {
            RangeTextInputRequest::ObjectPage(page) => Some(page),
            _ => None,
        })
        .expect("retry starts immediately after capacity is released");
    let page = restoration_object_page(object, &[], 82_001);
    input.update(cx, |input, cx| input.deliver_object_page(page, cx).unwrap());
    let text_page = input
        .update(cx, |input, _| input.take_request())
        .and_then(|request| match request {
            RangeTextInputRequest::Page(page) if page.key().purpose() == PagePurpose::Clipboard => {
                Some(page)
            }
            _ => None,
        })
        .expect("retry owns the newly available text residency slot");
    let page = page_for(source, 82_002, text_page);
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.deliver_page(page, window, cx).unwrap()
        })
    });
    let mut retry_lifecycle = Vec::new();
    let write = (0..4)
        .find_map(
            |_| match input.update(cx, |input, _| input.take_request()) {
                Some(RangeTextInputRequest::ClipboardWrite(write)) => Some(write),
                Some(request) => {
                    retry_lifecycle.push(request);
                    None
                }
                None => None,
            },
        )
        .expect("retry reaches the exact clipboard write");
    assert!(
        retry_lifecycle
            .iter()
            .all(|request| matches!(request, RangeTextInputRequest::ReleaseObjectPage(_)))
    );
    assert_eq!(write.text(), source);
    input.update(cx, |input, cx| {
        assert_eq!(
            input
                .settle_clipboard_write(write.key(), ClipboardWriteOutcome::Written, cx)
                .unwrap(),
            gpui_text_input::ClipboardCompletion::Copied
        );
    });
    let trailing = (0..8)
        .filter_map(|_| input.update(cx, |input, _| input.take_request()))
        .collect::<Vec<_>>();
    assert!(trailing.iter().all(|request| matches!(
        request,
        RangeTextInputRequest::ReleaseObjectPage(_) | RangeTextInputRequest::ReleasePage(_)
    )));
    input.read_with(cx, |input, _| assert!(input.is_quiescent()));
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
        ordinary_range(ByteRange::from_u64(0, source.len() as u64).unwrap()),
        0,
    );
    input.update(cx, |input, cx| {
        assert!(matches!(
            submit_admitted_history_plan(
                input,
                RangeHistoryPlan::new(
                    intent,
                    malformed,
                    MutationPositions::collapsed(ordinary_position(0)),
                ),
                source,
                1,
                cx,
            ),
            Err(gpui_text_input::RangeTextInputError::Stale)
        ));
    });
    let proposal = MutationProposal::new(
        intent.key(),
        MutationKind::Undo,
        ordinary_range(ByteRange::from_u64(0, source.len() as u64).unwrap()),
        0,
    );
    input.update(cx, |input, cx| {
        submit_admitted_history_plan(
            input,
            RangeHistoryPlan::new(
                intent,
                proposal,
                MutationPositions::collapsed(ordinary_position(0)),
            ),
            source,
            1,
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
        ordinary_range(ByteRange::from_u64(0, source.len() as u64).unwrap()),
        0,
    );
    input.update(cx, |input, cx| {
        submit_admitted_history_plan(
            input,
            RangeHistoryPlan::new(
                detached,
                proposal,
                MutationPositions::collapsed(ordinary_position(0)),
            ),
            source,
            1,
            cx,
        )
        .unwrap();
        input.accept_mutation_preflight(detached.key(), cx).unwrap();
        input
            .stage_history_fragment(MutationFragment::new(detached.key(), 0, terminal(0)), cx)
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
fn malformed_history_selection_requests_host_cancellation_before_quiescence(
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
        ordinary_range(ByteRange::from_u64(0, source.len() as u64).unwrap()),
        0,
    );
    input.update(cx, |input, cx| {
        submit_admitted_history_plan(
            input,
            RangeHistoryPlan::new(
                intent,
                proposal,
                MutationPositions::collapsed(ordinary_position(successor.len() as u64 + 1)),
            ),
            source,
            1,
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
    input.update(cx, |input, cx| {
        assert!(matches!(
            input.stage_history_fragment(
                MutationFragment::new(intent.key(), 1, terminal(successor.len() as u64 + 1),),
                cx,
            ),
            Err(gpui_text_input::RangeTextInputError::Contract(
                gpui_text_input::RangeContractError::ByteRangeOutsideExtent { byte_len: 2, .. }
            ))
        ));
        assert!(matches!(
            input.stage_history_fragment(
                MutationFragment::new(intent.key(), 1, terminal(successor.len() as u64)),
                cx,
            ),
            Err(gpui_text_input::RangeTextInputError::Stale)
        ));
        assert!(matches!(
            input.admit_mutation_commit(intent.key()),
            Err(gpui_text_input::RangeTextInputError::Mutation(
                gpui_text_input::MutationError::ObsoleteOperation(key)
            )) if key == intent.key()
        ));
        assert!(!input.is_quiescent());
        assert_eq!(input.surface().unwrap().binding(), binding(source, 1));
    });
    let drained = cx.update(|window, app| input.update(app, |input, cx| input.dispose(window, cx)));
    assert_eq!(
        drained
            .iter()
            .filter(|request| {
                matches!(request, RangeTextInputRequest::CancelMutation(key) if *key == intent.key())
            })
            .count(),
        1
    );
    assert!(!drained.iter().any(|request| {
        matches!(request, RangeTextInputRequest::MutationPreflight(proposal) if proposal.key() == intent.key())
            || matches!(request, RangeTextInputRequest::MutationFragment { key, .. }
                | RangeTextInputRequest::MutationCommit(key) if *key == intent.key())
    }));
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
        ordinary_range(ByteRange::from_u64(0, 2).unwrap()),
        0,
    );
    input.update(cx, |input, cx| {
        submit_admitted_history_plan(
            input,
            RangeHistoryPlan::new(
                intent,
                proposal,
                MutationPositions::collapsed(ordinary_position(0)),
            ),
            source,
            1,
            cx,
        )
        .unwrap();
    });
    let _ = drive_pages(&input, cx, source);
    input.update(cx, |input, cx| {
        input.accept_mutation_preflight(intent.key(), cx).unwrap();
        assert!(matches!(
            input.stage_history_fragment(MutationFragment::new(intent.key(), 0, terminal(0)), cx,),
            Err(gpui_text_input::RangeTextInputError::Mutation(
                gpui_text_input::MutationError::IncoherentSuccessor
            ))
        ));
        assert!(!input.is_quiescent());
        assert!(matches!(
            input.take_request(),
            Some(RangeTextInputRequest::CancelMutation(key)) if key == intent.key()
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
        ordinary_range(ByteRange::from_u64(0, source.len() as u64).unwrap()),
        0,
    );
    input.update(cx, |input, cx| {
        submit_admitted_history_plan(
            input,
            RangeHistoryPlan::new(
                intent,
                proposal,
                MutationPositions::collapsed(ordinary_position(successor.len() as u64)),
            ),
            source,
            1,
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
        let foreign_key = gpui_text_input::MutationKey::new(
            intent.key().binding(),
            intent.key().base_revision(),
            gpui_text_input::OperationId::new(99_001),
        );
        assert!(matches!(
            input.stage_history_fragment(
                MutationFragment::new(
                    foreign_key,
                    1,
                    MutationFragmentPayload::Utf8 {
                        inserted_offset: successor.len() as u64,
                        text: "foreign".to_owned(),
                    },
                ),
                cx,
            ),
            Err(gpui_text_input::RangeTextInputError::Mutation(
                gpui_text_input::MutationError::WrongKey { expected, actual }
            )) if expected == intent.key() && actual == foreign_key
        ));
        input
            .stage_history_fragment(
                MutationFragment::new(intent.key(), 1, terminal(successor.len() as u64)),
                cx,
            )
            .unwrap();
    });
    let staged = drive_pages(&input, cx, source);
    assert!(staged.iter().any(
        |request| matches!(request, RangeTextInputRequest::MutationCommit(key) if *key == intent.key())
    ));
    assert!(
        !staged
            .iter()
            .any(|request| matches!(request, RangeTextInputRequest::CancelMutation(_)))
    );
    input.update(cx, |input, _| {
        input.admit_mutation_commit(intent.key()).unwrap()
    });
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            settle_ordinary_commit(
                input,
                intent.key(),
                successor,
                2,
                successor.len() as u64,
                window,
                cx,
            )
            .unwrap();
        })
    });
    assert!(drive_pages(&input, cx, successor).is_empty());
    input.read_with(cx, |input, _| {
        assert_eq!(
            input.surface().unwrap().platform_selection().unwrap(),
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
    let RangeTextInputRequest::Page(request) =
        input.update(cx, |input, _| input.take_request()).unwrap()
    else {
        panic!("initial geometry-index text request")
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
        assert!(input.surface().is_none());
        assert!(input.is_quiescent());
    });

    assert!(matches!(
        cx.update(|window, app| {
            input.update(app, |input, cx| input.deliver_page(late, window, cx))
        }),
        Err(gpui_text_input::RangeTextInputError::Stale)
    ));
    assert!(matches!(
        input.update(cx, |input, _| input.take_request()),
        Some(RangeTextInputRequest::ReleasePage(key)) if key == request.key()
    ));
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
        Err(gpui_text_input::RangeTextInputError::Stale)
    ));
    assert!(matches!(
        input.update(cx, |input, _| input.take_request()),
        Some(RangeTextInputRequest::ReleasePage(key)) if key == target.key()
    ));
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
    let prior = input.read_with(cx, |input, _| input.surface().unwrap().geometry_key());
    input.update(cx, |input, cx| {
        input.set_layout(layout.clone(), style.clone(), cx).unwrap()
    });
    let request = loop {
        match input.update(cx, |input, _| input.take_request()).unwrap() {
            RangeTextInputRequest::Page(request) => break request,
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
        Err(gpui_text_input::RangeTextInputError::Stale)
    ));
    assert!(matches!(
        input.update(cx, |input, _| input.take_request()),
        Some(RangeTextInputRequest::ReleasePage(key)) if key == conflict_request.key()
    ));
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
        Err(gpui_text_input::RangeTextInputError::Stale)
    ));
    assert!(matches!(
        input.update(cx, |input, _| input.take_request()),
        Some(RangeTextInputRequest::ReleasePage(key)) if key == conflict_request.key()
    ));
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
        match input.update(cx, |input, _| input.take_request()).unwrap() {
            RangeTextInputRequest::Page(request) => {
                let page = page_for(source, 95_000, request);
                cx.update(|window, app| {
                    input.update(app, |input, cx| {
                        input.deliver_page(page, window, cx).unwrap()
                    })
                });
            }
            RangeTextInputRequest::ObjectPage(request) => break request,
            RangeTextInputRequest::ReleasePage(_) => {}
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
        Err(gpui_text_input::RangeTextInputError::Stale)
    ));
    assert!(matches!(
        input.update(cx, |input, _| input.take_request()),
        Some(RangeTextInputRequest::ReleaseObjectPage(key)) if key == request.key()
    ));
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
    let prior = input.read_with(cx, |input, _| input.surface().unwrap().geometry_key());
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
        match input.update(cx, |input, _| input.take_request()).unwrap() {
            RangeTextInputRequest::Page(request) => break request,
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
        Err(gpui_text_input::RangeTextInputError::Stale)
    ));
    assert!(matches!(
        input.update(cx, |input, _| input.take_request()),
        Some(RangeTextInputRequest::ReleaseObjectPage(key)) if key == conflict_request.key()
    ));
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

#[gpui::test]
fn host_proposal_uses_the_single_bounded_transaction_request_stream(cx: &mut gpui::TestAppContext) {
    cx.update(ensure_text_input_bindings);
    let source = "base";
    let (input, cx) = cx
        .add_window_view(|window, cx| RangeTextInput::new(config(source, 1), window, cx).unwrap());
    assert!(drive_pages(&input, cx, source).is_empty());
    let base = binding(source, 1);
    let key = gpui_text_input::MutationKey::new(
        base.binding(),
        base.revision(),
        gpui_text_input::OperationId::new(8_000),
    );
    let proposal = MutationProposal::new(
        key,
        MutationKind::Edit,
        ordinary_range(ByteRange::from_u64(0, 4).unwrap()),
        0,
    );
    let fragments = vec![
        MutationFragment::new(
            key,
            0,
            MutationFragmentPayload::Utf8 {
                inserted_offset: 0,
                text: "next".into(),
            },
        ),
        MutationFragment::new(key, 1, terminal(4)),
    ];
    let proposal_positions = [ordinary_position(0), ordinary_position(4)];
    let (proposal_text, proposal_objects) = admitted_sources(source, 1, &proposal_positions);
    let one_position = [ordinary_position(0)];
    let (one_text, one_objects) = admitted_sources(source, 1, &one_position);
    input.update(cx, |input, cx| {
        assert_eq!(
            input
                .propose_host_mutation(
                    proposal,
                    fragments.clone(),
                    &proposal_text,
                    &proposal_objects,
                    cx,
                )
                .unwrap(),
            key
        );
        let second = MutationProposal::new(
            gpui_text_input::MutationKey::new(
                base.binding(),
                base.revision(),
                gpui_text_input::OperationId::new(8_001),
            ),
            MutationKind::Edit,
            ordinary_range(ByteRange::from_u64(0, 0).unwrap()),
            0,
        );
        assert!(matches!(
            input.propose_host_mutation(
                second,
                vec![MutationFragment::new(second.key(), 0, terminal(0),)],
                &one_text,
                &one_objects,
                cx
            ),
            Err(gpui_text_input::RangeTextInputError::Mutation(
                gpui_text_input::MutationError::Busy(_)
            ))
        ));
    });
    let requests = drive_pages(&input, cx, source);
    assert!(matches!(
        requests.as_slice(),
        [
            RangeTextInputRequest::MutationPreflight(_),
            RangeTextInputRequest::MutationFragment { .. },
            RangeTextInputRequest::MutationFragment { .. },
            RangeTextInputRequest::MutationCommit(_),
        ]
    ));
    input.update(cx, |input, _| input.admit_mutation_commit(key).unwrap());
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input
                .settle_mutation(key, MutationOutcome::Rejected, window, cx)
                .unwrap();
        })
    });
    input.update(cx, |input, cx| {
        let missing_key = gpui_text_input::MutationKey::new(
            base.binding(),
            base.revision(),
            gpui_text_input::OperationId::new(8_002),
        );
        let missing = MutationProposal::new(
            missing_key,
            MutationKind::Edit,
            ordinary_range(ByteRange::from_u64(0, 4).unwrap()),
            0,
        );
        assert!(matches!(
            input.propose_host_mutation(
                missing,
                vec![MutationFragment::new(missing_key, 0, terminal(0))],
                &one_text,
                &one_objects,
                cx,
            ),
            Err(gpui_text_input::RangeTextInputError::Mutation(
                gpui_text_input::MutationError::InvalidObjectGapProof
            ))
        ));
        assert!(input.take_request().is_none());
        let malformed_key = gpui_text_input::MutationKey::new(
            base.binding(),
            base.revision(),
            gpui_text_input::OperationId::new(8_003),
        );
        let malformed = MutationProposal::new(
            malformed_key,
            MutationKind::Edit,
            ordinary_range(ByteRange::from_u64(0, 0).unwrap()),
            0,
        );
        assert!(
            input
                .propose_host_mutation(
                    malformed,
                    vec![MutationFragment::new(malformed_key, 1, terminal(0))],
                    &one_text,
                    &one_objects,
                    cx,
                )
                .is_err()
        );
        assert!(matches!(
            input.propose_host_mutation(
                malformed,
                vec![MutationFragment::new(malformed_key, 0, terminal(0))],
                &one_text,
                &one_objects,
                cx,
            ),
            Err(gpui_text_input::RangeTextInputError::Mutation(
                gpui_text_input::MutationError::ObsoleteOperation(key)
            )) if key == malformed_key
        ));
        assert!(matches!(
            input.admit_mutation_commit(malformed_key),
            Err(gpui_text_input::RangeTextInputError::Mutation(
                gpui_text_input::MutationError::ObsoleteOperation(key)
            )) if key == malformed_key
        ));
        assert!(input.take_request().is_none());
        assert!(input.is_quiescent());
    });
}

#[gpui::test]
fn committed_object_gap_is_retained_and_cannot_be_reused_as_no_objects(
    cx: &mut gpui::TestAppContext,
) {
    cx.update(ensure_text_input_bindings);
    let source = "ab";
    let (input, cx) = cx.add_window_view(|window, cx| {
        let input = RangeTextInput::new(config(source, 1), window, cx).unwrap();
        input.focus(window);
        input
    });
    assert!(drive_pages(&input, cx, source).is_empty());

    let first = object_neighbor(81, 10);
    let second = object_neighbor(82, 20);
    let between = SourcePosition::new(
        ByteOffset::new(1),
        InlineObjectGap::between(first, second).unwrap(),
    );
    let positions = MutationPositions::collapsed(between);
    let base = binding(source, 1);
    let key = gpui_text_input::MutationKey::new(
        base.binding(),
        base.revision(),
        gpui_text_input::OperationId::new(8_100),
    );
    let proposal = MutationProposal::new(
        key,
        MutationKind::Edit,
        SourceRange::new(between, between).unwrap(),
        0,
    );
    let facts = [object_fact(81, 1, 10), object_fact(82, 1, 20)];
    let (base_text, base_objects) = admitted_sources_with_facts(source, 1, &[between], &facts);
    input.update(cx, |input, cx| {
        input
            .propose_host_mutation(
                proposal,
                vec![MutationFragment::new(
                    key,
                    0,
                    MutationFragmentPayload::Terminal {
                        intended: positions,
                    },
                )],
                &base_text,
                &base_objects,
                cx,
            )
            .unwrap();
    });
    let requests = drive_pages(&input, cx, source);
    assert!(requests.iter().any(
        |request| matches!(request, RangeTextInputRequest::MutationCommit(active) if *active == key)
    ));
    input.update(cx, |input, _| input.admit_mutation_commit(key).unwrap());

    let (successor_text, successor_objects) =
        admitted_sources_with_facts(source, 2, &[between], &facts);
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input
                .settle_committed_mutation(
                    key,
                    binding(source, 2),
                    positions,
                    &successor_text,
                    &successor_objects,
                    window,
                    cx,
                )
                .unwrap();
            assert_eq!(input.adopted_mutation_positions(), Some(positions));
        })
    });

    cx.simulate_input("!");
    let requests = (0..16)
        .filter_map(|_| input.update(cx, |input, _| input.take_request()))
        .collect::<Vec<_>>();
    assert!(
        !requests
            .iter()
            .any(|request| matches!(request, RangeTextInputRequest::MutationPreflight(_)))
    );
    assert_eq!(
        input.read_with(cx, |input, _| input.adopted_mutation_positions()),
        Some(positions)
    );
}

#[gpui::test]
fn mounted_same_anchor_objects_are_exact_keyboard_steps_and_bounded_metadata(
    cx: &mut gpui::TestAppContext,
) {
    cx.update(ensure_text_input_bindings);
    let source = "ab";
    let facts = vec![
        object_fact(201, 1, 10),
        object_fact(202, 1, 20),
        object_fact_with_activation(203, 1, 30, false),
    ];
    let (input, cx) = cx.add_window_view(|window, cx| {
        let input = RangeTextInput::new(config(source, 1), window, cx).unwrap();
        input.focus(window);
        input
    });
    drive_pages_with_objects(&input, cx, source, &facts);
    let events = restoration_events(&input, cx);
    let first = object_neighbor(201, 10);
    let middle = object_neighbor(202, 20);
    let last = object_neighbor(203, 30);
    let before = SourcePosition::new(ByteOffset::new(1), InlineObjectGap::before(first));
    let gap_one = SourcePosition::new(
        ByteOffset::new(1),
        InlineObjectGap::between(first, middle).unwrap(),
    );
    let gap_two = SourcePosition::new(
        ByteOffset::new(1),
        InlineObjectGap::between(middle, last).unwrap(),
    );
    let after = SourcePosition::new(ByteOffset::new(1), InlineObjectGap::after(last));
    input.read_with(cx, |input, _| {
        let surface = input.surface().unwrap();
        assert_eq!(surface.realized_objects().len(), 3);
        let published = surface
            .realized_presentations(surface.publication_key())
            .unwrap()
            .collect::<Vec<_>>();
        let wrong_publication = gpui_text_input::GeometryJobKey::new(
            surface.geometry_key(),
            gpui_text_input::GeometryJobId::new(surface.publication_key().job().get() + 1),
        );
        assert!(surface.realized_presentations(wrong_publication).is_none());
        assert_eq!(published.len(), 3);
        assert!(
            published
                .iter()
                .all(|fact| fact.presentation().semantic_state() == 0)
        );
        assert_eq!(
            published
                .iter()
                .map(|fact| fact.presentation().activation_eligible())
                .collect::<Vec<_>>(),
            vec![true, true, false]
        );
        assert_eq!(published[0].geometry().leading(), before);
        assert_eq!(published[0].geometry().trailing(), gap_one);
        assert_eq!(published[1].geometry().leading(), gap_one);
        assert_eq!(published[1].geometry().trailing(), gap_two);
        assert_eq!(published[2].geometry().leading(), gap_two);
        assert_eq!(published[2].geometry().trailing(), after);
    });

    cx.simulate_keystrokes("right");
    drive_pages_with_objects(&input, cx, source, &facts);
    assert_eq!(
        input.read_with(cx, |input, _| input.surface().unwrap().caret()),
        before
    );
    cx.simulate_keystrokes("right");
    drive_pages_with_objects(&input, cx, source, &facts);
    input.read_with(cx, |input, _| {
        let surface = input.surface().unwrap();
        assert_eq!(
            surface.selection(),
            RangeSourceSelection {
                anchor: before,
                head: gap_one
            }
        );
        assert_eq!(
            input.active_inline_object().unwrap().object_id,
            InlineObjectId::new(201)
        );
        assert!(surface.platform_selection().is_none());
    });
    let keyboard_anchor = input.read_with(cx, |input, _| input.active_inline_object().unwrap());
    cx.simulate_keystrokes("enter space");
    let keyboard_activations = events
        .borrow()
        .iter()
        .filter_map(|event| match event {
            RangeTextInputEvent::InlineObjectActivated(activation) => Some(*activation),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(keyboard_activations.len(), 2);
    assert_eq!(keyboard_activations[0].anchor, keyboard_anchor);
    assert_eq!(keyboard_activations[1].anchor, keyboard_anchor);
    assert!(matches!(
        keyboard_activations[0].origin,
        gpui_text_input::InlineObjectInputOrigin::Keyboard {
            key: gpui_text_input::InlineObjectActivationKey::Enter
        }
    ));
    assert!(matches!(
        keyboard_activations[1].origin,
        gpui_text_input::InlineObjectInputOrigin::Keyboard {
            key: gpui_text_input::InlineObjectActivationKey::Space
        }
    ));

    cx.simulate_keystrokes("right");
    drive_pages_with_objects(&input, cx, source, &facts);
    cx.simulate_keystrokes("right");
    drive_pages_with_objects(&input, cx, source, &facts);
    input.read_with(cx, |input, _| {
        assert_eq!(
            input.surface().unwrap().selection(),
            RangeSourceSelection {
                anchor: gap_one,
                head: gap_two
            }
        );
        assert_eq!(
            input.active_inline_object().unwrap().object_id,
            InlineObjectId::new(202)
        );
    });
    cx.simulate_keystrokes("right");
    drive_pages_with_objects(&input, cx, source, &facts);
    cx.simulate_keystrokes("right");
    drive_pages_with_objects(&input, cx, source, &facts);
    input.read_with(cx, |input, _| {
        assert_eq!(
            input.surface().unwrap().selection(),
            RangeSourceSelection {
                anchor: gap_two,
                head: after
            }
        );
        assert_eq!(
            input.active_inline_object().unwrap().object_id,
            InlineObjectId::new(203)
        );
    });
    cx.simulate_keystrokes("shift-left");
    drive_pages_with_objects(&input, cx, source, &facts);
    input.read_with(cx, |input, _| {
        assert_eq!(
            input.surface().unwrap().selection(),
            RangeSourceSelection {
                anchor: gap_two,
                head: gap_two
            }
        );
    });
    cx.simulate_keystrokes("shift-left");
    drive_pages_with_objects(&input, cx, source, &facts);
    input.read_with(cx, |input, _| {
        assert_eq!(
            input.surface().unwrap().selection(),
            RangeSourceSelection {
                anchor: gap_two,
                head: gap_one
            }
        );
        assert_eq!(
            input.active_inline_object().unwrap().object_id,
            InlineObjectId::new(202)
        );
    });
    cx.simulate_keystrokes("shift-left");
    drive_pages_with_objects(&input, cx, source, &facts);
    input.read_with(cx, |input, _| {
        assert_eq!(
            input.surface().unwrap().selection(),
            RangeSourceSelection {
                anchor: gap_two,
                head: before
            }
        );
        assert!(input.active_inline_object().is_none());
    });

    cx.simulate_keystrokes("right");
    drive_pages_with_objects(&input, cx, source, &facts);
    cx.simulate_keystrokes("right");
    drive_pages_with_objects(&input, cx, source, &facts);
    assert_eq!(
        input.read_with(cx, |input, _| input
            .active_inline_object()
            .unwrap()
            .object_id),
        InlineObjectId::new(203)
    );
    for key in ["enter", "space"] {
        cx.simulate_keystrokes(key);
        let RangeTextInputRequest::MutationPreflight(proposal) =
            input.update(cx, |input, _| input.take_request()).unwrap()
        else {
            panic!("ineligible activation retains ordinary key behavior")
        };
        input.update(cx, |input, cx| {
            input.reject_mutation_preflight(proposal.key(), cx).unwrap();
        });
    }
    assert_eq!(
        events
            .borrow()
            .iter()
            .filter(|event| matches!(event, RangeTextInputEvent::InlineObjectActivated(_)))
            .count(),
        2
    );
}

#[gpui::test]
fn eligible_keyboard_activation_admission_rejection_is_inert_for_enter_and_space(
    cx: &mut gpui::TestAppContext,
) {
    cx.update(ensure_text_input_bindings);
    let source = "ab";
    let facts = [object_fact(211, 1, 10)];

    let mut rejected_config = config(source, 1);
    rejected_config.mutation_limits = MutationLimits::new(128, 256).unwrap();
    rejected_config.limits.max_surface_items = 345;
    let (rejected, cx) = cx.add_window_view(|window, cx| {
        let input = RangeTextInput::new(rejected_config, window, cx).unwrap();
        input.focus(window);
        input
    });
    drive_pages_with_objects(&rejected, cx, source, &facts);
    let events = restoration_events(&rejected, cx);
    cx.simulate_keystrokes("right");
    drive_pages_with_objects(&rejected, cx, source, &facts);
    cx.simulate_keystrokes("right");
    drive_pages_with_objects(&rejected, cx, source, &facts);
    retain_mutation_request_queue_capacity(&rejected, cx, source);
    let before = range_publication_fingerprint(&rejected, cx);
    let active = rejected.read_with(cx, |input, _| input.active_inline_object().unwrap());
    assert_eq!(active.object_id, InlineObjectId::new(211));

    for key in ["enter", "space"] {
        let event_count = events.borrow().len();
        cx.simulate_keystrokes(key);
        assert!(
            rejected
                .update(cx, |input, _| input.take_request())
                .is_none()
        );
        assert_eq!(range_publication_fingerprint(&rejected, cx), before);
        assert_eq!(
            rejected.read_with(cx, |input, _| input.active_inline_object()),
            Some(active)
        );
        assert_eq!(events.borrow().len(), event_count);
    }
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
fn one_object_backspace_delete_replacement_and_read_only_use_exact_remove(
    cx: &mut gpui::TestAppContext,
) {
    cx.update(ensure_text_input_bindings);
    let source = "ab";
    let facts = vec![object_fact(401, 1, 10)];
    let (input, cx) = cx.add_window_view(|window, cx| {
        let input = RangeTextInput::new(config(source, 1), window, cx).unwrap();
        input.focus(window);
        input
    });
    drive_pages_with_objects(&input, cx, source, &facts);
    cx.simulate_keystrokes("right");
    drive_pages_with_objects(&input, cx, source, &facts);
    cx.simulate_keystrokes("right");
    drive_pages_with_objects(&input, cx, source, &facts);
    let object = input.read_with(cx, |input, _| {
        input.surface().unwrap().realized_objects()[0]
    });
    let exact = SourceRange::new(object.leading(), object.trailing()).unwrap();

    for replacement in [None, Some("X")] {
        match replacement {
            None => cx.simulate_keystrokes("backspace"),
            Some(text) => cx.simulate_input(text),
        }
        let preflight = input
            .update(cx, |input, _| input.take_request())
            .expect("exact object mutation preflight");
        let RangeTextInputRequest::MutationPreflight(proposal) = preflight else {
            panic!("object mutation must begin with preflight")
        };
        assert_eq!(proposal.replacement(), exact);
        let staged = accept_and_collect_mutation(&input, cx, proposal.key());
        assert!(staged.iter().any(|request| matches!(
            request,
            RangeTextInputRequest::MutationFragment { fragment, .. }
                if matches!(
                    fragment.payload(),
                    MutationFragmentPayload::Object(gpui_text_input::ObjectChange::Remove { target })
                        if target.range() == exact
                            && target.id() == InlineObjectId::new(401)
                            && target.order() == InlineObjectOrder::new(10)
                )
        )));
        assert_eq!(
            staged
                .iter()
                .filter(|request| matches!(
                    request,
                    RangeTextInputRequest::MutationFragment { fragment, .. }
                        if matches!(fragment.payload(), MutationFragmentPayload::Utf8 { .. })
                ))
                .count(),
            usize::from(replacement.is_some())
        );
        input.update(cx, |input, cx| {
            input.reject_mutation_staging(proposal.key(), cx).unwrap();
        });
        assert_eq!(
            input.read_with(cx, |input, _| input.surface().unwrap().selection()),
            RangeSourceSelection {
                anchor: object.leading(),
                head: object.trailing(),
            }
        );
    }

    for (input_text, outcome) in [
        (None, MutationOutcome::Conflict),
        (Some("Y"), MutationOutcome::Error),
    ] {
        match input_text {
            None => cx.simulate_keystrokes("backspace"),
            Some(text) => cx.simulate_input(text),
        }
        let RangeTextInputRequest::MutationPreflight(proposal) =
            input.update(cx, |input, _| input.take_request()).unwrap()
        else {
            panic!("object mutation preflight")
        };
        let staged = accept_and_collect_mutation(&input, cx, proposal.key());
        assert!(staged.iter().any(
            |request| matches!(request, RangeTextInputRequest::MutationCommit(key) if *key == proposal.key())
        ));
        input.update(cx, |input, _| {
            input.admit_mutation_commit(proposal.key()).unwrap()
        });
        cx.update(|window, app| {
            input.update(app, |input, cx| {
                assert!(matches!(
                    input
                        .settle_mutation(proposal.key(), outcome, window, cx)
                        .unwrap(),
                    gpui_text_input::MutationSettlement::Current(settled) if settled == outcome
                ));
            })
        });
        input.read_with(cx, |input, _| {
            assert_eq!(
                input.surface().unwrap().selection(),
                RangeSourceSelection {
                    anchor: object.leading(),
                    head: object.trailing(),
                }
            );
            assert!(input.is_quiescent());
        });
    }

    let before_click = input.read_with(cx, |input, _| {
        input
            .surface()
            .unwrap()
            .realized_object_gaps()
            .iter()
            .find(|gap| gap.position() == object.leading())
            .unwrap()
            .caret_bounds()
            .origin
    });
    cx.simulate_event(MouseDownEvent {
        position: before_click,
        modifiers: Modifiers::none(),
        button: MouseButton::Left,
        click_count: 1,
        first_mouse: false,
    });
    drive_pages_with_objects(&input, cx, source, &facts);
    cx.simulate_keystrokes("delete");
    let RangeTextInputRequest::MutationPreflight(delete) =
        input.update(cx, |input, _| input.take_request()).unwrap()
    else {
        panic!("delete preflight")
    };
    assert_eq!(delete.replacement(), exact);
    input.update(cx, |input, cx| {
        input.reject_mutation_preflight(delete.key(), cx).unwrap();
        input.set_read_only(true, cx);
    });
    cx.simulate_keystrokes("delete backspace");
    assert!(input.update(cx, |input, _| input.take_request()).is_none());
}

#[gpui::test]
fn committed_object_removal_and_replacement_emit_exact_realization_loss(
    cx: &mut gpui::TestAppContext,
) {
    cx.update(ensure_text_input_bindings);
    for (case, replacement) in [(0u128, None), (1, Some("X"))] {
        let source = "ab";
        let object_id = 700 + case;
        let facts = [object_fact(object_id, 1, 10)];
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
        let active = input.read_with(cx, |input, _| input.active_inline_object().unwrap());

        match replacement {
            None => cx.simulate_keystrokes("backspace"),
            Some(text) => cx.simulate_input(text),
        }
        let RangeTextInputRequest::MutationPreflight(proposal) =
            input.update(cx, |input, _| input.take_request()).unwrap()
        else {
            panic!("object commit preflight")
        };
        let staged = accept_and_collect_mutation(&input, cx, proposal.key());
        let positions = staged
            .iter()
            .find_map(|request| match request {
                RangeTextInputRequest::MutationFragment { fragment, .. } => {
                    match fragment.payload() {
                        MutationFragmentPayload::Terminal { intended } => Some(*intended),
                        _ => None,
                    }
                }
                _ => None,
            })
            .expect("terminal successor positions");
        assert!(staged.iter().any(|request| matches!(
            request,
            RangeTextInputRequest::MutationFragment { fragment, .. }
                if matches!(
                    fragment.payload(),
                    MutationFragmentPayload::Object(gpui_text_input::ObjectChange::Remove { target })
                        if target.id() == InlineObjectId::new(object_id)
                )
        )));
        input.update(cx, |input, _| {
            input.admit_mutation_commit(proposal.key()).unwrap()
        });

        let successor = replacement.map_or_else(|| source.to_owned(), |text| format!("a{text}b"));
        let successor_positions = [
            positions.caret(),
            positions.selection_anchor(),
            positions.selection_head(),
        ];
        let (text, objects) = admitted_sources_with_facts(&successor, 2, &successor_positions, &[]);
        cx.update(|window, app| {
            input.update(app, |input, cx| {
                assert!(matches!(
                    input
                        .settle_committed_mutation(
                            proposal.key(),
                            binding(&successor, 2),
                            positions,
                            &text,
                            &objects,
                            window,
                            cx,
                        )
                        .unwrap(),
                    gpui_text_input::MutationSettlement::Current(MutationOutcome::Committed(_))
                ));
            })
        });
        drive_pages_with_objects(&input, cx, &successor, &[]);
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
                                == gpui_text_input::InlineObjectRealizationLossReason::Removed
                ))
                .count(),
            1
        );
    }
}

#[gpui::test]
fn same_anchor_first_middle_last_objects_stage_mutation_and_cut(cx: &mut gpui::TestAppContext) {
    cx.update(ensure_text_input_bindings);
    let source = "ab";
    let facts = [
        object_fact(801, 1, 10),
        object_fact(802, 1, 20),
        object_fact(803, 1, 30),
    ];
    for (index, expected_id, expected_order) in
        [(0usize, 801u128, 10u128), (1, 802, 20), (2, 803, 30)]
    {
        let (input, cx) = cx.add_window_view(|window, cx| {
            let input = RangeTextInput::new(config(source, 1), window, cx).unwrap();
            input.focus(window);
            input
        });
        drive_pages_with_objects(&input, cx, source, &facts);
        let object = input.read_with(cx, |input, _| {
            input.surface().unwrap().realized_objects()[index]
        });
        let exact = SourceRange::new(object.leading(), object.trailing()).unwrap();
        cx.simulate_event(MouseDownEvent {
            position: object.hit_bounds().origin + gpui::point(px(1.), px(1.)),
            modifiers: Modifiers::none(),
            button: MouseButton::Left,
            click_count: 1,
            first_mouse: false,
        });
        drive_pages_with_objects(&input, cx, source, &facts);

        cx.simulate_keystrokes("backspace");
        let RangeTextInputRequest::MutationPreflight(edit) =
            input.update(cx, |input, _| input.take_request()).unwrap()
        else {
            panic!("same-anchor object edit preflight")
        };
        assert_eq!(edit.replacement(), exact);
        let staged_edit = accept_and_collect_mutation(&input, cx, edit.key());
        assert!(staged_edit.iter().any(|request| matches!(
            request,
            RangeTextInputRequest::MutationFragment { fragment, .. }
                if matches!(
                    fragment.payload(),
                    MutationFragmentPayload::Object(gpui_text_input::ObjectChange::Remove { target })
                        if target.range() == exact
                            && target.id() == InlineObjectId::new(expected_id)
                            && target.order() == InlineObjectOrder::new(expected_order)
                )
        )));
        input.update(cx, |input, cx| {
            input.reject_mutation_staging(edit.key(), cx).unwrap();
            input
                .begin_clipboard(gpui_text_input::ClipboardKind::Cut, cx)
                .unwrap();
        });

        let write = drive_clipboard_write_with_objects(&input, cx, source, &facts);
        assert_eq!(write.text(), format!("[{expected_id}]"));
        input.update(cx, |input, cx| {
            assert!(matches!(
                input
                    .settle_clipboard_write(write.key(), ClipboardWriteOutcome::Written, cx)
                    .unwrap(),
                gpui_text_input::ClipboardCompletion::Delete(deletion)
                    if deletion.selection() == exact
            ));
        });
        let cut = (0..16)
            .find_map(
                |_| match input.update(cx, |input, _| input.take_request()) {
                    Some(RangeTextInputRequest::MutationPreflight(proposal)) => Some(proposal),
                    Some(RangeTextInputRequest::ReleasePage(_))
                    | Some(RangeTextInputRequest::ReleaseObjectPage(_)) => None,
                    Some(request) => panic!("unexpected cut request: {request:?}"),
                    None => None,
                },
            )
            .expect("cut mutation preflight");
        let staged_cut = accept_and_collect_mutation(&input, cx, cut.key());
        assert!(staged_cut.iter().any(|request| matches!(
            request,
            RangeTextInputRequest::MutationFragment { fragment, .. }
                if matches!(
                    fragment.payload(),
                    MutationFragmentPayload::Object(gpui_text_input::ObjectChange::Remove { target })
                        if target.range() == exact
                            && target.id() == InlineObjectId::new(expected_id)
                            && target.order() == InlineObjectOrder::new(expected_order)
                )
        )));
        input.update(cx, |input, cx| {
            input.reject_mutation_staging(cut.key(), cx).unwrap();
        });
    }
}

#[gpui::test]
fn object_gap_platform_composition_is_not_collapsed_and_lifecycle_loss_is_once(
    cx: &mut gpui::TestAppContext,
) {
    cx.update(ensure_text_input_bindings);
    let source = "ab";
    let facts = vec![object_fact(501, 1, 10)];
    let (input, cx) = cx.add_window_view(|window, cx| {
        let input = RangeTextInput::new(config(source, 1), window, cx).unwrap();
        input.focus(window);
        input
    });
    drive_pages_with_objects(&input, cx, source, &facts);
    let events = restoration_events(&input, cx);
    cx.simulate_keystrokes("right");
    drive_pages_with_objects(&input, cx, source, &facts);
    cx.simulate_keystrokes("right");
    drive_pages_with_objects(&input, cx, source, &facts);
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            assert!(input.selected_text_range(false, window, cx).is_none());
            input.replace_and_mark_text_in_range(None, "marked", None, window, cx);
        })
    });
    assert!(input.update(cx, |input, _| input.take_request()).is_none());
    input.read_with(cx, |input, _| {
        assert!(input.surface().unwrap().composition().is_none());
        assert!(input.active_inline_object().is_some());
    });

    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.rebind(binding(source, 2), None, window, cx).unwrap();
        })
    });
    drive_pages_with_objects(&input, cx, source, &facts);
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
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            let _ = input.dispose(window, cx);
            assert!(input.dispose(window, cx).is_empty());
        })
    });
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
        assert!(input.is_quiescent());
    });
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
        assert!(input.is_quiescent());
    });
    assert!(
        events
            .borrow()
            .iter()
            .all(|event| !matches!(event, RangeTextInputEvent::InlineObjectRealizationLost(_)))
    );
}

#[gpui::test]
fn rejected_true_rebind_preserves_active_coherent_surface_without_loss(
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

    assert!(matches!(
        cx.update(|window, app| input.update(app, |input, cx| input.rebind(
            binding(source, 2),
            None,
            window,
            cx
        ))),
        Err(gpui_text_input::RangeTextInputError::Geometry(
            gpui_text_input::ExactGeometryError::CapacityExceeded
        ))
    ));
    input.read_with(cx, |input, _| {
        let surface = input.surface().unwrap();
        assert_eq!(input.active_inline_object(), Some(active));
        assert_eq!(surface.geometry_key(), geometry);
        assert_eq!(surface.binding(), binding(source, 1));
        assert_eq!(surface.selection(), selection);
        assert_eq!(surface.realized_objects()[0].id(), InlineObjectId::new(905));
        assert!(input.is_quiescent());
    });
    assert!(
        events
            .borrow()
            .iter()
            .all(|event| !matches!(event, RangeTextInputEvent::InlineObjectRealizationLost(_)))
    );
}

#[gpui::test]
fn repeated_wheel_retarget_rejection_preserves_full_publication_fingerprint(
    cx: &mut gpui::TestAppContext,
) {
    let source = (0..100)
        .map(|line| format!("line-{line:03}\n"))
        .collect::<String>();
    let mut rejected_config = config(&source, 1);
    rejected_config.limits.max_surface_bytes = 39_536;
    rejected_config.limits.max_surface_items = 436;
    let (rejected, cx) =
        cx.add_window_view(|window, cx| RangeTextInput::new(rejected_config, window, cx).unwrap());
    drive_pages(&rejected, cx, &source);
    let events = restoration_events(&rejected, cx);
    let before = range_publication_fingerprint(&rejected, cx);
    cx.simulate_event(ScrollWheelEvent {
        position: point(px(1.), px(1.)),
        delta: ScrollDelta::Pixels(point(px(0.), px(-48.))),
        ..Default::default()
    });
    let Some(RangeTextInputRequest::Page(first_retarget)) =
        rejected.update(cx, |input, _| input.take_request())
    else {
        panic!("first wheel retarget request")
    };
    let first_retarget_key = first_retarget.key();
    let first_retarget_demand = first_retarget_key.demand();
    assert_eq!(first_retarget_key.purpose(), PagePurpose::GeometryTarget);
    assert!(matches!(
        first_retarget_demand,
        PageDemandEnvelope::Adjacent {
            direction: PageDirection::Forward,
            ..
        }
    ));
    let committed = range_publication_fingerprint(&rejected, cx);
    assert_ne!(committed.admission, before.admission);
    assert_eq!(committed.surface, before.surface);
    let event_count = events.borrow().len();
    cx.simulate_event(ScrollWheelEvent {
        position: point(px(1.), px(1.)),
        delta: ScrollDelta::Pixels(point(px(0.), px(-48.))),
        ..Default::default()
    });
    assert!(
        rejected
            .update(cx, |input, _| input.take_request())
            .is_none()
    );
    assert_eq!(range_publication_fingerprint(&rejected, cx), committed);
    assert_eq!(events.borrow().len(), event_count);
    cx.simulate_event(ScrollWheelEvent {
        position: point(px(1.), px(1.)),
        delta: ScrollDelta::Pixels(point(px(0.), px(-96.))),
        ..Default::default()
    });
    assert!(
        rejected
            .update(cx, |input, _| input.take_request())
            .is_none()
    );
    assert_eq!(range_publication_fingerprint(&rejected, cx), committed);
    assert_eq!(events.borrow().len(), event_count);
}

#[gpui::test]
fn repeated_rendered_scrollbar_retarget_rejection_preserves_full_publication_fingerprint(
    cx: &mut gpui::TestAppContext,
) {
    let source = (0..100)
        .map(|line| format!("line-{line:03}\n"))
        .collect::<String>();
    let mut configuration = config(&source, 1);
    configuration.limits.max_surface_bytes = 40_000;
    configuration.limits.max_surface_items = 430;
    let (input, cx) =
        cx.add_window_view(|window, cx| RangeTextInput::new(configuration, window, cx).unwrap());
    drive_pages(&input, cx, &source);
    let events = restoration_events(&input, cx);
    cx.simulate_event(ScrollWheelEvent {
        position: point(px(1.), px(1.)),
        delta: ScrollDelta::Pixels(point(px(0.), px(-48.))),
        ..Default::default()
    });
    let Some(RangeTextInputRequest::Page(first_retarget)) =
        input.update(cx, |input, _| input.take_request())
    else {
        panic!("scrollbar activation retarget request")
    };
    let first_retarget_key = first_retarget.key();
    let first_retarget_demand = first_retarget_key.demand();
    assert_eq!(first_retarget_key.purpose(), PagePurpose::GeometryTarget);
    assert!(matches!(
        first_retarget_demand,
        PageDemandEnvelope::Adjacent {
            direction: PageDirection::Forward,
            ..
        }
    ));
    let committed = range_publication_fingerprint(&input, cx);
    let event_count = events.borrow().len();
    let viewport = cx.update(|window, _| window.viewport_size());
    for fraction in [0.9, 0.75] {
        cx.simulate_event(MouseDownEvent {
            position: point(viewport.width - px(1.), viewport.height * fraction),
            modifiers: Modifiers::none(),
            button: MouseButton::Left,
            click_count: 1,
            first_mouse: false,
        });
        assert!(input.update(cx, |input, _| input.take_request()).is_none());
        assert_eq!(range_publication_fingerprint(&input, cx), committed);
        assert_eq!(events.borrow().len(), event_count);
    }
}

#[derive(Clone, Copy, Debug)]
enum FixedCandidateKind {
    Layout,
    Presentation,
    Rebind,
}

fn execute_fixed_candidate_with_mutation(
    kind: FixedCandidateKind,
    input: &gpui::Entity<RangeTextInput>,
    cx: &mut gpui::VisualTestContext,
    source: &str,
) -> (
    Result<(), gpui_text_input::RangeTextInputError>,
    Option<gpui_text_input::RangeSurfaceCharge>,
    RangePublicationFingerprint,
    RangePublicationFingerprint,
) {
    let (proposal, fragments, text, objects, _request_count) = bounded_mutation_stream(source, 128);
    let result = match kind {
        FixedCandidateKind::Layout => input.update(cx, move |input, cx| {
            input
                .propose_host_mutation(proposal, fragments, &text, &objects, cx)
                .unwrap();
            let before = range_publication_fingerprint_from(input);
            let mut layout = config(source, 1).layout;
            layout.wrap_width = px(96.);
            layout.limits.segment_bytes = 64 * 1024;
            let result = input.set_layout(layout, replacement_geometry_style(), cx);
            let admission = input.last_surface_admission_charge();
            let after = range_publication_fingerprint_from(input);
            (result, admission, before, after)
        }),
        FixedCandidateKind::Presentation => input.update(cx, move |input, cx| {
            input
                .propose_host_mutation(proposal, fragments, &text, &objects, cx)
                .unwrap();
            let before = range_publication_fingerprint_from(input);
            let result = input.set_presentation_generation(PresentationGeneration::new(2), cx);
            let admission = input.last_surface_admission_charge();
            let after = range_publication_fingerprint_from(input);
            (result, admission, before, after)
        }),
        FixedCandidateKind::Rebind => cx.update(|window, app| {
            input.update(app, move |input, cx| {
                input
                    .propose_host_mutation(proposal, fragments, &text, &objects, cx)
                    .unwrap();
                let before = range_publication_fingerprint_from(input);
                let result = input.rebind(binding(source, 2), None, window, cx);
                let admission = input.last_surface_admission_charge();
                let after = range_publication_fingerprint_from(input);
                (result, admission, before, after)
            })
        }),
    };
    result
}

#[gpui::test]
fn layout_presentation_and_rebind_candidates_use_fixed_exact_caps_and_reject_one_under(
    cx: &mut gpui::TestAppContext,
) {
    const SOURCE: &str = "candidate cap source";
    let cases = [
        (FixedCandidateKind::Layout, 181_600usize, 525usize),
        (FixedCandidateKind::Presentation, 181_604, 526),
        (FixedCandidateKind::Rebind, 148_324, 397),
    ];
    for (kind, exact_bytes, exact_items) in cases {
        for (bytes, items, succeeds) in [
            (exact_bytes, 32_768, true),
            (exact_bytes - 1, 32_768, false),
            (2 * 1024 * 1024, exact_items, true),
            (2 * 1024 * 1024, exact_items - 1, false),
        ] {
            let mut configuration = config(SOURCE, 1);
            configuration.mutation_limits = MutationLimits::new(128, 256).unwrap();
            if !matches!(kind, FixedCandidateKind::Layout) {
                configuration.layout.limits.segment_bytes = 64 * 1024;
                configuration.style = replacement_geometry_style();
            }
            configuration.limits.max_surface_bytes = bytes;
            configuration.limits.max_surface_items = items;
            let (input, cx) = cx.add_window_view(|window, cx| {
                RangeTextInput::new(configuration, window, cx).unwrap()
            });
            drive_pages(&input, cx, SOURCE);
            let events = restoration_events(&input, cx);
            let event_count = events.borrow().len();
            let (result, admission, before, after) =
                execute_fixed_candidate_with_mutation(kind, &input, cx, SOURCE);
            assert_eq!(
                result.is_ok(),
                succeeds,
                "{kind:?}: {bytes}/{items}: {result:?}; admission={admission:?}"
            );
            if succeeds {
                assert_eq!(
                    admission,
                    Some(gpui_text_input::RangeSurfaceCharge {
                        bytes: exact_bytes,
                        items: exact_items,
                    })
                );
                assert_eq!(after.surface, before.surface);
            } else {
                assert!(matches!(
                    result,
                    Err(gpui_text_input::RangeTextInputError::SurfaceCapacity)
                ));
                assert_eq!(after, before);
            }
            assert_eq!(events.borrow().len(), event_count);
        }
    }
}
