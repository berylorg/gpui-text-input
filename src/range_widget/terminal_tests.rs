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
        limits: RangeTextInputLimits::new(bytes, items, 32, 32, px(16.), 4).unwrap(),
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
        PageDemandEnvelope::Validation { .. } => unreachable!("geometry uses adjacent pages"),
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
    detached_edits: Vec<String>,
    segmentation: Option<String>,
    segmentation_action: Option<String>,
    platform: Option<String>,
    restoration: Option<String>,
    restoration_seed: Option<RangeRestorationSeed>,
    published_restoration: Option<RangeRestorationSeed>,
    replacement: Option<String>,
    pending_history: Option<String>,
    mutation_positions: Option<String>,
    adopted_positions: Option<String>,
    admitted_edit_proofs: Vec<String>,
    mutation_composition: Option<String>,
    pending_local_mutation: Option<String>,
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
        detached_edits: input
            .detached_edits
            .iter()
            .map(|edit| format!("{edit:?}"))
            .collect(),
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
    input.update(cx, |input, cx| {
        input.accept_mutation_finish(key, cx).unwrap()
    });
    assert!(matches!(
        input.update(cx, |input, _| input.take_request()),
        Some(RangeTextInputRequest::MutationCommit(commit)) if commit.key() == key
    ));
    (key, intended)
}

#[gpui::test]
fn terminal_target_replacement_accepts_fixed_exact_caps_and_rejects_one_under(
    cx: &mut gpui::TestAppContext,
) {
    const EXACT_BYTES: usize = 18_957;
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
                        bytes: 8_736,
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
        crate::OperationId::new(44),
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
    let key = crate::MutationKey::new(
        base.binding(),
        base.revision(),
        crate::OperationId::new(500),
    );
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
fn history_rejection_and_unbegun_rebind_cancellation_are_exact(cx: &mut gpui::TestAppContext) {
    let (input, cx) = cx.add_window_view(|window, cx| {
        RangeTextInput::new(config(2 * 1024 * 1024, 32_768), window, cx).unwrap()
    });
    drive_initial_surface(&input, cx);
    let current = input.read_with(cx, |input, _| {
        input.surface.as_ref().unwrap().selection().head
    });
    let (_, text, objects) = admitted_successor_sources(SOURCE, 1, &[current]);
    input.update(cx, |input, cx| {
        input.request_history(crate::MutationKind::Undo, cx)
    });
    let RangeTextInputRequest::HistoryIntent(intent) =
        input.update(cx, |input, _| input.take_request()).unwrap()
    else {
        panic!("history intent")
    };
    let proposal = crate::MutationProposal::new(
        intent.key(),
        intent.kind(),
        crate::MutationPositions::collapsed(current),
        crate::SourceRange::new(current, current).unwrap(),
        0,
    );
    let begin = crate::MutationBeginRequest::new(
        proposal,
        crate::MutationCursor::new(0),
        crate::MutationCursor::new(0),
    );
    input.update(cx, |input, cx| {
        input
            .submit_history_session(
                crate::RangeHistorySession::new(intent, begin),
                &[current],
                &text,
                &objects,
                cx,
            )
            .unwrap();
    });
    assert!(matches!(
        input.update(cx, |input, _| input.take_request()),
        Some(RangeTextInputRequest::MutationBegin(request)) if request == begin
    ));
    input.update(cx, |input, cx| {
        input.reject_mutation_preflight(intent.key(), cx).unwrap();
        assert!(input.pending_history.is_none());
    });

    input.update(cx, |input, cx| {
        input.request_history(crate::MutationKind::Redo, cx)
    });
    let RangeTextInputRequest::HistoryIntent(cancelled) =
        input.update(cx, |input, _| input.take_request()).unwrap()
    else {
        panic!("second history intent")
    };
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
        assert!(input.pending_history.is_none());
        assert!(input.requests.iter().any(|request| matches!(
            request,
            RangeTextInputRequest::CancelHistoryIntent(intent) if *intent == cancelled
        )));
    });
}

#[gpui::test]
fn history_page_collision_terminalizes_once_and_cleans_both_lanes(cx: &mut gpui::TestAppContext) {
    for lane in [crate::MutationLane::Source, crate::MutationLane::Proposal] {
        let (input, window_cx) = cx.add_window_view(|window, cx| {
            RangeTextInput::new(config(2 * 1024 * 1024, 32_768), window, cx).unwrap()
        });
        drive_initial_surface(&input, window_cx);
        let events = captured_events(&input, window_cx);
        let current = input.read_with(window_cx, |input, _| {
            input.surface.as_ref().unwrap().selection().head
        });
        let (_, text, objects) = admitted_successor_sources(SOURCE, 1, &[current]);
        input.update(window_cx, |input, cx| {
            input.request_history(crate::MutationKind::Undo, cx)
        });
        let RangeTextInputRequest::HistoryIntent(intent) = input
            .update(window_cx, |input, _| input.take_request())
            .unwrap()
        else {
            panic!("history collision intent")
        };
        let proposal = crate::MutationProposal::new(
            intent.key(),
            intent.kind(),
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
                .submit_history_session(
                    crate::RangeHistorySession::new(intent, begin),
                    &[current],
                    &text,
                    &objects,
                    cx,
                )
                .unwrap();
        });
        assert!(matches!(
            input.update(window_cx, |input, _| input.take_request()),
            Some(RangeTextInputRequest::MutationBegin(request)) if request == begin
        ));
        input.update(window_cx, |input, cx| {
            input.accept_mutation_preflight(intent.key(), cx).unwrap()
        });
        let page = |value: &str| {
            crate::MutationPage::new(
                crate::MutationPageKey::new(
                    intent.key(),
                    lane,
                    crate::MutationCursor::new(0),
                    0,
                    crate::MutationIdentity::ROOT,
                ),
                crate::MutationCursor::new(1),
                vec![crate::MutationPageItem::Utf8 {
                    inserted_offset: 0,
                    text: value.into(),
                }],
            )
            .unwrap()
        };
        input.update(window_cx, |input, cx| {
            input.submit_history_page(page("x"), cx).unwrap();
        });
        assert!(matches!(
            input.update(window_cx, |input, _| input.take_request()),
            Some(RangeTextInputRequest::MutationSourcePage(_))
                | Some(RangeTextInputRequest::MutationProposalPage(_))
        ));
        let collision = page("y");
        assert!(matches!(
            input.update(window_cx, |input, cx| {
                input.submit_history_page(collision.clone(), cx)
            }),
            Err(RangeTextInputError::Mutation(
                crate::MutationError::PageCollision
            ))
        ));
        let released = input.read_with(window_cx, |input, _| {
            assert!(input.pending_history.is_none());
            assert!(input.requests.is_empty());
            assert!(input.is_quiescent());
            input.export_restoration(None).unwrap();
            input.edits.released_counts()
        });
        let error_settlements = || {
            events
                .borrow()
                .iter()
                .filter(|event| {
                    matches!(
                        event,
                        RangeTextInputEvent::MutationSettled { key, outcome }
                            if *key == intent.key() && *outcome == crate::MutationOutcome::Error
                    )
                })
                .count()
        };
        assert_eq!(error_settlements(), 1);
        assert!(matches!(
            input.update(window_cx, |input, cx| {
                input.submit_mutation_page(collision, cx)
            }),
            Err(RangeTextInputError::Mutation(
                crate::MutationError::ObsoleteOperation(obsolete)
            )) if obsolete == intent.key()
        ));
        input.read_with(window_cx, |input, _| {
            assert!(input.is_quiescent());
            assert_eq!(input.edits.released_counts(), released);
        });
        assert_eq!(error_settlements(), 1);

        input.update(window_cx, |input, cx| {
            input.request_history(crate::MutationKind::Redo, cx)
        });
        let RangeTextInputRequest::HistoryIntent(fresh_intent) = input
            .update(window_cx, |input, _| input.take_request())
            .unwrap()
        else {
            panic!("fresh history intent")
        };
        let fresh_proposal = crate::MutationProposal::new(
            fresh_intent.key(),
            fresh_intent.kind(),
            crate::MutationPositions::collapsed(current),
            crate::SourceRange::new(current, current).unwrap(),
            0,
        );
        let fresh_begin = crate::MutationBeginRequest::new(
            fresh_proposal,
            crate::MutationCursor::new(0),
            crate::MutationCursor::new(0),
        );
        input.update(window_cx, |input, cx| {
            input
                .submit_history_session(
                    crate::RangeHistorySession::new(fresh_intent, fresh_begin),
                    &[current],
                    &text,
                    &objects,
                    cx,
                )
                .unwrap();
        });
        assert!(matches!(
            input.update(window_cx, |input, _| input.take_request()),
            Some(RangeTextInputRequest::MutationBegin(request)) if request == fresh_begin
        ));
        input.update(window_cx, |input, cx| {
            input
                .reject_mutation_preflight(fresh_intent.key(), cx)
                .unwrap();
            assert!(input.is_quiescent());
        });
    }
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
    let key = crate::MutationKey::new(
        base.binding(),
        base.revision(),
        crate::OperationId::new(700),
    );
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
    input.read_with(cx, |input, _| assert_eq!(input.detached_edits.len(), 1));
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input
                .settle_mutation(key, crate::MutationOutcome::Rejected, window, cx)
                .unwrap();
        })
    });
    input.read_with(cx, |input, _| assert!(input.detached_edits.is_empty()));
    let late = cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.settle_mutation(key, crate::MutationOutcome::Rejected, window, cx)
        })
    });
    assert!(matches!(late, Err(RangeTextInputError::Stale)));
    input.read_with(cx, |input, _| assert!(input.detached_edits.is_empty()));
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
                bytes: 8_736,
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
        bytes: 23_051,
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
