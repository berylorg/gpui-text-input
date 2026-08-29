mod clipboard;
mod geometry;
mod history;
mod ime;
mod interaction;
mod keyboard;
mod lifecycle;
mod object_edit;
mod object_surface;
mod page_delivery;
mod platform;
mod pointer;
mod realization;
mod render;
mod replacement;
mod response_custody;
mod restoration;
mod surface;
#[cfg(test)]
mod terminal_tests;
mod transition;
mod types;

pub use surface::{
    CoherentRangeSurface, RangeSurfaceCharge, RangeSurfaceFiller, RangeSurfaceHit,
    RealizedInlineObjectGeometry, RealizedInlineObjectPresentation, RealizedObjectGapGeometry,
};
pub use types::*;

use std::{cell::Cell, collections::VecDeque, rc::Rc};

use gpui::{Bounds, Context, EventEmitter, FocusHandle, Focusable, Pixels, Subscription, Window};
use gpui_scrollbar::{
    ScrollDirection, ScrollbarInteraction, ScrollbarMountGeneration, ScrollbarOwnerId,
    ScrollbarOwnerKey, ScrollbarScrollState, ScrollbarState, ScrollbarVisibilityUpdateCallback,
};

use crate::{
    ByteRange, ExactGeometryOwner, ObjectResidency, RangeClipboardCoordinator,
    RangeEditCoordinator, RangeResidency, SegmentationContinuation,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DispatchedClipboard {
    Provenance(crate::ClipboardProvenancePageKey),
    Write(crate::ClipboardKey),
}

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
    dispatched_clipboard: Option<DispatchedClipboard>,
    surface: Option<CoherentRangeSurface>,
    last_surface_admission: Option<RangeSurfaceCharge>,
    last_realization_step: RangeRealizationStep,
    realization_frame_generation: u64,
    realization_continuation_scheduled: bool,
    realization_high_water: RangeRealizationOwnership,
    surface_high_water: RangeSurfaceCharge,
    deferred_geometry_response: Option<geometry::DeferredGeometryResponse>,
    response_custody: VecDeque<response_custody::RangeResponseCustody>,
    active_response_processing: RangeSurfaceCharge,
    last_response_rejection: Option<RangeResponseRejectionClass>,
    response_rejection_count: u64,
    superseded_geometry_object_responses_settled: u64,
    pending_response_exact_geometry_failure_stage: Option<crate::ExactGeometryFailureStage>,
    last_response_rejection_stage: Option<crate::ExactGeometryFailureStage>,
    pending_target_intent: Option<realization::PendingTargetIntent>,
    pending_index_intent: bool,
    pending_layout_intent: Option<realization::PendingLayoutIntent>,
    pending_presentation_intent: Option<crate::PresentationGeneration>,
    pending_rebind_intent: Option<realization::PendingRebindIntent>,
    #[cfg(test)]
    last_widget_admission_components:
        std::cell::Cell<Option<transition::WidgetAdmissionComponents>>,
    desired: DesiredSurface,
    requests: VecDeque<RangeTextInputRequest>,
    dispatched_pages: realization::DispatchedKeys<crate::PageRequestKey>,
    dispatched_object_pages: realization::DispatchedKeys<crate::ObjectRequestKey>,
    dispatched_mutations: realization::DispatchedKeys<crate::MutationKey>,
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
    history_frontier: RangeHistoryFrontier,
    mutation_positions: Option<(crate::MutationKey, crate::MutationPositions)>,
    adopted_positions: Option<crate::MutationPositions>,
    admitted_edit_proofs: Vec<crate::range_edit::SourcePositionProof>,
    mutation_composition: Option<(crate::MutationKey, ByteRange, RangeSourceSelection)>,
    pending_local_mutation: Option<interaction::PendingLocalMutation>,
    prepared_local_operation: Option<crate::OperationId>,
    platform_ready: Option<(std::ops::Range<usize>, String)>,
    next_id: u64,
    mounted: bool,
    pointer_anchor: Option<crate::SourcePosition>,
    active_object: Option<ActiveInlineObject>,
    attached_inline_object_surface: Option<(u64, RealizedInlineObjectAnchor)>,
    next_inline_object_surface_attachment: u64,
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
            config.limits.max_realized_block_extent,
        ];
        if config.limits.page_bytes < 4
            || config.limits.platform_bytes < 4
            || config.limits.max_surface_bytes == 0
            || config.limits.max_surface_items == 0
            || config.limits.max_realization_work_per_frame == 0
            || config.limits.max_realized_block_extent <= Pixels::ZERO
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
            || usize::try_from(config.limits.page_bytes).is_err()
            || usize::try_from(config.limits.platform_bytes).is_err()
            || usize::try_from(config.geometry_limits.max_page_bytes()).is_err()
            || usize::try_from(config.residency_limits.max_pending_bytes()).is_err()
            || usize::try_from(config.clipboard_limits.max_text_page_bytes()).is_err()
            || usize::try_from(config.segmentation_limits.max_page_bytes()).is_err()
            || config.object_residency_limits.max_pending_requests() < 1
            || config.object_residency_limits.max_resident_objects()
                > config.object_residency_limits.max_pending_objects()
            || config.object_residency_limits.max_resident_bytes()
                > config.object_residency_limits.max_pending_bytes()
        {
            return Err(RangeTextInputError::InvalidLimits);
        }
        let initial_realization_extent = bounded_realization_extent(
            config.viewport_extent,
            config.limits.max_realized_block_extent,
        );
        let maximum_target_extent =
            f32::from(config.limits.max_realized_block_extent) + f32::from(config.overscan);
        let realization_owner = Self::realization_owner_charge();
        let response_custody_capacity = config
            .residency_limits
            .max_pending_requests()
            .checked_add(config.object_residency_limits.max_pending_requests())
            .ok_or(RangeTextInputError::InvalidLimits)?;
        let request_capacity =
            checked_request_capacity(&config).ok_or(RangeTextInputError::InvalidLimits)?;
        let requests = VecDeque::with_capacity(request_capacity);
        let request_storage = RangeSurfaceCharge {
            bytes: requests
                .capacity()
                .checked_mul(std::mem::size_of::<RangeTextInputRequest>())
                .ok_or(RangeTextInputError::InvalidLimits)?,
            items: requests.capacity(),
        };
        let response_custody_storage = RangeSurfaceCharge {
            bytes: response_custody_capacity
                .checked_mul(std::mem::size_of::<response_custody::RangeResponseCustody>())
                .ok_or(RangeTextInputError::InvalidLimits)?,
            items: response_custody_capacity,
        };
        let dispatch_charge = [
            realization::DispatchedKeys::<crate::PageRequestKey>::checked_allocation_charge(
                config.residency_limits.max_pending_requests(),
            ),
            realization::DispatchedKeys::<crate::ObjectRequestKey>::checked_allocation_charge(
                config.object_residency_limits.max_pending_requests(),
            ),
            realization::DispatchedKeys::<crate::MutationKey>::checked_allocation_charge(
                Self::MAX_QUEUED_MUTATION_REQUESTS,
            ),
        ]
        .into_iter()
        .try_fold(RangeSurfaceCharge::default(), |total, charge| {
            let charge = charge?;
            Some(RangeSurfaceCharge {
                bytes: total.bytes.checked_add(charge.bytes)?,
                items: total.items.checked_add(charge.items)?,
            })
        });
        let geometry_owner_charge =
            ExactGeometryOwner::initial_required_charge(&config.layout, &config.style)
                .map(|(bytes, items)| RangeSurfaceCharge { bytes, items })
                .map_err(RangeTextInputError::Geometry)?;
        let initial_owner_charge = [
            Some(realization_owner),
            Some(request_storage),
            Some(response_custody_storage),
            dispatch_charge,
            crate::residency::RangeResidency::checked_initial_owner_storage_charge(
                config.residency_limits,
            ),
            crate::object_residency::ObjectResidency::checked_initial_owner_storage_charge(
                config.object_residency_limits,
            ),
            Some(geometry_owner_charge),
        ]
        .into_iter()
        .try_fold(RangeSurfaceCharge::default(), |total, charge| {
            let charge = charge?;
            Some(RangeSurfaceCharge {
                bytes: total.bytes.checked_add(charge.bytes)?,
                items: total.items.checked_add(charge.items)?,
            })
        });
        let bounded_usize_ceilings = [
            usize::try_from(config.limits.page_bytes).ok(),
            usize::try_from(config.limits.platform_bytes).ok(),
            usize::try_from(config.geometry_limits.max_page_bytes()).ok(),
            usize::try_from(config.residency_limits.max_pending_bytes()).ok(),
            usize::try_from(config.clipboard_limits.max_text_page_bytes()).ok(),
            usize::try_from(config.segmentation_limits.max_page_bytes()).ok(),
            Some(config.object_residency_limits.max_pending_bytes()),
            Some(config.geometry_limits.max_retained_bytes()),
        ];
        if !maximum_target_extent.is_finite()
            || config
                .residency_limits
                .max_resident_pages()
                .checked_mul(2)
                .is_none()
            || config
                .object_residency_limits
                .max_resident_objects()
                .checked_mul(2)
                .is_none()
            || realization_owner.bytes > config.limits.max_surface_bytes
            || realization_owner.items > config.limits.max_surface_items
            || initial_owner_charge.is_none_or(|charge| {
                charge.bytes > config.limits.max_surface_bytes
                    || charge.items > config.limits.max_surface_items
            })
            || dispatch_charge.is_none_or(|dispatch| {
                realization_owner
                    .bytes
                    .checked_add(dispatch.bytes)
                    .is_none_or(|bytes| bytes > config.limits.max_surface_bytes)
                    || realization_owner
                        .items
                        .checked_add(dispatch.items)
                        .is_none_or(|items| items > config.limits.max_surface_items)
            })
            || bounded_usize_ceilings.iter().any(|ceiling| {
                ceiling.is_none_or(|ceiling| ceiling.checked_add(realization_owner.bytes).is_none())
            })
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
                config.atom_clipboard_policy,
                config.clipboard_limits,
            )
            .map_err(RangeTextInputError::Clipboard)?,
            pending_clipboard_page: None,
            clipboard_cut_proofs: None,
            dispatched_clipboard: None,
            surface: None,
            last_surface_admission: None,
            last_realization_step: RangeRealizationStep {
                spent: 0,
                remaining: config.limits.max_realization_work_per_frame,
                progressed: false,
                reached_external_boundary: false,
            },
            realization_frame_generation: 0,
            realization_continuation_scheduled: false,
            realization_high_water: RangeRealizationOwnership::default(),
            surface_high_water: RangeSurfaceCharge::default(),
            deferred_geometry_response: None,
            response_custody: VecDeque::with_capacity(response_custody_capacity),
            active_response_processing: RangeSurfaceCharge::default(),
            last_response_rejection: None,
            response_rejection_count: 0,
            superseded_geometry_object_responses_settled: 0,
            pending_response_exact_geometry_failure_stage: None,
            last_response_rejection_stage: None,
            pending_target_intent: None,
            pending_index_intent: false,
            pending_layout_intent: None,
            pending_presentation_intent: None,
            pending_rebind_intent: None,
            #[cfg(test)]
            last_widget_admission_components: std::cell::Cell::new(None),
            desired: DesiredSurface::origin(
                config.viewport_extent,
                initial_realization_extent,
                config.overscan,
            ),
            requests,
            dispatched_pages: realization::DispatchedKeys::with_capacity(
                config.residency_limits.max_pending_requests(),
            ),
            dispatched_object_pages: realization::DispatchedKeys::with_capacity(
                config.object_residency_limits.max_pending_requests(),
            ),
            dispatched_mutations: realization::DispatchedKeys::with_capacity(
                Self::MAX_QUEUED_MUTATION_REQUESTS,
            ),
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
            history_frontier: RangeHistoryFrontier::unavailable(config.binding),
            mutation_positions: None,
            adopted_positions: None,
            admitted_edit_proofs: Vec::new(),
            mutation_composition: None,
            pending_local_mutation: None,
            prepared_local_operation: None,
            platform_ready: None,
            next_id: 1,
            mounted: true,
            pointer_anchor: None,
            active_object: None,
            attached_inline_object_surface: None,
            next_inline_object_surface_attachment: 1,
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
        let initial = this.prepare_interaction_target_transition(
            this.desired,
            None,
            transition::ActiveObjectTransition::Preserve,
            true,
        )?;
        let progress = this.commit_widget_transition(initial, None);
        if !matches!(
            progress,
            crate::ExactGeometryProgress::Scanning | crate::ExactGeometryProgress::TargetComplete
        ) {
            return Err(RangeTextInputError::Stale);
        }
        this.observe_realization_ownership();
        let focus = this.focus_handle.clone();
        this.focus_subscription = Some(cx.on_focus_out(&focus, window, |input, _, _, cx| {
            let intent = input.focus_loss_intent();
            let _ = input.request_target_intent(intent, cx);
            cx.emit(RangeTextInputEvent::FocusLost);
            cx.notify();
        }));
        Ok(this)
    }

    pub fn surface(&self) -> Option<&CoherentRangeSurface> {
        self.surface.as_ref()
    }

    pub(super) fn current_surface_position_for_source_position(
        &self,
        position: crate::SourcePosition,
    ) -> Option<gpui::Point<Pixels>> {
        self.surface
            .as_ref()
            .filter(|surface| {
                surface.binding() == self.config.binding
                    && surface.geometry_key() == self.geometry.key()
            })
            .and_then(|surface| surface.position_for_source_position(position))
    }

    pub(super) fn interactive_surface(&self) -> Option<&CoherentRangeSurface> {
        self.surface.as_ref().filter(|surface| {
            let surface_geometry = surface.geometry_key();
            surface.binding() == self.config.binding
                && surface_geometry.binding() == self.config.binding.binding()
                && surface_geometry.revision() == self.config.binding.revision()
                && surface_geometry.presentation_generation() == self.config.presentation_generation
                && surface_geometry.epoch() == self.geometry.key().epoch()
                && self.mounted
                && self.pending_history.is_none()
                && self.pending_target_intent.is_none()
                && self.pending_layout_intent.is_none()
                && self.pending_presentation_intent.is_none()
                && self.pending_rebind_intent.is_none()
                && self
                    .surface_candidate
                    .is_none_or(|candidate| candidate.kind == SurfaceCandidateKind::IndexRefinement)
        })
    }

    pub(super) fn scroll_reference_surface(&self) -> Option<&CoherentRangeSurface> {
        self.surface.as_ref().filter(|surface| {
            self.mounted
                && surface.binding() == self.config.binding
                && self.pending_history.is_none()
                && self.pending_rebind_intent.is_none()
                && self.restoration.is_none()
                && self.restoration_seed.is_none()
        })
    }

    pub fn is_surface_current_and_interactive(&self) -> bool {
        self.interactive_surface().is_some()
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
                self.dispatched_clipboard = Some(DispatchedClipboard::Write(write.key()));
            }
            RangeTextInputRequest::ClipboardProvenancePage(page) => {
                self.dispatched_clipboard = Some(DispatchedClipboard::Provenance(page.key()));
            }
            _ => {}
        }
        self.observe_realization_ownership();
        Some(request)
    }

    pub fn is_quiescent(&self) -> bool {
        self.active_geometry.is_none()
            && self.pending_geometry_page.is_none()
            && self.pending_geometry_object.is_none()
            && self.deferred_geometry_response.is_none()
            && self.response_custody.is_empty()
            && self.pending_target_intent.is_none()
            && !self.pending_index_intent
            && self.pending_layout_intent.is_none()
            && self.pending_presentation_intent.is_none()
            && self.pending_rebind_intent.is_none()
            && !self.realization_continuation_scheduled
            && self.residency.counts().pending_requests == 0
            && self.object_residency.counts().pending_requests == 0
            && self.platform_ready.is_none()
            && self.restoration.is_none()
            && self.restoration_seed.is_none()
            && self.surface_candidate.is_none()
            && self.is_semantically_quiescent()
            && self.requests.is_empty()
            && self.attached_inline_object_surface.is_none()
    }

    pub fn is_semantically_quiescent(&self) -> bool {
        self.replacement.is_none()
            && self.segmentation.is_none()
            && self.segmentation_action.is_none()
            && self.platform.is_none()
            && self.pending_local_mutation.is_none()
            && self.prepared_local_operation.is_none()
            && matches!(
                self.edits.state(),
                crate::MutationState::Idle | crate::MutationState::Settled
            )
            && self.dispatched_mutations.len() == 0
            && self.pending_history.is_none()
            && matches!(self.clipboard.state(), crate::ClipboardState::Idle)
            && self.dispatched_clipboard.is_none()
            && self.requests.iter().all(|request| {
                matches!(
                    request,
                    RangeTextInputRequest::Page(_)
                        | RangeTextInputRequest::CancelPage(_)
                        | RangeTextInputRequest::ReleasePage(_)
                        | RangeTextInputRequest::ObjectPage(_)
                        | RangeTextInputRequest::CancelObjectPage(_)
                        | RangeTextInputRequest::ReleaseObjectPage(_)
                        | RangeTextInputRequest::CancelClipboardProvenancePage(_)
                        | RangeTextInputRequest::CancelClipboardWrite(_)
                )
            })
    }

    pub fn focus(&self, window: &mut Window) {
        if self.mounted && self.enabled {
            window.focus(&self.focus_handle);
        }
    }

    pub const fn is_enabled(&self) -> bool {
        self.enabled
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
        let progress = self.request_layout_intent(layout, style, cx)?;
        debug_assert!(
            progress.is_none_or(|progress| { progress == crate::ExactGeometryProgress::Scanning })
        );
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
            self.pending_presentation_intent = None;
            return Ok(());
        }
        let progress = self.request_presentation_intent(presentation_generation, cx)?;
        debug_assert!(
            progress.is_none_or(|progress| { progress == crate::ExactGeometryProgress::Scanning })
        );
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

    fn next_local_operation(&mut self) -> Result<crate::OperationId, RangeTextInputError> {
        if self.prepared_local_operation.is_some() {
            return Err(RangeTextInputError::Busy);
        }
        let operation = self.config.settlement_coordinator.allocate_operation()?;
        self.prepared_local_operation = Some(operation);
        Ok(operation)
    }

    fn push_request(
        &mut self,
        request: RangeTextInputRequest,
        cx: &mut Context<Self>,
    ) -> Result<(), RangeTextInputError> {
        if self.requests.len() == self.requests.capacity() {
            return Err(RangeTextInputError::SurfaceCapacity);
        }
        let payload = transition::queued_request_payload_charge(
            std::iter::once(&request),
            self.clipboard.current_provenance_page(),
        )?;
        let current = self.current_realization_ownership();
        let peak = RangeSurfaceCharge {
            bytes: current
                .owned_bytes
                .checked_add(payload.bytes)
                .ok_or(RangeTextInputError::SurfaceCapacity)?,
            items: current
                .owned_items
                .checked_add(payload.items)
                .ok_or(RangeTextInputError::SurfaceCapacity)?,
        };
        if peak.bytes > self.config.limits.max_surface_bytes
            || peak.items > self.config.limits.max_surface_items
        {
            return Err(RangeTextInputError::SurfaceCapacity);
        }
        self.observe_surface_admission_peak(peak);
        self.requests.push_back(request);
        self.observe_realization_ownership();
        cx.notify();
        Ok(())
    }

    pub(super) fn commit_prepared_request(&mut self, request: RangeTextInputRequest) {
        assert!(
            self.requests.len() < self.requests.capacity(),
            "prepared request exceeds the admitted fixed queue"
        );
        self.requests.push_back(request);
    }

    fn mutation_queue_has_capacity(&self, key: crate::MutationKey) -> bool {
        self.pending_history.is_none()
            && self.queued_mutation_requests(key) < Self::MAX_QUEUED_MUTATION_REQUESTS
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

pub(super) fn bounded_realization_extent(
    viewport_extent: Pixels,
    max_realized_block_extent: Pixels,
) -> Pixels {
    viewport_extent.min(max_realized_block_extent)
}

fn checked_request_capacity(config: &RangeTextInputConfig) -> Option<usize> {
    config
        .residency_limits
        .max_pending_requests()
        .checked_add(config.residency_limits.max_resident_pages())
        .and_then(|count| count.checked_mul(2))
        .and_then(|count| {
            config
                .object_residency_limits
                .max_pending_requests()
                .checked_add(config.object_residency_limits.max_resident_pages())
                .and_then(|objects| objects.checked_mul(2))
                .and_then(|objects| count.checked_add(objects))
        })
        .and_then(|count| count.checked_add(RangeTextInput::MAX_QUEUED_MUTATION_REQUESTS * 2))
        .and_then(|count| count.checked_add(16))
}
