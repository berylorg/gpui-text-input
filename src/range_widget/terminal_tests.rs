use std::{cell::RefCell, rc::Rc, sync::Arc};

use gpui::{SharedString, StreamingLayoutPosition, TextRun, black, font, px};

use super::geometry::{GeometryPageWait, TerminalResponseItemComponents};
use super::transition::WidgetAdmissionComponents;
use super::*;
use crate::{
    AtomFact, AtomId, BindingId, ByteOffset, ByteRange, ClipboardLimits, ExactGeometryLimits,
    ExactGeometryProgress, InlineObjectFact, InlineObjectId, InlineObjectOrder,
    InlineObjectPresentation, LogicalExtent, MutationLimits, ObjectDemand, ObjectDemandEnvelope,
    ObjectDirection, ObjectPage, ObjectPageEdgeFact, ObjectPageId, ObjectPurpose, ObjectRequestId,
    ObjectResidencyLimits, PageDemand, PageDemandEnvelope, PageDirection, PageEdgeFact, PageId,
    PagePurpose, PageRequest, PageRequestId, PresentationGeneration, RangeBinding, RangePage,
    RangeResidency, RangeSourceSelection, RangeTextInputConfig, RangeTextInputEvent,
    RangeTextInputLimits, RangeTextInputRequest, ResidencyLimits, SegmentationLimits,
    SourcePosition, SourceRevision, StreamingGeometryStyle, StreamingOversizePresentation,
    TextInputTheme,
};

const SOURCE: &str = "resident payload";

fn binding() -> RangeBinding {
    RangeBinding::new(
        BindingId::new(71),
        SourceRevision::new(1),
        LogicalExtent::new(SOURCE.len() as u64, 1),
    )
}

fn config(bytes: usize, items: usize) -> RangeTextInputConfig {
    let layout = gpui::StreamingLayoutBinding {
        input_id: 11,
        segment_policy_id: 13,
        start_position: StreamingLayoutPosition::at(0),
        wrap_width: px(120.),
        font_size: px(12.),
        line_height: px(16.),
        limits: gpui::StreamingLayoutLimits {
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
        binding: binding(),
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
        limits: RangeTextInputLimits::new(bytes, items, 32, 32, px(16.)).unwrap(),
        settlement_coordinator: crate::RangeSettlementCoordinator::new(4).unwrap(),
        viewport_extent: px(80.),
        overscan: px(32.),
        placeholder: SharedString::new_static("Value"),
        theme: TextInputTheme::default(),
        scrollbar_style: gpui_scrollbar::ScrollbarStyle::default(),
    }
}

fn page_for(request: PageRequest, id: u64) -> RangePage {
    page_for_source(request, id, SOURCE)
}

fn page_for_source(request: PageRequest, id: u64, source: &str) -> RangePage {
    let (start, end) = match request.key().demand() {
        PageDemandEnvelope::Adjacent {
            anchor,
            direction: PageDirection::Forward,
            max_payload_bytes,
        } => (
            anchor.get() as usize,
            (anchor.get() as usize + max_payload_bytes as usize).min(source.len()),
        ),
        PageDemandEnvelope::Adjacent {
            anchor,
            direction: PageDirection::Backward,
            max_payload_bytes,
        } => (
            (anchor.get() as usize).saturating_sub(max_payload_bytes as usize),
            anchor.get() as usize,
        ),
        PageDemandEnvelope::Validation {
            candidate,
            max_payload_bytes,
        } => {
            assert!(candidate.get() <= source.len() as u64);
            assert!(source.len() as u64 <= max_payload_bytes);
            (0, source.len())
        }
    };
    RangePage::new(
        PageId::new(id),
        request.key(),
        crate::ByteRange::from_u64(start as u64, end as u64).unwrap(),
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

fn drive_surface_for_source(
    input: &gpui::Entity<RangeTextInput>,
    cx: &mut gpui::VisualTestContext,
    source: &str,
) {
    for id in 1..256 {
        match input.update(cx, |input, _| input.take_request()) {
            Some(RangeTextInputRequest::Page(request)) => {
                let page = page_for_source(request, id, source);
                cx.update(|window, app| {
                    input.update(app, |input, cx| {
                        input.deliver_page(page, window, cx).unwrap()
                    })
                });
            }
            Some(RangeTextInputRequest::ObjectPage(request)) => {
                let page = ObjectPage::new(
                    ObjectPageId::new(id),
                    request.key(),
                    vec![],
                    ObjectPageEdgeFact::EnvelopeBoundary,
                    ObjectPageEdgeFact::EnvelopeBoundary,
                    true,
                    None,
                )
                .unwrap();
                cx.update(|window, app| {
                    input.update(app, |input, cx| {
                        input
                            .deliver_object_page_in_window(page, window, cx)
                            .unwrap()
                    })
                });
            }
            Some(RangeTextInputRequest::ReleasePage(_))
            | Some(RangeTextInputRequest::ReleaseObjectPage(_)) => {}
            Some(request) => panic!("unexpected geometry request: {request:?}"),
            None => break,
        }
    }
    input.read_with(cx, |input, _| {
        assert!(input.surface.is_some());
        assert!(input.requests.is_empty());
    });
}

fn admitted_successor_sources(
    source: &str,
    revision: u64,
    positions: &[SourcePosition],
) -> (RangeBinding, RangeResidency, crate::ObjectResidency) {
    let binding = RangeBinding::new(
        BindingId::new(71),
        SourceRevision::new(revision),
        LogicalExtent::new(source.len() as u64, 1),
    );
    let mut text = RangeResidency::new(
        binding,
        ResidencyLimits::new(8, 128 * 1024, 8, 256).unwrap(),
    );
    let PageDemand::Requested(request) = text
        .demand(
            PageRequestId::new(90_000 + revision),
            PagePurpose::Caret,
            PageDemandEnvelope::Adjacent {
                anchor: ByteOffset::new(0),
                direction: PageDirection::Forward,
                max_payload_bytes: source.len() as u64,
            },
        )
        .unwrap()
    else {
        panic!("successor text request")
    };
    text.admit(
        RangePage::new(
            PageId::new(90_000 + revision),
            request.key(),
            crate::ByteRange::from_u64(0, source.len() as u64).unwrap(),
            source.to_owned(),
            vec![],
            PageEdgeFact::DocumentBoundary,
            PageEdgeFact::DocumentBoundary,
            true,
        )
        .unwrap(),
    )
    .unwrap();
    let mut objects = crate::ObjectResidency::new(
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
            1,
            4096,
        )
        .unwrap();
        let ObjectDemand::Requested(request) = objects
            .demand(
                ObjectRequestId::new(91_000 + revision + offsets.len() as u64),
                ObjectPurpose::MutationSuccessor,
                demand,
            )
            .unwrap()
        else {
            panic!("successor object request")
        };
        let page = ObjectPage::new(
            ObjectPageId::new(91_000 + revision + offsets.len() as u64),
            request.key(),
            vec![],
            ObjectPageEdgeFact::EnvelopeBoundary,
            ObjectPageEdgeFact::EnvelopeBoundary,
            true,
            None,
        )
        .unwrap();
        let proofs = text.prove_object_page_anchors(binding, &page).unwrap();
        objects.admit(page, proofs).unwrap();
    }
    (binding, text, objects)
}

fn drive_initial_surface(input: &gpui::Entity<RangeTextInput>, cx: &mut gpui::VisualTestContext) {
    for id in 1..64 {
        match input.update(cx, |input, _| input.take_request()) {
            Some(RangeTextInputRequest::Page(request)) => {
                let page = page_for(request, id);
                cx.update(|window, app| {
                    input.update(app, |input, cx| {
                        input.deliver_page(page, window, cx).unwrap()
                    })
                });
            }
            Some(RangeTextInputRequest::ObjectPage(request)) => {
                let page = ObjectPage::new(
                    ObjectPageId::new(id),
                    request.key(),
                    vec![],
                    ObjectPageEdgeFact::EnvelopeBoundary,
                    ObjectPageEdgeFact::EnvelopeBoundary,
                    true,
                    None,
                )
                .unwrap();
                cx.update(|window, app| {
                    input.update(app, |input, cx| {
                        input
                            .deliver_object_page_in_window(page, window, cx)
                            .unwrap()
                    })
                });
            }
            Some(RangeTextInputRequest::ReleasePage(_))
            | Some(RangeTextInputRequest::ReleaseObjectPage(_)) => {}
            Some(request) => panic!("unexpected initial request: {request:?}"),
            None => break,
        }
    }
    input.read_with(cx, |input, _| {
        assert!(input.surface.is_some());
        assert!(input.requests.is_empty());
    });
}

fn admit_history(
    input: &gpui::Entity<RangeTextInput>,
    cx: &mut gpui::VisualTestContext,
    frontier: crate::RangeHistoryFrontier,
    kind: crate::MutationKind,
) -> crate::RangeHistoryIntent {
    input.update(cx, |input, cx| {
        input
            .set_history_frontier(input.history_frontier(), frontier)
            .unwrap();
        input.request_history(kind, cx);
    });
    let RangeTextInputRequest::HistoryIntent(intent) =
        input.update(cx, |input, _| input.take_request()).unwrap()
    else {
        panic!("history intent")
    };
    input.update(cx, |input, _| {
        input
            .submit_history_session(crate::RangeHistorySession::new(intent))
            .unwrap();
    });
    intent
}

fn rebind_revision(
    input: &gpui::Entity<RangeTextInput>,
    cx: &mut gpui::VisualTestContext,
    revision: u64,
) -> RangeBinding {
    let current = input.read_with(cx, |input, _| input.config.binding);
    let binding = RangeBinding::new(
        current.binding(),
        SourceRevision::new(revision),
        current.extent(),
    );
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.rebind(binding, None, window, cx).unwrap()
        })
    });
    binding
}

fn install_resident_payloads(input: &mut RangeTextInput) -> RangeSurfaceCharge {
    let PageDemand::Requested(text_request) = input
        .residency
        .demand(
            PageRequestId::new(80_000),
            PagePurpose::Caret,
            PageDemandEnvelope::Adjacent {
                anchor: ByteOffset::new(0),
                direction: PageDirection::Forward,
                max_payload_bytes: SOURCE.len() as u64,
            },
        )
        .unwrap()
    else {
        panic!("fresh text residency request")
    };
    let text_page = page_for(text_request, 80_000);
    let text_key = text_page.key();
    assert!(text_page.retained_bytes() >= SOURCE.len());
    let text_charge = text_page.retained_charge();
    input.residency.admit(text_page).unwrap();

    let anchor = ByteOffset::new(4);
    let demand =
        ObjectDemandEnvelope::anchor(anchor, None, ObjectDirection::Forward, 1, 16 * 1024).unwrap();
    let ObjectDemand::Requested(object_request) = input
        .object_residency
        .demand(
            ObjectRequestId::new(80_000),
            ObjectPurpose::Viewport,
            demand,
        )
        .unwrap()
    else {
        panic!("fresh object residency request")
    };
    let presentation = InlineObjectPresentation::new(
        51,
        SharedString::new_static("resident-object-display"),
        px(18.),
        px(16.),
        px(12.),
        None,
        0,
        true,
    )
    .unwrap();
    let object_page = ObjectPage::new(
        ObjectPageId::new(80_000),
        object_request.key(),
        vec![InlineObjectFact::new(
            InlineObjectId::new(51),
            anchor,
            InlineObjectOrder::new(7),
            "resident-object-fallback",
            presentation,
        )],
        ObjectPageEdgeFact::EnvelopeBoundary,
        ObjectPageEdgeFact::EnvelopeBoundary,
        true,
        None,
    )
    .unwrap();
    assert!(object_page.retained_charge().presentation_bytes() > 0);
    let object_charge = object_page.retained_charge();
    let object_items = object_page.objects().len() + 1;
    let proofs = input
        .residency
        .prove_object_page_anchors(input.config.binding, &object_page)
        .unwrap();
    input.object_residency.admit(object_page, proofs).unwrap();

    input
        .requests
        .push_back(RangeTextInputRequest::ReleasePage(text_key));
    RangeSurfaceCharge {
        bytes: text_charge.bytes() + object_charge.bytes(),
        items: text_charge.items() + object_items,
    }
}

fn install_empty_terminal_residency(input: &mut RangeTextInput, id: u64, purpose: ObjectPurpose) {
    let PageDemand::Requested(text_request) = input
        .residency
        .demand(
            PageRequestId::new(id),
            PagePurpose::Caret,
            PageDemandEnvelope::Adjacent {
                anchor: ByteOffset::new(0),
                direction: PageDirection::Forward,
                max_payload_bytes: SOURCE.len() as u64,
            },
        )
        .unwrap()
    else {
        panic!("fresh retained text request")
    };
    input.residency.admit(page_for(text_request, id)).unwrap();

    install_empty_terminal_object_page(input, id, purpose);
}

fn stage_external_terminal_target_object(
    input: &gpui::Entity<RangeTextInput>,
    cx: &mut gpui::VisualTestContext,
) -> crate::ObjectRequest {
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            install_empty_terminal_residency(input, 10, ObjectPurpose::GeometryTarget);
            drop(input.object_residency.take_resident_pages());
            let surface = input.surface.as_ref().expect("prior surface");
            let geometry = surface.geometry_key();
            let position = surface.selection().head;
            input.active_object = Some(ActiveInlineObject {
                anchor: crate::RealizedInlineObjectAnchor {
                    binding: surface.binding(),
                    object_id: InlineObjectId::new(99),
                    order: InlineObjectOrder::new(1),
                    presentation_generation: geometry.presentation_generation(),
                    layout_epoch: geometry.epoch(),
                    bounds: gpui::Bounds::default(),
                },
                leading: position,
                trailing: position,
                activation_eligible: true,
            });
            let candidate = input
                .prepare_target_transition(input.desired, None)
                .unwrap();
            input.commit_widget_transition_internal(candidate);
            input
                .service_resident_target_page(window, cx, true)
                .expect("resident target text service")
                .unwrap();
        })
    });
    loop {
        match input.update(cx, |input, _| input.take_request()).unwrap() {
            RangeTextInputRequest::ObjectPage(request) => return request,
            RangeTextInputRequest::ReleasePage(_) | RangeTextInputRequest::ReleaseObjectPage(_) => {
            }
            request => panic!("unexpected terminal target request: {request:?}"),
        }
    }
}

fn install_empty_terminal_object_page(input: &mut RangeTextInput, id: u64, purpose: ObjectPurpose) {
    install_empty_object_page_for_range(
        input,
        id,
        purpose,
        ByteRange::from_u64(0, SOURCE.len() as u64).unwrap(),
    );
}

fn install_empty_object_page_for_range(
    input: &mut RangeTextInput,
    id: u64,
    purpose: ObjectPurpose,
    range: ByteRange,
) {
    let demand =
        ObjectDemandEnvelope::range(range, None, ObjectDirection::Forward, 32, 128 * 1024).unwrap();
    let ObjectDemand::Requested(object_request) = input
        .object_residency
        .demand(ObjectRequestId::new(id), purpose, demand)
        .unwrap()
    else {
        panic!("fresh retained object request")
    };
    let object_page = ObjectPage::new(
        ObjectPageId::new(id),
        object_request.key(),
        vec![],
        ObjectPageEdgeFact::EnvelopeBoundary,
        ObjectPageEdgeFact::EnvelopeBoundary,
        true,
        None,
    )
    .unwrap();
    let proofs = input
        .residency
        .prove_object_page_anchors(input.config.binding, &object_page)
        .unwrap();
    input.object_residency.admit(object_page, proofs).unwrap();
}

type DesiredFingerprint = (
    Option<RangeSourceSelection>,
    Option<crate::ByteRange>,
    ByteOffset,
    gpui::Pixels,
    gpui::Pixels,
    gpui::Pixels,
    gpui::Pixels,
    bool,
    bool,
    Option<DesiredInlineObjectInteraction>,
);

#[derive(Debug, PartialEq)]
struct GeometryFingerprint {
    key: crate::GeometryKey,
    desired_target: Option<crate::GeometryJobKey>,
    counts: crate::ExactGeometryCounts,
    high_water: (usize, usize),
    active_text_page: Option<PageId>,
    index: Option<String>,
    target: Option<String>,
}

#[derive(Debug, PartialEq)]
enum GeometryPageWaitFingerprint {
    Resident(PageId),
    Coalesced(crate::PageRequestKey),
}

#[derive(Debug, PartialEq)]
struct PendingGeometryPageFingerprint {
    job: crate::GeometryJobKey,
    request: crate::PageRequestKey,
    wait: GeometryPageWaitFingerprint,
}

#[derive(Debug, PartialEq)]
enum GeometryObjectWaitFingerprint {
    Resident(ObjectPageId),
    Coalesced(crate::ObjectRequestKey),
}

#[derive(Debug, PartialEq)]
struct PendingGeometryObjectFingerprint {
    job: crate::GeometryJobKey,
    request: crate::ObjectRequestKey,
    text_page: PageId,
    wait: GeometryObjectWaitFingerprint,
}

#[derive(Debug, PartialEq)]
struct SurfaceCandidateFingerprint {
    job: crate::GeometryJobKey,
    binding: RangeBinding,
    desired: DesiredFingerprint,
    restoration: Option<RangeRestorationSeed>,
}

#[derive(Debug, PartialEq)]
struct TransitionFingerprint {
    geometry: GeometryFingerprint,
    active_geometry: Option<crate::GeometryJobKey>,
    pending_geometry_page: Option<PendingGeometryPageFingerprint>,
    pending_geometry_object: Option<PendingGeometryObjectFingerprint>,
    surface_candidate: Option<SurfaceCandidateFingerprint>,
    active_object: Option<ActiveInlineObject>,
    dispatched_pages: Vec<String>,
    dispatched_object_pages: Vec<String>,
    dispatched_mutations: Vec<String>,
    dispatched_clipboard_write: Option<crate::ClipboardKey>,
    edits: String,
    clipboard: String,
    pending_clipboard_page: bool,
    clipboard_cut_proofs: Option<String>,
    pending_page_aliases: usize,
    settlement_count: usize,
    segmentation: Option<String>,
    segmentation_action: Option<String>,
    platform: Option<String>,
    restoration: Option<String>,
    restoration_seed: Option<RangeRestorationSeed>,
    published_restoration: Option<RangeRestorationSeed>,
    replacement: Option<String>,
    pending_history: Option<String>,
    history_frontier: crate::RangeHistoryFrontier,
    mutation_positions: Option<String>,
    adopted_positions: Option<String>,
    admitted_edit_proofs: Vec<String>,
    mutation_composition: Option<String>,
    pending_local_mutation: Option<String>,
    prepared_local_operation: Option<crate::OperationId>,
    platform_ready: Option<String>,
    pending_select_all: bool,
    pointer_anchor: Option<crate::SourcePosition>,
    scrollbar_owner: gpui_scrollbar::ScrollbarOwnerKey,
    scrollbar_state_owner: Option<gpui_scrollbar::ScrollbarOwnerKey>,
    scrollbar_model: Option<gpui_scrollbar::ScrollbarScrollState>,
}

#[derive(Debug, PartialEq)]
struct TerminalFingerprint {
    surface: Option<String>,
    desired: DesiredFingerprint,
    requests: Vec<String>,
    admission: Option<RangeSurfaceCharge>,
    next_id: u64,
    text_residency: TextResidencyFingerprint,
    object_residency: ObjectResidencyFingerprint,
    lifecycle: LifecycleFingerprint,
    transition: TransitionFingerprint,
}

#[derive(Debug, PartialEq)]
struct TextResidencyFingerprint {
    counts: String,
    pending: Vec<crate::PageRequestKey>,
    pages: Vec<TextPageFingerprint>,
}

#[derive(Debug, PartialEq)]
struct TextPageFingerprint {
    id: PageId,
    key: crate::PageRequestKey,
    range: crate::ByteRange,
    text: String,
    atoms: Vec<(crate::AtomId, crate::ByteRange, crate::ByteRange, String)>,
    preceding: PageEdgeFact,
    following: PageEdgeFact,
    end_of_source: bool,
}

#[derive(Debug, PartialEq)]
struct ObjectResidencyFingerprint {
    counts: String,
    pending: Vec<crate::ObjectRequestKey>,
    pages: Vec<ObjectPageFingerprint>,
}

#[derive(Debug, PartialEq)]
struct ObjectPageFingerprint {
    id: ObjectPageId,
    key: crate::ObjectRequestKey,
    objects: Vec<ObjectFactFingerprint>,
    preceding: ObjectPageEdgeFact,
    following: ObjectPageEdgeFact,
    complete: bool,
    continuation: Option<crate::ObjectCursor>,
}

#[derive(Debug, PartialEq)]
struct ObjectFactFingerprint {
    id: InlineObjectId,
    anchor: ByteOffset,
    order: InlineObjectOrder,
    fallback: String,
    presentation_key: u64,
    display: String,
    width: gpui::Pixels,
    height: gpui::Pixels,
    baseline: gpui::Pixels,
    background: Option<gpui::Hsla>,
    semantic_state: u64,
    activation_eligible: bool,
}

#[derive(Debug, PartialEq)]
struct LifecycleFingerprint {
    enabled: bool,
    read_only: bool,
    mounted: bool,
    last_bounds: Option<gpui::Bounds<gpui::Pixels>>,
    has_focus_subscription: bool,
}

fn desired_fingerprint(desired: DesiredSurface) -> DesiredFingerprint {
    (
        desired.source_selection,
        desired.composition,
        desired.scroll.source,
        desired.scroll.intra_anchor,
        desired.target_block,
        desired.viewport_extent,
        desired.overscan,
        desired.preserve_scroll_anchor,
        desired.reveal_caret,
        desired.inline_object_interaction,
    )
}

fn sorted_debug<'a, T: std::fmt::Debug + 'a>(
    values: impl IntoIterator<Item = &'a T>,
) -> Vec<String> {
    let mut values: Vec<_> = values
        .into_iter()
        .map(|value| format!("{value:?}"))
        .collect();
    values.sort_unstable();
    values
}

fn transition_fingerprint(input: &RangeTextInput) -> TransitionFingerprint {
    let active_geometry = input.active_geometry;
    let pending_geometry_page =
        input
            .pending_geometry_page
            .as_ref()
            .map(|pending| PendingGeometryPageFingerprint {
                job: pending.job,
                request: pending.request.key(),
                wait: match pending.wait {
                    geometry::GeometryPageWait::Resident(page) => {
                        GeometryPageWaitFingerprint::Resident(page)
                    }
                    geometry::GeometryPageWait::Coalesced(request) => {
                        GeometryPageWaitFingerprint::Coalesced(request)
                    }
                },
            });
    let pending_geometry_object =
        input
            .pending_geometry_object
            .as_ref()
            .map(|pending| PendingGeometryObjectFingerprint {
                job: pending.job,
                request: pending.request.key(),
                text_page: pending.text_page,
                wait: match pending.wait {
                    geometry::GeometryObjectWait::Resident(page) => {
                        GeometryObjectWaitFingerprint::Resident(page)
                    }
                    geometry::GeometryObjectWait::Coalesced(request) => {
                        GeometryObjectWaitFingerprint::Coalesced(request)
                    }
                },
            });
    let surface_candidate = input
        .surface_candidate
        .map(|candidate| SurfaceCandidateFingerprint {
            job: candidate.job,
            binding: candidate.binding,
            desired: desired_fingerprint(candidate.desired),
            restoration: candidate.restoration,
        });
    TransitionFingerprint {
        geometry: GeometryFingerprint {
            key: input.geometry.key(),
            desired_target: input.geometry.desired_target_key(),
            counts: input.geometry.counts(),
            high_water: (
                input.geometry.retained_high_water_bytes(),
                input.geometry.retained_high_water_items(),
            ),
            active_text_page: active_geometry.and_then(|job| input.geometry.active_text_page(job)),
            index: input.geometry.index().map(|index| format!("{index:?}")),
            target: input.geometry.target().map(|target| format!("{target:?}")),
        },
        active_geometry,
        pending_geometry_page,
        pending_geometry_object,
        surface_candidate,
        active_object: input.active_object,
        dispatched_pages: sorted_debug(&input.dispatched_pages),
        dispatched_object_pages: sorted_debug(&input.dispatched_object_pages),
        dispatched_mutations: sorted_debug(&input.dispatched_mutations),
        dispatched_clipboard_write: input.dispatched_clipboard_write,
        edits: format!("{:?}", input.edits),
        clipboard: format!("{:?}", input.clipboard),
        pending_clipboard_page: input.pending_clipboard_page.is_some(),
        clipboard_cut_proofs: input
            .clipboard_cut_proofs
            .as_ref()
            .map(|proofs| format!("{proofs:?}")),
        pending_page_aliases: input.pending_page_aliases.len(),
        settlement_count: input.config.settlement_coordinator.retained_count(),
        segmentation: input
            .segmentation
            .as_ref()
            .map(|state| format!("{state:?}")),
        segmentation_action: input
            .segmentation_action
            .as_ref()
            .map(|action| format!("{action:?}")),
        platform: input.platform.as_ref().map(|state| format!("{state:?}")),
        restoration: input.restoration.as_ref().map(|state| format!("{state:?}")),
        restoration_seed: input.restoration_seed,
        published_restoration: input.published_restoration,
        replacement: input.replacement.as_ref().map(|state| format!("{state:?}")),
        pending_history: input
            .pending_history
            .as_ref()
            .map(|state| format!("{state:?}")),
        history_frontier: input.history_frontier,
        mutation_positions: input
            .mutation_positions
            .as_ref()
            .map(|state| format!("{state:?}")),
        adopted_positions: input
            .adopted_positions
            .as_ref()
            .map(|state| format!("{state:?}")),
        admitted_edit_proofs: input
            .admitted_edit_proofs
            .iter()
            .map(|proof| format!("{proof:?}"))
            .collect(),
        mutation_composition: input
            .mutation_composition
            .as_ref()
            .map(|state| format!("{state:?}")),
        pending_local_mutation: input
            .pending_local_mutation
            .as_ref()
            .map(|state| format!("{state:?}")),
        prepared_local_operation: input.prepared_local_operation,
        platform_ready: input
            .platform_ready
            .as_ref()
            .map(|state| format!("{state:?}")),
        pending_select_all: input.pending_select_all,
        pointer_anchor: input.pointer_anchor,
        scrollbar_owner: input.scrollbar.owner,
        scrollbar_state_owner: input.scrollbar.state.current_owner(),
        scrollbar_model: input.scrollbar.model.get(),
    }
}

fn text_residency_fingerprint(input: &RangeTextInput) -> TextResidencyFingerprint {
    TextResidencyFingerprint {
        counts: format!("{:?}", input.residency.counts()),
        pending: input.residency.pending_requests().collect(),
        pages: input
            .residency
            .resident_pages()
            .map(|page| TextPageFingerprint {
                id: page.id(),
                key: page.key(),
                range: page.range(),
                text: page.text().to_owned(),
                atoms: page
                    .atoms()
                    .iter()
                    .map(|atom| {
                        (
                            atom.id(),
                            atom.global_range(),
                            atom.fragment_range(),
                            atom.fallback_copy().to_owned(),
                        )
                    })
                    .collect(),
                preceding: page.preceding(),
                following: page.following(),
                end_of_source: page.end_of_source(),
            })
            .collect(),
    }
}

fn object_residency_fingerprint(input: &RangeTextInput) -> ObjectResidencyFingerprint {
    ObjectResidencyFingerprint {
        counts: format!("{:?}", input.object_residency.counts()),
        pending: input.object_residency.pending_requests().collect(),
        pages: input
            .object_residency
            .resident_pages()
            .map(|page| ObjectPageFingerprint {
                id: page.id(),
                key: page.key(),
                objects: page
                    .objects()
                    .iter()
                    .map(|object| {
                        let presentation = object.presentation();
                        ObjectFactFingerprint {
                            id: object.id(),
                            anchor: object.anchor(),
                            order: object.order(),
                            fallback: object.fallback_copy().to_owned(),
                            presentation_key: presentation.presentation_key(),
                            display: presentation.display().to_string(),
                            width: presentation.width(),
                            height: presentation.height(),
                            baseline: presentation.baseline(),
                            background: presentation.background(),
                            semantic_state: presentation.semantic_state(),
                            activation_eligible: presentation.activation_eligible(),
                        }
                    })
                    .collect(),
                preceding: page.preceding(),
                following: page.following(),
                complete: page.complete(),
                continuation: page.continuation(),
            })
            .collect(),
    }
}

fn fingerprint(input: &RangeTextInput) -> TerminalFingerprint {
    TerminalFingerprint {
        surface: input.surface.as_ref().map(|surface| format!("{surface:?}")),
        desired: desired_fingerprint(input.desired),
        requests: input
            .requests
            .iter()
            .map(|request| format!("{request:?}"))
            .collect(),
        admission: input.last_surface_admission,
        next_id: input.next_id,
        text_residency: text_residency_fingerprint(input),
        object_residency: object_residency_fingerprint(input),
        lifecycle: LifecycleFingerprint {
            enabled: input.enabled,
            read_only: input.read_only,
            mounted: input.mounted,
            last_bounds: input.last_bounds,
            has_focus_subscription: input.focus_subscription.is_some(),
        },
        transition: transition_fingerprint(input),
    }
}

fn captured_events(
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

fn drive_local_insert_to_commit_pending(
    input: &gpui::Entity<RangeTextInput>,
    cx: &mut gpui::VisualTestContext,
) -> (crate::MutationKey, crate::MutationPositions) {
    let (key, intended) = drive_local_insert_to_finish_pending(input, cx);
    input.update(cx, |input, cx| {
        input.accept_mutation_finish(key, cx).unwrap()
    });
    assert!(matches!(
        input.update(cx, |input, _| input.take_request()),
        Some(RangeTextInputRequest::MutationCommit(commit)) if commit.key() == key
    ));
    (key, intended)
}

fn drive_local_insert_to_finish_pending(
    input: &gpui::Entity<RangeTextInput>,
    cx: &mut gpui::VisualTestContext,
) -> (crate::MutationKey, crate::MutationPositions) {
    let current_position = input.read_with(cx, |input, _| {
        input.surface.as_ref().unwrap().selection().head
    });
    let (_, current_text, current_objects) =
        admitted_successor_sources(SOURCE, 1, &[current_position]);
    input.update(cx, |input, _| {
        input
            .admit_edit_positions(&[current_position], &current_text, &current_objects)
            .unwrap();
    });
    input.update(cx, |input, cx| {
        input.insert_text("x".to_owned(), cx).unwrap()
    });
    let RangeTextInputRequest::MutationBegin(begin) =
        input.update(cx, |input, _| input.take_request()).unwrap()
    else {
        panic!("local mutation begin")
    };
    let key = begin.proposal().key();
    input.update(cx, |input, cx| {
        input.accept_mutation_preflight(key, cx).unwrap()
    });
    let mut intended = None;
    while let Some(request) = input.update(cx, |input, _| input.take_request()) {
        if let RangeTextInputRequest::MutationFinishInput(finish) = request {
            intended = Some(finish.intended());
        }
    }
    let intended = intended.expect("local mutation finish");
    (key, intended)
}

#[gpui::test]
fn terminal_target_replacement_accepts_fixed_exact_caps_and_rejects_one_under(
    cx: &mut gpui::TestAppContext,
) {
    const EXACT_BYTES: usize = 18_989;
    const EXACT_ITEMS: usize = 104;
    for (bytes, items, succeeds) in [
        (EXACT_BYTES, 32_768, true),
        (EXACT_BYTES - 1, 32_768, false),
        (2 * 1024 * 1024, EXACT_ITEMS, true),
        (2 * 1024 * 1024, EXACT_ITEMS - 1, false),
    ] {
        let (input, cx) = cx.add_window_view(|window, cx| {
            RangeTextInput::new(config(2 * 1024 * 1024, 32_768), window, cx).unwrap()
        });
        drive_initial_surface(&input, cx);
        let resident_charge = input.update(cx, |input, _| install_resident_payloads(input));
        input.update(cx, |input, _| {
            input.config.limits.max_surface_bytes = bytes;
            input.config.limits.max_surface_items = items;
        });
        assert_eq!(
            resident_charge,
            RangeSurfaceCharge {
                bytes: 783,
                items: 3,
            }
        );
        let events = captured_events(&input, cx);
        let before = input.read_with(cx, |input, _| fingerprint(input));
        let event_count = events.borrow().len();
        let result = input.update(cx, |input, cx| {
            input.request_absolute_scroll(px(100_000.), cx)
        });
        assert_eq!(result.is_ok(), succeeds, "{result:?}");
        if succeeds {
            let components = input.read_with(cx, |input, _| {
                input.last_widget_admission_components.get().unwrap()
            });
            assert_eq!(
                components,
                WidgetAdmissionComponents {
                    prior_surface: RangeSurfaceCharge {
                        bytes: 5_470,
                        items: 90,
                    },
                    current_request_storage: RangeSurfaceCharge {
                        bytes: 560,
                        items: 1,
                    },
                    mutation_request_payload: RangeSurfaceCharge::default(),
                    candidate_record: RangeSurfaceCharge {
                        bytes: 8_768,
                        items: 1,
                    },
                    geometry: RangeSurfaceCharge {
                        bytes: 528,
                        items: 1,
                    },
                    resident_payload: resident_charge,
                    publication_allocation: RangeSurfaceCharge {
                        bytes: 640,
                        items: 4,
                    },
                    effect_storage: RangeSurfaceCharge {
                        bytes: 1_680,
                        items: 3,
                    },
                    event_storage: RangeSurfaceCharge { bytes: 0, items: 0 },
                    page_demand: RangeSurfaceCharge { bytes: 0, items: 0 },
                    object_rebind: RangeSurfaceCharge { bytes: 0, items: 0 },
                    residency_rebind: RangeSurfaceCharge { bytes: 0, items: 0 },
                    detached_edit_storage: RangeSurfaceCharge { bytes: 0, items: 0 },
                    destination_request_storage: RangeSurfaceCharge {
                        bytes: 560,
                        items: 1,
                    },
                    proof_storage: RangeSurfaceCharge { bytes: 0, items: 0 },
                }
            );
            assert_eq!(
                components.checked_total(),
                Some(RangeSurfaceCharge {
                    bytes: EXACT_BYTES,
                    items: EXACT_ITEMS,
                })
            );
            let after = input.read_with(cx, |input, _| fingerprint(input));
            assert_ne!(
                after, before,
                "admitted transition must change the fingerprint"
            );
            assert_eq!(
                input.read_with(cx, |input, _| input.last_surface_admission),
                Some(RangeSurfaceCharge {
                    bytes: EXACT_BYTES,
                    items: EXACT_ITEMS,
                })
            );
        } else {
            assert!(matches!(result, Err(RangeTextInputError::SurfaceCapacity)));
            input.read_with(cx, |input, _| assert_eq!(fingerprint(input), before));
            assert_eq!(events.borrow().len(), event_count);
        }
    }
}

#[gpui::test]
fn prepaint_stops_before_nonresident_successor_and_defers_explicit_progress(
    cx: &mut gpui::TestAppContext,
) {
    let source = (0..48)
        .map(|line| format!("line-{line:02}\n"))
        .collect::<String>();
    let mut configuration = config(2 * 1024 * 1024, 32_768);
    configuration.binding = RangeBinding::new(
        BindingId::new(71),
        SourceRevision::new(1),
        LogicalExtent::new(source.len() as u64, 48),
    );
    configuration.residency_limits = ResidencyLimits::new(3, 128 * 1024, 3, 256).unwrap();
    let (input, cx) =
        cx.add_window_view(|window, cx| RangeTextInput::new(configuration, window, cx).unwrap());
    drive_surface_for_source(&input, cx, &source);

    input.update(cx, |input, _| {
        let PageDemand::Requested(text_request) = input
            .residency
            .demand(
                PageRequestId::new(90_000),
                PagePurpose::Caret,
                PageDemandEnvelope::Adjacent {
                    anchor: ByteOffset::new(0),
                    direction: PageDirection::Forward,
                    max_payload_bytes: 32,
                },
            )
            .unwrap()
        else {
            panic!("fresh retained text request")
        };
        input
            .residency
            .admit(page_for_source(text_request, 90_000, &source))
            .unwrap();
    });

    input.update(cx, |input, _| {
        let mut desired = input.desired;
        desired.target_block = px(160.);
        let candidate = input.prepare_target_transition(desired, None).unwrap();
        input.commit_widget_transition_internal(candidate);
    });
    input.read_with(cx, |input, _| {
        assert!(
            input.requests.is_empty(),
            "transition must begin from residency: {:?}",
            input.requests
        );
        assert!(matches!(
            input
                .pending_geometry_page
                .as_ref()
                .map(|pending| pending.wait),
            Some(GeometryPageWait::Resident(_))
        ));
    });

    let before = input.read_with(cx, |input, _| fingerprint(input));
    let pending_key = input.read_with(cx, |input, _| {
        input.pending_geometry_page.as_ref().unwrap().request.key()
    });
    input.update(cx, |input, _| input.config.limits.max_surface_bytes = 1);
    let rejected = cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.service_admitted_geometry_for_prepaint(window, cx)
        })
    });
    assert!(matches!(
        rejected,
        Err(RangeTextInputError::SurfaceCapacity)
    ));
    input.read_with(cx, |input, _| {
        assert_eq!(fingerprint(input), before);
        assert_eq!(
            input.pending_geometry_page.as_ref().unwrap().request.key(),
            pending_key
        );
        assert!(input.requests.is_empty());
    });

    input.update(cx, |input, _| {
        input.config.limits.max_surface_bytes = 2 * 1024 * 1024
    });
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            let before = fingerprint(input);
            input
                .service_admitted_geometry_for_prepaint(window, cx)
                .unwrap();
            assert_eq!(fingerprint(input), before);
            assert!(input.requests.is_empty());
        })
    });
    cx.run_until_parked();
    input.read_with(cx, |input, _| {
        assert!(input.requests.iter().any(|request| matches!(
            request,
            RangeTextInputRequest::Page(_) | RangeTextInputRequest::ObjectPage(_)
        )));
        assert_ne!(fingerprint(input), before);
    });
}

#[gpui::test]
fn prepaint_defers_terminal_resident_index_object_publication(cx: &mut gpui::TestAppContext) {
    let (input, cx) = cx.add_window_view(|window, cx| {
        RangeTextInput::new(config(2 * 1024 * 1024, 32_768), window, cx).unwrap()
    });
    drive_initial_surface(&input, cx);
    let events = captured_events(&input, cx);

    let event_count = events.borrow().len();
    let before = cx.update(|window, app| {
        input.update(app, |input, cx| {
            install_empty_terminal_residency(input, 10, ObjectPurpose::GeometryIndex);
            install_empty_terminal_object_page(input, 11, ObjectPurpose::GeometryTarget);
            input.desired.source_selection = None;
            input.desired.composition = None;
            input.desired.target_block = px(1_000.);
            input.desired.viewport_extent = px(0.);
            input.desired.overscan = px(0.);
            input.start_index().unwrap();
            input
                .service_resident_index_page(window, cx, true)
                .expect("resident index text service")
                .unwrap();
            assert!(input.pending_geometry_object.is_some());
            assert_eq!(
                input
                    .pending_geometry_object
                    .as_ref()
                    .unwrap()
                    .request
                    .key()
                    .purpose(),
                ObjectPurpose::GeometryIndex
            );
            while let Some(request) = input.take_request() {
                assert!(matches!(
                    request,
                    RangeTextInputRequest::ReleasePage(_)
                        | RangeTextInputRequest::ReleaseObjectPage(_)
                ));
            }
            let before = fingerprint(input);
            input
                .service_admitted_geometry_for_prepaint(window, cx)
                .unwrap();
            assert_eq!(fingerprint(input), before);
            assert!(input.requests.is_empty());
            assert_eq!(events.borrow().len(), event_count);
            before
        })
    });

    cx.run_until_parked();
    input.read_with(cx, |input, _| {
        assert_ne!(fingerprint(input), before);
        assert!(input.pending_geometry_object.is_none());
        assert!(input.active_geometry.is_none());
        assert!(input.geometry.index().is_some());
        assert!(input.surface.is_some());
    });
}

#[gpui::test]
fn prepaint_defers_terminal_resident_target_object_publication(cx: &mut gpui::TestAppContext) {
    let (input, cx) = cx.add_window_view(|window, cx| {
        RangeTextInput::new(config(2 * 1024 * 1024, 32_768), window, cx).unwrap()
    });
    drive_initial_surface(&input, cx);
    let events = captured_events(&input, cx);
    let event_count = events.borrow().len();

    let before = cx.update(|window, app| {
        input.update(app, |input, cx| {
            install_empty_terminal_residency(input, 10, ObjectPurpose::GeometryTarget);
            let surface = input.surface.as_ref().expect("prior surface");
            let geometry = surface.geometry_key();
            let position = surface.selection().head;
            input.active_object = Some(ActiveInlineObject {
                anchor: crate::RealizedInlineObjectAnchor {
                    binding: surface.binding(),
                    object_id: InlineObjectId::new(99),
                    order: InlineObjectOrder::new(1),
                    presentation_generation: geometry.presentation_generation(),
                    layout_epoch: geometry.epoch(),
                    bounds: gpui::Bounds::default(),
                },
                leading: position,
                trailing: position,
                activation_eligible: true,
            });
            let candidate = input
                .prepare_target_transition(input.desired, None)
                .unwrap();
            input.commit_widget_transition_internal(candidate);
            input
                .service_resident_target_page(window, cx, true)
                .expect("resident target text service")
                .unwrap();
            assert_eq!(
                input
                    .pending_geometry_object
                    .as_ref()
                    .unwrap()
                    .request
                    .key()
                    .purpose(),
                ObjectPurpose::GeometryTarget
            );
            while let Some(request) = input.take_request() {
                assert!(matches!(
                    request,
                    RangeTextInputRequest::ReleasePage(_)
                        | RangeTextInputRequest::ReleaseObjectPage(_)
                ));
            }
            let before = fingerprint(input);
            input
                .service_admitted_geometry_for_prepaint(window, cx)
                .unwrap();
            assert_eq!(fingerprint(input), before);
            assert!(input.requests.is_empty());
            assert_eq!(events.borrow().len(), event_count);
            before
        })
    });

    cx.run_until_parked();
    input.read_with(cx, |input, _| {
        assert_ne!(fingerprint(input), before);
        assert!(input.pending_geometry_object.is_none());
        assert!(input.active_geometry.is_none());
        assert!(input.surface.is_some());
        assert!(input.active_object.is_none());
    });
    assert!(events.borrow().len() > event_count);
    assert!(events.borrow().iter().any(|event| matches!(
        event,
        RangeTextInputEvent::InlineObjectRealizationLost(loss)
            if loss.reason == crate::InlineObjectRealizationLossReason::Unrealized
    )));
}

#[gpui::test]
fn terminal_object_response_counts_release_and_deferred_event_records_exactly(
    cx: &mut gpui::TestAppContext,
) {
    const EXACT_ITEMS: usize = 209;
    let (probe, probe_cx) = cx.add_window_view(|window, cx| {
        RangeTextInput::new(config(2 * 1024 * 1024, 32_768), window, cx).unwrap()
    });
    drive_initial_surface(&probe, probe_cx);
    let request = stage_external_terminal_target_object(&probe, probe_cx);
    let key = request.key();
    let page = || {
        ObjectPage::new(
            ObjectPageId::new(71),
            key,
            vec![],
            ObjectPageEdgeFact::EnvelopeBoundary,
            ObjectPageEdgeFact::EnvelopeBoundary,
            true,
            None,
        )
        .unwrap()
    };
    let components = probe_cx.update(|window, app| {
        probe.update(app, |input, _| {
            let (job, text_page_id) = {
                let pending = input.pending_geometry_object.as_ref().unwrap();
                (pending.job, pending.text_page)
            };
            let page = page();
            let proofs = input
                .residency
                .prove_object_page_anchors(input.config.binding, &page)
                .unwrap();
            let object_admission = input.object_residency.prepare_admit(page, proofs).unwrap();
            let successor = input.target_response_successor().unwrap();
            let geometry = {
                let text_page = input.residency.peek_page_by_id(text_page_id).unwrap();
                input
                    .geometry
                    .prepare_target_object_page(
                        job,
                        text_page,
                        object_admission.page(),
                        window.text_system(),
                        successor,
                    )
                    .unwrap()
            };
            assert_eq!(geometry.progress(), ExactGeometryProgress::TargetComplete);
            let candidate = input
                .prepare_terminal_response_publication(
                    geometry,
                    None,
                    Some(object_admission),
                    Some(text_page_id),
                    None,
                    None,
                    Some(key),
                    None,
                )
                .unwrap();
            input.terminal_response_item_components(&candidate).unwrap()
        })
    });
    assert_eq!(
        components,
        TerminalResponseItemComponents {
            prior_surface: 90,
            current_request_storage: 1,
            candidate_record: 1,
            geometry: 90,
            page_admission_allocation: 1,
            resident_payload: 2,
            publication_allocation: 21,
            destination_request_storage: 1,
            release_records: 1,
            deferred_events: 1,
        }
    );
    assert_eq!(components.checked_total(), Some(EXACT_ITEMS));

    for (items, succeeds) in [(EXACT_ITEMS, true), (EXACT_ITEMS - 1, false)] {
        let (input, cx) = cx.add_window_view(|window, cx| {
            RangeTextInput::new(config(2 * 1024 * 1024, 32_768), window, cx).unwrap()
        });
        drive_initial_surface(&input, cx);
        let request = stage_external_terminal_target_object(&input, cx);
        let key = request.key();
        let page = ObjectPage::new(
            ObjectPageId::new(71),
            key,
            vec![],
            ObjectPageEdgeFact::EnvelopeBoundary,
            ObjectPageEdgeFact::EnvelopeBoundary,
            true,
            None,
        )
        .unwrap();
        let events = captured_events(&input, cx);
        let event_count = events.borrow().len();
        let before = input.read_with(cx, |input, _| fingerprint(input));
        input.update(cx, |input, _| input.config.limits.max_surface_items = items);
        let result = cx.update(|window, app| {
            input.update(app, |input, cx| {
                input.deliver_object_page_in_window(page, window, cx)
            })
        });
        assert_eq!(result.is_ok(), succeeds, "{result:?}");
        if succeeds {
            let releases = input.update(cx, |input, _| {
                std::iter::from_fn(|| input.take_request()).collect::<Vec<_>>()
            });
            assert_eq!(releases.len(), 1);
            assert!(matches!(
                releases.as_slice(),
                [RangeTextInputRequest::ReleaseObjectPage(released)] if *released == key
            ));
            assert_eq!(events.borrow().len(), event_count + 1);
            input.read_with(cx, |input, _| assert!(input.active_object.is_none()));
        } else {
            assert!(matches!(result, Err(RangeTextInputError::SurfaceCapacity)));
            input.read_with(cx, |input, _| {
                assert_eq!(fingerprint(input), before);
                assert!(input.dispatched_object_pages.contains(&key));
                assert!(input.active_object.is_some());
            });
            assert_eq!(events.borrow().len(), event_count);
        }
    }
}

#[gpui::test]
fn geometry_index_text_response_rejection_preserves_identical_key_for_nonterminal_retry(
    cx: &mut gpui::TestAppContext,
) {
    let (input, cx) = cx.add_window_view(|window, cx| {
        RangeTextInput::new(config(2 * 1024 * 1024, 32_768), window, cx).unwrap()
    });
    let RangeTextInputRequest::Page(request) =
        input.update(cx, |input, _| input.take_request()).unwrap()
    else {
        panic!("initial geometry-index text request")
    };
    assert_eq!(request.key().purpose(), PagePurpose::GeometryIndex);
    let key = request.key();
    let before = input.read_with(cx, |input, _| fingerprint(input));
    input.update(cx, |input, _| input.config.limits.max_surface_bytes = 1);
    let rejected = cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.deliver_page(page_for(request, 1), window, cx)
        })
    });
    assert!(matches!(
        rejected,
        Err(RangeTextInputError::SurfaceCapacity)
    ));
    input.read_with(cx, |input, _| {
        assert_eq!(fingerprint(input), before);
        assert!(input.dispatched_pages.contains(&key));
    });

    input.update(cx, |input, _| {
        input.config.limits.max_surface_bytes = 2 * 1024 * 1024
    });
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input
                .deliver_page(page_for(request, 1), window, cx)
                .unwrap()
        })
    });
    input.read_with(cx, |input, _| {
        assert_ne!(fingerprint(input), before);
        assert!(!input.dispatched_pages.contains(&key));
        assert!(input.pending_geometry_object.is_some());
    });
}

#[gpui::test]
fn geometry_index_object_response_rejection_preserves_identical_key_for_terminal_retry(
    cx: &mut gpui::TestAppContext,
) {
    let (input, cx) = cx.add_window_view(|window, cx| {
        RangeTextInput::new(config(2 * 1024 * 1024, 32_768), window, cx).unwrap()
    });
    let RangeTextInputRequest::Page(text_request) =
        input.update(cx, |input, _| input.take_request()).unwrap()
    else {
        panic!("initial geometry-index text request")
    };
    let atom_range = ByteRange::from_u64(0, 1).unwrap();
    let text_page = RangePage::new(
        PageId::new(1),
        text_request.key(),
        ByteRange::from_u64(0, SOURCE.len() as u64).unwrap(),
        SOURCE.to_owned(),
        vec![AtomFact::new(
            AtomId::new(1),
            atom_range,
            atom_range,
            "resident-object-fallback",
        )],
        PageEdgeFact::DocumentBoundary,
        PageEdgeFact::DocumentBoundary,
        true,
    )
    .unwrap();
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.deliver_page(text_page, window, cx).unwrap()
        })
    });
    let request = loop {
        match input.update(cx, |input, _| input.take_request()).unwrap() {
            RangeTextInputRequest::ObjectPage(request) => break request,
            RangeTextInputRequest::ReleasePage(_) => {}
            other => panic!("unexpected geometry-index request: {other:?}"),
        }
    };
    assert_eq!(request.key().purpose(), ObjectPurpose::GeometryIndex);
    let key = request.key();
    let page = || {
        ObjectPage::new(
            ObjectPageId::new(2),
            key,
            vec![],
            ObjectPageEdgeFact::EnvelopeBoundary,
            ObjectPageEdgeFact::EnvelopeBoundary,
            true,
            None,
        )
        .unwrap()
    };
    let before = input.read_with(cx, |input, _| fingerprint(input));
    input.update(cx, |input, _| input.config.limits.max_surface_bytes = 1);
    let rejected = cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.deliver_object_page_in_window(page(), window, cx)
        })
    });
    assert!(matches!(
        rejected,
        Err(RangeTextInputError::SurfaceCapacity)
    ));
    input.read_with(cx, |input, _| {
        assert_eq!(fingerprint(input), before);
        assert!(input.dispatched_object_pages.contains(&key));
    });

    input.update(cx, |input, _| {
        input.config.limits.max_surface_bytes = 2 * 1024 * 1024
    });
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input
                .deliver_object_page_in_window(page(), window, cx)
                .unwrap()
        })
    });
    input.read_with(cx, |input, _| {
        assert_ne!(fingerprint(input), before);
        assert!(!input.dispatched_object_pages.contains(&key));
        assert!(input.geometry.index().is_some());
    });
}

#[gpui::test]
fn active_object_candidate_accepts_exact_fit_and_rejects_one_under_atomically(
    cx: &mut gpui::TestAppContext,
) {
    let (input, cx) = cx.add_window_view(|window, cx| {
        RangeTextInput::new(config(2 * 1024 * 1024, 32_768), window, cx).unwrap()
    });
    drive_initial_surface(&input, cx);
    input.update(cx, |input, _| {
        let surface = input.surface.as_ref().expect("initial surface");
        let geometry = surface.geometry_key();
        let position = surface.selection().head;
        input.active_object = Some(ActiveInlineObject {
            anchor: crate::RealizedInlineObjectAnchor {
                binding: surface.binding(),
                object_id: InlineObjectId::new(99),
                order: InlineObjectOrder::new(1),
                presentation_generation: geometry.presentation_generation(),
                layout_epoch: geometry.epoch(),
                bounds: gpui::Bounds::default(),
            },
            leading: position,
            trailing: position,
            activation_eligible: true,
        });
        let transition = super::transition::ActiveObjectTransition::Clear(
            crate::InlineObjectRealizationLossReason::SelectionChanged,
        );
        let exact = input
            .prepare_active_object_transition(transition)
            .expect("probe candidate")
            .admission_charge();
        let before = fingerprint(input);
        input.config.limits.max_surface_bytes = exact.bytes - 1;
        assert!(matches!(
            input.prepare_active_object_transition(transition),
            Err(RangeTextInputError::SurfaceCapacity)
        ));
        assert_eq!(fingerprint(input), before);
        input.config.limits.max_surface_bytes = exact.bytes;
        input.config.limits.max_surface_items = exact.items;
        let candidate = input
            .prepare_active_object_transition(transition)
            .expect("exact fit");
        assert_eq!(candidate.admission_charge(), exact);
    });
}

#[gpui::test]
fn rejected_terminal_edit_is_quiescent_restorable_and_late_result_is_inert(
    cx: &mut gpui::TestAppContext,
) {
    let (input, cx) = cx.add_window_view(|window, cx| {
        RangeTextInput::new(config(2 * 1024 * 1024, 32_768), window, cx).unwrap()
    });
    drive_initial_surface(&input, cx);
    let current_position = input.read_with(cx, |input, _| {
        input.surface.as_ref().unwrap().selection().head
    });
    let (_, current_text, current_objects) =
        admitted_successor_sources(SOURCE, 1, &[current_position]);
    input.update(cx, |input, _| {
        input
            .admit_edit_positions(&[current_position], &current_text, &current_objects)
            .unwrap();
    });
    input.update(cx, |input, cx| {
        input.insert_text("x".to_owned(), cx).unwrap()
    });
    let RangeTextInputRequest::MutationBegin(begin) =
        input.update(cx, |input, _| input.take_request()).unwrap()
    else {
        panic!("insert begin")
    };
    let key = begin.proposal().key();
    input.update(cx, |input, cx| {
        input.accept_mutation_preflight(key, cx).unwrap()
    });
    while let Some(request) = input.update(cx, |input, _| input.take_request()) {
        if matches!(request, RangeTextInputRequest::MutationFinishInput(_)) {
            input.update(cx, |input, cx| {
                input.accept_mutation_finish(key, cx).unwrap()
            });
        }
    }
    let settlement = cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.settle_mutation(key, crate::MutationOutcome::Rejected, window, cx)
        })
    });
    assert_eq!(
        settlement.unwrap(),
        crate::MutationSettlement::Current(crate::MutationOutcome::Rejected)
    );
    let (seed, released) = input.read_with(cx, |input, _| {
        assert!(input.is_quiescent());
        (
            input.export_restoration(None).unwrap(),
            input.edits.released_counts(),
        )
    });
    let late = cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.settle_mutation(key, crate::MutationOutcome::Rejected, window, cx)
        })
    });
    assert!(matches!(
        late,
        Err(RangeTextInputError::Mutation(
            crate::MutationError::ObsoleteOperation(obsolete)
        )) if obsolete == key
    ));
    input.read_with(cx, |input, _| {
        assert!(input.is_quiescent());
        assert_eq!(input.export_restoration(None).unwrap(), seed);
        assert_eq!(input.edits.released_counts(), released);
    });
}

#[gpui::test]
fn rejected_page_with_valid_prefix_is_atomic_and_exact_retry_succeeds(
    cx: &mut gpui::TestAppContext,
) {
    let (input, cx) = cx.add_window_view(|window, cx| {
        RangeTextInput::new(config(2 * 1024 * 1024, 32_768), window, cx).unwrap()
    });
    drive_initial_surface(&input, cx);
    let current_position = input.read_with(cx, |input, _| {
        input.surface.as_ref().unwrap().selection().head
    });
    let (_, current_text, current_objects) =
        admitted_successor_sources(SOURCE, 1, &[current_position]);
    let binding = input.read_with(cx, |input, _| input.edits.binding());
    let key = crate::MutationKey::new(
        binding.binding(),
        binding.revision(),
        crate::OperationId::new(1),
    );
    let proposal = crate::MutationProposal::new(
        key,
        crate::MutationKind::Edit,
        crate::MutationPositions::collapsed(current_position),
        crate::SourceRange::new(current_position, current_position).unwrap(),
        0,
    );
    let begin = crate::MutationBeginRequest::new(
        proposal,
        crate::MutationCursor::new(10),
        crate::MutationCursor::new(20),
    );
    input.update(cx, |input, cx| {
        input
            .begin_host_mutation(
                begin,
                &[current_position],
                &current_text,
                &current_objects,
                cx,
            )
            .unwrap()
    });
    assert!(matches!(
        input.update(cx, |input, _| input.take_request()),
        Some(RangeTextInputRequest::MutationBegin(_))
    ));
    input.update(cx, |input, cx| {
        input.accept_mutation_preflight(key, cx).unwrap()
    });
    let invalid = crate::MutationPage::new(
        crate::MutationPageKey::new(
            key,
            crate::MutationLane::Proposal,
            crate::MutationCursor::new(20),
            0,
            crate::MutationIdentity::ROOT,
        ),
        crate::MutationCursor::new(21),
        vec![
            crate::MutationPageItem::Utf8 {
                inserted_offset: 0,
                text: "x".into(),
            },
            crate::MutationPageItem::Atom(crate::AtomChange::Insert {
                id: crate::AtomId::new(8),
                inserted_range: ByteRange::from_u64(0, 2).unwrap(),
                fallback_copy: "x".into(),
            }),
        ],
    )
    .unwrap();
    let before = input.read_with(cx, |input, _| {
        (
            input
                .edits
                .stream_finish(key, crate::MutationLane::Proposal)
                .unwrap(),
            input.requests.len(),
            input.edits.active_object_effect(),
        )
    });
    assert!(matches!(
        input.update(cx, |input, cx| input.submit_mutation_page(invalid, cx)),
        Err(RangeTextInputError::Mutation(
            crate::MutationError::MalformedAtomChange
        ))
    ));
    input.read_with(cx, |input, _| {
        assert_eq!(input.edits.state(), crate::MutationState::InputStreaming);
        assert_eq!(
            (
                input
                    .edits
                    .stream_finish(key, crate::MutationLane::Proposal)
                    .unwrap(),
                input.requests.len(),
                input.edits.active_object_effect(),
            ),
            before
        );
    });
    let corrected = crate::MutationPage::new(
        crate::MutationPageKey::new(
            key,
            crate::MutationLane::Proposal,
            crate::MutationCursor::new(20),
            0,
            crate::MutationIdentity::ROOT,
        ),
        crate::MutationCursor::new(21),
        vec![
            crate::MutationPageItem::Utf8 {
                inserted_offset: 0,
                text: "x".into(),
            },
            crate::MutationPageItem::Atom(crate::AtomChange::Insert {
                id: crate::AtomId::new(8),
                inserted_range: ByteRange::from_u64(0, 1).unwrap(),
                fallback_copy: "x".into(),
            }),
        ],
    )
    .unwrap();
    assert!(matches!(
        input.update(cx, |input, cx| input.submit_mutation_page(corrected, cx)),
        Ok(crate::MutationPageAcceptance::Accepted { .. })
    ));
    input.read_with(cx, |input, _| assert_eq!(input.requests.len(), 1));
}

#[gpui::test]
fn host_stream_pages_finish_and_commit_through_widget_protocol(cx: &mut gpui::TestAppContext) {
    let (input, cx) = cx.add_window_view(|window, cx| {
        RangeTextInput::new(config(2 * 1024 * 1024, 32_768), window, cx).unwrap()
    });
    drive_initial_surface(&input, cx);
    let current = input.read_with(cx, |input, _| {
        input.surface.as_ref().unwrap().selection().head
    });
    let (_, text, objects) = admitted_successor_sources(SOURCE, 1, &[current]);
    let base = input.read_with(cx, |input, _| input.config.binding);
    let key = crate::MutationKey::new(base.binding(), base.revision(), crate::OperationId::new(1));
    let proposal = crate::MutationProposal::new(
        key,
        crate::MutationKind::Edit,
        crate::MutationPositions::collapsed(current),
        crate::SourceRange::new(current, current).unwrap(),
        0,
    );
    let begin = crate::MutationBeginRequest::new(
        proposal,
        crate::MutationCursor::new(10),
        crate::MutationCursor::new(20),
    );
    input.update(cx, |input, cx| {
        input
            .begin_host_mutation(begin, &[current], &text, &objects, cx)
            .unwrap()
    });
    assert!(matches!(
        input.update(cx, |input, _| input.take_request()),
        Some(RangeTextInputRequest::MutationBegin(request)) if request == begin
    ));
    input.update(cx, |input, cx| {
        input.accept_mutation_preflight(key, cx).unwrap()
    });
    for (lane, cursor, next, value) in [
        (crate::MutationLane::Source, 10, 11, "s"),
        (crate::MutationLane::Proposal, 20, 21, "x"),
    ] {
        let page = crate::MutationPage::new(
            crate::MutationPageKey::new(
                key,
                lane,
                crate::MutationCursor::new(cursor),
                0,
                crate::MutationIdentity::ROOT,
            ),
            crate::MutationCursor::new(next),
            vec![crate::MutationPageItem::Utf8 {
                inserted_offset: 0,
                text: value.into(),
            }],
        )
        .unwrap();
        input.update(cx, |input, cx| {
            input.submit_mutation_page(page, cx).unwrap()
        });
        assert!(matches!(
            input.update(cx, |input, _| input.take_request()),
            Some(RangeTextInputRequest::MutationSourcePage(_))
                | Some(RangeTextInputRequest::MutationProposalPage(_))
        ));
    }
    let intended = crate::MutationPositions::collapsed(SourcePosition::new(
        ByteOffset::new(current.byte_offset.get() + 1),
        crate::InlineObjectGap::NoObjects,
    ));
    let finish = input.read_with(cx, |input, _| {
        crate::MutationFinishInput::new(
            key,
            input
                .edits
                .stream_finish(key, crate::MutationLane::Source)
                .unwrap(),
            input
                .edits
                .stream_finish(key, crate::MutationLane::Proposal)
                .unwrap(),
            LogicalExtent::new(base.extent().byte_len() + 1, base.extent().line_count()),
            intended,
        )
    });
    input.update(cx, |input, cx| {
        input.submit_mutation_finish(finish, cx).unwrap()
    });
    assert!(matches!(
        input.update(cx, |input, _| input.take_request()),
        Some(RangeTextInputRequest::MutationFinishInput(request)) if request == finish
    ));
    input.update(cx, |input, cx| {
        input.accept_mutation_finish(key, cx).unwrap()
    });
    assert!(matches!(
        input.update(cx, |input, _| input.take_request()),
        Some(RangeTextInputRequest::MutationCommit(commit)) if commit.key() == key
    ));
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input
                .settle_mutation(key, crate::MutationOutcome::Rejected, window, cx)
                .unwrap();
        })
    });
}

#[gpui::test]
fn unchanged_binding_cancellation_is_keyed_exact_once_and_commit_pending_waits(
    cx: &mut gpui::TestAppContext,
) {
    let (input, window_cx) = cx.add_window_view(|window, cx| {
        RangeTextInput::new(config(2 * 1024 * 1024, 32_768), window, cx).unwrap()
    });
    drive_initial_surface(&input, window_cx);
    let events = captured_events(&input, window_cx);
    let current = input.read_with(window_cx, |input, _| {
        input.surface.as_ref().unwrap().selection().head
    });
    let (_, text, objects) = admitted_successor_sources(SOURCE, 1, &[current]);
    let base = input.read_with(window_cx, |input, _| input.config.binding);
    let key = crate::MutationKey::new(base.binding(), base.revision(), crate::OperationId::new(1));
    let proposal = crate::MutationProposal::new(
        key,
        crate::MutationKind::Edit,
        crate::MutationPositions::collapsed(current),
        crate::SourceRange::new(current, current).unwrap(),
        0,
    );
    let begin = crate::MutationBeginRequest::new(
        proposal,
        crate::MutationCursor::new(0),
        crate::MutationCursor::new(0),
    );
    input.update(window_cx, |input, cx| {
        input
            .begin_host_mutation(begin, &[current], &text, &objects, cx)
            .unwrap();
    });
    assert!(matches!(
        input.update(window_cx, |input, _| input.take_request()),
        Some(RangeTextInputRequest::MutationBegin(request)) if request == begin
    ));
    input.update(window_cx, |input, cx| {
        input.accept_mutation_preflight(key, cx).unwrap()
    });
    let source_page = crate::MutationPage::new(
        crate::MutationPageKey::new(
            key,
            crate::MutationLane::Source,
            crate::MutationCursor::new(0),
            0,
            crate::MutationIdentity::ROOT,
        ),
        crate::MutationCursor::new(1),
        vec![crate::MutationPageItem::Utf8 {
            inserted_offset: 0,
            text: "source".into(),
        }],
    )
    .unwrap();
    input.update(window_cx, |input, cx| {
        input.submit_mutation_page(source_page, cx).unwrap();
    });
    let wrong = crate::MutationKey::new(
        base.binding(),
        base.revision(),
        crate::OperationId::new(701),
    );
    let before_wrong = input.read_with(window_cx, |input, _| fingerprint(input));
    assert!(matches!(
        input.update(window_cx, |input, cx| input.cancel_mutation(wrong, cx)),
        Err(RangeTextInputError::Mutation(
            crate::MutationError::WrongKey { expected, actual }
        )) if expected == key && actual == wrong
    ));
    input.read_with(window_cx, |input, _| {
        assert_eq!(fingerprint(input), before_wrong)
    });
    assert!(matches!(
        input.update(window_cx, |input, cx| input.cancel_mutation(key, cx)),
        Ok(crate::MutationCancellation::Cancelled)
    ));
    assert!(matches!(
        input.update(window_cx, |input, _| input.take_request()),
        Some(RangeTextInputRequest::CancelMutation(request)) if request.key() == key
    ));
    input.read_with(window_cx, |input, _| assert!(input.is_quiescent()));
    let cancelled_events = || {
        events
            .borrow()
            .iter()
            .filter(|event| {
                matches!(
                    event,
                    RangeTextInputEvent::MutationSettled { key: event_key, outcome }
                        if *event_key == key && *outcome == crate::MutationOutcome::Cancelled
                )
            })
            .count()
    };
    assert_eq!(cancelled_events(), 1);
    assert!(matches!(
        input.update(window_cx, |input, cx| input.cancel_mutation(key, cx)),
        Err(RangeTextInputError::Mutation(
            crate::MutationError::ObsoleteOperation(obsolete)
        )) if obsolete == key
    ));
    assert_eq!(cancelled_events(), 1);

    input.update(window_cx, |input, _| {
        input
            .admit_edit_positions(&[current], &text, &objects)
            .unwrap();
    });
    input.update(window_cx, |input, cx| {
        input.insert_text("x".to_owned(), cx).unwrap()
    });
    let local_key = input.read_with(window_cx, |input, _| {
        let RangeTextInputRequest::MutationBegin(begin) = input.requests.front().unwrap() else {
            panic!("local preflight")
        };
        begin.proposal().key()
    });
    assert!(matches!(
        input.update(window_cx, |input, cx| input.cancel_mutation(local_key, cx)),
        Ok(crate::MutationCancellation::Cancelled)
    ));
    assert!(matches!(
        input.update(window_cx, |input, _| input.take_request()),
        Some(RangeTextInputRequest::CancelMutation(request)) if request.key() == local_key
    ));
    input.read_with(window_cx, |input, _| assert!(input.is_quiescent()));

    let (commit_key, _) = drive_local_insert_to_commit_pending(&input, window_cx);
    assert!(matches!(
        input.update(window_cx, |input, cx| input.cancel_mutation(commit_key, cx)),
        Ok(crate::MutationCancellation::AwaitingHostSettlement)
    ));
    input.read_with(window_cx, |input, _| assert!(input.requests.is_empty()));
    window_cx.update(|window, app| {
        input.update(app, |input, cx| {
            input
                .settle_mutation(commit_key, crate::MutationOutcome::Rejected, window, cx)
                .unwrap();
        })
    });
}

#[gpui::test]
fn detached_commit_settles_once_after_rebind_and_late_duplicate_is_inert(
    cx: &mut gpui::TestAppContext,
) {
    let (input, cx) = cx.add_window_view(|window, cx| {
        RangeTextInput::new(config(2 * 1024 * 1024, 32_768), window, cx).unwrap()
    });
    drive_initial_surface(&input, cx);
    let (key, _) = drive_local_insert_to_commit_pending(&input, cx);
    let rebound = RangeBinding::new(
        binding().binding(),
        SourceRevision::new(2),
        binding().extent(),
    );
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.rebind(rebound, None, window, cx).unwrap()
        })
    });
    input.read_with(cx, |input, _| {
        assert_eq!(input.config.settlement_coordinator.retained_count(), 1)
    });
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input
                .settle_mutation(key, crate::MutationOutcome::Rejected, window, cx)
                .unwrap();
        })
    });
    input.read_with(cx, |input, _| {
        assert_eq!(input.config.settlement_coordinator.retained_count(), 0)
    });
    let late = cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.settle_mutation(key, crate::MutationOutcome::Rejected, window, cx)
        })
    });
    assert!(matches!(late, Err(RangeTextInputError::Stale)));
    input.read_with(cx, |input, _| {
        assert_eq!(input.config.settlement_coordinator.retained_count(), 0)
    });
}

#[gpui::test]
fn one_page_mutation_queue_is_fixed_and_accounts_with_geometry_capacity(
    cx: &mut gpui::TestAppContext,
) {
    let (input, cx) = cx.add_window_view(|window, cx| {
        RangeTextInput::new(config(2 * 1024 * 1024, 32_768), window, cx).unwrap()
    });
    drive_initial_surface(&input, cx);
    let current_position = input.read_with(cx, |input, _| {
        input.surface.as_ref().unwrap().selection().head
    });
    let (_, current_text, current_objects) =
        admitted_successor_sources(SOURCE, 1, &[current_position]);
    input.update(cx, |input, _| {
        input
            .admit_edit_positions(&[current_position], &current_text, &current_objects)
            .unwrap();
    });
    input.update(cx, |input, cx| {
        input.insert_text("x".to_owned(), cx).unwrap()
    });
    let key = input.read_with(cx, |input, _| {
        let RangeTextInputRequest::MutationBegin(begin) = input.requests.front().unwrap() else {
            panic!("insert begin")
        };
        begin.proposal().key()
    });
    assert!(matches!(
        input.update(cx, |input, cx| input.accept_mutation_preflight(key, cx)),
        Err(RangeTextInputError::Busy)
    ));
    input.read_with(cx, |input, _| {
        assert_eq!(input.edits.state(), crate::MutationState::PreflightPending);
        assert_eq!(input.requests.len(), 1);
    });
    let RangeTextInputRequest::MutationBegin(begin) =
        input.update(cx, |input, _| input.take_request()).unwrap()
    else {
        panic!("insert begin")
    };
    assert_eq!(begin.proposal().key(), key);
    input.update(cx, |input, cx| {
        input.accept_mutation_preflight(key, cx).unwrap()
    });
    let page = input.read_with(cx, |input, _| {
        assert_eq!(
            input
                .requests
                .iter()
                .filter(|request| match request {
                    RangeTextInputRequest::MutationProposalPage(page) => {
                        page.page().key().key() == key
                    }
                    RangeTextInputRequest::MutationFinishInput(finish) => finish.key() == key,
                    _ => false,
                })
                .count(),
            RangeTextInput::MAX_QUEUED_MUTATION_REQUESTS
        );
        input
            .requests
            .iter()
            .find_map(|request| match request {
                RangeTextInputRequest::MutationProposalPage(page) => Some(page.page().clone()),
                _ => None,
            })
            .unwrap()
    });
    assert!(matches!(
        input.update(cx, |input, cx| input.submit_mutation_page(page, cx)),
        Err(RangeTextInputError::Busy)
    ));
    input.read_with(cx, |input, _| {
        assert_eq!(input.edits.state(), crate::MutationState::FinishPending);
        assert_eq!(
            input.requests.len(),
            RangeTextInput::MAX_QUEUED_MUTATION_REQUESTS
        );
    });
    input.update(cx, |input, _| {
        input
            .prepare_focus_loss_transition(input.desired)
            .expect("queued page coexistence probe");
    });
    let components = input.read_with(cx, |input, _| {
        input.last_widget_admission_components.get().unwrap()
    });
    assert_eq!(
        components.mutation_request_payload,
        RangeSurfaceCharge {
            bytes: std::mem::size_of::<crate::MutationPageItem>() + 1,
            items: 2,
        }
    );
    let exact = components.checked_total().unwrap();
    let before = input.read_with(cx, |input, _| fingerprint(input));
    input.update(cx, |input, _| {
        input.config.limits.max_surface_bytes = exact.bytes - 1;
        assert!(matches!(
            input.prepare_focus_loss_transition(input.desired),
            Err(RangeTextInputError::SurfaceCapacity)
        ));
    });
    input.read_with(cx, |input, _| assert_eq!(fingerprint(input), before));
    input.update(cx, |input, _| {
        input.config.limits.max_surface_bytes = 2 * 1024 * 1024;
        input.config.limits.max_surface_items = exact.items - 1;
        assert!(matches!(
            input.prepare_focus_loss_transition(input.desired),
            Err(RangeTextInputError::SurfaceCapacity)
        ));
    });
    input.read_with(cx, |input, _| assert_eq!(fingerprint(input), before));
    input.update(cx, |input, _| {
        input.config.limits.max_surface_bytes = exact.bytes;
        input.config.limits.max_surface_items = exact.items;
        input
            .prepare_focus_loss_transition(input.desired)
            .expect("exact queued page coexistence capacity");
    });
    input.read_with(cx, |input, _| assert_eq!(fingerprint(input), before));
}

#[gpui::test]
fn committed_settlement_accepts_exact_fit_and_one_under_is_retryable(
    cx: &mut gpui::TestAppContext,
) {
    let (input, cx) = cx.add_window_view(|window, cx| {
        RangeTextInput::new(config(2 * 1024 * 1024, 32_768), window, cx).unwrap()
    });
    drive_initial_surface(&input, cx);
    let current_position = input.read_with(cx, |input, _| {
        input.surface.as_ref().unwrap().selection().head
    });
    let (_, current_text, current_objects) =
        admitted_successor_sources(SOURCE, 1, &[current_position]);
    input.update(cx, |input, _| {
        input
            .admit_edit_positions(&[current_position], &current_text, &current_objects)
            .unwrap();
    });
    input.update(cx, |input, cx| {
        input.insert_text("x".to_owned(), cx).unwrap()
    });
    let RangeTextInputRequest::MutationBegin(begin) =
        input.update(cx, |input, _| input.take_request()).unwrap()
    else {
        panic!("insert preflight")
    };
    input.update(cx, |input, cx| {
        input
            .accept_mutation_preflight(begin.proposal().key(), cx)
            .unwrap()
    });
    let mut positions = None;
    while let Some(request) = input.update(cx, |input, _| input.take_request()) {
        if let RangeTextInputRequest::MutationFinishInput(finish) = request {
            positions = Some(finish.intended());
        }
    }
    let positions = positions.expect("authenticated successor positions");
    input.update(cx, |input, cx| {
        input
            .accept_mutation_finish(begin.proposal().key(), cx)
            .unwrap()
    });
    assert!(matches!(
        input.update(cx, |input, _| input.take_request()),
        Some(RangeTextInputRequest::MutationCommit(_))
    ));
    let successor = format!("x{SOURCE}");
    let successor_positions = [
        positions.caret(),
        positions.selection_anchor(),
        positions.selection_head(),
    ];
    let (successor_binding, text, objects) =
        admitted_successor_sources(&successor, 2, &successor_positions);
    let outcome = crate::MutationOutcome::Committed(
        crate::MutationCommit::from_admitted_sources(successor_binding, positions, &text, &objects)
            .unwrap(),
    );
    input.update(cx, |input, _| {
        let prior = input.scrollbar.owner;
        let replacement = gpui_scrollbar::ScrollbarOwnerKey::new(
            prior.owner_id,
            gpui_scrollbar::ScrollbarMountGeneration::new(prior.mount_generation.get() + 1),
        );
        let _candidate = input
            .prepare_rebind_transition(
                successor_binding,
                Some(RangeSourceSelection {
                    anchor: positions.selection_anchor(),
                    head: positions.selection_head(),
                }),
                prior,
                replacement,
                crate::InlineObjectRealizationLossReason::Superseded,
                Some((begin.proposal().key(), outcome)),
                None,
                Some((positions, outcome_commit_proofs(outcome))),
            )
            .unwrap();
    });
    let components = input.read_with(cx, |input, _| {
        input.last_widget_admission_components.get().unwrap()
    });
    assert_eq!(
        components,
        WidgetAdmissionComponents {
            prior_surface: RangeSurfaceCharge {
                bytes: 5_470,
                items: 90,
            },
            current_request_storage: RangeSurfaceCharge {
                bytes: 2_240,
                items: 4,
            },
            mutation_request_payload: RangeSurfaceCharge::default(),
            candidate_record: RangeSurfaceCharge {
                bytes: 8_768,
                items: 1,
            },
            geometry: RangeSurfaceCharge {
                bytes: 2_493,
                items: 23,
            },
            resident_payload: RangeSurfaceCharge { bytes: 0, items: 0 },
            publication_allocation: RangeSurfaceCharge { bytes: 0, items: 0 },
            effect_storage: RangeSurfaceCharge {
                bytes: 2_240,
                items: 4,
            },
            event_storage: RangeSurfaceCharge {
                bytes: 832,
                items: 1,
            },
            page_demand: RangeSurfaceCharge { bytes: 0, items: 0 },
            object_rebind: RangeSurfaceCharge { bytes: 0, items: 1 },
            residency_rebind: RangeSurfaceCharge { bytes: 0, items: 2 },
            detached_edit_storage: RangeSurfaceCharge { bytes: 0, items: 0 },
            destination_request_storage: RangeSurfaceCharge {
                bytes: 560,
                items: 1,
            },
            proof_storage: RangeSurfaceCharge {
                bytes: 480,
                items: 3,
            },
        }
    );
    let exact = RangeSurfaceCharge {
        bytes: 23_083,
        items: 130,
    };
    assert_eq!(components.checked_total(), Some(exact));
    let events = captured_events(&input, cx);
    input.update(cx, |input, _| {
        input.config.limits.max_surface_bytes = exact.bytes - 1;
        input.config.limits.max_surface_items = 32_768;
    });
    let before = input.read_with(cx, |input, _| fingerprint(input));
    let event_count = events.borrow().len();
    let rejected = cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.settle_committed_mutation(
                begin.proposal().key(),
                successor_binding,
                positions,
                &text,
                &objects,
                window,
                cx,
            )
        })
    });
    assert!(matches!(
        rejected,
        Err(RangeTextInputError::SurfaceCapacity)
    ));
    input.read_with(cx, |input, _| {
        assert_eq!(fingerprint(input), before);
        assert_eq!(input.edits.active_key(), Some(begin.proposal().key()));
    });
    assert_eq!(events.borrow().len(), event_count);
    input.update(cx, |input, _| {
        input.config.limits.max_surface_bytes = exact.bytes;
        input.config.limits.max_surface_items = exact.items - 1;
    });
    let rejected = cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.settle_committed_mutation(
                begin.proposal().key(),
                successor_binding,
                positions,
                &text,
                &objects,
                window,
                cx,
            )
        })
    });
    assert!(matches!(
        rejected,
        Err(RangeTextInputError::SurfaceCapacity)
    ));
    input.read_with(cx, |input, _| {
        assert_eq!(fingerprint(input), before);
        assert_eq!(input.edits.active_key(), Some(begin.proposal().key()));
    });
    assert_eq!(events.borrow().len(), event_count);
    input.update(cx, |input, _| {
        input.config.limits.max_surface_items = exact.items;
    });
    let accepted = cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.settle_committed_mutation(
                begin.proposal().key(),
                successor_binding,
                positions,
                &text,
                &objects,
                window,
                cx,
            )
        })
    });
    assert!(accepted.is_ok(), "{accepted:?}");
    input.read_with(cx, |input, _| {
        assert_eq!(input.last_surface_admission, Some(exact));
        assert_eq!(input.adopted_positions, Some(positions));
        assert_eq!(input.admitted_edit_proofs.len(), 3);
    });
    input.update(cx, |input, _| {
        let predecessor_key = begin.proposal().key();
        assert_eq!(
            input
                .edits
                .settle(predecessor_key, crate::MutationOutcome::Rejected),
            Err(crate::MutationError::ObsoleteOperation(predecessor_key))
        );
        let fresh_key = crate::MutationKey::new(
            successor_binding.binding(),
            successor_binding.revision(),
            predecessor_key.operation(),
        );
        let at = positions.caret();
        let proposal = crate::MutationProposal::new(
            fresh_key,
            crate::MutationKind::Edit,
            positions,
            crate::SourceRange::new(at, at).unwrap(),
            0,
        );
        input
            .edits
            .begin(crate::MutationBeginRequest::new(
                proposal,
                crate::MutationCursor::new(0),
                crate::MutationCursor::new(0),
            ))
            .unwrap();
        assert_eq!(
            input.edits.cancel(fresh_key).unwrap(),
            crate::MutationCancellation::Cancelled
        );
    });
}

fn outcome_commit_proofs(
    outcome: crate::MutationOutcome,
) -> Vec<crate::range_edit::SourcePositionProof> {
    let crate::MutationOutcome::Committed(commit) = outcome else {
        unreachable!()
    };
    commit.proofs().as_array().to_vec()
}

#[gpui::test]
fn rebind_rejects_mismatched_scrollbar_state_owner_without_mutation(cx: &mut gpui::TestAppContext) {
    let (input, cx) = cx.add_window_view(|window, cx| {
        RangeTextInput::new(config(2 * 1024 * 1024, 32_768), window, cx).unwrap()
    });
    drive_initial_surface(&input, cx);
    let (expected_owner, scrollbar_state) = input.read_with(cx, |input, _| {
        assert!(input.surface.is_some());
        (input.scrollbar.owner, input.scrollbar.state.clone())
    });
    let replacement_owner = gpui_scrollbar::ScrollbarOwnerKey::new(
        expected_owner.owner_id,
        gpui_scrollbar::ScrollbarMountGeneration::new(
            expected_owner
                .mount_generation
                .get()
                .checked_add(100)
                .unwrap(),
        ),
    );
    cx.update(|window, app| {
        assert!(scrollbar_state.replace_owner(expected_owner, replacement_owner, window, app,));
    });

    let events = captured_events(&input, cx);
    let before = input.read_with(cx, |input, _| {
        assert_eq!(input.scrollbar.owner, expected_owner);
        assert_eq!(
            input.scrollbar.state.current_owner(),
            Some(replacement_owner)
        );
        fingerprint(input)
    });
    let event_count = events.borrow().len();
    let rebound = RangeBinding::new(
        BindingId::new(71),
        SourceRevision::new(2),
        LogicalExtent::new(SOURCE.len() as u64, 1),
    );
    let result = cx.update(|window, app| {
        input.update(app, |input, cx| input.rebind(rebound, None, window, cx))
    });

    assert!(matches!(result, Err(RangeTextInputError::Stale)));
    input.read_with(cx, |input, _| {
        assert_eq!(fingerprint(input), before);
        assert_eq!(input.scrollbar.owner, expected_owner);
        assert_eq!(
            input.scrollbar.state.current_owner(),
            Some(replacement_owner)
        );
    });
    assert_eq!(events.borrow().len(), event_count);

    cx.update(|window, app| {
        assert!(scrollbar_state.replace_owner(replacement_owner, expected_owner, window, app,));
    });
}

#[gpui::test]
fn direct_history_commit_adopts_exact_successor_without_mutation_stream(
    cx: &mut gpui::TestAppContext,
) {
    let (input, cx) = cx.add_window_view(|window, cx| {
        RangeTextInput::new(config(2 * 1024 * 1024, 32_768), window, cx).unwrap()
    });
    drive_initial_surface(&input, cx);
    let predecessor_binding = input.read_with(cx, |input, _| input.config.binding);
    let predecessor_caret = input.read_with(cx, |input, _| input.surface.as_ref().unwrap().caret());
    let anchor = SourcePosition::new(ByteOffset::new(0), crate::InlineObjectGap::NoObjects);
    let head = SourcePosition::new(ByteOffset::new(4), crate::InlineObjectGap::NoObjects);
    let selection = crate::RangeSourceSelection { anchor, head };
    let frontier = crate::RangeHistoryFrontier {
        binding: predecessor_binding,
        id: 41,
        undo_available: true,
        redo_available: false,
    };
    let intent = admit_history(&input, cx, frontier, crate::MutationKind::Undo);
    assert_eq!(intent.caret(), predecessor_caret);
    assert_eq!(
        intent.selection(),
        crate::RangeSourceSelection::caret(intent.caret())
    );
    let successor = RangeBinding::new(
        BindingId::new(72),
        predecessor_binding.revision(),
        binding().extent(),
    );
    let successor_frontier = crate::RangeHistoryFrontier {
        binding: successor,
        id: 42,
        undo_available: false,
        redo_available: true,
    };
    let commit = crate::RangeHistoryCommit::new(successor, head, selection, successor_frontier);
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            assert_eq!(
                input
                    .settle_history(
                        intent,
                        crate::RangeHistoryOutcome::Committed(commit),
                        window,
                        cx,
                    )
                    .unwrap(),
                crate::RangeHistorySettlement::Current(crate::RangeHistoryOutcome::Committed(
                    commit
                ))
            );
        })
    });
    input.read_with(cx, |input, _| {
        assert_eq!(input.config.binding, successor);
        assert_eq!(input.history_frontier(), successor_frontier);
        assert!(input.pending_history.is_none());
        assert_eq!(input.config.settlement_coordinator.retained_count(), 0);
        assert_eq!(
            input.surface.as_ref().unwrap().binding(),
            predecessor_binding
        );
        assert!(input.interactive_surface().is_none());
        assert!(input.admitted_edit_proofs.is_empty());
        assert!(input.mutation_positions.is_none());
        assert!(input.requests.iter().all(|request| !matches!(
            request,
            RangeTextInputRequest::MutationBegin(_)
                | RangeTextInputRequest::MutationSourcePage(_)
                | RangeTextInputRequest::MutationProposalPage(_)
                | RangeTextInputRequest::MutationFinishInput(_)
                | RangeTextInputRequest::MutationCommit(_)
        )));
    });
    drive_initial_surface(&input, cx);
    input.read_with(cx, |input, _| {
        let surface = input.surface.as_ref().unwrap();
        assert_eq!(surface.binding(), successor);
        assert_eq!(surface.caret(), head);
        assert_eq!(surface.selection(), selection);
        assert!(input.interactive_surface().is_some());
        let seed = input.export_restoration(Some(successor_frontier)).unwrap();
        assert_eq!(seed.binding, successor);
        assert_eq!(seed.caret, head);
        assert_eq!(seed.selection, selection);
        assert_eq!(seed.history, Some(successor_frontier));
    });
}

#[gpui::test]
fn delayed_or_extent_mismatched_history_frontier_is_exactly_inert(cx: &mut gpui::TestAppContext) {
    let (input, window_cx) = cx.add_window_view(|window, cx| {
        RangeTextInput::new(config(2 * 1024 * 1024, 32_768), window, cx).unwrap()
    });
    drive_initial_surface(&input, window_cx);
    let delayed = crate::RangeHistoryFrontier {
        binding: binding(),
        id: 51,
        undo_available: true,
        redo_available: false,
    };
    rebind_revision(&input, window_cx, 2);
    drive_initial_surface(&input, window_cx);
    let current = input.read_with(window_cx, |input, _| input.config.binding);
    let before = input.read_with(window_cx, |input, _| fingerprint(input));
    input.update(window_cx, |input, cx| {
        assert!(matches!(
            input.set_history_frontier(
                delayed,
                crate::RangeHistoryFrontier {
                    binding: current,
                    ..delayed
                },
            ),
            Err(RangeTextInputError::Stale)
        ));
        let extent_mismatch = crate::RangeHistoryFrontier {
            binding: RangeBinding::new(
                current.binding(),
                current.revision(),
                LogicalExtent::new(current.extent().byte_len() + 1, 1),
            ),
            id: 52,
            undo_available: true,
            redo_available: false,
        };
        assert!(matches!(
            input.set_history_frontier(input.history_frontier(), extent_mismatch),
            Err(RangeTextInputError::Stale)
        ));
        input.request_history(crate::MutationKind::Undo, cx);
        assert!(input.take_request().is_none());
    });
    input.read_with(window_cx, |input, _| {
        assert_eq!(input.config.binding, current);
        assert_eq!(
            input.history_frontier(),
            crate::RangeHistoryFrontier::unavailable(current)
        );
        assert_eq!(fingerprint(input), before);
        assert!(input.pending_history.is_none());
        assert_eq!(input.config.settlement_coordinator.retained_count(), 0);
    });
}

#[gpui::test]
fn exact_history_frontier_restoration_imports_only_on_coherent_publication(
    cx: &mut gpui::TestAppContext,
) {
    let (source, source_cx) = cx.add_window_view(|window, cx| {
        RangeTextInput::new(config(2 * 1024 * 1024, 32_768), window, cx).unwrap()
    });
    drive_initial_surface(&source, source_cx);
    let frontier = crate::RangeHistoryFrontier {
        binding: binding(),
        id: 61,
        undo_available: true,
        redo_available: true,
    };
    source.update(source_cx, |input, _| {
        input
            .set_history_frontier(input.history_frontier(), frontier)
            .unwrap()
    });
    let seed = source.read_with(source_cx, |input, _| {
        input.export_restoration(Some(frontier)).unwrap()
    });
    assert_eq!(seed.history, Some(frontier));

    let (restored, restored_cx) = cx.add_window_view(|window, cx| {
        RangeTextInput::new(config(2 * 1024 * 1024, 32_768), window, cx).unwrap()
    });
    drive_initial_surface(&restored, restored_cx);
    let malformed = crate::RangeRestorationSeed {
        history: Some(crate::RangeHistoryFrontier {
            binding: RangeBinding::new(
                seed.binding.binding(),
                seed.binding.revision(),
                LogicalExtent::new(seed.binding.extent().byte_len() + 1, 1),
            ),
            ..frontier
        }),
        ..seed
    };
    let before = restored.read_with(restored_cx, |input, _| fingerprint(input));
    restored.update(restored_cx, |input, cx| {
        assert!(matches!(
            input.import_restoration(malformed, cx),
            Err(RangeTextInputError::MalformedSeed)
        ));
    });
    restored.read_with(restored_cx, |input, _| {
        assert_eq!(fingerprint(input), before);
        assert_eq!(
            input.history_frontier(),
            crate::RangeHistoryFrontier::unavailable(seed.binding)
        );
    });

    let unavailable = crate::RangeHistoryFrontier::unavailable(seed.binding);
    let successor = crate::RangeHistoryFrontier {
        id: frontier.id + 1,
        undo_available: false,
        redo_available: true,
        ..frontier
    };

    restored.update(restored_cx, |input, cx| {
        input.import_restoration(seed, cx).unwrap();
        assert_eq!(
            input.history_frontier(),
            crate::RangeHistoryFrontier::unavailable(seed.binding)
        );
    });
    let pending = restored.read_with(restored_cx, |input, _| fingerprint(input));
    restored.update(restored_cx, |input, _| {
        assert!(matches!(
            input.set_history_frontier(unavailable, successor),
            Err(RangeTextInputError::Busy)
        ));
    });
    restored.read_with(restored_cx, |input, _| {
        assert_eq!(fingerprint(input), pending)
    });
    drive_initial_surface(&restored, restored_cx);
    restored.read_with(restored_cx, |input, _| {
        assert_eq!(input.history_frontier(), frontier);
        assert_eq!(input.export_restoration(Some(frontier)).unwrap(), seed);
    });
    restored.update(restored_cx, |input, _| {
        input.set_history_frontier(frontier, successor).unwrap();
    });
    let successor_seed = crate::RangeRestorationSeed {
        history: Some(successor),
        ..seed
    };
    restored.read_with(restored_cx, |input, _| {
        assert_eq!(input.history_frontier(), successor);
        assert_eq!(
            input.export_restoration(Some(successor)).unwrap(),
            successor_seed
        );
    });
    let flag_mismatch = crate::RangeHistoryFrontier {
        redo_available: false,
        ..successor
    };
    let before_stale = restored.read_with(restored_cx, |input, _| fingerprint(input));
    restored.update(restored_cx, |input, _| {
        assert!(matches!(
            input.set_history_frontier(unavailable, successor),
            Err(RangeTextInputError::Stale)
        ));
        assert!(matches!(
            input.set_history_frontier(unavailable, frontier),
            Err(RangeTextInputError::Stale)
        ));
        assert!(matches!(
            input.set_history_frontier(flag_mismatch, frontier),
            Err(RangeTextInputError::Stale)
        ));
    });
    restored.read_with(restored_cx, |input, _| {
        assert_eq!(fingerprint(input), before_stale);
        assert_eq!(input.published_restoration, Some(successor_seed));
        assert_eq!(input.history_frontier(), successor);
    });
    restored_cx.update(|window, app| {
        restored.update(app, |input, cx| {
            let _ = input.dispose(window, cx);
        })
    });
    let disposed = restored.read_with(restored_cx, |input, _| fingerprint(input));
    let disposed_frontier = restored.read_with(restored_cx, |input, _| input.history_frontier());
    restored.update(restored_cx, |input, _| {
        assert!(matches!(
            input.set_history_frontier(frontier, successor),
            Err(RangeTextInputError::NotMounted)
        ));
    });
    restored.read_with(restored_cx, |input, _| {
        assert_eq!(fingerprint(input), disposed);
        assert_eq!(input.history_frontier(), disposed_frontier);
    });
}

#[gpui::test]
fn direct_history_unavailable_and_no_change_outcomes_preserve_projection(
    cx: &mut gpui::TestAppContext,
) {
    let outcomes = [
        crate::RangeHistoryOutcome::Rejected,
        crate::RangeHistoryOutcome::Conflict,
        crate::RangeHistoryOutcome::Cancelled,
        crate::RangeHistoryOutcome::Error,
    ];
    for (id, outcome) in outcomes.into_iter().enumerate() {
        let (input, window_cx) = cx.add_window_view(|window, cx| {
            RangeTextInput::new(config(2 * 1024 * 1024, 32_768), window, cx).unwrap()
        });
        drive_initial_surface(&input, window_cx);
        let current_binding = input.read_with(window_cx, |input, _| input.config.binding);
        let unavailable = crate::RangeHistoryFrontier {
            binding: current_binding,
            id: id as u64,
            undo_available: false,
            redo_available: false,
        };
        input.update(window_cx, |input, cx| {
            input
                .set_history_frontier(input.history_frontier(), unavailable)
                .unwrap();
            input.request_history(crate::MutationKind::Undo, cx);
            input.request_history(crate::MutationKind::Redo, cx);
            assert!(input.take_request().is_none());
        });
        let frontier = crate::RangeHistoryFrontier {
            binding: current_binding,
            id: id as u64 + 100,
            undo_available: true,
            redo_available: false,
        };
        let prior = input.read_with(window_cx, |input, _| {
            let surface = input.surface.as_ref().unwrap();
            (
                input.config.binding,
                surface.caret(),
                surface.selection(),
                format!("{:?}", input.surface),
            )
        });
        input.update(window_cx, |input, cx| {
            input
                .set_history_frontier(input.history_frontier(), frontier)
                .unwrap();
            input.request_history(crate::MutationKind::Undo, cx);
        });
        let RangeTextInputRequest::HistoryIntent(intent) = input
            .update(window_cx, |input, _| input.take_request())
            .unwrap()
        else {
            panic!("available history intent")
        };
        input.update(window_cx, |input, _| {
            input
                .submit_history_session(crate::RangeHistorySession::new(intent))
                .unwrap();
        });
        window_cx.update(|window, app| {
            input.update(app, |input, cx| {
                assert_eq!(
                    input.settle_history(intent, outcome, window, cx).unwrap(),
                    crate::RangeHistorySettlement::Current(outcome)
                );
            })
        });
        input.read_with(window_cx, |input, _| {
            assert_eq!(input.config.binding, prior.0);
            assert_eq!(input.history_frontier(), frontier);
            assert_eq!(input.surface.as_ref().unwrap().caret(), prior.1);
            assert_eq!(input.surface.as_ref().unwrap().selection(), prior.2);
            assert_eq!(format!("{:?}", input.surface), prior.3);
            assert!(input.pending_history.is_none());
            assert_eq!(input.config.settlement_coordinator.retained_count(), 0);
            assert!(input.is_quiescent());
        });
    }
}

#[gpui::test]
fn direct_history_lifecycle_cuts_detach_admitted_and_make_completion_inert(
    cx: &mut gpui::TestAppContext,
) {
    let (input, window_cx) = cx.add_window_view(|window, cx| {
        RangeTextInput::new(config(2 * 1024 * 1024, 32_768), window, cx).unwrap()
    });
    drive_initial_surface(&input, window_cx);
    let frontier = crate::RangeHistoryFrontier {
        binding: binding(),
        id: 71,
        undo_available: true,
        redo_available: false,
    };
    input.update(window_cx, |input, cx| {
        input
            .set_history_frontier(input.history_frontier(), frontier)
            .unwrap();
        input.request_history(crate::MutationKind::Undo, cx);
    });
    let RangeTextInputRequest::HistoryIntent(unadmitted) = input
        .update(window_cx, |input, _| input.take_request())
        .unwrap()
    else {
        panic!("unadmitted history intent")
    };
    let rebound = RangeBinding::new(
        binding().binding(),
        SourceRevision::new(2),
        binding().extent(),
    );
    window_cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.rebind(rebound, None, window, cx).unwrap()
        })
    });
    input.read_with(window_cx, |input, _| {
        assert!(input.pending_history.is_none());
        assert_eq!(input.config.settlement_coordinator.retained_count(), 0);
        assert!(input.requests.iter().any(|request| matches!(
            request,
            RangeTextInputRequest::CancelHistoryIntent(intent) if *intent == unadmitted
        )));
    });
    assert!(matches!(
        input.update(window_cx, |input, _| input.take_request()),
        Some(RangeTextInputRequest::CancelHistoryIntent(intent)) if intent == unadmitted
    ));
    drive_initial_surface(&input, window_cx);
    let rebound_frontier = crate::RangeHistoryFrontier {
        binding: rebound,
        ..frontier
    };
    input.update(window_cx, |input, cx| {
        input
            .set_history_frontier(input.history_frontier(), rebound_frontier)
            .unwrap();
        input.request_history(crate::MutationKind::Undo, cx);
    });
    let RangeTextInputRequest::HistoryIntent(admitted) = input
        .update(window_cx, |input, _| input.take_request())
        .unwrap()
    else {
        panic!("admitted history intent")
    };
    input.update(window_cx, |input, _| {
        input
            .submit_history_session(crate::RangeHistorySession::new(admitted))
            .unwrap();
    });
    let second_rebind = RangeBinding::new(
        binding().binding(),
        SourceRevision::new(3),
        binding().extent(),
    );
    window_cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.rebind(second_rebind, None, window, cx).unwrap()
        })
    });
    input.read_with(window_cx, |input, _| {
        assert_eq!(input.config.settlement_coordinator.retained_count(), 1);
        assert!(
            input
                .config
                .settlement_coordinator
                .contains_history(admitted)
        );
        assert!(input.pending_history.is_none());
        assert!(!input.is_quiescent());
    });
    let before = input.read_with(window_cx, |input, _| input.config.binding);
    window_cx.update(|window, app| {
        input.update(app, |input, cx| {
            assert_eq!(
                input
                    .settle_history(admitted, crate::RangeHistoryOutcome::Rejected, window, cx)
                    .unwrap(),
                crate::RangeHistorySettlement::Obsolete(crate::RangeHistoryOutcome::Rejected)
            );
            assert_eq!(
                input
                    .settle_history(admitted, crate::RangeHistoryOutcome::Error, window, cx)
                    .unwrap(),
                crate::RangeHistorySettlement::Obsolete(crate::RangeHistoryOutcome::Error)
            );
        })
    });
    input.read_with(window_cx, |input, _| {
        assert_eq!(input.config.binding, before);
        assert_eq!(input.config.settlement_coordinator.retained_count(), 0);
    });
    drive_initial_surface(&input, window_cx);
    input.read_with(window_cx, |input, _| assert!(input.is_quiescent()));
}

#[gpui::test]
fn direct_history_dispose_retains_compact_custody_and_never_cancels_admitted_work(
    cx: &mut gpui::TestAppContext,
) {
    let (input, cx) = cx.add_window_view(|window, cx| {
        RangeTextInput::new(config(2 * 1024 * 1024, 32_768), window, cx).unwrap()
    });
    drive_initial_surface(&input, cx);
    input.update(cx, |input, cx| {
        let expected = input.history_frontier();
        input
            .set_history_frontier(
                expected,
                crate::RangeHistoryFrontier {
                    binding: binding(),
                    id: 91,
                    undo_available: true,
                    redo_available: false,
                },
            )
            .unwrap();
        input.request_history(crate::MutationKind::Undo, cx);
    });
    let RangeTextInputRequest::HistoryIntent(intent) =
        input.update(cx, |input, _| input.take_request()).unwrap()
    else {
        panic!("admitted dispose intent")
    };
    input.update(cx, |input, _| {
        input
            .submit_history_session(crate::RangeHistorySession::new(intent))
            .unwrap();
    });
    let disposed =
        cx.update(|window, app| input.update(app, |input, cx| input.dispose(window, cx)));
    assert!(!disposed.iter().any(|request| matches!(
        request,
        RangeTextInputRequest::CancelHistoryIntent(cancel) if *cancel == intent
    )));
    input.read_with(cx, |input, _| {
        assert_eq!(input.config.settlement_coordinator.retained_count(), 1);
        assert!(input.config.settlement_coordinator.contains_history(intent));
        assert!(input.pending_history.is_none());
    });
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            assert_eq!(
                input
                    .settle_history(intent, crate::RangeHistoryOutcome::Cancelled, window, cx)
                    .unwrap(),
                crate::RangeHistorySettlement::Obsolete(crate::RangeHistoryOutcome::Cancelled)
            );
        })
    });
    input.read_with(cx, |input, _| {
        assert_eq!(input.config.settlement_coordinator.retained_count(), 0);
    });
}

#[gpui::test]
fn direct_history_caret_selection_mismatch_is_exactly_no_change(cx: &mut gpui::TestAppContext) {
    let (input, cx) = cx.add_window_view(|window, cx| {
        RangeTextInput::new(config(2 * 1024 * 1024, 32_768), window, cx).unwrap()
    });
    drive_initial_surface(&input, cx);
    let frontier = crate::RangeHistoryFrontier {
        binding: binding(),
        id: 101,
        undo_available: true,
        redo_available: false,
    };
    let intent = admit_history(&input, cx, frontier, crate::MutationKind::Undo);
    let before = input.read_with(cx, |input, _| {
        (
            input.config.binding,
            input.history_frontier(),
            fingerprint(input),
        )
    });
    let head = SourcePosition::new(ByteOffset::new(0), crate::InlineObjectGap::NoObjects);
    let mismatched_caret =
        SourcePosition::new(ByteOffset::new(1), crate::InlineObjectGap::NoObjects);
    let successor = RangeBinding::new(
        before.0.binding(),
        SourceRevision::new(2),
        before.0.extent(),
    );
    let commit = crate::RangeHistoryCommit::new(
        successor,
        mismatched_caret,
        crate::RangeSourceSelection::caret(head),
        crate::RangeHistoryFrontier {
            binding: successor,
            id: 102,
            undo_available: false,
            redo_available: true,
        },
    );
    let result = cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.settle_history(
                intent,
                crate::RangeHistoryOutcome::Committed(commit),
                window,
                cx,
            )
        })
    });
    assert!(matches!(result, Err(RangeTextInputError::Stale)));
    input.read_with(cx, |input, _| {
        assert_eq!(input.config.binding, before.0);
        assert_eq!(input.history_frontier(), before.1);
        assert_eq!(fingerprint(input), before.2);
        assert_eq!(input.config.settlement_coordinator.retained_count(), 1);
        assert!(input.config.settlement_coordinator.contains_history(intent));
    });
}

#[gpui::test]
fn admitted_history_generations_survive_rebind_and_dispose_without_overwrite(
    cx: &mut gpui::TestAppContext,
) {
    let (input, window_cx) = cx.add_window_view(|window, cx| {
        RangeTextInput::new(config(2 * 1024 * 1024, 32_768), window, cx).unwrap()
    });
    drive_initial_surface(&input, window_cx);
    let h1 = admit_history(
        &input,
        window_cx,
        crate::RangeHistoryFrontier {
            binding: binding(),
            id: 111,
            undo_available: true,
            redo_available: false,
        },
        crate::MutationKind::Undo,
    );
    rebind_revision(&input, window_cx, 2);
    drive_initial_surface(&input, window_cx);
    input.read_with(window_cx, |input, _| {
        assert_eq!(
            input.history_frontier(),
            crate::RangeHistoryFrontier::unavailable(input.config.binding)
        );
        assert_eq!(input.config.settlement_coordinator.retained_count(), 1);
    });
    let h2 = admit_history(
        &input,
        window_cx,
        crate::RangeHistoryFrontier {
            binding: RangeBinding::new(
                binding().binding(),
                SourceRevision::new(2),
                binding().extent(),
            ),
            id: 112,
            undo_available: true,
            redo_available: false,
        },
        crate::MutationKind::Undo,
    );
    let before_dispose = input.read_with(window_cx, |input, _| input.config.binding);
    let disposed =
        window_cx.update(|window, app| input.update(app, |input, cx| input.dispose(window, cx)));
    assert!(!disposed.iter().any(|request| matches!(
        request,
        RangeTextInputRequest::CancelHistoryIntent(intent) if *intent == h1 || *intent == h2
    )));
    input.read_with(window_cx, |input, _| {
        assert_eq!(input.config.settlement_coordinator.retained_count(), 2);
        assert!(input.config.settlement_coordinator.contains_history(h1));
        assert!(input.config.settlement_coordinator.contains_history(h2));
    });
    for (intent, outcome) in [
        (h2, crate::RangeHistoryOutcome::Error),
        (h1, crate::RangeHistoryOutcome::Rejected),
    ] {
        window_cx.update(|window, app| {
            input.update(app, |input, cx| {
                assert_eq!(
                    input.settle_history(intent, outcome, window, cx).unwrap(),
                    crate::RangeHistorySettlement::Obsolete(outcome)
                );
            })
        });
    }
    let unknown = crate::RangeHistoryIntent::new(
        crate::MutationKey::new(
            h1.key().binding(),
            h1.key().base_revision(),
            crate::OperationId::new(99_999),
        ),
        h1.binding(),
        h1.kind(),
        h1.frontier(),
        h1.caret(),
        h1.selection(),
    );
    window_cx.update(|window, app| {
        input.update(app, |input, cx| {
            assert_eq!(
                input
                    .settle_history(h1, crate::RangeHistoryOutcome::Cancelled, window, cx)
                    .unwrap(),
                crate::RangeHistorySettlement::Obsolete(crate::RangeHistoryOutcome::Cancelled)
            );
            assert_eq!(
                input
                    .settle_history(unknown, crate::RangeHistoryOutcome::Error, window, cx)
                    .unwrap(),
                crate::RangeHistorySettlement::Obsolete(crate::RangeHistoryOutcome::Error)
            );
        })
    });
    input.read_with(window_cx, |input, _| {
        assert_eq!(input.config.binding, before_dispose);
        assert_eq!(input.config.settlement_coordinator.retained_count(), 0);
    });
}

#[gpui::test]
fn history_custody_capacity_exhaustion_releases_and_reuses_exact_slots(
    cx: &mut gpui::TestAppContext,
) {
    let (input, window_cx) = cx.add_window_view(|window, cx| {
        let mut config = config(2 * 1024 * 1024, 32_768);
        config.settlement_coordinator = crate::RangeSettlementCoordinator::new(2).unwrap();
        RangeTextInput::new(config, window, cx).unwrap()
    });
    drive_initial_surface(&input, window_cx);
    let h1 = admit_history(
        &input,
        window_cx,
        crate::RangeHistoryFrontier {
            binding: binding(),
            id: 121,
            undo_available: true,
            redo_available: false,
        },
        crate::MutationKind::Undo,
    );
    rebind_revision(&input, window_cx, 2);
    drive_initial_surface(&input, window_cx);
    let h2 = admit_history(
        &input,
        window_cx,
        crate::RangeHistoryFrontier {
            binding: RangeBinding::new(
                binding().binding(),
                SourceRevision::new(2),
                binding().extent(),
            ),
            id: 122,
            undo_available: true,
            redo_available: false,
        },
        crate::MutationKind::Undo,
    );
    rebind_revision(&input, window_cx, 3);
    drive_initial_surface(&input, window_cx);

    let frontier3 = crate::RangeHistoryFrontier {
        binding: RangeBinding::new(
            binding().binding(),
            SourceRevision::new(3),
            binding().extent(),
        ),
        id: 123,
        undo_available: true,
        redo_available: false,
    };
    input.update(window_cx, |input, cx| {
        input
            .set_history_frontier(input.history_frontier(), frontier3)
            .unwrap();
        input.request_history(crate::MutationKind::Undo, cx);
    });
    let RangeTextInputRequest::HistoryIntent(h3) = input
        .update(window_cx, |input, _| input.take_request())
        .unwrap()
    else {
        panic!("capacity history intent")
    };
    let before_exhaustion = input.read_with(window_cx, |input, _| {
        (
            input.config.binding,
            input.history_frontier(),
            format!("{:?}", input.surface),
            input.config.settlement_coordinator.retained_count(),
        )
    });
    input.update(window_cx, |input, _| {
        assert!(matches!(
            input.submit_history_session(crate::RangeHistorySession::new(h3)),
            Err(RangeTextInputError::DetachedCapacity)
        ));
    });
    input.read_with(window_cx, |input, _| {
        assert_eq!(input.config.binding, before_exhaustion.0);
        assert_eq!(input.history_frontier(), before_exhaustion.1);
        assert_eq!(format!("{:?}", input.surface), before_exhaustion.2);
        assert_eq!(
            input.config.settlement_coordinator.retained_count(),
            before_exhaustion.3
        );
        assert_eq!(input.pending_history.unwrap().intent(), h3);
        assert!(!input.pending_history.unwrap().is_admitted());
    });
    window_cx.update(|window, app| {
        input.update(app, |input, cx| {
            assert_eq!(
                input
                    .settle_history(h1, crate::RangeHistoryOutcome::Rejected, window, cx)
                    .unwrap(),
                crate::RangeHistorySettlement::Obsolete(crate::RangeHistoryOutcome::Rejected)
            );
            input
                .submit_history_session(crate::RangeHistorySession::new(h3))
                .unwrap();
        })
    });
    input.read_with(window_cx, |input, _| {
        assert_eq!(input.config.settlement_coordinator.retained_count(), 2)
    });
    rebind_revision(&input, window_cx, 4);
    drive_initial_surface(&input, window_cx);
    input.read_with(window_cx, |input, _| {
        assert!(matches!(
            input.export_restoration(None),
            Err(RangeTextInputError::NotQuiescent)
        ));
    });

    let mut oldest = h2;
    let mut newest = h3;
    let mut retained_high_water = 2;
    for revision in 5..10 {
        window_cx.update(|window, app| {
            input.update(app, |input, cx| {
                assert_eq!(
                    input
                        .settle_history(oldest, crate::RangeHistoryOutcome::Cancelled, window, cx,)
                        .unwrap(),
                    crate::RangeHistorySettlement::Obsolete(crate::RangeHistoryOutcome::Cancelled)
                );
            })
        });
        let next = admit_history(
            &input,
            window_cx,
            crate::RangeHistoryFrontier {
                binding: input.read_with(window_cx, |input, _| input.config.binding),
                id: 200 + revision,
                undo_available: true,
                redo_available: false,
            },
            crate::MutationKind::Undo,
        );
        retained_high_water = retained_high_water.max(input.read_with(window_cx, |input, _| {
            assert!(
                input.config.settlement_coordinator.retained_count()
                    <= input.config.settlement_coordinator.capacity()
            );
            input.config.settlement_coordinator.retained_count()
        }));
        rebind_revision(&input, window_cx, revision);
        drive_initial_surface(&input, window_cx);
        oldest = newest;
        newest = next;
    }
    assert_eq!(retained_high_water, 2);
    for intent in [oldest, newest] {
        window_cx.update(|window, app| {
            input.update(app, |input, cx| {
                input
                    .settle_history(intent, crate::RangeHistoryOutcome::Error, window, cx)
                    .unwrap();
            })
        });
    }
    input.read_with(window_cx, |input, _| {
        assert_eq!(input.config.settlement_coordinator.retained_count(), 0);
        assert!(input.is_quiescent());
    });
}

#[gpui::test]
fn history_and_ordinary_edits_share_one_live_operation_slot(cx: &mut gpui::TestAppContext) {
    let (input, window_cx) = cx.add_window_view(|window, cx| {
        RangeTextInput::new(config(2 * 1024 * 1024, 32_768), window, cx).unwrap()
    });
    drive_initial_surface(&input, window_cx);
    let (key, _) = drive_local_insert_to_commit_pending(&input, window_cx);
    input.update(window_cx, |input, cx| {
        let expected = input.history_frontier();
        input
            .set_history_frontier(
                expected,
                crate::RangeHistoryFrontier {
                    binding: input.config.binding,
                    id: 301,
                    undo_available: true,
                    redo_available: false,
                },
            )
            .unwrap();
        input.request_history(crate::MutationKind::Undo, cx);
        assert!(input.take_request().is_none());
    });
    window_cx.update(|window, app| {
        input.update(app, |input, cx| {
            input
                .settle_mutation(key, crate::MutationOutcome::Rejected, window, cx)
                .unwrap();
        })
    });
    let history = admit_history(
        &input,
        window_cx,
        crate::RangeHistoryFrontier {
            binding: binding(),
            id: 302,
            undo_available: true,
            redo_available: false,
        },
        crate::MutationKind::Undo,
    );
    input.update(window_cx, |input, cx| {
        assert!(matches!(
            input.insert_text("blocked".to_owned(), cx),
            Err(RangeTextInputError::Busy)
        ));
        assert!(input.take_request().is_none());
    });
    window_cx.update(|window, app| {
        input.update(app, |input, cx| {
            input
                .settle_history(history, crate::RangeHistoryOutcome::Rejected, window, cx)
                .unwrap();
        })
    });
    input.read_with(window_cx, |input, _| {
        assert_eq!(input.config.settlement_coordinator.retained_count(), 0);
        assert!(input.is_quiescent());
    });
}

#[gpui::test]
fn shared_history_settlement_coordinator_bounds_disposed_widget_generations(
    cx: &mut gpui::TestAppContext,
) {
    let coordinator = crate::RangeSettlementCoordinator::new(1).unwrap();
    let mut first_config = config(2 * 1024 * 1024, 32_768);
    first_config.settlement_coordinator = coordinator.clone();
    let (first, first_cx) =
        cx.add_window_view(|window, cx| RangeTextInput::new(first_config, window, cx).unwrap());
    drive_initial_surface(&first, first_cx);
    let h1 = admit_history(
        &first,
        first_cx,
        crate::RangeHistoryFrontier {
            binding: binding(),
            id: 401,
            undo_available: true,
            redo_available: false,
        },
        crate::MutationKind::Undo,
    );
    assert_eq!(coordinator.retained_count(), 1);
    assert_eq!(coordinator.capacity(), 1);
    first_cx.update(|window, app| {
        first.update(app, |input, cx| {
            let _ = input.dispose(window, cx);
        })
    });
    drop(first);

    let mut second_config = config(2 * 1024 * 1024, 32_768);
    second_config.settlement_coordinator = coordinator.clone();
    let (second, second_cx) =
        cx.add_window_view(|window, cx| RangeTextInput::new(second_config, window, cx).unwrap());
    drive_initial_surface(&second, second_cx);
    second.update(second_cx, |input, cx| {
        let replacement = crate::RangeHistoryFrontier {
            binding: input.config.binding,
            id: 402,
            undo_available: true,
            redo_available: false,
        };
        input
            .set_history_frontier(input.history_frontier(), replacement)
            .unwrap();
        input.request_history(crate::MutationKind::Undo, cx);
    });
    let RangeTextInputRequest::HistoryIntent(h2) = second
        .update(second_cx, |input, _| input.take_request())
        .unwrap()
    else {
        panic!("second history intent")
    };
    let before_denied = second.read_with(second_cx, |input, _| {
        (input.config.binding, input.history_frontier())
    });
    second.update(second_cx, |input, _| {
        assert!(matches!(
            input.submit_history_session(crate::RangeHistorySession::new(h2)),
            Err(RangeTextInputError::DetachedCapacity)
        ));
    });
    second.read_with(second_cx, |input, _| {
        assert_eq!(
            (input.config.binding, input.history_frontier()),
            before_denied
        );
    });
    assert_eq!(coordinator.retained_count(), 1);
    assert_ne!(h1.key().operation(), h2.key().operation());
    assert!(coordinator.settle_history(h1));
    assert!(!coordinator.settle_history(h1));
    let unknown = crate::RangeHistoryIntent::new(
        crate::MutationKey::new(
            h1.key().binding(),
            h1.key().base_revision(),
            crate::OperationId::new(u64::MAX - 1),
        ),
        h1.binding(),
        h1.kind(),
        h1.frontier(),
        h1.caret(),
        h1.selection(),
    );
    assert!(!coordinator.settle_history(unknown));
    second.update(second_cx, |input, _| {
        input
            .submit_history_session(crate::RangeHistorySession::new(h2))
            .unwrap();
    });
    assert_eq!(coordinator.retained_count(), 1);
    second_cx.update(|window, app| {
        second.update(app, |input, cx| {
            let _ = input.dispose(window, cx);
        })
    });
    drop(second);

    let mut third_config = config(2 * 1024 * 1024, 32_768);
    third_config.settlement_coordinator = coordinator.clone();
    let (third, third_cx) =
        cx.add_window_view(|window, cx| RangeTextInput::new(third_config, window, cx).unwrap());
    drive_initial_surface(&third, third_cx);
    let (edit, _) = drive_local_insert_to_finish_pending(&third, third_cx);
    third.update(third_cx, |input, cx| {
        assert!(matches!(
            input.accept_mutation_finish(edit, cx),
            Err(RangeTextInputError::DetachedCapacity)
        ));
    });
    assert_eq!(coordinator.retained_count(), 1);
    assert!(coordinator.settle_history(h2));
    assert!(!coordinator.settle_history(h2));
    third.update(third_cx, |input, cx| {
        input.accept_mutation_finish(edit, cx).unwrap();
    });
    assert!(matches!(
        third.update(third_cx, |input, _| input.take_request()),
        Some(RangeTextInputRequest::MutationCommit(commit)) if commit.key() == edit
    ));
    assert_ne!(edit.operation(), h1.key().operation());
    assert_ne!(edit.operation(), h2.key().operation());
    assert_eq!(coordinator.retained_count(), 1);
    third_cx.update(|window, app| {
        third.update(app, |input, cx| {
            let _ = input.dispose(window, cx);
        })
    });
    drop(third);
    assert!(coordinator.settle_mutation(edit));
    assert!(!coordinator.settle_mutation(edit));
    assert_eq!(coordinator.retained_count(), 0);
}

#[gpui::test]
fn shared_host_operation_claim_prevents_cross_generation_replay(cx: &mut gpui::TestAppContext) {
    let exhaustion = crate::RangeSettlementCoordinator::new(1).unwrap();
    assert_eq!(exhaustion.allocate_operation().unwrap().get(), 1);
    assert!(matches!(
        exhaustion.claim_host_operation(crate::OperationId::new(u64::MAX)),
        Err(RangeTextInputError::Stale)
    ));
    assert_eq!(exhaustion.allocate_operation().unwrap().get(), 2);
    for replay in [0, 1, 2] {
        assert!(matches!(
            exhaustion.claim_host_operation(crate::OperationId::new(replay)),
            Err(RangeTextInputError::Stale)
        ));
    }

    let near_exhaustion =
        crate::RangeSettlementCoordinator::new_with_next_operation(1, u64::MAX - 1).unwrap();
    let mut prior_config = config(2 * 1024 * 1024, 32_768);
    prior_config.settlement_coordinator = near_exhaustion.clone();
    let (prior, prior_cx) =
        cx.add_window_view(|window, cx| RangeTextInput::new(prior_config, window, cx).unwrap());
    drive_initial_surface(&prior, prior_cx);
    prior_cx.update(|window, app| {
        prior.update(app, |input, cx| {
            let _ = input.dispose(window, cx);
        })
    });
    drop(prior);

    let mut near_config = config(2 * 1024 * 1024, 32_768);
    near_config.settlement_coordinator = near_exhaustion.clone();
    let (near, near_cx) =
        cx.add_window_view(|window, cx| RangeTextInput::new(near_config, window, cx).unwrap());
    drive_initial_surface(&near, near_cx);
    let (near_base, near_current, local_next) = near.read_with(near_cx, |input, _| {
        (
            input.config.binding,
            input.surface.as_ref().unwrap().selection().head,
            input.next_id,
        )
    });
    let (_, near_text, near_objects) = admitted_successor_sources(SOURCE, 1, &[near_current]);
    let near_key = crate::MutationKey::new(
        near_base.binding(),
        near_base.revision(),
        crate::OperationId::new(u64::MAX - 1),
    );
    let near_begin = crate::MutationBeginRequest::new(
        crate::MutationProposal::new(
            near_key,
            crate::MutationKind::Edit,
            crate::MutationPositions::collapsed(near_current),
            crate::SourceRange::new(near_current, near_current).unwrap(),
            0,
        ),
        crate::MutationCursor::new(0),
        crate::MutationCursor::new(0),
    );
    let before_near_claim = near.read_with(near_cx, |input, _| fingerprint(input));
    near.update(near_cx, |input, cx| {
        assert!(matches!(
            input.begin_host_mutation(near_begin, &[], &near_text, &near_objects, cx),
            Err(RangeTextInputError::Mutation(
                crate::MutationError::MissingPositionProof(_)
            ))
        ));
    });
    near.read_with(near_cx, |input, _| {
        assert_eq!(fingerprint(input), before_near_claim);
        assert_eq!(input.next_id, local_next);
    });
    assert!(matches!(
        near_exhaustion.allocate_operation(),
        Err(RangeTextInputError::Stale)
    ));
    assert!(matches!(
        near_exhaustion.allocate_operation(),
        Err(RangeTextInputError::Stale)
    ));
    near.update(near_cx, |input, cx| {
        let clipboard = input
            .begin_clipboard(crate::ClipboardKind::Copy, cx)
            .unwrap();
        assert_eq!(clipboard.id().get(), local_next);
        assert_eq!(input.next_id, local_next + 1);
    });

    let coordinator = crate::RangeSettlementCoordinator::new(1).unwrap();
    let mut first_config = config(2 * 1024 * 1024, 32_768);
    first_config.settlement_coordinator = coordinator.clone();
    let (first, first_cx) =
        cx.add_window_view(|window, cx| RangeTextInput::new(first_config, window, cx).unwrap());
    drive_initial_surface(&first, first_cx);
    let (first_key, _) = drive_local_insert_to_commit_pending(&first, first_cx);
    assert_eq!(coordinator.retained_count(), 1);
    first_cx.update(|window, app| {
        first.update(app, |input, cx| {
            assert_eq!(
                input
                    .settle_mutation(first_key, crate::MutationOutcome::Rejected, window, cx)
                    .unwrap(),
                crate::MutationSettlement::Current(crate::MutationOutcome::Rejected)
            );
            let _ = input.dispose(window, cx);
        })
    });
    assert_eq!(coordinator.retained_count(), 0);
    drop(first);

    let mut second_config = config(2 * 1024 * 1024, 32_768);
    second_config.settlement_coordinator = coordinator.clone();
    let (second, second_cx) =
        cx.add_window_view(|window, cx| RangeTextInput::new(second_config, window, cx).unwrap());
    drive_initial_surface(&second, second_cx);
    let (base, current) = second.read_with(second_cx, |input, _| {
        (
            input.config.binding,
            input.surface.as_ref().unwrap().selection().head,
        )
    });
    let (_, text, objects) = admitted_successor_sources(SOURCE, 1, &[current]);
    let host_begin = |operation| {
        let key = crate::MutationKey::new(base.binding(), base.revision(), operation);
        crate::MutationBeginRequest::new(
            crate::MutationProposal::new(
                key,
                crate::MutationKind::Edit,
                crate::MutationPositions::collapsed(current),
                crate::SourceRange::new(current, current).unwrap(),
                0,
            ),
            crate::MutationCursor::new(0),
            crate::MutationCursor::new(0),
        )
    };
    for replay in [
        first_key.operation(),
        crate::OperationId::new(first_key.operation().get().saturating_sub(1)),
        crate::OperationId::new(u64::MAX - 1),
    ] {
        let before = second.read_with(second_cx, |input, _| fingerprint(input));
        second.update(second_cx, |input, cx| {
            assert!(matches!(
                input.begin_host_mutation(host_begin(replay), &[current], &text, &objects, cx),
                Err(RangeTextInputError::Stale)
            ));
        });
        second.read_with(second_cx, |input, _| assert_eq!(fingerprint(input), before));
    }

    second.update(second_cx, |input, _| {
        input
            .admit_edit_positions(&[current], &text, &objects)
            .unwrap();
    });
    second.update(second_cx, |input, cx| {
        input.insert_text("x".to_owned(), cx).unwrap()
    });
    let RangeTextInputRequest::MutationBegin(generated_begin) = second
        .update(second_cx, |input, _| input.take_request())
        .unwrap()
    else {
        panic!("generated operation after future rejection")
    };
    let generated_key = generated_begin.proposal().key();
    second.update(second_cx, |input, cx| {
        input.cancel_mutation(generated_key, cx).unwrap();
    });
    assert!(matches!(
        second.update(second_cx, |input, _| input.take_request()),
        Some(RangeTextInputRequest::CancelMutation(cancel)) if cancel.key() == generated_key
    ));

    let failed_operation = crate::OperationId::new(generated_key.operation().get() + 1);
    let failed_begin = host_begin(failed_operation);
    let before_failed_begin = second.read_with(second_cx, |input, _| fingerprint(input));
    second.update(second_cx, |input, cx| {
        assert!(matches!(
            input.begin_host_mutation(failed_begin, &[], &text, &objects, cx),
            Err(RangeTextInputError::Mutation(
                crate::MutationError::MissingPositionProof(_)
            ))
        ));
    });
    second.read_with(second_cx, |input, _| {
        assert_eq!(fingerprint(input), before_failed_begin)
    });
    second.update(second_cx, |input, cx| {
        assert!(matches!(
            input.begin_host_mutation(failed_begin, &[current], &text, &objects, cx),
            Err(RangeTextInputError::Stale)
        ));
    });

    let second_operation = crate::OperationId::new(failed_operation.get() + 1);
    let second_begin = host_begin(second_operation);
    let second_key = second_begin.proposal().key();
    second.update(second_cx, |input, cx| {
        assert_eq!(
            input
                .begin_host_mutation(second_begin, &[current], &text, &objects, cx)
                .unwrap(),
            second_key
        );
    });
    assert!(matches!(
        second.update(second_cx, |input, _| input.take_request()),
        Some(RangeTextInputRequest::MutationBegin(begin)) if begin == second_begin
    ));
    second.update(second_cx, |input, cx| {
        input.accept_mutation_preflight(second_key, cx).unwrap();
        let finish = crate::MutationStreamFinish {
            next_cursor: crate::MutationCursor::new(0),
            next_ordinal: 0,
            cumulative_identity: crate::MutationIdentity::ROOT,
            totals: crate::MutationTotals::default(),
        };
        input
            .submit_mutation_finish(
                crate::MutationFinishInput::new(
                    second_key,
                    finish,
                    finish,
                    base.extent(),
                    crate::MutationPositions::collapsed(current),
                ),
                cx,
            )
            .unwrap();
    });
    assert!(matches!(
        second.update(second_cx, |input, _| input.take_request()),
        Some(RangeTextInputRequest::MutationFinishInput(finish)) if finish.key() == second_key
    ));
    second.update(second_cx, |input, cx| {
        input.accept_mutation_finish(second_key, cx).unwrap();
    });
    assert!(matches!(
        second.update(second_cx, |input, _| input.take_request()),
        Some(RangeTextInputRequest::MutationCommit(commit)) if commit.key() == second_key
    ));
    assert_eq!(coordinator.retained_count(), 1);
    assert!(!coordinator.settle_mutation(first_key));
    second.read_with(second_cx, |input, _| {
        assert_eq!(input.edits.active_key(), Some(second_key));
        assert_eq!(input.edits.state(), crate::MutationState::CommitPending);
        assert_eq!(input.config.settlement_coordinator.retained_count(), 1);
    });
    second_cx.update(|window, app| {
        second.update(app, |input, cx| {
            input
                .settle_mutation(second_key, crate::MutationOutcome::Rejected, window, cx)
                .unwrap();
        })
    });
    assert_eq!(coordinator.retained_count(), 0);
    assert!(!coordinator.settle_mutation(first_key));
    assert!(!coordinator.settle_mutation(second_key));
}
