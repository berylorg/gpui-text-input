mod clipboard;
mod geometry;
mod history;
mod ime;
mod interaction;
mod keyboard;
mod lifecycle;
mod page_delivery;
mod platform;
mod pointer;
mod render;
mod replacement;
mod restoration;
mod surface;
#[cfg(test)]
mod terminal_tests;
mod transition;
mod types;

pub use surface::{
    CoherentRangeSurface, RangeSurfaceCharge, RangeSurfaceHit, RealizedInlineObjectGeometry,
    RealizedInlineObjectPresentation, RealizedObjectGapGeometry,
};
pub use types::*;

use std::{
    cell::Cell,
    collections::{HashSet, VecDeque},
    rc::Rc,
};

use gpui::{Bounds, Context, EventEmitter, FocusHandle, Focusable, Pixels, Subscription, Window};
use gpui_scrollbar::{
    ScrollDirection, ScrollbarInteraction, ScrollbarMountGeneration, ScrollbarOwnerId,
    ScrollbarOwnerKey, ScrollbarScrollState, ScrollbarState, ScrollbarVisibilityUpdateCallback,
};

use crate::{
    ByteRange, ExactGeometryOwner, ObjectResidency, RangeClipboardCoordinator,
    RangeEditCoordinator, RangeResidency, SegmentationContinuation,
};

pub struct RangeTextInput {
    focus_handle: FocusHandle,
    enabled: bool,
    read_only: bool,
    config: RangeTextInputConfig,
    residency: RangeResidency,
    object_residency: ObjectResidency,
    geometry: ExactGeometryOwner,
    edits: RangeEditCoordinator,
    clipboard: RangeClipboardCoordinator,
    pending_clipboard_page: Option<clipboard::PendingClipboardPage>,
    clipboard_cut_proofs: Option<(
        crate::ClipboardKey,
        Vec<crate::range_edit::SourcePositionProof>,
    )>,
    dispatched_clipboard_write: Option<crate::ClipboardKey>,
    surface: Option<CoherentRangeSurface>,
    last_surface_admission: Option<RangeSurfaceCharge>,
    #[cfg(test)]
    last_widget_admission_components:
        std::cell::Cell<Option<transition::WidgetAdmissionComponents>>,
    desired: DesiredSurface,
    requests: VecDeque<RangeTextInputRequest>,
    dispatched_pages: HashSet<crate::PageRequestKey>,
    dispatched_object_pages: HashSet<crate::ObjectRequestKey>,
    dispatched_mutations: HashSet<crate::MutationKey>,
    active_geometry: Option<crate::GeometryJobKey>,
    pending_geometry_page: Option<geometry::PendingGeometryPage>,
    pending_geometry_object: Option<geometry::PendingGeometryObject>,
    pending_page_aliases: Vec<page_delivery::PendingPageAlias>,
    surface_candidate: Option<SurfaceCandidate>,
    segmentation: Option<SegmentationContinuation>,
    segmentation_action: Option<interaction::PendingBoundaryAction>,
    platform: Option<platform::PlatformReplay>,
    restoration: Option<restoration::RestorationValidation>,
    restoration_seed: Option<RangeRestorationSeed>,
    published_restoration: Option<RangeRestorationSeed>,
    replacement: Option<replacement::ReplacementScan>,
    pending_history: Option<history::PendingHistory>,
    detached_edits: Vec<RangeEditCoordinator>,
    mutation_positions: Option<(crate::MutationKey, crate::MutationPositions)>,
    adopted_positions: Option<crate::MutationPositions>,
    admitted_edit_proofs: Vec<crate::range_edit::SourcePositionProof>,
    mutation_composition: Option<(crate::MutationKey, ByteRange, RangeSourceSelection)>,
    pending_local_mutation: Option<interaction::PendingLocalMutation>,
    platform_ready: Option<(std::ops::Range<usize>, String)>,
    next_id: u64,
    mounted: bool,
    pointer_anchor: Option<crate::SourcePosition>,
    active_object: Option<ActiveInlineObject>,
    pending_select_all: bool,
    scrollbar: RangeScrollbar,
    last_bounds: Option<Bounds<Pixels>>,
    focus_subscription: Option<Subscription>,
}

struct RangeScrollbar {
    owner: ScrollbarOwnerKey,
    state: ScrollbarState,
    model: Rc<Cell<Option<ScrollbarScrollState>>>,
    interaction: ScrollbarInteraction,
    on_visibility_update: ScrollbarVisibilityUpdateCallback,
}

#[derive(Clone, Copy)]
enum PendingScroll {
    Set(Pixels),
    Page(ScrollDirection, Pixels),
}

impl Focusable for RangeTextInput {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl EventEmitter<RangeTextInputEvent> for RangeTextInput {}

impl RangeTextInput {
    const MAX_QUEUED_MUTATION_REQUESTS: usize = 2;

    pub fn new(
        config: RangeTextInputConfig,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<Self, RangeTextInputError> {
        let finite_metrics = [
            config.viewport_extent,
            config.overscan,
            config.limits.max_intra_anchor,
        ];
        if config.limits.page_bytes < 4
            || config.limits.platform_bytes < 4
            || config.viewport_extent <= Pixels::ZERO
            || config.overscan < Pixels::ZERO
            || finite_metrics
                .iter()
                .any(|value| !f32::from(*value).is_finite())
            || config.geometry_limits.max_page_bytes() > config.residency_limits.max_pending_bytes()
            || config.clipboard_limits.max_text_page_bytes()
                > config.residency_limits.max_pending_bytes()
            || config.segmentation_limits.max_page_bytes()
                > config.residency_limits.max_pending_bytes()
            || config.limits.page_bytes > config.residency_limits.max_pending_bytes()
            || config.limits.platform_bytes > config.residency_limits.max_pending_bytes()
            || config.object_residency_limits.max_pending_requests() < 1
            || config.object_residency_limits.max_resident_objects()
                > config.object_residency_limits.max_pending_objects()
            || config.object_residency_limits.max_resident_bytes()
                > config.object_residency_limits.max_pending_bytes()
        {
            return Err(RangeTextInputError::InvalidLimits);
        }
        let binding = config.binding;
        let scrollbar_owner = ScrollbarOwnerKey::new(
            ScrollbarOwnerId::new(cx.entity_id().as_u64()),
            ScrollbarMountGeneration::new(1),
        );
        let scrollbar_model = Rc::new(Cell::new(None));
        let pending_scroll = Rc::new(Cell::new(None));
        let weak = cx.weak_entity();
        let scrollbar_interaction = ScrollbarInteraction::new(
            {
                let model = scrollbar_model.clone();
                move || model.get()
            },
            {
                let pending = pending_scroll.clone();
                move |_, offset| pending.set(Some(PendingScroll::Set(offset)))
            },
            {
                let pending = pending_scroll.clone();
                move |_, direction, distance| {
                    pending.set(Some(PendingScroll::Page(direction, distance)));
                }
            },
            |_| {},
            |_| {},
            {
                let weak = weak.clone();
                move |_, window, cx| {
                    let Some(request) = pending_scroll.take() else {
                        return;
                    };
                    let _ = weak.update(cx, |input, cx| {
                        input.apply_scrollbar(request, window, cx);
                    });
                }
            },
        );
        let on_visibility_update = Rc::new(move |_, _: &mut Window, cx: &mut gpui::App| {
            let _ = weak.update(cx, |_, cx| cx.notify());
        });
        let mut this = Self {
            focus_handle: cx.focus_handle(),
            enabled: true,
            read_only: false,
            residency: RangeResidency::new(binding, config.residency_limits),
            object_residency: ObjectResidency::new(
                binding,
                config.presentation_generation,
                config.object_residency_limits,
            ),
            geometry: ExactGeometryOwner::new(
                binding,
                config.presentation_generation,
                config.layout.clone(),
                config.style.clone(),
                config.geometry_limits,
            )?,
            edits: RangeEditCoordinator::new(binding, config.mutation_limits),
            clipboard: RangeClipboardCoordinator::new_composite(
                binding,
                config.presentation_generation,
                config.clipboard_limits,
            ),
            pending_clipboard_page: None,
            clipboard_cut_proofs: None,
            dispatched_clipboard_write: None,
            surface: None,
            last_surface_admission: None,
            #[cfg(test)]
            last_widget_admission_components: std::cell::Cell::new(None),
            desired: DesiredSurface::origin(config.viewport_extent, config.overscan),
            requests: VecDeque::new(),
            dispatched_pages: HashSet::new(),
            dispatched_object_pages: HashSet::new(),
            dispatched_mutations: HashSet::new(),
            active_geometry: None,
            pending_geometry_page: None,
            pending_geometry_object: None,
            pending_page_aliases: Vec::new(),
            surface_candidate: None,
            segmentation: None,
            segmentation_action: None,
            platform: None,
            restoration: None,
            restoration_seed: None,
            published_restoration: None,
            replacement: None,
            pending_history: None,
            detached_edits: Vec::new(),
            mutation_positions: None,
            adopted_positions: None,
            admitted_edit_proofs: Vec::new(),
            mutation_composition: None,
            pending_local_mutation: None,
            platform_ready: None,
            next_id: 1,
            mounted: true,
            pointer_anchor: None,
            active_object: None,
            pending_select_all: false,
            scrollbar: RangeScrollbar {
                owner: scrollbar_owner,
                state: ScrollbarState::new(scrollbar_owner),
                model: scrollbar_model,
                interaction: scrollbar_interaction,
                on_visibility_update,
            },
            last_bounds: None,
            focus_subscription: None,
            config,
        };
        this.start_index()?;
        let focus = this.focus_handle.clone();
        this.focus_subscription = Some(cx.on_focus_out(&focus, window, |input, _, _, cx| {
            if let Ok(candidate) = input.prepare_focus_loss_transition(input.desired) {
                let _ = input.commit_widget_transition(candidate, Some(cx));
            }
            cx.emit(RangeTextInputEvent::FocusLost);
            cx.notify();
        }));
        Ok(this)
    }

    pub fn surface(&self) -> Option<&CoherentRangeSurface> {
        self.surface.as_ref()
    }

    pub(super) fn interactive_surface(&self) -> Option<&CoherentRangeSurface> {
        self.surface.as_ref().filter(|surface| {
            surface.binding() == self.config.binding
                && surface.geometry_key() == self.geometry.key()
                && self.mounted
        })
    }

    pub fn geometry_estimate(&self) -> Option<crate::StreamingGeometryEstimate> {
        self.geometry.estimate()
    }

    pub const fn last_surface_admission_charge(&self) -> Option<RangeSurfaceCharge> {
        self.last_surface_admission
    }

    pub fn take_request(&mut self) -> Option<RangeTextInputRequest> {
        let request = self.requests.pop_front()?;
        match &request {
            RangeTextInputRequest::Page(page) => {
                self.dispatched_pages.insert(page.key());
            }
            RangeTextInputRequest::ObjectPage(page) => {
                self.dispatched_object_pages.insert(page.key());
            }
            RangeTextInputRequest::MutationBegin(begin) => {
                self.dispatched_mutations.insert(begin.proposal().key());
            }
            RangeTextInputRequest::MutationSourcePage(request)
            | RangeTextInputRequest::MutationProposalPage(request) => {
                self.dispatched_mutations.insert(request.page().key().key());
            }
            RangeTextInputRequest::MutationFinishInput(finish) => {
                self.dispatched_mutations.insert(finish.key());
            }
            RangeTextInputRequest::MutationCommit(commit) => {
                self.dispatched_mutations.insert(commit.key());
            }
            RangeTextInputRequest::ClipboardWrite(write) => {
                self.dispatched_clipboard_write = Some(write.key());
            }
            _ => {}
        }
        Some(request)
    }

    pub fn is_quiescent(&self) -> bool {
        self.active_geometry.is_none()
            && self.pending_geometry_page.is_none()
            && self.pending_geometry_object.is_none()
            && self.residency.counts().pending_requests == 0
            && self.object_residency.counts().pending_requests == 0
            && self.segmentation.is_none()
            && self.platform.is_none()
            && self.platform_ready.is_none()
            && self.restoration.is_none()
            && self.restoration_seed.is_none()
            && self.surface_candidate.is_none()
            && self.replacement.is_none()
            && self.pending_history.is_none()
            && matches!(self.clipboard.state(), crate::ClipboardState::Idle)
            && self.dispatched_clipboard_write.is_none()
            && matches!(
                self.edits.state(),
                crate::MutationState::Idle | crate::MutationState::Settled
            )
            && self.detached_edits.is_empty()
            && self.requests.is_empty()
    }

    pub fn focus(&self, window: &mut Window) {
        if self.mounted && self.enabled {
            window.focus(&self.focus_handle);
        }
    }

    pub fn set_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if self.enabled != enabled {
            let active = if !enabled && self.active_object.is_some() {
                transition::ActiveObjectTransition::Clear(
                    InlineObjectRealizationLossReason::Disabled,
                )
            } else {
                transition::ActiveObjectTransition::Preserve
            };
            let pointer_anchor = enabled.then_some(self.pointer_anchor).flatten();
            if let Ok(candidate) =
                self.prepare_interaction_state_transition(enabled, pointer_anchor, active)
            {
                self.commit_active_object_transition(candidate, cx);
            }
        }
    }

    pub fn set_read_only(&mut self, read_only: bool, cx: &mut Context<Self>) {
        if self.read_only != read_only {
            self.read_only = read_only;
            cx.notify();
        }
    }

    pub fn set_layout(
        &mut self,
        layout: gpui::StreamingLayoutBinding,
        style: crate::StreamingGeometryStyle,
        cx: &mut Context<Self>,
    ) -> Result<(), RangeTextInputError> {
        if !self.mounted {
            return Err(RangeTextInputError::NotMounted);
        }
        let candidate = self.prepare_layout_transition(layout, style)?;
        let progress = self.commit_widget_transition(candidate, Some(cx));
        debug_assert_eq!(progress, crate::ExactGeometryProgress::Scanning);
        Ok(())
    }

    pub fn set_presentation_generation(
        &mut self,
        presentation_generation: crate::PresentationGeneration,
        cx: &mut Context<Self>,
    ) -> Result<(), RangeTextInputError> {
        if !self.mounted {
            return Err(RangeTextInputError::NotMounted);
        }
        if self.config.presentation_generation == presentation_generation {
            return Ok(());
        }
        let candidate = self.prepare_presentation_transition(presentation_generation)?;
        let progress = self.commit_widget_transition(candidate, Some(cx));
        debug_assert_eq!(progress, crate::ExactGeometryProgress::Scanning);
        Ok(())
    }

    pub fn request_absolute_scroll(
        &mut self,
        block_offset: Pixels,
        cx: &mut Context<Self>,
    ) -> Result<(), RangeTextInputError> {
        if !self.mounted {
            return Err(RangeTextInputError::NotMounted);
        }
        if !f32::from(block_offset).is_finite() || block_offset < Pixels::ZERO {
            return Err(RangeTextInputError::InvalidLimits);
        }
        let mut desired = self.desired;
        desired.target_block = block_offset;
        desired.preserve_scroll_anchor = false;
        desired.reveal_caret = false;
        if self.restoration.is_none() && self.restoration_seed.is_none() {
            let candidate = self.prepare_target_transition(desired, None)?;
            let _ = self.commit_widget_transition(candidate, Some(cx));
        } else {
            // Restoration owns its own exact rejection path until that task is retired.
            self.desired = desired;
            self.start_target(cx)?;
        }
        Ok(())
    }

    fn next_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id = self
            .next_id
            .checked_add(1)
            .expect("range widget id exhausted");
        id
    }

    fn push_request(&mut self, request: RangeTextInputRequest, cx: &mut Context<Self>) {
        self.requests.push_back(request);
        cx.notify();
    }

    fn mutation_queue_has_capacity(&self, key: crate::MutationKey) -> bool {
        self.queued_mutation_requests(key) < Self::MAX_QUEUED_MUTATION_REQUESTS
    }

    fn queued_mutation_requests(&self, key: crate::MutationKey) -> usize {
        self.requests
            .iter()
            .filter(|request| match request {
                RangeTextInputRequest::MutationBegin(begin) => begin.proposal().key() == key,
                RangeTextInputRequest::MutationSourcePage(page)
                | RangeTextInputRequest::MutationProposalPage(page) => {
                    page.page().key().key() == key
                }
                RangeTextInputRequest::MutationFinishInput(finish) => finish.key() == key,
                RangeTextInputRequest::MutationCommit(commit) => commit.key() == key,
                RangeTextInputRequest::CancelMutation(cancel) => cancel.key() == key,
                RangeTextInputRequest::DetachedMutation(detached) => *detached == key,
                _ => false,
            })
            .take(Self::MAX_QUEUED_MUTATION_REQUESTS)
            .count()
    }
}
