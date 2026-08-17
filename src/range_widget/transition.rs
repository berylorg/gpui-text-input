use std::{collections::VecDeque, mem::size_of};

use gpui::Context;

use crate::object_residency::PreparedObjectRebind;
use crate::range_geometry::PreparedGeometryTransition;
use crate::residency::{PreparedPageDemand, PreparedResidencyRebind};
use crate::{
    ExactGeometryProgress, GeometryJobId, PageDemand, PageRequestId, RangeTextInputError,
    RangeTextInputEvent, RangeTextInputRequest,
};

use super::geometry::{PreparedTargetPublication, TerminalTargetPreparation};
use super::{DesiredSurface, RangeTextInput, SurfaceCandidate};

pub(super) struct WidgetTransitionCandidate {
    expected_next_id: u64,
    committed_next_id: u64,
    geometry: PreparedGeometryTransition,
    page: Option<PreparedPageDemand>,
    desired: Option<DesiredSurface>,
    surface_candidate: Option<SurfaceCandidate>,
    target_publication: Option<PreparedTargetPublication>,
    config_update: Option<TransitionConfigUpdate>,
    residency_rebind: Option<PreparedResidencyRebind>,
    object_rebind: Option<PreparedObjectRebind>,
    clipboard_rebind: Option<crate::ClipboardCancellation>,
    replacement_edits: Option<crate::RangeEditCoordinator>,
    edit_disposal: Option<crate::MutationDisposal>,
    replacement_detached_edits: Option<Vec<crate::RangeEditCoordinator>>,
    scrollbar_replacement: Option<(
        gpui_scrollbar::ScrollbarOwnerKey,
        gpui_scrollbar::ScrollbarOwnerKey,
    )>,
    effects: Vec<RangeTextInputRequest>,
    events: Vec<RangeTextInputEvent>,
    active_object: Option<Option<super::ActiveInlineObject>>,
    pointer_anchor: Option<Option<crate::SourcePosition>>,
    requests: VecDeque<RangeTextInputRequest>,
    admission_charge: crate::RangeSurfaceCharge,
    reject_restoration: bool,
    settling_mutation: Option<crate::MutationKey>,
    adopted_mutation: Option<(
        crate::MutationPositions,
        Vec<crate::range_edit::SourcePositionProof>,
    )>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct WidgetAdmissionComponents {
    pub(super) prior_surface: crate::RangeSurfaceCharge,
    pub(super) current_request_storage: crate::RangeSurfaceCharge,
    pub(super) candidate_record: crate::RangeSurfaceCharge,
    pub(super) geometry: crate::RangeSurfaceCharge,
    pub(super) resident_payload: crate::RangeSurfaceCharge,
    pub(super) publication_allocation: crate::RangeSurfaceCharge,
    pub(super) effect_storage: crate::RangeSurfaceCharge,
    pub(super) event_storage: crate::RangeSurfaceCharge,
    pub(super) page_demand: crate::RangeSurfaceCharge,
    pub(super) object_rebind: crate::RangeSurfaceCharge,
    pub(super) residency_rebind: crate::RangeSurfaceCharge,
    pub(super) detached_edit_storage: crate::RangeSurfaceCharge,
    pub(super) destination_request_storage: crate::RangeSurfaceCharge,
    pub(super) proof_storage: crate::RangeSurfaceCharge,
}

impl WidgetAdmissionComponents {
    pub(super) fn checked_total(self) -> Option<crate::RangeSurfaceCharge> {
        [
            self.prior_surface,
            self.current_request_storage,
            self.candidate_record,
            self.geometry,
            self.resident_payload,
            self.publication_allocation,
            self.effect_storage,
            self.event_storage,
            self.page_demand,
            self.object_rebind,
            self.residency_rebind,
            self.detached_edit_storage,
            self.destination_request_storage,
            self.proof_storage,
        ]
        .into_iter()
        .try_fold(crate::RangeSurfaceCharge::default(), |total, charge| {
            Some(crate::RangeSurfaceCharge {
                bytes: total.bytes.checked_add(charge.bytes)?,
                items: total.items.checked_add(charge.items)?,
            })
        })
    }
}

pub(super) struct PreparedIndexResponseTarget {
    pub(super) job: crate::GeometryJobKey,
    pub(super) desired: DesiredSurface,
    pub(super) surface_candidate: SurfaceCandidate,
    pub(super) committed_next_id: u64,
}

pub(super) struct ActiveObjectTransitionCandidate {
    active_object: Option<super::ActiveInlineObject>,
    enabled: bool,
    pointer_anchor: Option<crate::SourcePosition>,
    events: Vec<RangeTextInputEvent>,
    admission_charge: crate::RangeSurfaceCharge,
}

impl ActiveObjectTransitionCandidate {
    #[cfg(test)]
    pub(super) const fn admission_charge(&self) -> crate::RangeSurfaceCharge {
        self.admission_charge
    }
}

#[derive(Clone, Copy)]
pub(super) enum ActiveObjectTransition {
    Preserve,
    Set {
        active: super::ActiveInlineObject,
        activation: Option<crate::InlineObjectInputOrigin>,
    },
    Clear(crate::InlineObjectRealizationLossReason),
}

enum TransitionConfigUpdate {
    Layout(gpui::StreamingLayoutBinding, crate::StreamingGeometryStyle),
    Presentation(crate::PresentationGeneration),
    Rebind {
        binding: crate::RangeBinding,
        active_loss_reason: crate::InlineObjectRealizationLossReason,
        settlement: Option<(crate::MutationKey, crate::MutationOutcome)>,
    },
}

pub(super) struct CommittedWidgetTransition {
    progress: ExactGeometryProgress,
    effects: Vec<RangeTextInputRequest>,
    events: Vec<RangeTextInputEvent>,
}

impl RangeTextInput {
    pub(super) fn prepare_index_response_target(
        &self,
        geometry: &crate::range_geometry::PreparedTargetResponse,
    ) -> Result<PreparedIndexResponseTarget, RangeTextInputError> {
        let index = geometry
            .terminal_index()
            .ok_or(RangeTextInputError::Stale)?;
        let restoration = self
            .surface_candidate
            .as_ref()
            .and_then(|candidate| candidate.restoration)
            .or(self.restoration_seed);
        let mut desired = self.desired;
        if self.pending_select_all {
            desired.source_selection = Some(index.document_selection());
            desired.composition = None;
            desired.target_block = index.aggregate().content_height();
            desired.reveal_caret = true;
            desired.inline_object_interaction = self.active_object.map(|_| {
                super::DesiredInlineObjectInteraction::Clear(
                    crate::InlineObjectRealizationLossReason::SelectionChanged,
                )
            });
        }
        let committed_next_id = self
            .next_id
            .checked_add(2)
            .ok_or(RangeTextInputError::Busy)?;
        let job = crate::GeometryJobKey::new(
            geometry.key().geometry(),
            crate::GeometryJobId::new(self.next_id),
        );
        Ok(PreparedIndexResponseTarget {
            job,
            desired,
            surface_candidate: SurfaceCandidate {
                job,
                binding: self.config.binding,
                desired,
                restoration,
            },
            committed_next_id,
        })
    }

    pub(super) fn prepare_active_object_transition(
        &self,
        transition: ActiveObjectTransition,
    ) -> Result<ActiveObjectTransitionCandidate, RangeTextInputError> {
        self.prepare_interaction_state_transition(self.enabled, self.pointer_anchor, transition)
    }

    pub(super) fn prepare_interaction_state_transition(
        &self,
        enabled: bool,
        pointer_anchor: Option<crate::SourcePosition>,
        transition: ActiveObjectTransition,
    ) -> Result<ActiveObjectTransitionCandidate, RangeTextInputError> {
        let (active_object, events) = match transition {
            ActiveObjectTransition::Preserve => {
                if enabled == self.enabled && pointer_anchor == self.pointer_anchor {
                    return Err(RangeTextInputError::Stale);
                }
                (self.active_object, Vec::new())
            }
            ActiveObjectTransition::Clear(reason) => {
                let mut events = Vec::with_capacity(usize::from(self.active_object.is_some()));
                if let Some(active) = self.active_object {
                    events.push(RangeTextInputEvent::InlineObjectRealizationLost(
                        crate::InlineObjectRealizationLoss {
                            anchor: active.anchor,
                            reason,
                        },
                    ));
                }
                (None, events)
            }
            ActiveObjectTransition::Set { active, activation } => {
                let event_capacity = usize::from(
                    self.active_object
                        .is_some_and(|prior| prior.anchor != active.anchor),
                )
                .checked_add(usize::from(
                    activation.is_some() && active.activation_eligible,
                ))
                .ok_or(RangeTextInputError::SurfaceCapacity)?;
                let mut events = Vec::with_capacity(event_capacity);
                if let Some(prior) = self
                    .active_object
                    .filter(|prior| prior.anchor != active.anchor)
                {
                    events.push(RangeTextInputEvent::InlineObjectRealizationLost(
                        crate::InlineObjectRealizationLoss {
                            anchor: prior.anchor,
                            reason: crate::InlineObjectRealizationLossReason::SelectionChanged,
                        },
                    ));
                }
                if let Some(origin) = activation.filter(|_| active.activation_eligible) {
                    events.push(RangeTextInputEvent::InlineObjectActivated(
                        crate::InlineObjectActivation {
                            anchor: active.anchor,
                            origin,
                        },
                    ));
                }
                (Some(active), events)
            }
        };
        let prior = self
            .surface
            .as_ref()
            .map_or(crate::RangeSurfaceCharge::default(), |surface| {
                surface.charge()
            });
        let event_bytes = events
            .capacity()
            .checked_mul(size_of::<RangeTextInputEvent>())
            .ok_or(RangeTextInputError::SurfaceCapacity)?;
        let candidate_bytes = size_of::<ActiveObjectTransitionCandidate>()
            .checked_add(event_bytes)
            .ok_or(RangeTextInputError::SurfaceCapacity)?;
        let candidate_items = 1usize
            .checked_add(events.capacity())
            .ok_or(RangeTextInputError::SurfaceCapacity)?;
        let admission_charge = crate::RangeSurfaceCharge {
            bytes: prior
                .bytes
                .checked_add(
                    self.requests
                        .capacity()
                        .checked_mul(size_of::<RangeTextInputRequest>())
                        .ok_or(RangeTextInputError::SurfaceCapacity)?,
                )
                .and_then(|bytes| bytes.checked_add(candidate_bytes))
                .ok_or(RangeTextInputError::SurfaceCapacity)?,
            items: prior
                .items
                .checked_add(self.requests.capacity())
                .and_then(|items| items.checked_add(candidate_items))
                .ok_or(RangeTextInputError::SurfaceCapacity)?,
        };
        if admission_charge.bytes > self.config.limits.max_surface_bytes
            || admission_charge.items > self.config.limits.max_surface_items
        {
            return Err(RangeTextInputError::SurfaceCapacity);
        }
        Ok(ActiveObjectTransitionCandidate {
            active_object,
            enabled,
            pointer_anchor,
            events,
            admission_charge,
        })
    }

    pub(super) fn commit_active_object_transition(
        &mut self,
        candidate: ActiveObjectTransitionCandidate,
        cx: &mut Context<Self>,
    ) {
        self.active_object = candidate.active_object;
        self.enabled = candidate.enabled;
        self.pointer_anchor = candidate.pointer_anchor;
        self.last_surface_admission = Some(candidate.admission_charge);
        for event in candidate.events {
            cx.emit(event);
        }
        cx.notify();
    }

    pub(super) fn prepare_index_transition(
        &self,
    ) -> Result<WidgetTransitionCandidate, RangeTextInputError> {
        let (job_id, request_id, committed_next_id) = self.transition_ids()?;
        let geometry = self.geometry.prepare_start_index(job_id, request_id)?;
        self.prepare_widget_transition(
            geometry,
            None,
            None,
            None,
            None,
            committed_next_id,
            ActiveObjectTransition::Preserve,
            None,
        )
    }

    pub(super) fn prepare_target_transition(
        &self,
        desired: DesiredSurface,
        restoration: Option<crate::RangeRestorationSeed>,
    ) -> Result<WidgetTransitionCandidate, RangeTextInputError> {
        self.prepare_interaction_target_transition(
            desired,
            restoration,
            ActiveObjectTransition::Preserve,
        )
    }

    pub(super) fn prepare_pointer_target_transition(
        &self,
        desired: DesiredSurface,
        pointer_anchor: Option<crate::SourcePosition>,
    ) -> Result<WidgetTransitionCandidate, RangeTextInputError> {
        let mut candidate = self.prepare_target_transition(desired, None)?;
        candidate.pointer_anchor = Some(pointer_anchor);
        Ok(candidate)
    }

    pub(super) fn prepare_focus_loss_transition(
        &self,
        mut desired: DesiredSurface,
    ) -> Result<WidgetTransitionCandidate, RangeTextInputError> {
        desired.composition = None;
        let mut candidate = self.prepare_interaction_target_transition(
            desired,
            None,
            ActiveObjectTransition::Clear(crate::InlineObjectRealizationLossReason::FocusLost),
        )?;
        candidate.pointer_anchor = Some(None);
        Ok(candidate)
    }

    pub(super) fn prepare_interaction_target_transition(
        &self,
        desired: DesiredSurface,
        restoration: Option<crate::RangeRestorationSeed>,
        interaction: ActiveObjectTransition,
    ) -> Result<WidgetTransitionCandidate, RangeTextInputError> {
        let (job_id, request_id, committed_next_id) = self.transition_ids()?;
        let target_anchor = restoration.map(|seed| seed.scroll.position).or_else(|| {
            desired
                .reveal_caret
                .then_some(desired.source_selection)
                .flatten()
                .map(|selection| selection.head)
                .filter(|position| {
                    let offset = position.byte_offset.get();
                    (offset == 0 || offset == self.config.binding.extent().byte_len())
                        && !matches!(position.gap, crate::InlineObjectGap::NoObjects)
                })
        });
        let geometry = self.geometry.prepare_target_replacement(
            job_id,
            request_id,
            desired.target(),
            target_anchor,
        )?;
        let state = SurfaceCandidate {
            job: geometry.key(),
            binding: self.config.binding,
            desired,
            restoration,
        };
        let (surface_candidate, target_publication) = if geometry.terminal_target().is_some() {
            match self.prepare_terminal_target_publication(&geometry, state)? {
                TerminalTargetPreparation::Retarget(desired) => {
                    return self.prepare_interaction_target_transition(
                        desired,
                        restoration,
                        interaction,
                    );
                }
                TerminalTargetPreparation::Publication(publication) => (None, Some(publication)),
            }
        } else {
            (Some(state), None)
        };
        self.prepare_widget_transition(
            geometry,
            Some(desired),
            surface_candidate,
            target_publication,
            None,
            committed_next_id,
            interaction,
            None,
        )
    }

    pub(super) fn prepare_layout_transition(
        &self,
        layout: gpui::StreamingLayoutBinding,
        style: crate::StreamingGeometryStyle,
    ) -> Result<WidgetTransitionCandidate, RangeTextInputError> {
        let (job_id, request_id, committed_next_id) = self.transition_ids()?;
        let geometry = self.geometry.prepare_layout_and_index(
            layout.clone(),
            style.clone(),
            job_id,
            request_id,
        )?;
        let mut desired = self.desired;
        desired.preserve_scroll_anchor = true;
        self.prepare_widget_transition(
            geometry,
            Some(desired),
            None,
            None,
            Some(TransitionConfigUpdate::Layout(layout, style)),
            committed_next_id,
            ActiveObjectTransition::Preserve,
            None,
        )
    }

    pub(super) fn prepare_presentation_transition(
        &self,
        presentation_generation: crate::PresentationGeneration,
    ) -> Result<WidgetTransitionCandidate, RangeTextInputError> {
        let (job_id, request_id, committed_next_id) = self.transition_ids()?;
        let geometry = self.geometry.prepare_presentation_and_index(
            presentation_generation,
            job_id,
            request_id,
        )?;
        let mut desired = self.desired;
        desired.preserve_scroll_anchor = true;
        self.prepare_widget_transition(
            geometry,
            Some(desired),
            None,
            None,
            Some(TransitionConfigUpdate::Presentation(
                presentation_generation,
            )),
            committed_next_id,
            ActiveObjectTransition::Preserve,
            None,
        )
    }

    pub(super) fn prepare_rebind_transition(
        &self,
        binding: crate::RangeBinding,
        selection: Option<crate::RangeSourceSelection>,
        expected_scrollbar: gpui_scrollbar::ScrollbarOwnerKey,
        replacement_scrollbar: gpui_scrollbar::ScrollbarOwnerKey,
        active_loss_reason: crate::InlineObjectRealizationLossReason,
        settlement: Option<(crate::MutationKey, crate::MutationOutcome)>,
        composition: Option<crate::ByteRange>,
        adopted_mutation: Option<(
            crate::MutationPositions,
            Vec<crate::range_edit::SourcePositionProof>,
        )>,
    ) -> Result<WidgetTransitionCandidate, RangeTextInputError> {
        let (job_id, request_id, committed_next_id) = self.transition_ids()?;
        let geometry = self.geometry.prepare_rebind_and_index(
            binding,
            self.config.presentation_generation,
            job_id,
            request_id,
        )?;
        let mut desired = DesiredSurface::origin(self.config.viewport_extent, self.config.overscan);
        desired.source_selection = selection;
        desired.scroll.source = selection.map_or(crate::ByteOffset::new(0), |selection| {
            selection.head.byte_offset
        });
        desired.reveal_caret = true;
        desired.composition = composition;
        let mut candidate = self.prepare_widget_transition(
            geometry,
            Some(desired),
            None,
            None,
            Some(TransitionConfigUpdate::Rebind {
                binding,
                active_loss_reason,
                settlement,
            }),
            committed_next_id,
            ActiveObjectTransition::Preserve,
            adopted_mutation,
        )?;
        candidate.scrollbar_replacement = Some((expected_scrollbar, replacement_scrollbar));
        Ok(candidate)
    }

    fn prepare_widget_transition(
        &self,
        geometry: PreparedGeometryTransition,
        desired: Option<DesiredSurface>,
        surface_candidate: Option<SurfaceCandidate>,
        target_publication: Option<PreparedTargetPublication>,
        config_update: Option<TransitionConfigUpdate>,
        committed_next_id: u64,
        interaction: ActiveObjectTransition,
        adopted_mutation: Option<(
            crate::MutationPositions,
            Vec<crate::range_edit::SourcePositionProof>,
        )>,
    ) -> Result<WidgetTransitionCandidate, RangeTextInputError> {
        let rebind_binding = match config_update {
            Some(TransitionConfigUpdate::Rebind { binding, .. }) => Some(binding),
            _ => None,
        };
        let settling_mutation = match &config_update {
            Some(TransitionConfigUpdate::Rebind {
                settlement: Some((key, _)),
                ..
            }) => Some(*key),
            _ => None,
        };
        let successor_page = geometry.page_request();
        let residency_rebind = rebind_binding
            .map(|binding| match successor_page {
                Some(request) => self.residency.prepare_rebind_with_demand(binding, request),
                None => Ok(self.residency.prepare_rebind(binding)),
            })
            .transpose()
            .map_err(|_| RangeTextInputError::Busy)?;
        let page = if residency_rebind.is_some() {
            None
        } else {
            successor_page
                .map(|request| {
                    self.residency.prepare_demand_after_retirement(
                        request.key().id(),
                        request.key().purpose(),
                        request.key().demand(),
                        &geometry.release().pages,
                    )
                })
                .transpose()
                .map_err(|_| RangeTextInputError::Busy)?
        };
        let reject_restoration = config_update.is_some()
            && (self.restoration.is_some() || self.restoration_seed.is_some());
        let object_rebind = match &config_update {
            Some(TransitionConfigUpdate::Presentation(generation)) => Some(
                self.object_residency
                    .prepare_rebind(self.config.binding, *generation),
            ),
            Some(TransitionConfigUpdate::Layout(_, _)) if reject_restoration => Some(
                self.object_residency
                    .prepare_rebind(self.config.binding, self.config.presentation_generation),
            ),
            Some(TransitionConfigUpdate::Rebind { binding, .. }) => Some(
                self.object_residency
                    .prepare_rebind(*binding, self.config.presentation_generation),
            ),
            _ => None,
        };
        let clipboard_rebind = rebind_binding.and_then(|_| self.clipboard.preview_rebind());
        let edit_disposal = rebind_binding.and_then(|_| {
            let key = self.edits.active_key()?;
            if settling_mutation == Some(key) {
                return None;
            }
            Some(
                if matches!(
                    self.edits.state(),
                    crate::MutationState::CommitPending | crate::MutationState::DetachedCommit
                ) {
                    crate::MutationDisposal::Detached(key)
                } else {
                    crate::MutationDisposal::Cancelled(key)
                },
            )
        });
        let replacement_edits = rebind_binding
            .map(|binding| crate::RangeEditCoordinator::new(binding, self.config.mutation_limits));
        let replacement_detached_edits =
            if matches!(edit_disposal, Some(crate::MutationDisposal::Detached(_))) {
                Some(Vec::with_capacity(
                    self.detached_edits
                        .len()
                        .checked_add(1)
                        .ok_or(RangeTextInputError::SurfaceCapacity)?,
                ))
            } else {
                None
            };
        let effect_capacity = checked_capacity_sum([
            geometry.release().pages.len(),
            geometry.release().object_pages.len(),
            usize::from(
                page.as_ref()
                    .is_some_and(|page| matches!(page.outcome(), PageDemand::Requested(_))),
            ),
            object_rebind
                .as_ref()
                .map_or(0, |rebind| rebind.cancelled().len()),
            residency_rebind
                .as_ref()
                .map_or(0, |rebind| rebind.cancelled().len()),
            usize::from(
                residency_rebind
                    .as_ref()
                    .and_then(PreparedResidencyRebind::successor)
                    .is_some(),
            ),
            3,
        ])?;
        let mut effects = Vec::with_capacity(effect_capacity);
        for key in &geometry.release().pages {
            if self.dispatched_pages.contains(key) {
                effects.push(RangeTextInputRequest::CancelPage(*key));
            }
        }
        for key in &geometry.release().object_pages {
            if self.dispatched_object_pages.contains(key) {
                effects.push(RangeTextInputRequest::CancelObjectPage(*key));
            }
        }
        if let Some(rebind) = &object_rebind {
            for key in rebind.cancelled() {
                if !geometry.release().object_pages.contains(key)
                    && self.dispatched_object_pages.contains(key)
                {
                    effects.push(RangeTextInputRequest::CancelObjectPage(*key));
                }
            }
        }
        if let Some(rebind) = &residency_rebind {
            for key in rebind.cancelled() {
                if !geometry.release().pages.contains(key) && self.dispatched_pages.contains(key) {
                    effects.push(RangeTextInputRequest::CancelPage(*key));
                }
            }
        }
        if let Some(cancellation) = clipboard_rebind {
            if let Some(key) = cancellation.pending_text_page()
                && !geometry.release().pages.contains(&key)
                && residency_rebind
                    .as_ref()
                    .is_none_or(|rebind| !rebind.cancelled().contains(&key))
                && self.dispatched_pages.contains(&key)
            {
                effects.push(RangeTextInputRequest::CancelPage(key));
            }
            if let Some(key) = cancellation.pending_object_page()
                && !geometry.release().object_pages.contains(&key)
                && object_rebind
                    .as_ref()
                    .is_none_or(|rebind| !rebind.cancelled().contains(&key))
                && self.dispatched_object_pages.contains(&key)
            {
                effects.push(RangeTextInputRequest::CancelObjectPage(key));
            }
            if cancellation.awaiting_write()
                && self.dispatched_clipboard_write == Some(cancellation.key())
            {
                effects.push(RangeTextInputRequest::CancelClipboardWrite(
                    cancellation.key(),
                ));
            }
        }
        if let Some(disposal) = edit_disposal {
            let (key, detached) = match disposal {
                crate::MutationDisposal::Cancelled(key) => (key, false),
                crate::MutationDisposal::Detached(key) => (key, true),
            };
            if self.dispatched_mutations.contains(&key) {
                effects.push(if detached {
                    RangeTextInputRequest::DetachedMutation(key)
                } else {
                    RangeTextInputRequest::CancelMutation(key)
                });
            }
        }
        if rebind_binding.is_some() {
            for key in &self.dispatched_pages {
                if !effects.iter().any(
                    |effect| matches!(effect, RangeTextInputRequest::CancelPage(existing) if existing == key),
                ) {
                    effects.push(RangeTextInputRequest::CancelPage(*key));
                }
            }
            for key in &self.dispatched_object_pages {
                if !effects.iter().any(
                    |effect| matches!(effect, RangeTextInputRequest::CancelObjectPage(existing) if existing == key),
                ) {
                    effects.push(RangeTextInputRequest::CancelObjectPage(*key));
                }
            }
        }
        if let Some(history) = self.pending_history
            && !history.is_planned()
            && !self.requests.iter().any(|request| {
                matches!(request, RangeTextInputRequest::HistoryIntent(intent) if *intent == history.intent())
            })
        {
            effects.push(RangeTextInputRequest::CancelHistoryIntent(history.intent()));
        }
        if let Some(PageDemand::Requested(request)) = page.as_ref().map(|page| page.outcome()) {
            effects.push(RangeTextInputRequest::Page(request));
        }
        if let Some(request) = residency_rebind
            .as_ref()
            .and_then(PreparedResidencyRebind::successor)
        {
            effects.push(RangeTextInputRequest::Page(request));
        }
        let interaction_event_capacity = match interaction {
            ActiveObjectTransition::Preserve => 0,
            ActiveObjectTransition::Clear(_) => usize::from(self.active_object.is_some()),
            ActiveObjectTransition::Set { activation, .. } => {
                usize::from(self.active_object.is_some()) + usize::from(activation.is_some())
            }
        };
        let event_capacity = checked_capacity_sum([
            usize::from(self.active_object.is_some() && config_update.is_some()),
            usize::from(reject_restoration),
            usize::from(settling_mutation.is_some()),
            usize::from(
                matches!(interaction, ActiveObjectTransition::Preserve)
                    && target_publication
                        .as_ref()
                        .and_then(PreparedTargetPublication::active_loss)
                        .is_some(),
            ),
            usize::from(
                matches!(interaction, ActiveObjectTransition::Preserve)
                    && target_publication
                        .as_ref()
                        .and_then(PreparedTargetPublication::activation)
                        .is_some(),
            ),
            interaction_event_capacity,
        ])?;
        let mut events = Vec::with_capacity(event_capacity);
        let mut active_object = None;
        if let Some(active) = self.active_object.filter(|_| config_update.is_some()) {
            let reason = match &config_update {
                Some(TransitionConfigUpdate::Rebind {
                    active_loss_reason, ..
                }) => *active_loss_reason,
                _ => crate::InlineObjectRealizationLossReason::Superseded,
            };
            events.push(RangeTextInputEvent::InlineObjectRealizationLost(
                crate::InlineObjectRealizationLoss {
                    anchor: active.anchor,
                    reason,
                },
            ));
            active_object = Some(None);
        }
        if let Some((key, outcome)) = match &config_update {
            Some(TransitionConfigUpdate::Rebind { settlement, .. }) => *settlement,
            _ => None,
        } {
            events.push(RangeTextInputEvent::MutationSettled { key, outcome });
        }
        if reject_restoration {
            events.push(RangeTextInputEvent::RestorationRejected);
        }
        if matches!(interaction, ActiveObjectTransition::Preserve)
            && let Some(loss) = target_publication
                .as_ref()
                .and_then(PreparedTargetPublication::active_loss)
            && !events.iter().any(|event| {
                matches!(event, RangeTextInputEvent::InlineObjectRealizationLost(existing) if *existing == loss)
            })
        {
            events.push(RangeTextInputEvent::InlineObjectRealizationLost(loss));
        }
        if matches!(interaction, ActiveObjectTransition::Preserve)
            && let Some(activation) = target_publication
                .as_ref()
                .and_then(PreparedTargetPublication::activation)
        {
            events.push(RangeTextInputEvent::InlineObjectActivated(activation));
        }
        match interaction {
            ActiveObjectTransition::Preserve => {}
            ActiveObjectTransition::Clear(reason) => {
                if let Some(active) = self.active_object {
                    events.push(RangeTextInputEvent::InlineObjectRealizationLost(
                        crate::InlineObjectRealizationLoss {
                            anchor: active.anchor,
                            reason,
                        },
                    ));
                }
                active_object = Some(None);
            }
            ActiveObjectTransition::Set { active, activation } => {
                if let Some(prior) = self
                    .active_object
                    .filter(|prior| prior.anchor != active.anchor)
                {
                    events.push(RangeTextInputEvent::InlineObjectRealizationLost(
                        crate::InlineObjectRealizationLoss {
                            anchor: prior.anchor,
                            reason: crate::InlineObjectRealizationLossReason::SelectionChanged,
                        },
                    ));
                }
                if let Some(origin) = activation.filter(|_| active.activation_eligible) {
                    events.push(RangeTextInputEvent::InlineObjectActivated(
                        crate::InlineObjectActivation {
                            anchor: active.anchor,
                            origin,
                        },
                    ));
                }
                active_object = Some(Some(active));
            }
        }
        let surviving_requests = self
            .requests
            .iter()
            .filter(|request| {
                request_survives_transition(
                    request,
                    geometry.release(),
                    residency_rebind.as_ref(),
                    object_rebind.as_ref(),
                    clipboard_rebind,
                    edit_disposal,
                    rebind_binding.is_some(),
                )
            })
            .count();
        let destination_capacity = surviving_requests
            .checked_add(effects.len())
            .ok_or(RangeTextInputError::SurfaceCapacity)?;
        let requests = VecDeque::with_capacity(destination_capacity);
        let effect_bytes =
            checked_capacity_product(effects.capacity(), size_of::<RangeTextInputRequest>())?;
        let event_bytes =
            checked_capacity_product(events.capacity(), size_of::<RangeTextInputEvent>())?;
        let detached_edit_bytes = checked_capacity_product(
            replacement_detached_edits.as_ref().map_or(0, Vec::capacity),
            size_of::<crate::RangeEditCoordinator>(),
        )?;
        let destination_request_bytes =
            checked_capacity_product(requests.capacity(), size_of::<RangeTextInputRequest>())?;
        let proof_bytes = checked_capacity_product(
            adopted_mutation
                .as_ref()
                .map_or(0, |(_, proofs)| proofs.capacity()),
            size_of::<crate::range_edit::SourcePositionProof>(),
        )?;
        let proof_items = adopted_mutation
            .as_ref()
            .map_or(0, |(_, proofs)| proofs.capacity());
        let current_request_bytes = self
            .requests
            .capacity()
            .checked_mul(size_of::<RangeTextInputRequest>())
            .ok_or(RangeTextInputError::SurfaceCapacity)?;
        let prior_charge = self
            .surface
            .as_ref()
            .map_or(crate::RangeSurfaceCharge::default(), |surface| {
                surface.charge()
            });
        let admission_components = WidgetAdmissionComponents {
            prior_surface: prior_charge,
            current_request_storage: crate::RangeSurfaceCharge {
                bytes: current_request_bytes,
                items: self.requests.capacity(),
            },
            candidate_record: crate::RangeSurfaceCharge {
                bytes: size_of::<WidgetTransitionCandidate>(),
                items: 1,
            },
            geometry: crate::RangeSurfaceCharge {
                bytes: geometry.retained_bytes(),
                items: geometry.retained_items(),
            },
            resident_payload: target_publication.as_ref().map_or(
                crate::RangeSurfaceCharge::default(),
                PreparedTargetPublication::resident_payload_charge,
            ),
            publication_allocation: target_publication.as_ref().map_or(
                crate::RangeSurfaceCharge::default(),
                PreparedTargetPublication::prepared_allocation_charge,
            ),
            effect_storage: crate::RangeSurfaceCharge {
                bytes: effect_bytes,
                items: effects.capacity(),
            },
            event_storage: crate::RangeSurfaceCharge {
                bytes: event_bytes,
                items: events.capacity(),
            },
            page_demand: crate::RangeSurfaceCharge {
                bytes: page.as_ref().map_or(0, PreparedPageDemand::retained_bytes),
                items: page.as_ref().map_or(0, PreparedPageDemand::retained_items),
            },
            object_rebind: crate::RangeSurfaceCharge {
                bytes: object_rebind
                    .as_ref()
                    .map_or(0, PreparedObjectRebind::retained_bytes),
                items: object_rebind
                    .as_ref()
                    .map_or(0, PreparedObjectRebind::retained_items),
            },
            residency_rebind: crate::RangeSurfaceCharge {
                bytes: residency_rebind
                    .as_ref()
                    .map_or(0, PreparedResidencyRebind::retained_bytes),
                items: residency_rebind
                    .as_ref()
                    .map_or(0, PreparedResidencyRebind::retained_items),
            },
            detached_edit_storage: crate::RangeSurfaceCharge {
                bytes: detached_edit_bytes,
                items: replacement_detached_edits.as_ref().map_or(0, Vec::capacity),
            },
            destination_request_storage: crate::RangeSurfaceCharge {
                bytes: destination_request_bytes,
                items: requests.capacity(),
            },
            proof_storage: crate::RangeSurfaceCharge {
                bytes: proof_bytes,
                items: proof_items,
            },
        };
        let admission_charge = admission_components
            .checked_total()
            .ok_or(RangeTextInputError::SurfaceCapacity)?;
        if admission_charge.bytes > self.config.limits.max_surface_bytes
            || admission_charge.items > self.config.limits.max_surface_items
            || target_publication.as_ref().is_some_and(|publication| {
                publication.final_charge().bytes > self.config.limits.max_surface_bytes
                    || publication.final_charge().items > self.config.limits.max_surface_items
            })
        {
            return Err(RangeTextInputError::SurfaceCapacity);
        }
        #[cfg(test)]
        self.last_widget_admission_components
            .set(Some(admission_components));
        Ok(WidgetTransitionCandidate {
            expected_next_id: self.next_id,
            committed_next_id,
            geometry,
            page,
            desired,
            surface_candidate,
            target_publication,
            config_update,
            residency_rebind,
            object_rebind,
            clipboard_rebind,
            replacement_edits,
            edit_disposal,
            replacement_detached_edits,
            scrollbar_replacement: None,
            effects,
            events,
            active_object,
            pointer_anchor: None,
            requests,
            admission_charge,
            reject_restoration,
            settling_mutation,
            adopted_mutation,
        })
    }

    fn transition_ids(&self) -> Result<(GeometryJobId, PageRequestId, u64), RangeTextInputError> {
        let request = self
            .next_id
            .checked_add(1)
            .ok_or(RangeTextInputError::Stale)?;
        let next = request.checked_add(1).ok_or(RangeTextInputError::Stale)?;
        Ok((
            GeometryJobId::new(self.next_id),
            PageRequestId::new(request),
            next,
        ))
    }

    pub(super) fn commit_widget_transition(
        &mut self,
        candidate: WidgetTransitionCandidate,
        cx: Option<&mut Context<Self>>,
    ) -> ExactGeometryProgress {
        let committed = self.commit_widget_transition_internal(candidate);
        self.flush_widget_transition(committed, cx)
    }

    pub(super) fn commit_widget_transition_internal(
        &mut self,
        candidate: WidgetTransitionCandidate,
    ) -> CommittedWidgetTransition {
        debug_assert_eq!(self.next_id, candidate.expected_next_id);
        let WidgetTransitionCandidate {
            committed_next_id,
            geometry,
            page,
            desired,
            surface_candidate,
            target_publication,
            config_update,
            residency_rebind,
            object_rebind,
            clipboard_rebind,
            replacement_edits,
            edit_disposal,
            mut replacement_detached_edits,
            scrollbar_replacement,
            effects,
            events,
            active_object,
            pointer_anchor,
            requests,
            admission_charge,
            reject_restoration,
            settling_mutation,
            adopted_mutation,
            ..
        } = candidate;
        let mut prior_requests = std::mem::replace(&mut self.requests, requests);
        while let Some(request) = prior_requests.pop_front() {
            if request_survives_transition(
                &request,
                geometry.release(),
                residency_rebind.as_ref(),
                object_rebind.as_ref(),
                clipboard_rebind,
                edit_disposal,
                rebind_binding_from_update(&config_update).is_some(),
            ) {
                self.requests.push_back(request);
            }
        }
        let successor_page = geometry.page_request();
        let start = self.geometry.commit_prepared_transition(geometry);
        self.next_id = committed_next_id;
        let page_outcome = page.map(|page| self.residency.commit_prepared_demand(page));
        if let Some(rebind) = residency_rebind {
            let cancelled = self.residency.commit_prepared_rebind(rebind);
            for key in cancelled {
                self.remove_queued_page(key);
                self.dispatched_pages.remove(&key);
            }
        }
        if let Some(rebind) = object_rebind {
            let cancelled = self.object_residency.commit_prepared_rebind(rebind);
            for key in cancelled {
                self.remove_queued_object_page(key);
                self.dispatched_object_pages.remove(&key);
            }
        }
        self.commit_geometry_retirement(start.release());
        if let Some(desired) = desired {
            self.desired = desired;
        }
        let committed_rebind_binding = rebind_binding_from_update(&config_update);
        match config_update {
            Some(TransitionConfigUpdate::Layout(layout, style)) => {
                self.config.layout = layout;
                self.config.style = style;
            }
            Some(TransitionConfigUpdate::Presentation(generation)) => {
                self.config.presentation_generation = generation;
            }
            Some(TransitionConfigUpdate::Rebind { binding, .. }) => {
                self.config.binding = binding;
                self.pending_insert = None;
                self.pending_object_remove = None;
                self.pending_select_all = false;
                self.mutation_positions = None;
                self.adopted_positions = None;
                self.admitted_edit_proofs.clear();
                self.mutation_composition = None;
                self.pending_geometry_object = None;
                self.pending_page_aliases.clear();
                self.pending_clipboard_page = None;
                self.clipboard_cut_proofs = None;
                self.segmentation = None;
                self.segmentation_action = None;
                self.platform = None;
                self.restoration = None;
                self.restoration_seed = None;
                self.published_restoration = None;
                self.replacement = None;
                self.platform_ready = None;
            }
            None => {}
        }
        if let Some(binding) = committed_rebind_binding {
            self.remove_all_queued_source_requests();
            self.clipboard
                .commit_prepared_rebind(binding, clipboard_rebind);
            self.dispatched_clipboard_write = None;
            if let Some(cancellation) = clipboard_rebind {
                self.remove_queued_clipboard_write(cancellation.key());
            }
            if let Some(replacement) = replacement_edits {
                let mut prior = std::mem::replace(&mut self.edits, replacement);
                let actual_disposal = prior.dispose();
                debug_assert_eq!(actual_disposal, edit_disposal);
                if matches!(actual_disposal, Some(crate::MutationDisposal::Detached(_))) {
                    let mut detached = replacement_detached_edits
                        .take()
                        .expect("detached capacity prepared");
                    detached.append(&mut self.detached_edits);
                    detached.push(prior);
                    self.detached_edits = detached;
                }
                if let Some(disposal) = actual_disposal {
                    let key = match disposal {
                        crate::MutationDisposal::Cancelled(key)
                        | crate::MutationDisposal::Detached(key) => key,
                    };
                    self.remove_queued_mutation(key);
                    self.dispatched_mutations.remove(&key);
                }
            }
            if let Some(key) = settling_mutation {
                self.remove_queued_mutation(key);
                self.dispatched_mutations.remove(&key);
            }
            self.pending_history = None;
            self.remove_queued_history();
        }
        if let Some((expected, replacement)) = scrollbar_replacement {
            debug_assert_eq!(self.scrollbar.owner, expected);
            self.scrollbar.owner = replacement;
            self.scrollbar.model.set(None);
        }
        self.surface_candidate = surface_candidate;
        if reject_restoration {
            self.active_geometry = None;
            self.pending_geometry_page = None;
            self.pending_geometry_object = None;
            self.surface_candidate = None;
            self.restoration_seed = None;
            self.published_restoration = None;
            self.desired =
                DesiredSurface::origin(self.config.viewport_extent, self.config.overscan);
        }
        match start.progress() {
            ExactGeometryProgress::Scanning => {
                self.active_geometry = Some(start.key());
                if let (Some(request), Some(demand)) = (successor_page, page_outcome) {
                    self.install_prepared_geometry_page(start.key(), request, demand);
                }
            }
            ExactGeometryProgress::TargetComplete => self.active_geometry = None,
            ExactGeometryProgress::PendingIndex => {}
            ExactGeometryProgress::NeedObjects | ExactGeometryProgress::IndexComplete => {
                unreachable!("prepared transitions start with text or complete")
            }
        }
        if let Some(publication) = target_publication {
            debug_assert_eq!(start.progress(), ExactGeometryProgress::TargetComplete);
            self.commit_prepared_target_publication(publication, admission_charge);
        }
        if let Some(active_object) = active_object {
            self.active_object = active_object;
        }
        if let Some(pointer_anchor) = pointer_anchor {
            self.pointer_anchor = pointer_anchor;
        }
        if let Some((positions, proofs)) = adopted_mutation {
            self.adopted_positions = Some(positions);
            self.admitted_edit_proofs = proofs;
        }
        self.last_surface_admission = Some(admission_charge);
        CommittedWidgetTransition {
            progress: start.progress(),
            effects,
            events,
        }
    }

    pub(super) fn flush_widget_transition(
        &mut self,
        committed: CommittedWidgetTransition,
        cx: Option<&mut Context<Self>>,
    ) -> ExactGeometryProgress {
        for effect in committed.effects {
            debug_assert!(self.requests.len() < self.requests.capacity());
            self.requests.push_back(effect);
        }
        if let Some(cx) = cx {
            for event in committed.events {
                cx.emit(event);
            }
            cx.notify();
        }
        committed.progress
    }

    fn commit_geometry_retirement(&mut self, release: &crate::ExactGeometryRelease) {
        if self.pending_geometry_page.as_ref().is_some_and(|pending| {
            release.jobs.contains(&pending.job) || release.pages.contains(&pending.request.key())
        }) {
            self.pending_geometry_page = None;
        }
        if self
            .pending_geometry_object
            .as_ref()
            .is_some_and(|pending| {
                release.jobs.contains(&pending.job)
                    || release.object_pages.contains(&pending.request.key())
            })
        {
            self.pending_geometry_object = None;
        }
        for key in &release.pages {
            self.remove_queued_page(*key);
            self.dispatched_pages.remove(key);
            let _ = self.residency.cancel(*key);
        }
        for key in &release.object_pages {
            self.remove_queued_object_page(*key);
            self.dispatched_object_pages.remove(key);
            let _ = self.object_residency.cancel(*key);
        }
    }

    fn remove_queued_page(&mut self, key: crate::PageRequestKey) {
        if let Some(index) = self.requests.iter().position(
            |request| matches!(request, RangeTextInputRequest::Page(page) if page.key() == key),
        ) {
            self.requests.remove(index);
        }
    }

    fn remove_queued_object_page(&mut self, key: crate::ObjectRequestKey) {
        if let Some(index) = self.requests.iter().position(
            |request| matches!(request, RangeTextInputRequest::ObjectPage(page) if page.key() == key),
        ) {
            self.requests.remove(index);
        }
    }

    fn remove_queued_mutation(&mut self, key: crate::MutationKey) {
        self.requests.retain(|request| {
            !matches!(request,
                RangeTextInputRequest::MutationPreflight(proposal) if proposal.key() == key
            ) && !matches!(request,
                RangeTextInputRequest::MutationFragment { key: request_key, .. }
                    | RangeTextInputRequest::MutationCommit(request_key) if *request_key == key
            )
        });
    }

    fn remove_queued_clipboard_write(&mut self, key: crate::ClipboardKey) {
        if let Some(index) = self.requests.iter().position(
            |request| matches!(request, RangeTextInputRequest::ClipboardWrite(write) if write.key() == key),
        ) {
            self.requests.remove(index);
        }
    }

    fn remove_queued_history(&mut self) {
        self.requests
            .retain(|request| !matches!(request, RangeTextInputRequest::HistoryIntent(_)));
    }

    fn remove_all_queued_source_requests(&mut self) {
        self.requests.retain(|request| {
            !matches!(
                request,
                RangeTextInputRequest::Page(_) | RangeTextInputRequest::ObjectPage(_)
            )
        });
    }
}

fn request_survives_transition(
    request: &RangeTextInputRequest,
    geometry_release: &crate::ExactGeometryRelease,
    residency_rebind: Option<&PreparedResidencyRebind>,
    object_rebind: Option<&PreparedObjectRebind>,
    clipboard_rebind: Option<crate::ClipboardCancellation>,
    edit_disposal: Option<crate::MutationDisposal>,
    is_rebind: bool,
) -> bool {
    match request {
        RangeTextInputRequest::Page(page) => {
            !is_rebind
                && !geometry_release.pages.contains(&page.key())
                && residency_rebind.is_none_or(|rebind| !rebind.cancelled().contains(&page.key()))
        }
        RangeTextInputRequest::ObjectPage(page) => {
            !is_rebind
                && !geometry_release.object_pages.contains(&page.key())
                && object_rebind.is_none_or(|rebind| !rebind.cancelled().contains(&page.key()))
        }
        RangeTextInputRequest::ClipboardWrite(write) => {
            clipboard_rebind.is_none_or(|rebind| rebind.key() != write.key())
        }
        RangeTextInputRequest::MutationPreflight(proposal) => {
            edit_disposal.is_none_or(|disposal| mutation_disposal_key(disposal) != proposal.key())
        }
        RangeTextInputRequest::MutationFragment { key, .. }
        | RangeTextInputRequest::MutationCommit(key) => {
            edit_disposal.is_none_or(|disposal| mutation_disposal_key(disposal) != *key)
        }
        RangeTextInputRequest::HistoryIntent(_) => !is_rebind,
        _ => true,
    }
}

fn mutation_disposal_key(disposal: crate::MutationDisposal) -> crate::MutationKey {
    match disposal {
        crate::MutationDisposal::Cancelled(key) | crate::MutationDisposal::Detached(key) => key,
    }
}

fn checked_capacity_sum(
    values: impl IntoIterator<Item = usize>,
) -> Result<usize, RangeTextInputError> {
    values.into_iter().try_fold(0usize, |total, value| {
        total
            .checked_add(value)
            .ok_or(RangeTextInputError::SurfaceCapacity)
    })
}

fn checked_capacity_product(count: usize, width: usize) -> Result<usize, RangeTextInputError> {
    count
        .checked_mul(width)
        .ok_or(RangeTextInputError::SurfaceCapacity)
}

fn rebind_binding_from_update(
    update: &Option<TransitionConfigUpdate>,
) -> Option<crate::RangeBinding> {
    match update {
        Some(TransitionConfigUpdate::Rebind { binding, .. }) => Some(*binding),
        _ => None,
    }
}
