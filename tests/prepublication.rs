use std::{cell::Cell, rc::Rc, sync::Arc};

use gpui::{
    AppContext, SharedString, StreamingLayoutBinding, StreamingLayoutLimits,
    StreamingLayoutPosition, TestAppContext, TextRun, WindowTextSystem, black, font, px,
};
use gpui_scrollbar::ScrollbarStyle;
use gpui_text_input::{
    BindingId, ByteOffset, ByteRange, ClipboardLimits, ExactGeometryLimits, InlineObjectFact,
    InlineObjectGap, InlineObjectId, InlineObjectNeighbor, InlineObjectOrder,
    InlineObjectPresentation, LogicalExtent, MutationLimits, ObjectDirection, ObjectPage,
    ObjectPageEdgeFact, ObjectPageId, ObjectPurpose, ObjectResidencyLimits, PageDemandEnvelope,
    PageDirection, PageEdgeFact, PageId, PagePurpose, PresentationGeneration, RangeBinding,
    RangeHistoryFrontier, RangePage, RangePrepublicationAdoptionError,
    RangePrepublicationCleanupEffect, RangePrepublicationCleanupLedger, RangePrepublicationCurrent,
    RangePrepublicationDelivery, RangePrepublicationEffect, RangePrepublicationEnvironment,
    RangePrepublicationFailure, RangePrepublicationSession, RangePrepublicationStatus,
    RangePrepublicationValidationResponse, RangeRestorationScrollAnchor, RangeRestorationSeed,
    RangeSettlementCoordinator, RangeSourceSelection, RangeSurfaceCharge, RangeTextInput,
    RangeTextInputConfig, RangeTextInputLimits, ResidencyLimits, SegmentationLimits,
    SourcePosition, SourceRevision, StreamingGeometryStyle, StreamingOversizePresentation,
    TextInputAtomClipboardPolicy, TextInputEnterKey, TextInputRichPastePolicy, TextInputTheme,
};

fn binding(source: &str, revision: u64) -> RangeBinding {
    RangeBinding::new(
        BindingId::new(41),
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

fn position(offset: u64) -> SourcePosition {
    SourcePosition::new(ByteOffset::new(offset), InlineObjectGap::NoObjects)
}

fn seed(source: &str, revision: u64, offset: u64) -> RangeRestorationSeed {
    let binding = binding(source, revision);
    let caret = position(offset);
    RangeRestorationSeed {
        binding,
        caret,
        selection: RangeSourceSelection::caret(caret),
        scroll: RangeRestorationScrollAnchor {
            position: caret,
            intra_anchor: px(0.),
        },
        history: Some(RangeHistoryFrontier {
            binding,
            id: 7,
            undo_available: true,
            redo_available: false,
        }),
    }
}

fn config(source: &str, revision: u64, page_bytes: u64) -> RangeTextInputConfig {
    let layout = StreamingLayoutBinding {
        input_id: 23,
        segment_policy_id: 29,
        start_position: StreamingLayoutPosition::at(0),
        wrap_width: px(160.),
        font_size: px(12.),
        line_height: px(16.),
        limits: StreamingLayoutLimits {
            segment_bytes: 256,
            runs: 8,
            decorations: 8,
            glyphs: 2048,
            wraps: 1024,
            maps: 2049,
            fragments: 8,
            retained_items: 16_384,
            retained_bytes: 512 * 1024,
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
        presentation_generation: PresentationGeneration::new(3),
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
        geometry_limits: ExactGeometryLimits::new(page_bytes, 16, 2 * 1024 * 1024, 32_768).unwrap(),
        residency_limits: ResidencyLimits::new(8, 1024 * 1024, 4, page_bytes * 4).unwrap(),
        object_residency_limits: ObjectResidencyLimits::new(
            4,
            64,
            256 * 1024,
            64 * 1024,
            4,
            64,
            256 * 1024,
        )
        .unwrap(),
        mutation_limits: MutationLimits::new(8, 256).unwrap(),
        clipboard_limits: ClipboardLimits::new(4096, page_bytes).unwrap(),
        segmentation_limits: SegmentationLimits::new(page_bytes, 256).unwrap(),
        limits: RangeTextInputLimits::new(
            8 * 1024 * 1024,
            131_072,
            8,
            px(128.),
            page_bytes,
            page_bytes,
            px(16.),
        )
        .unwrap(),
        settlement_coordinator: RangeSettlementCoordinator::new(4).unwrap(),
        viewport_extent: px(96.),
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

fn empty_object_page(id: u64, request: gpui_text_input::ObjectRequest) -> ObjectPage {
    let demand = request.key().demand();
    let cursor_edge = demand.cursor().map_or(
        ObjectPageEdgeFact::EnvelopeBoundary,
        ObjectPageEdgeFact::Continues,
    );
    let (preceding, following) = match demand.direction() {
        ObjectDirection::Forward => (cursor_edge, ObjectPageEdgeFact::EnvelopeBoundary),
        ObjectDirection::Backward => (ObjectPageEdgeFact::EnvelopeBoundary, cursor_edge),
    };
    ObjectPage::new(
        ObjectPageId::new(id),
        request.key(),
        vec![],
        preceding,
        following,
        true,
        None,
    )
    .unwrap()
}

fn object_fact(id: u128, anchor: u64, order: u128) -> InlineObjectFact {
    InlineObjectFact::new(
        InlineObjectId::new(id),
        ByteOffset::new(anchor),
        InlineObjectOrder::new(order),
        format!("[{id}]"),
        InlineObjectPresentation::new(
            id as u64,
            SharedString::new_static(""),
            px(20.),
            px(16.),
            px(12.),
            None,
            0,
            true,
        )
        .unwrap(),
    )
}

fn object_page_for(
    id: u64,
    request: gpui_text_input::ObjectRequest,
    facts: &[InlineObjectFact],
) -> ObjectPage {
    let demand = request.key().demand();
    let mut objects = facts
        .iter()
        .filter(|fact| demand.contains_anchor(fact.anchor()))
        .filter(|fact| {
            demand
                .cursor()
                .is_none_or(|cursor| match demand.direction() {
                    ObjectDirection::Forward => fact.cursor() > cursor,
                    ObjectDirection::Backward => fact.cursor() < cursor,
                })
        })
        .take(demand.max_objects())
        .cloned()
        .collect::<Vec<_>>();
    objects.sort_by_key(InlineObjectFact::cursor);
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

fn drive(
    session: &mut RangePrepublicationSession,
    source: &str,
    text_system: &Arc<WindowTextSystem>,
    cleanup: &RangePrepublicationCleanupLedger,
) -> (gpui_text_input::RangePrepublicationCandidate, usize, usize) {
    drive_with_objects(session, source, text_system, cleanup, &[])
}

fn drive_with_objects(
    session: &mut RangePrepublicationSession,
    source: &str,
    text_system: &Arc<WindowTextSystem>,
    cleanup: &RangePrepublicationCleanupLedger,
    objects: &[InlineObjectFact],
) -> (gpui_text_input::RangePrepublicationCandidate, usize, usize) {
    let mut response_id = 1000u64;
    let mut steps = 0usize;
    let mut max_bytes = 0usize;
    loop {
        steps += 1;
        assert!(
            steps < 50_000,
            "status={:?} ownership={:?} cleanup={:?}",
            session.status(),
            session.ownership(),
            cleanup.ownership()
        );
        let step = session.service(text_system);
        for effect in step.effects {
            match effect {
                RangePrepublicationEffect::ValidateOwner(request) => {
                    assert_eq!(
                        session.deliver_validation(RangePrepublicationValidationResponse {
                            key: request.key,
                            binding: request.binding,
                            history: request.history,
                            current: true,
                        }),
                        RangePrepublicationDelivery::Accepted
                    );
                }
                RangePrepublicationEffect::Page {
                    generation,
                    request,
                    ..
                } => {
                    response_id += 1;
                    assert_eq!(
                        session.deliver_page(generation, page_for(source, response_id, request)),
                        RangePrepublicationDelivery::Accepted
                    );
                }
                RangePrepublicationEffect::ObjectPage {
                    generation,
                    request,
                    ..
                } => {
                    response_id += 1;
                    let page = if objects.is_empty() {
                        empty_object_page(response_id, request)
                    } else {
                        object_page_for(response_id, request, objects)
                    };
                    assert_eq!(
                        session.deliver_object_page(generation, page),
                        RangePrepublicationDelivery::Accepted
                    );
                }
            }
        }
        let _ = drain_cleanup(cleanup);
        max_bytes = max_bytes.max(session.ownership().bytes);
        match step.status {
            RangePrepublicationStatus::Ready => {
                return (session.take_candidate().unwrap(), steps, max_bytes);
            }
            RangePrepublicationStatus::Failed(failure) => panic!("failed: {failure:?}"),
            RangePrepublicationStatus::Cancelled | RangePrepublicationStatus::Stale => {
                panic!("unexpected terminal status")
            }
            _ => {}
        }
    }
}

fn drive_failure_with_objects(
    session: &mut RangePrepublicationSession,
    source: &str,
    text_system: &Arc<WindowTextSystem>,
    cleanup: &RangePrepublicationCleanupLedger,
    objects: &[InlineObjectFact],
) -> (
    RangePrepublicationFailure,
    Vec<RangePrepublicationCleanupEffect>,
) {
    let mut response_id = 50_000u64;
    let mut cleanup_effects = Vec::new();
    for _ in 0..50_000 {
        let step = session.service(text_system);
        for effect in step.effects {
            match effect {
                RangePrepublicationEffect::ValidateOwner(request) => {
                    session.deliver_validation(RangePrepublicationValidationResponse {
                        key: request.key,
                        binding: request.binding,
                        history: request.history,
                        current: true,
                    });
                }
                RangePrepublicationEffect::Page {
                    generation,
                    request,
                    ..
                } => {
                    response_id += 1;
                    session.deliver_page(generation, page_for(source, response_id, request));
                }
                RangePrepublicationEffect::ObjectPage {
                    generation,
                    request,
                    ..
                } => {
                    response_id += 1;
                    session.deliver_object_page(
                        generation,
                        object_page_for(response_id, request, objects),
                    );
                }
            }
        }
        cleanup_effects.extend(drain_cleanup(cleanup));
        if let RangePrepublicationStatus::Failed(failure) = step.status {
            return (failure, cleanup_effects);
        }
    }
    panic!("session did not fail within the bounded step limit")
}

fn drain_cleanup(
    cleanup: &RangePrepublicationCleanupLedger,
) -> Vec<RangePrepublicationCleanupEffect> {
    let mut observed = Vec::new();
    loop {
        let step = cleanup.service(cleanup.ownership().slots.max(1));
        if step.effects.is_empty() {
            return observed;
        }
        for effect in step.effects {
            cleanup.acknowledge(effect.token());
            observed.push(effect);
        }
    }
}

fn make_environment(
    id: u64,
    config: RangeTextInputConfig,
    text_system: &Arc<WindowTextSystem>,
) -> (
    RangePrepublicationEnvironment,
    RangePrepublicationCleanupLedger,
) {
    let cleanup = RangePrepublicationCleanupLedger::new(text_system, 16).unwrap();
    let environment =
        RangePrepublicationEnvironment::new(id, config, text_system, cleanup.clone()).unwrap();
    (environment, cleanup)
}

#[path = "prepublication/candidate_cases.rs"]
mod candidate_cases;

#[path = "prepublication/cleanup_capacity_cases.rs"]
mod cleanup_capacity_cases;

#[path = "prepublication/pre_dispatch_cases.rs"]
mod pre_dispatch_cases;
