//! Mounted range-backed multiline widget lifecycle.

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
mod types;

pub use surface::{CoherentRangeSurface, RangeSurfaceCharge};
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
    ByteOffset, ByteRange, ExactGeometryOwner, RangeClipboardCoordinator, RangeEditCoordinator,
    RangeResidency, SegmentationContinuation,
};

/// GPUI entity implementing the app-neutral range-backed multiline editor.
pub struct RangeTextInput {
    focus_handle: FocusHandle,
    enabled: bool,
    read_only: bool,
    config: RangeTextInputConfig,
    residency: RangeResidency,
    geometry: ExactGeometryOwner,
    edits: RangeEditCoordinator,
    clipboard: RangeClipboardCoordinator,
    pending_clipboard_page: Option<clipboard::PendingClipboardPage>,
    surface: Option<CoherentRangeSurface>,
    last_surface_admission: Option<RangeSurfaceCharge>,
    desired: DesiredSurface,
    requests: VecDeque<RangeTextInputRequest>,
    dispatched_pages: HashSet<crate::PageRequestKey>,
    dispatched_mutations: HashSet<crate::MutationKey>,
    active_geometry: Option<crate::GeometryJobKey>,
    pending_geometry_page: Option<geometry::PendingGeometryPage>,
    pending_page_aliases: Vec<page_delivery::PendingPageAlias>,
    surface_candidate: Option<SurfaceCandidate>,
    segmentation: Option<SegmentationContinuation>,
    segmentation_action: Option<interaction::PendingBoundaryAction>,
    platform: Option<platform::PlatformReplay>,
    restoration: Option<restoration::RestorationValidation>,
    replacement: Option<replacement::ReplacementScan>,
    pending_history: Option<history::PendingHistory>,
    detached_edits: Vec<RangeEditCoordinator>,
    mutation_selection: Option<(crate::MutationKey, RangeSelection)>,
    mutation_composition: Option<(crate::MutationKey, ByteRange, RangeSelection)>,
    pending_insert: Option<(crate::MutationKey, String, ByteOffset)>,
    platform_ready: Option<(std::ops::Range<usize>, String)>,
    next_id: u64,
    mounted: bool,
    pointer_anchor: Option<crate::ByteOffset>,
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
    /// Creates a range-backed widget and starts its exact background index.
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
            || config.clipboard_limits.max_page_bytes()
                > config.residency_limits.max_pending_bytes()
            || config.segmentation_limits.max_page_bytes()
                > config.residency_limits.max_pending_bytes()
            || config.limits.page_bytes > config.residency_limits.max_pending_bytes()
            || config.limits.platform_bytes > config.residency_limits.max_pending_bytes()
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
            geometry: ExactGeometryOwner::new(
                binding,
                config.layout.clone(),
                config.style.clone(),
                config.geometry_limits,
            )?,
            edits: RangeEditCoordinator::new(binding, config.mutation_limits),
            clipboard: RangeClipboardCoordinator::new(binding, config.clipboard_limits),
            pending_clipboard_page: None,
            surface: None,
            last_surface_admission: None,
            desired: DesiredSurface::origin(config.viewport_extent, config.overscan),
            requests: VecDeque::new(),
            dispatched_pages: HashSet::new(),
            dispatched_mutations: HashSet::new(),
            active_geometry: None,
            pending_geometry_page: None,
            pending_page_aliases: Vec::new(),
            surface_candidate: None,
            segmentation: None,
            segmentation_action: None,
            platform: None,
            restoration: None,
            replacement: None,
            pending_history: None,
            detached_edits: Vec::new(),
            mutation_selection: None,
            mutation_composition: None,
            pending_insert: None,
            platform_ready: None,
            next_id: 1,
            mounted: true,
            pointer_anchor: None,
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
            input.pointer_anchor = None;
            if input.desired.composition.take().is_some() && input.geometry.index().is_some() {
                let _ = input.start_target();
            }
            cx.emit(RangeTextInputEvent::FocusLost);
            cx.notify();
        }));
        Ok(this)
    }

    /// Returns the currently painted coherent surface, if realization has completed.
    pub fn surface(&self) -> Option<&CoherentRangeSurface> {
        self.surface.as_ref()
    }

    /// Returns the publication that belongs to the exact mounted binding and layout epoch.
    pub(super) fn interactive_surface(&self) -> Option<&CoherentRangeSurface> {
        self.surface.as_ref().filter(|surface| {
            surface.binding() == self.config.binding
                && surface.geometry_key() == self.geometry.key()
                && self.mounted
        })
    }

    /// Returns presentation-only lower bounds while the current exact index is scanning.
    pub fn geometry_estimate(&self) -> Option<crate::StreamingGeometryEstimate> {
        self.geometry.estimate()
    }

    /// Returns the exact byte and semantic-item peak of the last published surface admission.
    pub const fn last_surface_admission_charge(&self) -> Option<RangeSurfaceCharge> {
        self.last_surface_admission
    }

    /// Removes and returns the next exact request for host dispatch.
    pub fn take_request(&mut self) -> Option<RangeTextInputRequest> {
        let request = self.requests.pop_front()?;
        match &request {
            RangeTextInputRequest::Page(page) => {
                self.dispatched_pages.insert(page.key());
            }
            RangeTextInputRequest::MutationPreflight(proposal) => {
                self.dispatched_mutations.insert(proposal.key());
            }
            RangeTextInputRequest::MutationFragment { key, .. }
            | RangeTextInputRequest::MutationCommit(key) => {
                self.dispatched_mutations.insert(*key);
            }
            _ => {}
        }
        Some(request)
    }

    /// Returns whether the widget has no unpublished or externally pending work.
    pub fn is_quiescent(&self) -> bool {
        self.active_geometry.is_none()
            && self.pending_geometry_page.is_none()
            && self.residency.counts().pending_requests == 0
            && self.segmentation.is_none()
            && self.platform.is_none()
            && self.platform_ready.is_none()
            && self.restoration.is_none()
            && self.surface_candidate.is_none()
            && self.replacement.is_none()
            && self.pending_history.is_none()
            && matches!(self.clipboard.state(), crate::ClipboardState::Idle)
            && matches!(self.edits.state(), crate::MutationState::Idle)
            && self.detached_edits.is_empty()
            && self.requests.is_empty()
    }

    /// Focuses the mounted widget.
    pub fn focus(&self, window: &mut Window) {
        if self.mounted && self.enabled {
            window.focus(&self.focus_handle);
        }
    }

    /// Enables or disables mounted interaction without discarding the coherent surface.
    pub fn set_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if self.enabled != enabled {
            self.enabled = enabled;
            if !enabled {
                self.pointer_anchor = None;
            }
            cx.notify();
        }
    }

    /// Selects editable or read-only behavior.
    pub fn set_read_only(&mut self, read_only: bool, cx: &mut Context<Self>) {
        if self.read_only != read_only {
            self.read_only = read_only;
            cx.notify();
        }
    }

    /// Replaces every shaping input under a fresh layout epoch while retaining the prior surface.
    pub fn set_layout(
        &mut self,
        layout: gpui::StreamingLayoutBinding,
        style: crate::StreamingGeometryStyle,
        cx: &mut Context<Self>,
    ) -> Result<(), RangeTextInputError> {
        if !self.mounted {
            return Err(RangeTextInputError::NotMounted);
        }
        let release = self.geometry.set_layout(layout.clone(), style.clone())?;
        self.release_geometry(&release, None, Some(cx));
        self.config.layout = layout;
        self.config.style = style;
        self.active_geometry = None;
        self.desired.preserve_scroll_anchor = true;
        self.start_index()?;
        cx.notify();
        Ok(())
    }

    /// Records an absolute block target; source positioning waits for the complete exact index.
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
        self.desired.target_block = block_offset;
        self.desired.preserve_scroll_anchor = false;
        self.desired.reveal_caret = false;
        self.start_target()?;
        cx.notify();
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
}
