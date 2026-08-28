mod diagnostics;
mod rebind;

pub(super) use rebind::PendingRebindIntent;

use super::transition::ActiveObjectTransition;
use super::*;
use crate::ExactGeometryProgress;

pub(super) struct DispatchedKeys<K> {
    keys: Vec<K>,
}

impl<K: Copy + Eq> DispatchedKeys<K> {
    pub(super) fn with_capacity(capacity: usize) -> Self {
        Self {
            keys: Vec::with_capacity(capacity),
        }
    }

    pub(super) fn insert(&mut self, key: K) -> bool {
        if self.keys.contains(&key) {
            return false;
        }
        debug_assert!(self.keys.len() < self.keys.capacity());
        self.keys.push(key);
        true
    }

    pub(super) fn contains(&self, key: &K) -> bool {
        self.keys.contains(key)
    }

    pub(super) fn remove(&mut self, key: &K) -> bool {
        let Some(index) = self.keys.iter().position(|existing| existing == key) else {
            return false;
        };
        self.keys.swap_remove(index);
        true
    }

    pub(super) fn len(&self) -> usize {
        self.keys.len()
    }

    pub(super) fn allocation_charge(&self) -> RangeSurfaceCharge {
        RangeSurfaceCharge {
            bytes: self.keys.capacity() * std::mem::size_of::<K>(),
            items: self.keys.capacity(),
        }
    }

    pub(super) fn checked_allocation_charge(capacity: usize) -> Option<RangeSurfaceCharge> {
        Some(RangeSurfaceCharge {
            bytes: capacity.checked_mul(std::mem::size_of::<K>())?,
            items: capacity,
        })
    }

    pub(super) fn release_backing(&mut self) {
        self.keys = Vec::new();
    }
}

impl<'a, K> IntoIterator for &'a DispatchedKeys<K> {
    type Item = &'a K;
    type IntoIter = std::slice::Iter<'a, K>;

    fn into_iter(self) -> Self::IntoIter {
        self.keys.iter()
    }
}

#[derive(Clone, Copy)]
pub(super) struct PendingTargetIntent {
    pub(super) desired: DesiredSurface,
    pub(super) restoration: Option<crate::RangeRestorationSeed>,
    pub(super) interaction: ActiveObjectTransition,
    pub(super) pointer_anchor: Option<Option<crate::SourcePosition>>,
    pub(super) allow_incomplete_index: bool,
}

pub(super) struct PendingLayoutIntent {
    pub(super) layout: gpui::StreamingLayoutBinding,
    pub(super) style: crate::StreamingGeometryStyle,
}

impl PendingLayoutIntent {
    pub(super) fn charge(&self) -> RangeSurfaceCharge {
        let (bytes, items) =
            ExactGeometryOwner::pending_layout_style_charge(&self.layout, &self.style);
        RangeSurfaceCharge { bytes, items }
    }
}

impl PendingTargetIntent {
    pub(super) const fn ordinary(desired: DesiredSurface) -> Self {
        Self {
            desired,
            restoration: None,
            interaction: ActiveObjectTransition::Preserve,
            pointer_anchor: None,
            allow_incomplete_index: true,
        }
    }

    pub(super) const fn absolute(desired: DesiredSurface) -> Self {
        Self {
            desired,
            restoration: None,
            interaction: ActiveObjectTransition::Preserve,
            pointer_anchor: None,
            allow_incomplete_index: false,
        }
    }
}

impl RangeTextInput {
    pub(super) fn target_intent_desired(&self) -> DesiredSurface {
        self.pending_target_intent
            .map_or(self.desired, |intent| intent.desired)
    }

    pub(super) fn begin_realization_frame(&mut self) {
        self.realization_frame_generation = self.realization_frame_generation.wrapping_add(1);
        self.realization_continuation_scheduled = false;
        self.last_realization_step = RangeRealizationStep {
            spent: 0,
            remaining: self.config.limits.max_realization_work_per_frame,
            progressed: false,
            reached_external_boundary: false,
        };
    }

    pub(super) const fn realization_owner_charge() -> RangeSurfaceCharge {
        RangeSurfaceCharge {
            bytes: std::mem::size_of::<RangeTextInput>()
                - std::mem::size_of::<RangeResidency>()
                - std::mem::size_of::<ObjectResidency>()
                - std::mem::size_of::<ExactGeometryOwner>(),
            items: 1,
        }
    }

    pub(super) fn page_alias_storage_charge(
        aliases: &Vec<super::page_delivery::PendingPageAlias>,
    ) -> Result<RangeSurfaceCharge, RangeTextInputError> {
        Ok(RangeSurfaceCharge {
            bytes: aliases
                .capacity()
                .checked_mul(std::mem::size_of::<super::page_delivery::PendingPageAlias>())
                .ok_or(RangeTextInputError::SurfaceCapacity)?,
            items: aliases.capacity(),
        })
    }

    pub(super) fn current_auxiliary_realization_charge(
        &self,
    ) -> Result<RangeSurfaceCharge, RangeTextInputError> {
        let residency = self.residency.counts();
        let objects = self.object_residency.counts();
        let pending_page_bytes = usize::try_from(residency.pending_bytes)
            .map_err(|_| RangeTextInputError::SurfaceCapacity)?;
        let deferred = self
            .deferred_geometry_response
            .as_ref()
            .map_or(RangeSurfaceCharge::default(), |response| {
                response.incremental_charge()
            });
        let pending_layout = self
            .pending_layout_intent
            .as_ref()
            .map_or(RangeSurfaceCharge::default(), PendingLayoutIntent::charge);
        let pending_rebind = self
            .pending_rebind_intent
            .as_ref()
            .map_or(RangeSurfaceCharge::default(), PendingRebindIntent::charge);
        let aliases = Self::page_alias_storage_charge(&self.pending_page_aliases)?;
        let response_custody = self.response_custody_storage_charge();
        let residency_owners = [
            self.residency.owner_storage_charge(),
            self.object_residency.owner_storage_charge(),
        ]
        .into_iter()
        .try_fold(RangeSurfaceCharge::default(), |total, charge| {
            Some(RangeSurfaceCharge {
                bytes: total.bytes.checked_add(charge.bytes)?,
                items: total.items.checked_add(charge.items)?,
            })
        })
        .ok_or(RangeTextInputError::SurfaceCapacity)?;
        let dispatched = [
            self.dispatched_pages.allocation_charge(),
            self.dispatched_object_pages.allocation_charge(),
            self.dispatched_mutations.allocation_charge(),
        ]
        .into_iter()
        .try_fold(RangeSurfaceCharge::default(), |total, charge| {
            Some(RangeSurfaceCharge {
                bytes: total.bytes.checked_add(charge.bytes)?,
                items: total.items.checked_add(charge.items)?,
            })
        })
        .ok_or(RangeTextInputError::SurfaceCapacity)?;
        Ok(RangeSurfaceCharge {
            bytes: pending_page_bytes
                .checked_add(objects.pending_bytes)
                .and_then(|total| total.checked_add(deferred.bytes))
                .and_then(|total| total.checked_add(pending_layout.bytes))
                .and_then(|total| total.checked_add(pending_rebind.bytes))
                .and_then(|total| total.checked_add(aliases.bytes))
                .and_then(|total| total.checked_add(response_custody.bytes))
                .and_then(|total| total.checked_add(self.active_response_processing.bytes))
                .and_then(|total| total.checked_add(dispatched.bytes))
                .and_then(|total| total.checked_add(residency_owners.bytes))
                .ok_or(RangeTextInputError::SurfaceCapacity)?,
            items: residency
                .pending_requests
                .checked_add(objects.pending_requests)
                .and_then(|total| total.checked_add(deferred.items))
                .and_then(|total| total.checked_add(pending_layout.items))
                .and_then(|total| total.checked_add(pending_rebind.items))
                .and_then(|total| total.checked_add(aliases.items))
                .and_then(|total| total.checked_add(response_custody.items))
                .and_then(|total| total.checked_add(self.active_response_processing.items))
                .and_then(|total| total.checked_add(dispatched.items))
                .and_then(|total| total.checked_add(residency_owners.items))
                .ok_or(RangeTextInputError::SurfaceCapacity)?,
        })
    }

    pub(super) fn non_surface_resident_charge(
        &self,
    ) -> Result<RangeSurfaceCharge, RangeTextInputError> {
        let pages = self.residency.resident_page_iter();
        let objects = self.object_residency.resident_page_iter();
        Self::resident_publication_payload_charge(pages, objects)
    }

    pub(super) fn spend_realization_credit(&mut self) {
        debug_assert!(self.last_realization_step.remaining > 0);
        self.last_realization_step.spent = self.last_realization_step.spent.saturating_add(1);
        self.last_realization_step.remaining =
            self.last_realization_step.remaining.saturating_sub(1);
        self.last_realization_step.progressed = true;
    }

    pub(super) fn try_spend_realization_credit(&mut self, cx: &mut Context<Self>) -> bool {
        if self.last_realization_step.remaining == 0 {
            self.schedule_realization_continuation(cx);
            return false;
        }
        self.spend_realization_credit();
        true
    }

    pub(super) fn refund_realization_credit(&mut self) {
        debug_assert!(self.last_realization_step.spent > 0);
        self.last_realization_step.spent -= 1;
        self.last_realization_step.remaining += 1;
        self.last_realization_step.progressed = self.last_realization_step.spent > 0;
    }

    pub(super) fn schedule_realization_continuation(&mut self, cx: &mut Context<Self>) {
        if !self.realization_continuation_scheduled {
            self.realization_continuation_scheduled = true;
            self.observe_realization_ownership();
            cx.notify();
        }
    }

    pub(super) fn defer_realization_continuation(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.realization_continuation_scheduled {
            self.realization_continuation_scheduled = true;
            self.observe_realization_ownership();
            let frame_generation = self.realization_frame_generation;
            cx.defer_in(window, move |input, window, cx| {
                if !input.mounted
                    || !input.realization_continuation_scheduled
                    || input.realization_frame_generation != frame_generation
                {
                    return;
                }
                input.realization_continuation_scheduled = false;
                let _ = input.service_geometry_until_external_boundary(window, cx);
            });
        }
    }

    pub(super) fn obsolete_realization_continuation(&mut self) {
        self.realization_frame_generation = self.realization_frame_generation.wrapping_add(1);
        self.realization_continuation_scheduled = false;
    }

    pub(super) fn request_target_intent(
        &mut self,
        intent: PendingTargetIntent,
        cx: &mut Context<Self>,
    ) -> Result<Option<ExactGeometryProgress>, RangeTextInputError> {
        self.pending_target_intent = Some(intent);
        self.service_pending_target_intent(cx)
    }

    pub(super) fn service_pending_target_intent(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Result<Option<ExactGeometryProgress>, RangeTextInputError> {
        let Some(intent) = self.pending_target_intent else {
            return Ok(None);
        };
        if !self.try_spend_realization_credit(cx) {
            return Ok(None);
        }
        let candidate = self.prepare_interaction_target_transition(
            intent.desired,
            intent.restoration,
            intent.interaction,
            intent.allow_incomplete_index,
        );
        let mut candidate = match candidate {
            Ok(candidate) => candidate,
            Err(error) => {
                self.refund_realization_credit();
                return Err(error);
            }
        };
        if let Some(pointer_anchor) = intent.pointer_anchor {
            candidate.pointer_anchor = Some(pointer_anchor);
        }
        self.pending_target_intent = None;
        Ok(Some(self.commit_widget_transition(candidate, Some(cx))))
    }

    pub(super) fn request_layout_intent(
        &mut self,
        layout: gpui::StreamingLayoutBinding,
        style: crate::StreamingGeometryStyle,
        cx: &mut Context<Self>,
    ) -> Result<Option<ExactGeometryProgress>, RangeTextInputError> {
        let pending = PendingLayoutIntent { layout, style };
        let current = self.current_realization_ownership();
        let charge = pending.charge();
        let replaced = self
            .pending_layout_intent
            .as_ref()
            .map_or(RangeSurfaceCharge::default(), PendingLayoutIntent::charge);
        let retained_bytes = current
            .owned_bytes
            .checked_sub(replaced.bytes)
            .ok_or(RangeTextInputError::SurfaceCapacity)?;
        let retained_items = current
            .owned_items
            .checked_sub(replaced.items)
            .ok_or(RangeTextInputError::SurfaceCapacity)?;
        let peak = RangeSurfaceCharge {
            bytes: current
                .owned_bytes
                .checked_add(charge.bytes)
                .zip(
                    retained_bytes
                        .checked_add(charge.bytes)
                        .and_then(|bytes| bytes.checked_add(charge.bytes)),
                )
                .map(|(replacement, service)| replacement.max(service))
                .ok_or(RangeTextInputError::SurfaceCapacity)?,
            items: current
                .owned_items
                .checked_add(charge.items)
                .zip(
                    retained_items
                        .checked_add(charge.items)
                        .and_then(|items| items.checked_add(charge.items)),
                )
                .map(|(replacement, service)| replacement.max(service))
                .ok_or(RangeTextInputError::SurfaceCapacity)?,
        };
        if peak.bytes > self.config.limits.max_surface_bytes
            || peak.items > self.config.limits.max_surface_items
        {
            return Err(RangeTextInputError::SurfaceCapacity);
        }
        self.observe_surface_admission_peak(peak);
        self.pending_layout_intent = Some(pending);
        self.observe_realization_ownership();
        self.service_pending_configuration_intent(cx)
    }

    pub(super) fn request_presentation_intent(
        &mut self,
        generation: crate::PresentationGeneration,
        cx: &mut Context<Self>,
    ) -> Result<Option<ExactGeometryProgress>, RangeTextInputError> {
        self.pending_presentation_intent = Some(generation);
        self.observe_realization_ownership();
        self.service_pending_configuration_intent(cx)
    }

    pub(super) fn service_pending_configuration_intent(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Result<Option<ExactGeometryProgress>, RangeTextInputError> {
        if self.pending_layout_intent.is_none() && self.pending_presentation_intent.is_none() {
            return Ok(None);
        }
        if !self.try_spend_realization_credit(cx) {
            return Ok(None);
        }
        let target = self.pending_target_intent;
        let layout_pending = self.pending_layout_intent.is_some();
        let candidate = if let Some(pending) = self.pending_layout_intent.as_ref() {
            self.prepare_layout_transition(pending.layout.clone(), pending.style.clone(), target)
        } else {
            self.prepare_presentation_transition(
                self.pending_presentation_intent
                    .expect("pending presentation intent exists"),
                target,
            )
        };
        let candidate = match candidate {
            Ok(candidate) => candidate,
            Err(error) => {
                self.refund_realization_credit();
                return Err(error);
            }
        };
        let progress = self.commit_widget_transition(candidate, Some(cx));
        self.pending_target_intent = None;
        if layout_pending {
            self.pending_layout_intent = None;
        } else {
            self.pending_presentation_intent = None;
        }
        Ok(Some(progress))
    }

    pub(super) fn focus_loss_intent(&self) -> PendingTargetIntent {
        let mut desired = self.target_intent_desired();
        desired.composition = None;
        let interaction = if self
            .attached_inline_object_surface
            .is_some_and(|(_, anchor)| {
                self.active_object.map(|active| active.anchor) == Some(anchor)
            }) {
            ActiveObjectTransition::Preserve
        } else {
            ActiveObjectTransition::Clear(crate::InlineObjectRealizationLossReason::FocusLost)
        };
        PendingTargetIntent {
            desired,
            restoration: None,
            interaction,
            pointer_anchor: Some(None),
            allow_incomplete_index: true,
        }
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
        let mut desired = self.target_intent_desired();
        let block_offset = self
            .scroll_reference_surface()
            .map(|surface| {
                let max_scroll =
                    (surface.content_height() - desired.viewport_extent).max(Pixels::ZERO);
                block_offset.min(max_scroll)
            })
            .unwrap_or(block_offset);
        desired.target_block = block_offset;
        desired.realization_anchor_block = block_offset;
        desired.capacity_saturated = false;
        desired.preserve_scroll_anchor = false;
        desired.reveal_caret = false;
        if self.restoration.is_some() || self.restoration_seed.is_some() {
            return Err(RangeTextInputError::Busy);
        }
        let _ = self.request_target_intent(PendingTargetIntent::absolute(desired), cx)?;
        Ok(())
    }

    pub fn request_filler_reanchor(
        &mut self,
        viewport_block: Pixels,
        cx: &mut Context<Self>,
    ) -> Result<(), RangeTextInputError> {
        if !f32::from(viewport_block).is_finite() || viewport_block < Pixels::ZERO {
            return Err(RangeTextInputError::InvalidLimits);
        }
        let surface = self
            .interactive_surface()
            .ok_or(RangeTextInputError::IncompleteSurface)?;
        let logical_block = surface.scroll_block() + viewport_block;
        let filler = surface
            .filler_at(logical_block)
            .ok_or(RangeTextInputError::IncompleteSurface)?;
        let mut desired = self.target_intent_desired();
        desired.realization_anchor_block = filler.successor_block();
        desired.capacity_saturated = false;
        desired.preserve_scroll_anchor = false;
        desired.reveal_caret = false;
        let _ = self.request_target_intent(PendingTargetIntent::ordinary(desired), cx)?;
        Ok(())
    }

    pub(super) fn set_realization_viewport_extent(
        &mut self,
        extent: Pixels,
        cx: &mut Context<Self>,
    ) -> Result<(), RangeTextInputError> {
        if extent <= Pixels::ZERO || !f32::from(extent).is_finite() {
            return Err(RangeTextInputError::InvalidLimits);
        }
        let pending_extent = self
            .pending_target_intent
            .map(|intent| intent.desired.viewport_extent);
        if pending_extent == Some(extent)
            || (pending_extent.is_none() && self.desired.viewport_extent == extent)
        {
            return Ok(());
        }
        let mut intent = self
            .pending_target_intent
            .unwrap_or_else(|| PendingTargetIntent::ordinary(self.desired));
        intent.desired.viewport_extent = extent;
        intent.desired.realization_extent =
            bounded_realization_extent(extent, self.config.limits.max_realized_block_extent);
        intent.desired.capacity_saturated = false;
        let _ = self.request_target_intent(intent, cx)?;
        Ok(())
    }
}
