use std::{cell::RefCell, rc::Rc, sync::Arc};

use gpui::{SharedString, StreamingLayoutPosition, TextRun, black, font, px};

use super::*;
use crate::{
    BindingId, ByteOffset, ClipboardLimits, ExactGeometryLimits, InlineObjectFact, InlineObjectId,
    InlineObjectOrder, InlineObjectPresentation, LogicalExtent, MutationLimits, ObjectDemand,
    ObjectDemandEnvelope, ObjectDirection, ObjectPage, ObjectPageEdgeFact, ObjectPageId,
    ObjectPurpose, ObjectRequestId, ObjectResidencyLimits, PageDemand, PageDemandEnvelope,
    PageDirection, PageEdgeFact, PageId, PagePurpose, PageRequest, PageRequestId,
    PresentationGeneration, RangeBinding, RangePage, RangeResidency, RangeSourceSelection,
    RangeTextInputConfig, RangeTextInputEvent, RangeTextInputLimits, RangeTextInputRequest,
    ResidencyLimits, SegmentationLimits, SourcePosition, SourceRevision, StreamingGeometryStyle,
    StreamingOversizePresentation, TextInputTheme,
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
    let (start, end) = match request.key().demand() {
        PageDemandEnvelope::Adjacent {
            anchor,
            direction: PageDirection::Forward,
            max_payload_bytes,
        } => (
            anchor.get() as usize,
            (anchor.get() as usize + max_payload_bytes as usize).min(SOURCE.len()),
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
        SOURCE[start..end].to_owned(),
        vec![],
        if start == 0 {
            PageEdgeFact::DocumentBoundary
        } else {
            PageEdgeFact::Continues
        },
        if end == SOURCE.len() {
            PageEdgeFact::DocumentBoundary
        } else {
            PageEdgeFact::Continues
        },
        end == SOURCE.len(),
    )
    .unwrap()
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
        .prove_object_page_anchors(binding(), &object_page)
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
    pending_insert: Option<String>,
    pending_object_remove: Option<String>,
    platform_ready: Option<String>,
    pending_select_all: bool,
    pointer_anchor: Option<crate::SourcePosition>,
    scrollbar_owner: gpui_scrollbar::ScrollbarOwnerKey,
    scrollbar_state_owner: Option<gpui_scrollbar::ScrollbarOwnerKey>,
    scrollbar_model: Option<gpui_scrollbar::ScrollbarScrollState>,
}

#[derive(Debug, PartialEq)]
struct TerminalFingerprint {
    surface: String,
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
        pending_insert: input
            .pending_insert
            .as_ref()
            .map(|state| format!("{state:?}")),
        pending_object_remove: input
            .pending_object_remove
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
    let surface = input.surface.as_ref().unwrap();
    TerminalFingerprint {
        surface: format!("{surface:?}"),
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

#[gpui::test]
fn terminal_target_replacement_accepts_fixed_exact_caps_and_rejects_one_under(
    cx: &mut gpui::TestAppContext,
) {
    const EXACT_BYTES: usize = 15_341;
    const EXACT_ITEMS: usize = 102;
    const RESIDENT_BYTES: usize = 783;
    const RESIDENT_ITEMS: usize = 3;
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
                bytes: RESIDENT_BYTES,
                items: RESIDENT_ITEMS,
            }
        );
        // These fixed differences are the prior formula's omitted initialized page graph. The
        // oracle comes from the public page-record charges above, not candidate introspection.
        assert_eq!(EXACT_BYTES - RESIDENT_BYTES, 14_558);
        assert_eq!(EXACT_ITEMS - RESIDENT_ITEMS, 99);
        let events = captured_events(&input, cx);
        let before = input.read_with(cx, |input, _| fingerprint(input));
        let event_count = events.borrow().len();
        let result = input.update(cx, |input, cx| {
            input.request_absolute_scroll(px(100_000.), cx)
        });
        assert_eq!(result.is_ok(), succeeds, "{result:?}");
        if succeeds {
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
    let RangeTextInputRequest::MutationPreflight(proposal) =
        input.update(cx, |input, _| input.take_request()).unwrap()
    else {
        panic!("insert preflight")
    };
    input.update(cx, |input, cx| {
        input.accept_mutation_preflight(proposal.key(), cx).unwrap()
    });
    let mut positions = None;
    while let Some(request) = input.update(cx, |input, _| input.take_request()) {
        if let RangeTextInputRequest::MutationFragment { fragment, .. } = request
            && let crate::MutationFragmentPayload::Terminal { intended } = fragment.payload()
        {
            positions = Some(*intended);
        }
    }
    let positions = positions.expect("terminal successor positions");
    input.update(cx, |input, _| {
        input.admit_mutation_commit(proposal.key()).unwrap()
    });
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
    let exact = input.update(cx, |input, _| {
        let prior = input.scrollbar.owner;
        let replacement = gpui_scrollbar::ScrollbarOwnerKey::new(
            prior.owner_id,
            gpui_scrollbar::ScrollbarMountGeneration::new(prior.mount_generation.get() + 1),
        );
        input
            .prepare_rebind_transition(
                successor_binding,
                Some(RangeSourceSelection {
                    anchor: positions.selection_anchor(),
                    head: positions.selection_head(),
                }),
                prior,
                replacement,
                crate::InlineObjectRealizationLossReason::Superseded,
                Some((proposal.key(), outcome)),
                None,
                Some((positions, outcome_commit_proofs(outcome))),
            )
            .unwrap()
            .admission_charge()
    });
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
                proposal.key(),
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
        assert_eq!(input.edits.active_key(), Some(proposal.key()));
    });
    assert_eq!(events.borrow().len(), event_count);
    input.update(cx, |input, _| {
        input.config.limits.max_surface_bytes = exact.bytes;
    });
    let accepted = cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.settle_committed_mutation(
                proposal.key(),
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
