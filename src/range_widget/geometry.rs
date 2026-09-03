mod deferred;
mod response_commit;
mod response_preparation;
mod terminal_failure;

pub(super) use deferred::DeferredGeometryResponse;

use std::{collections::VecDeque, mem::size_of};

use gpui::{Context, Window};

use super::response_custody::ResponseDeliveryProgress;
use super::surface::PreparedCoherentRangeSurface;
use super::{
    CoherentRangeSurface, DesiredSurface, RangeScrollAnchor, RangeTextInput, RangeTextInputError,
    RangeTextInputRequest, SurfaceCandidate,
};
use crate::{
    ExactGeometryProgress, ObjectDemand, ObjectPageId, ObjectRequestId, ObjectRequestKey,
    PageDemand, PageId, PageRequest, PageRequestId, PageRequestKey, RangePage, RangeTextInputEvent,
};

pub(super) struct PendingGeometryPage {
    pub(super) job: crate::GeometryJobKey,
    pub(super) request: PageRequest,
    pub(super) wait: GeometryPageWait,
}

pub(super) struct PendingGeometryObject {
    pub(super) job: crate::GeometryJobKey,
    pub(super) request: crate::ObjectRequest,
    pub(super) text_page: PageId,
    pub(super) wait: GeometryObjectWait,
}

pub(super) struct PreparedTargetPublication {
    state: SurfaceCandidate,
    surface: PreparedCoherentRangeSurface,
    resident_payload_charge: crate::RangeSurfaceCharge,
    prepared_allocation_charge: crate::RangeSurfaceCharge,
    pages: Vec<crate::RangePage>,
    object_pages: Vec<crate::ObjectPage>,
    select_all: Option<crate::RangeSourceSelection>,
    active_loss: Option<crate::InlineObjectRealizationLoss>,
    active_result: Option<Option<super::ActiveInlineObject>>,
    activation: Option<crate::InlineObjectActivation>,
}

pub(super) struct PreparedTerminalResponsePublication {
    geometry: crate::range_geometry::PreparedTargetResponse,
    text_admission: Option<crate::residency::PreparedRangePageAdmission>,
    object_admission: Option<crate::object_residency::PreparedObjectPageAdmission>,
    text_touch: Option<PageId>,
    object_touch: Option<ObjectPageId>,
    publication: PreparedTargetPublication,
    release_request: Option<RangeTextInputRequest>,
    destination_requests: VecDeque<RangeTextInputRequest>,
    completed_page: Option<PageRequestKey>,
    completed_object_page: Option<ObjectRequestKey>,
    admission_charge: crate::RangeSurfaceCharge,
    next_id: Option<u64>,
}

struct PreparedNonterminalResponsePublication {
    geometry: crate::range_geometry::PreparedTargetResponse,
    text_admission: Option<crate::residency::PreparedRangePageAdmission>,
    object_admission: Option<crate::object_residency::PreparedObjectPageAdmission>,
    text_demand: Option<crate::residency::PreparedPageDemand>,
    object_demand: Option<crate::object_residency::PreparedObjectDemand>,
    text_touches: [Option<PageId>; 2],
    object_touches: [Option<ObjectPageId>; 2],
    pending_page: Option<PendingGeometryPage>,
    pending_object: Option<PendingGeometryObject>,
    effects: [Option<RangeTextInputRequest>; 2],
    destination_requests: VecDeque<RangeTextInputRequest>,
    completed_page: Option<PageRequestKey>,
    completed_object_page: Option<ObjectRequestKey>,
    next_id: u64,
    desired: Option<DesiredSurface>,
    surface_candidate: Option<SurfaceCandidate>,
}

impl PreparedNonterminalResponsePublication {
    fn initiates_external_request(&self) -> bool {
        self.effects.iter().flatten().any(|effect| {
            matches!(
                effect,
                RangeTextInputRequest::Page(_) | RangeTextInputRequest::ObjectPage(_)
            )
        })
    }
}

pub(super) enum TerminalTargetPreparation {
    Retarget(DesiredSurface),
    Publication(PreparedTargetPublication),
}

impl PreparedTargetPublication {
    pub(super) const fn resident_payload_charge(&self) -> crate::RangeSurfaceCharge {
        self.resident_payload_charge
    }

    pub(super) const fn prepared_allocation_charge(&self) -> crate::RangeSurfaceCharge {
        self.prepared_allocation_charge
    }

    pub(super) const fn final_charge(&self) -> crate::RangeSurfaceCharge {
        self.surface.charge()
    }

    pub(super) const fn active_loss(&self) -> Option<crate::InlineObjectRealizationLoss> {
        self.active_loss
    }

    pub(super) const fn activation(&self) -> Option<crate::InlineObjectActivation> {
        self.activation
    }
}

fn resolve_prepared_active_object(
    current: Option<super::ActiveInlineObject>,
    desired: DesiredSurface,
    surface: &PreparedCoherentRangeSurface,
) -> Result<
    (
        Option<Option<super::ActiveInlineObject>>,
        Option<crate::InlineObjectRealizationLoss>,
        Option<crate::InlineObjectActivation>,
    ),
    RangeTextInputError,
> {
    match desired.inline_object_interaction {
        Some(super::DesiredInlineObjectInteraction::Set {
            object_id,
            order,
            activation_eligible,
            origin,
        }) => {
            let object = surface
                .object_selected_by(surface.selection())
                .filter(|object| object.id() == object_id && object.order() == order)
                .ok_or(RangeTextInputError::IncompleteSurface)?;
            let key = surface.geometry_key();
            let active = super::ActiveInlineObject {
                anchor: crate::RealizedInlineObjectAnchor {
                    binding: surface.binding(),
                    object_id,
                    order,
                    presentation_generation: key.presentation_generation(),
                    layout_epoch: key.epoch(),
                    bounds: object.bounds(),
                },
                leading: object.leading(),
                trailing: object.trailing(),
                activation_eligible,
            };
            let loss = current
                .filter(|prior| prior.anchor != active.anchor)
                .map(|prior| crate::InlineObjectRealizationLoss {
                    anchor: prior.anchor,
                    reason: crate::InlineObjectRealizationLossReason::SelectionChanged,
                });
            let activation = origin.filter(|_| activation_eligible).map(|origin| {
                crate::InlineObjectActivation {
                    anchor: active.anchor,
                    origin,
                }
            });
            Ok((Some(Some(active)), loss, activation))
        }
        Some(super::DesiredInlineObjectInteraction::Clear(reason)) => Ok((
            Some(None),
            current.map(|active| crate::InlineObjectRealizationLoss {
                anchor: active.anchor,
                reason,
            }),
            None,
        )),
        None => {
            let loss = current.and_then(|active| {
                let key = surface.geometry_key();
                let same_key = surface.binding() == active.anchor.binding
                    && key.presentation_generation() == active.anchor.presentation_generation
                    && key.epoch() == active.anchor.layout_epoch;
                let still_realized =
                    surface
                        .object_selected_by(surface.selection())
                        .is_some_and(|object| {
                            object.id() == active.anchor.object_id
                                && object.order() == active.anchor.order
                                && object.bounds() == active.anchor.bounds
                        });
                (!same_key || !still_realized).then(|| crate::InlineObjectRealizationLoss {
                    anchor: active.anchor,
                    reason: if same_key {
                        crate::InlineObjectRealizationLossReason::Unrealized
                    } else {
                        crate::InlineObjectRealizationLossReason::Superseded
                    },
                })
            });
            Ok((loss.map(|_| None), loss, None))
        }
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum GeometryObjectWait {
    Resident(ObjectPageId),
    Coalesced(ObjectRequestKey),
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum GeometryPageWait {
    Resident(PageId),
    Coalesced(PageRequestKey),
}

impl RangeTextInput {
    pub(super) fn target_response_successor(
        &self,
    ) -> Result<crate::range_geometry::TargetResponseSuccessor, RangeTextInputError> {
        let desired = self
            .surface_candidate
            .as_ref()
            .filter(|candidate| Some(candidate.job) == self.active_geometry)
            .map(|candidate| candidate.desired)
            .ok_or(RangeTextInputError::Stale)?;
        self.next_id
            .checked_add(1)
            .ok_or(RangeTextInputError::Busy)?;
        Ok(crate::range_geometry::TargetResponseSuccessor {
            target_job_id: crate::GeometryJobId::new(self.next_id),
            page_id: PageRequestId::new(self.next_id),
            object_id: ObjectRequestId::new(self.next_id),
            max_objects: self.config.object_residency_limits.max_resident_objects(),
            max_object_bytes: self.config.object_residency_limits.max_resident_bytes(),
            target: desired.target(),
            anchor: None,
            select_all: false,
        })
    }

    pub(super) fn index_response_successor(
        &self,
    ) -> Result<crate::range_geometry::TargetResponseSuccessor, RangeTextInputError> {
        let page_id = self
            .next_id
            .checked_add(1)
            .ok_or(RangeTextInputError::Busy)?;
        self.next_id
            .checked_add(2)
            .ok_or(RangeTextInputError::Busy)?;
        let restoration = self
            .surface_candidate
            .as_ref()
            .and_then(|candidate| candidate.restoration)
            .or(self.restoration_seed);
        let mut target = self.desired.target();
        let anchor = restoration
            .map(super::restoration::restoration_layout_anchor)
            .or_else(|| self.desired.exact_interaction_anchor())
            .or_else(|| {
                matches!(
                    self.desired.priority(),
                    crate::RangeRealizationPriority::Caret
                        | crate::RangeRealizationPriority::Ime
                        | crate::RangeRealizationPriority::DirectedSelection
                        | crate::RangeRealizationPriority::ActiveInteraction
                )
                .then_some(self.desired.source_selection)
                .flatten()
                .map(|selection| selection.head)
                .and_then(|anchor| {
                    if anchor.byte_offset.get() == self.config.binding.extent().byte_len() {
                        return Some(anchor);
                    }
                    match self.current_surface_position_for_source_position(anchor) {
                        Some(position) => {
                            let end = target.block_offset()
                                + target.viewport_extent()
                                + target.overscan();
                            if position.y < target.block_offset() || position.y >= end {
                                target = crate::BlockTarget::new(
                                    position.y,
                                    target.viewport_extent(),
                                    target.overscan(),
                                );
                            }
                            None
                        }
                        None => Some(anchor),
                    }
                })
            });
        Ok(crate::range_geometry::TargetResponseSuccessor {
            target_job_id: crate::GeometryJobId::new(self.next_id),
            page_id: PageRequestId::new(page_id),
            object_id: ObjectRequestId::new(page_id),
            max_objects: self.config.object_residency_limits.max_resident_objects(),
            max_object_bytes: self.config.object_residency_limits.max_resident_bytes(),
            target,
            anchor,
            select_all: self.pending_select_all,
        })
    }
    pub(super) fn install_prepared_geometry_page(
        &mut self,
        job: crate::GeometryJobKey,
        request: PageRequest,
        demand: PageDemand,
    ) {
        match demand {
            PageDemand::Requested(expected) => {
                debug_assert_eq!(expected.key(), request.key());
            }
            PageDemand::ResidentAdjacent(page) => {
                self.pending_geometry_page = Some(PendingGeometryPage {
                    job,
                    request,
                    wait: GeometryPageWait::Resident(page),
                });
            }
            PageDemand::Coalesced(existing) => {
                self.pending_geometry_page = Some(PendingGeometryPage {
                    job,
                    request,
                    wait: GeometryPageWait::Coalesced(existing),
                });
            }
            PageDemand::ResidentValidation { .. } => {
                unreachable!("geometry uses adjacent page demand")
            }
        }
    }

    pub(super) fn start_index(&mut self) -> Result<(), RangeTextInputError> {
        let candidate = self.prepare_index_transition()?;
        match self.commit_widget_transition(candidate, None) {
            ExactGeometryProgress::Scanning => Ok(()),
            _ => Err(RangeTextInputError::Stale),
        }
    }

    fn service_pending_index_intent(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Result<bool, RangeTextInputError> {
        if !self.pending_index_intent || self.active_geometry.is_some() {
            return Ok(false);
        }
        if self.geometry.index().is_some() {
            self.pending_index_intent = false;
            return Ok(false);
        }
        if !self.try_spend_realization_credit(cx) {
            return Ok(false);
        }
        let candidate = match self.prepare_index_transition() {
            Ok(candidate) => candidate,
            Err(error) => {
                self.refund_realization_credit();
                return Err(error);
            }
        };
        let progress = self.commit_widget_transition(candidate, Some(cx));
        if progress != ExactGeometryProgress::Scanning {
            self.refund_realization_credit();
            return Err(RangeTextInputError::Stale);
        }
        self.pending_index_intent = false;
        Ok(true)
    }

    fn abort_geometry_demand(
        &mut self,
        job: crate::GeometryJobKey,
        cx: Option<&mut Context<Self>>,
    ) -> Result<(), RangeTextInputError> {
        let release = self.geometry.cancel(job)?;
        self.release_geometry(&release, None, None, cx);
        if self.active_geometry == Some(job) {
            self.active_geometry = None;
        }
        Ok(())
    }

    pub(super) fn service_geometry_page(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), RangeTextInputError> {
        let result = if self.pending_geometry_object.is_some() {
            self.service_geometry_object(window, cx)
        } else {
            self.service_geometry_page_inner(window, cx)
        };
        let closed_terminal_failure =
            matches!(&result, Err(RangeTextInputError::IncompleteSurface))
                && self.active_geometry.is_none();
        if result.is_err() && !closed_terminal_failure {
            self.reject_restoration_geometry(cx)?;
        }
        result
    }

    pub(super) fn service_geometry_until_external_boundary(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), RangeTextInputError> {
        if self.requests.is_empty() && self.service_pending_index_intent(cx)? {
            self.last_realization_step.reached_external_boundary = true;
            return Ok(());
        }
        loop {
            if !self.try_spend_realization_credit(cx) {
                return Ok(());
            }
            let before = self.geometry_service_marker();
            let result = self.service_geometry_page(window, cx);
            let progressed = self.geometry_service_marker() != before;
            if !progressed || result.is_err() {
                self.refund_realization_credit();
            }
            result?;
            if !progressed {
                self.last_realization_step.reached_external_boundary = true;
                return Ok(());
            }
            self.observe_realization_ownership();
        }
    }

    pub(super) fn service_admitted_geometry_for_prepaint(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), RangeTextInputError> {
        let _ = self.service_pending_restoration_completion(cx)?;
        if self.deferred_geometry_response.is_some() {
            if let ResponseDeliveryProgress::Rejected(error) =
                self.service_deferred_geometry_response(window, cx)
            {
                return Err(error);
            }
        }
        if self.requests.is_empty() && self.service_pending_index_intent(cx)? {
            self.last_realization_step.reached_external_boundary = true;
            return Ok(());
        }
        if !self.requests.is_empty() {
            self.last_realization_step.reached_external_boundary = true;
            return Ok(());
        }
        loop {
            if !self.requests.is_empty() {
                self.last_realization_step.reached_external_boundary = true;
                return Ok(());
            }
            if !self.try_spend_realization_credit(cx) {
                return Ok(());
            }
            let before = self.geometry_service_marker();
            let result = if matches!(
                self.pending_geometry_object
                    .as_ref()
                    .map(|pending| pending.wait),
                Some(GeometryObjectWait::Resident(_))
            ) {
                self.service_resident_geometry_object(window, cx, false)
            } else if matches!(
                self.pending_geometry_page
                    .as_ref()
                    .map(|pending| pending.wait),
                Some(GeometryPageWait::Resident(_))
            ) {
                self.service_resident_geometry_page(window, cx, false)
            } else {
                self.refund_realization_credit();
                return Ok(());
            };
            let result = result.expect("resident geometry wait was present");
            let progressed = self.geometry_service_marker() != before;
            if !progressed || result.is_err() {
                self.refund_realization_credit();
            }
            result?;
            if !progressed {
                self.last_realization_step.reached_external_boundary = true;
                self.defer_realization_continuation(window, cx);
                return Ok(());
            }
            self.observe_realization_ownership();
        }
    }

    fn geometry_service_marker(
        &self,
    ) -> (
        Option<crate::GeometryJobKey>,
        Option<(crate::GeometryJobKey, PageRequestKey, GeometryPageWait)>,
        Option<(crate::GeometryJobKey, ObjectRequestKey, GeometryObjectWait)>,
        usize,
    ) {
        (
            self.active_geometry,
            self.pending_geometry_page
                .as_ref()
                .map(|pending| (pending.job, pending.request.key(), pending.wait)),
            self.pending_geometry_object
                .as_ref()
                .map(|pending| (pending.job, pending.request.key(), pending.wait)),
            self.requests.len(),
        )
    }

    fn service_resident_target_object(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        allow_external_successor: bool,
    ) -> Option<Result<(), RangeTextInputError>> {
        let pending = self.pending_geometry_object.as_ref()?;
        let GeometryObjectWait::Resident(object_page_id) = pending.wait else {
            return None;
        };
        if pending.request.key().purpose() != crate::ObjectPurpose::GeometryTarget {
            return None;
        }
        let job = pending.job;
        let key = pending.request.key();
        let text_page_id = pending.text_page;
        if self.active_geometry != Some(job) {
            return Some(Err(RangeTextInputError::Stale));
        }
        let geometry = {
            let Some(text_page) = self.residency.peek_page_by_id(text_page_id) else {
                return Some(Err(RangeTextInputError::Stale));
            };
            let Some(object_page) = self.object_residency.peek_page_by_id(object_page_id) else {
                return Some(Err(RangeTextInputError::Stale));
            };
            self.geometry.prepare_target_resident_object_page(
                job,
                text_page,
                object_page,
                window.text_system(),
                match self.target_response_successor() {
                    Ok(successor) => successor,
                    Err(error) => return Some(Err(error)),
                },
            )
        };
        let geometry = match geometry {
            Ok(geometry) => geometry,
            Err(failure) => {
                return Some(Err(RangeTextInputError::Geometry(failure.error().clone())));
            }
        };
        if !allow_external_successor && geometry.progress() == ExactGeometryProgress::TargetComplete
        {
            return Some(Ok(()));
        }
        if geometry.progress() == ExactGeometryProgress::TargetComplete {
            let preparation = match self.prepare_terminal_response_publication(
                geometry,
                None,
                None,
                Some(text_page_id),
                Some(object_page_id),
                None,
                Some(key),
            ) {
                Ok(preparation) => preparation,
                Err(error) => return Some(Err(error)),
            };
            return Some(self.commit_terminal_response_preparation(preparation, cx));
        }
        let candidate = match self.prepare_nonterminal_response_publication(
            geometry,
            None,
            None,
            Some(text_page_id),
            Some(object_page_id),
            None,
            Some(key),
            None,
            None,
            None,
        ) {
            Ok(candidate) => candidate,
            Err(error) => return Some(Err(error)),
        };
        if !allow_external_successor && candidate.initiates_external_request() {
            return Some(Ok(()));
        }
        self.commit_nonterminal_response_publication(candidate, cx);
        Some(Ok(()))
    }

    fn service_resident_index_object(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        allow_external_successor: bool,
    ) -> Option<Result<(), RangeTextInputError>> {
        let pending = self.pending_geometry_object.as_ref()?;
        let GeometryObjectWait::Resident(object_page_id) = pending.wait else {
            return None;
        };
        if pending.request.key().purpose() != crate::ObjectPurpose::GeometryIndex {
            return None;
        }
        let job = pending.job;
        let key = pending.request.key();
        let text_page_id = pending.text_page;
        if self.active_geometry != Some(job) {
            return Some(Err(RangeTextInputError::Stale));
        }
        let geometry = {
            let Some(text_page) = self.residency.peek_page_by_id(text_page_id) else {
                return Some(Err(RangeTextInputError::Stale));
            };
            let Some(object_page) = self.object_residency.peek_page_by_id(object_page_id) else {
                return Some(Err(RangeTextInputError::Stale));
            };
            self.geometry.prepare_index_resident_object_page(
                job,
                text_page,
                object_page,
                window.text_system(),
                match self.index_response_successor() {
                    Ok(successor) => successor,
                    Err(error) => return Some(Err(error)),
                },
            )
        };
        let geometry = match geometry {
            Ok(geometry) => geometry,
            Err(failure) => {
                return Some(Err(RangeTextInputError::Geometry(failure.error().clone())));
            }
        };
        if !allow_external_successor && geometry.progress() == ExactGeometryProgress::TargetComplete
        {
            return Some(Ok(()));
        }
        if geometry.progress() == ExactGeometryProgress::TargetComplete {
            let preparation = match self.prepare_terminal_response_publication(
                geometry,
                None,
                None,
                Some(text_page_id),
                Some(object_page_id),
                None,
                Some(key),
            ) {
                Ok(preparation) => preparation,
                Err(error) => return Some(Err(error)),
            };
            return Some(self.commit_terminal_response_preparation(preparation, cx));
        }
        let index_target = match geometry
            .terminal_index()
            .map(|_| self.prepare_index_response_target(&geometry))
            .transpose()
        {
            Ok(index_target) => index_target,
            Err(error) => return Some(Err(error)),
        };
        let candidate = match self.prepare_nonterminal_response_publication(
            geometry,
            None,
            None,
            Some(text_page_id),
            Some(object_page_id),
            None,
            Some(key),
            None,
            None,
            index_target,
        ) {
            Ok(candidate) => candidate,
            Err(error) => return Some(Err(error)),
        };
        if !allow_external_successor && candidate.initiates_external_request() {
            return Some(Ok(()));
        }
        self.commit_nonterminal_response_publication(candidate, cx);
        Some(Ok(()))
    }

    fn service_resident_geometry_object(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        allow_external_successor: bool,
    ) -> Option<Result<(), RangeTextInputError>> {
        self.service_resident_target_object(window, cx, allow_external_successor)
            .or_else(|| self.service_resident_index_object(window, cx, allow_external_successor))
    }

    fn service_geometry_object(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), RangeTextInputError> {
        if let Some(result) = self.service_resident_geometry_object(window, cx, true) {
            return result;
        }
        let Some(mut pending) = self.pending_geometry_object.take() else {
            return Ok(());
        };
        if self.active_geometry != Some(pending.job) {
            return Err(RangeTextInputError::Stale);
        }
        if let GeometryObjectWait::Coalesced(existing) = pending.wait {
            if self
                .object_residency
                .pending_requests()
                .any(|request| request == existing)
            {
                self.pending_geometry_object = Some(pending);
                return Ok(());
            }
            let maximum = super::checked_request_capacity(&self.config)
                .expect("constructed range widget retains a valid request capacity");
            if self.requests.len() >= maximum {
                self.pending_geometry_object = Some(pending);
                return Ok(());
            }
            let demand = match self.object_residency.demand(
                pending.request.key().id(),
                pending.request.key().purpose(),
                pending.request.key().demand(),
            ) {
                Ok(demand) => demand,
                Err(_) => {
                    self.abort_geometry_demand(pending.job, Some(cx))?;
                    return Err(RangeTextInputError::Busy);
                }
            };
            pending.wait = match demand {
                ObjectDemand::Resident(page) => {
                    return self.reissue_geometry_object_from_resident(pending, page, Some(cx));
                }
                ObjectDemand::Coalesced(request) => GeometryObjectWait::Coalesced(request),
                ObjectDemand::Requested(request) if request.key() == pending.request.key() => {
                    self.requests
                        .push_back(RangeTextInputRequest::ObjectPage(request));
                    self.pending_geometry_object = Some(pending);
                    cx.notify();
                    return Ok(());
                }
                _ => {
                    self.abort_geometry_demand(pending.job, Some(cx))?;
                    return Err(RangeTextInputError::Stale);
                }
            };
        }
        self.pending_geometry_object = Some(pending);
        self.service_resident_geometry_object(window, cx, true)
            .unwrap_or(Ok(()))
    }

    fn reissue_geometry_object_from_resident(
        &mut self,
        mut pending: PendingGeometryObject,
        page: ObjectPageId,
        cx: Option<&mut Context<Self>>,
    ) -> Result<(), RangeTextInputError> {
        self.object_residency
            .peek_page_by_id(page)
            .ok_or(RangeTextInputError::Stale)?;
        pending.wait = GeometryObjectWait::Resident(page);
        self.pending_geometry_object = Some(pending);
        if let Some(cx) = cx {
            cx.notify();
        }
        Ok(())
    }

    pub(super) fn service_resident_target_page(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        allow_external_successor: bool,
    ) -> Option<Result<(), RangeTextInputError>> {
        let pending = self.pending_geometry_page.as_ref()?;
        let GeometryPageWait::Resident(page_id) = pending.wait else {
            return None;
        };
        if pending.request.key().purpose() != crate::PagePurpose::GeometryTarget {
            return None;
        }
        let job = pending.job;
        let key = pending.request.key();
        if self.active_geometry != Some(job) {
            return Some(Err(RangeTextInputError::Stale));
        }
        let geometry = {
            let Some(page) = self.residency.peek_page_by_id(page_id) else {
                return Some(Err(RangeTextInputError::Stale));
            };
            self.geometry.prepare_target_resident_page(
                job,
                page,
                window.text_system(),
                match self.target_response_successor() {
                    Ok(successor) => successor,
                    Err(error) => return Some(Err(error)),
                },
            )
        };
        let geometry = match geometry {
            Ok(geometry) => geometry,
            Err(failure) => {
                return Some(Err(RangeTextInputError::Geometry(failure.error().clone())));
            }
        };
        assert_ne!(
            geometry.progress(),
            ExactGeometryProgress::TargetComplete,
            "a target text response must require an object successor or forward replay"
        );
        let candidate = match self.prepare_nonterminal_response_publication(
            geometry,
            None,
            None,
            Some(page_id),
            None,
            Some(key),
            None,
            None,
            None,
            None,
        ) {
            Ok(candidate) => candidate,
            Err(error) => return Some(Err(error)),
        };
        if !allow_external_successor && candidate.initiates_external_request() {
            return Some(Ok(()));
        }
        self.commit_nonterminal_response_publication(candidate, cx);
        Some(Ok(()))
    }

    pub(super) fn service_resident_index_page(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        allow_external_successor: bool,
    ) -> Option<Result<(), RangeTextInputError>> {
        let pending = self.pending_geometry_page.as_ref()?;
        let GeometryPageWait::Resident(page_id) = pending.wait else {
            return None;
        };
        if pending.request.key().purpose() != crate::PagePurpose::GeometryIndex {
            return None;
        }
        let job = pending.job;
        let key = pending.request.key();
        if self.active_geometry != Some(job) {
            return Some(Err(RangeTextInputError::Stale));
        }
        let geometry = {
            let Some(page) = self.residency.peek_page_by_id(page_id) else {
                return Some(Err(RangeTextInputError::Stale));
            };
            self.geometry.prepare_index_resident_page(
                job,
                page,
                window.text_system(),
                match self.index_response_successor() {
                    Ok(successor) => successor,
                    Err(error) => return Some(Err(error)),
                },
            )
        };
        let geometry = match geometry {
            Ok(geometry) => geometry,
            Err(failure) => {
                return Some(Err(RangeTextInputError::Geometry(failure.error().clone())));
            }
        };
        assert_ne!(
            geometry.progress(),
            ExactGeometryProgress::TargetComplete,
            "an index text response must require an object successor or forward replay"
        );
        let index_target = match geometry
            .terminal_index()
            .map(|_| self.prepare_index_response_target(&geometry))
            .transpose()
        {
            Ok(index_target) => index_target,
            Err(error) => return Some(Err(error)),
        };
        let candidate = match self.prepare_nonterminal_response_publication(
            geometry,
            None,
            None,
            Some(page_id),
            None,
            Some(key),
            None,
            None,
            None,
            index_target,
        ) {
            Ok(candidate) => candidate,
            Err(error) => return Some(Err(error)),
        };
        if !allow_external_successor && candidate.initiates_external_request() {
            return Some(Ok(()));
        }
        self.commit_nonterminal_response_publication(candidate, cx);
        Some(Ok(()))
    }

    fn service_resident_geometry_page(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        allow_external_successor: bool,
    ) -> Option<Result<(), RangeTextInputError>> {
        self.service_resident_target_page(window, cx, allow_external_successor)
            .or_else(|| self.service_resident_index_page(window, cx, allow_external_successor))
    }

    fn service_geometry_page_inner(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), RangeTextInputError> {
        if let Some(result) = self.service_resident_geometry_page(window, cx, true) {
            return result;
        }
        let Some(mut pending) = self.pending_geometry_page.take() else {
            return Ok(());
        };
        if self.active_geometry != Some(pending.job) {
            return Err(RangeTextInputError::Stale);
        }
        if let GeometryPageWait::Coalesced(existing) = pending.wait {
            if self
                .residency
                .pending_requests()
                .any(|request| request == existing)
            {
                self.pending_geometry_page = Some(pending);
                return Ok(());
            }
            let maximum = super::checked_request_capacity(&self.config)
                .expect("constructed range widget retains a valid request capacity");
            if self.requests.len() >= maximum {
                self.pending_geometry_page = Some(pending);
                return Ok(());
            }
            let demand = match self.residency.demand(
                pending.request.key().id(),
                pending.request.key().purpose(),
                pending.request.key().demand(),
            ) {
                Ok(demand) => demand,
                Err(_) => {
                    self.pending_geometry_page = Some(pending);
                    return Err(RangeTextInputError::Busy);
                }
            };
            pending.wait = match demand {
                PageDemand::ResidentAdjacent(page) => GeometryPageWait::Resident(page),
                PageDemand::Coalesced(request) => GeometryPageWait::Coalesced(request),
                PageDemand::Requested(request) if request.key() == pending.request.key() => {
                    self.requests
                        .push_back(RangeTextInputRequest::Page(request));
                    cx.notify();
                    return Ok(());
                }
                _ => return Err(RangeTextInputError::Stale),
            };
            if matches!(pending.wait, GeometryPageWait::Resident(_)) {
                self.pending_geometry_page = Some(pending);
                return self
                    .service_resident_geometry_page(window, cx, true)
                    .expect("resident geometry page was restored");
            }
        }
        self.pending_geometry_page = Some(pending);
        self.service_resident_geometry_page(window, cx, true)
            .unwrap_or(Ok(()))
    }

    pub(super) fn geometry_waits_on(&self, key: PageRequestKey) -> bool {
        self.pending_geometry_page
            .as_ref()
            .is_some_and(|pending| matches!(pending.wait, GeometryPageWait::Coalesced(existing) if existing == key))
    }

    pub(in crate::range_widget) fn deliver_geometry_object_page_inner(
        &mut self,
        page: crate::ObjectPage,
        credit_spent: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<ResponseDeliveryProgress, RangeTextInputError> {
        let key = page.key();
        let pending = self
            .pending_geometry_object
            .as_ref()
            .ok_or(RangeTextInputError::Stale)?;
        let pending_key = pending.request.key();
        let coalesced = matches!(pending.wait, GeometryObjectWait::Coalesced(wait) if wait == key);
        let detached_job = self.active_geometry != Some(pending.job);
        if coalesced && (pending_key != key || detached_job) {
            if key.purpose() != crate::ObjectPurpose::GeometryIndex
                || !self.dispatched_object_pages.contains(&key)
            {
                return Err(RangeTextInputError::Stale);
            }
            return self.deliver_residency_geometry_object_page(
                page,
                pending.job,
                pending_key,
                detached_job,
                cx,
            );
        }
        if key.purpose() != crate::ObjectPurpose::GeometryIndex
            || pending_key != key
            || self.active_geometry != Some(pending.job)
            || !self.dispatched_object_pages.contains(&key)
        {
            return Err(RangeTextInputError::Stale);
        }
        let job = pending.job;
        let text_page_id = pending.text_page;
        let proofs = match self
            .residency
            .prove_object_page_anchors(self.config.binding, &page)
        {
            Ok(proofs) => proofs,
            Err(_) => {
                let error =
                    RangeTextInputError::Geometry(crate::ExactGeometryError::SourceContract);
                return self.reject_terminal_object_response(job, key, error, cx);
            }
        };
        let object_admission = match self.object_residency.prepare_admit(page, proofs) {
            Ok(admission) => admission,
            Err(crate::ObjectPageAdmissionError::Malformed(_)) => {
                return self.reject_terminal_object_response(
                    job,
                    key,
                    RangeTextInputError::Geometry(crate::ExactGeometryError::SourceContract),
                    cx,
                );
            }
            Err(
                crate::ObjectPageAdmissionError::Stale(_)
                | crate::ObjectPageAdmissionError::Cancelled(_)
                | crate::ObjectPageAdmissionError::Unavailable(_),
            ) => return Err(RangeTextInputError::Stale),
            Err(crate::ObjectPageAdmissionError::LimitExceeded(_)) => {
                return self.settle_terminal_object_response(
                    job,
                    key,
                    RangeTextInputError::SurfaceCapacity,
                    cx,
                );
            }
        };
        if !credit_spent {
            if !self.try_spend_realization_credit(cx) {
                self.defer_geometry_response(
                    DeferredGeometryResponse::IndexObject(object_admission.into_page()),
                    cx,
                )?;
                return Ok(ResponseDeliveryProgress::Progressed);
            }
        }
        let geometry = {
            let text_page = self
                .residency
                .peek_page_by_id(text_page_id)
                .ok_or(RangeTextInputError::Stale)?;
            self.geometry.prepare_index_object_page(
                job,
                text_page,
                object_admission.page(),
                window.text_system(),
                self.index_response_successor()?,
            )
        };
        let geometry = match geometry {
            Ok(geometry) => geometry,
            Err(failure) => {
                self.pending_response_exact_geometry_failure_stage = Some(failure.stage());
                let error = RangeTextInputError::Geometry(failure.error().clone());
                if matches!(failure.error(), crate::ExactGeometryError::SourceContract) {
                    return self.reject_terminal_object_response(job, key, error, cx);
                }
                return self.settle_terminal_object_response(job, key, error, cx);
            }
        };
        if geometry.progress() == ExactGeometryProgress::TargetComplete {
            let preparation = match self.prepare_terminal_response_publication(
                geometry,
                None,
                Some(object_admission),
                Some(text_page_id),
                None,
                None,
                Some(key),
            ) {
                Ok(preparation) => preparation,
                Err(RangeTextInputError::SurfaceCapacity) => {
                    return Ok(ResponseDeliveryProgress::RetryableTerminalSurfaceCapacity);
                }
                Err(error) => return Err(error),
            };
            return Ok(
                match self.commit_terminal_response_preparation(preparation, cx) {
                    Ok(()) => ResponseDeliveryProgress::Progressed,
                    Err(error) => ResponseDeliveryProgress::AcceptedTerminal(error),
                },
            );
        }
        let index_target = geometry
            .terminal_index()
            .map(|_| self.prepare_index_response_target(&geometry))
            .transpose()?;
        let candidate = match self.prepare_nonterminal_response_publication(
            geometry,
            None,
            Some(object_admission),
            Some(text_page_id),
            None,
            None,
            Some(key),
            None,
            Some(key),
            index_target,
        ) {
            Ok(candidate) => candidate,
            Err(error) => return self.settle_terminal_object_response(job, key, error, cx),
        };
        self.commit_nonterminal_response_publication(candidate, cx);
        Ok(
            match self.service_geometry_until_external_boundary(window, cx) {
                Ok(()) => ResponseDeliveryProgress::Progressed,
                Err(error) => ResponseDeliveryProgress::AcceptedTerminal(error),
            },
        )
    }

    pub(super) fn fail_geometry_object_page(
        &mut self,
        key: ObjectRequestKey,
        failure: crate::ObjectPageFailure,
        cx: &mut Context<Self>,
    ) -> Result<(), RangeTextInputError> {
        let pending = self
            .pending_geometry_object
            .take()
            .ok_or(RangeTextInputError::Stale)?;
        if pending.request.key() != key || self.active_geometry != Some(pending.job) {
            self.pending_geometry_object = Some(pending);
            return Err(RangeTextInputError::Stale);
        }
        let _ = self.object_residency.settle(key, failure);
        let release = self.geometry.fail_object_page(pending.job, key)?;
        self.release_geometry(&release, None, Some(key), Some(cx));
        self.active_geometry = None;
        Err(RangeTextInputError::Stale)
    }

    pub(in crate::range_widget) fn deliver_geometry_target_page_inner(
        &mut self,
        page: RangePage,
        credit_spent: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<ResponseDeliveryProgress, RangeTextInputError> {
        let key = page.key();
        let job = self.active_geometry.ok_or(RangeTextInputError::Stale)?;
        if key.purpose() != crate::PagePurpose::GeometryTarget
            || !self.dispatched_pages.contains(&key)
        {
            return Err(RangeTextInputError::Stale);
        }
        let text_admission = match self.residency.prepare_admit(page) {
            Ok(admission) => admission,
            Err(crate::PageAdmissionError::Malformed(_)) => {
                return self.reject_terminal_page_response(
                    job,
                    key,
                    RangeTextInputError::Geometry(crate::ExactGeometryError::SourceContract),
                    cx,
                );
            }
            Err(
                crate::PageAdmissionError::Stale(_)
                | crate::PageAdmissionError::Cancelled(_)
                | crate::PageAdmissionError::Unavailable(_),
            ) => return Err(RangeTextInputError::Stale),
            Err(crate::PageAdmissionError::LimitExceeded(_)) => {
                return self.settle_terminal_page_response(
                    job,
                    key,
                    RangeTextInputError::SurfaceCapacity,
                    cx,
                );
            }
        };
        if !credit_spent {
            if !self.try_spend_realization_credit(cx) {
                self.defer_geometry_response(
                    DeferredGeometryResponse::TargetPage(text_admission.into_page()),
                    cx,
                )?;
                return Ok(ResponseDeliveryProgress::Progressed);
            }
        }
        let geometry = self.geometry.prepare_target_page(
            job,
            text_admission.page(),
            window.text_system(),
            self.target_response_successor()?,
        );
        let geometry = match geometry {
            Ok(geometry) => geometry,
            Err(failure) => {
                self.pending_response_exact_geometry_failure_stage = Some(failure.stage());
                let error = RangeTextInputError::Geometry(failure.error().clone());
                if matches!(failure.error(), crate::ExactGeometryError::SourceContract) {
                    return self.reject_terminal_page_response(job, key, error, cx);
                }
                return self.settle_terminal_page_response(job, key, error, cx);
            }
        };
        if geometry.progress() == ExactGeometryProgress::TargetComplete {
            let preparation = match self.prepare_terminal_response_publication(
                geometry,
                Some(text_admission),
                None,
                None,
                None,
                Some(key),
                None,
            ) {
                Ok(preparation) => preparation,
                Err(RangeTextInputError::SurfaceCapacity) => {
                    return Ok(ResponseDeliveryProgress::RetryableTerminalSurfaceCapacity);
                }
                Err(error) => return Err(error),
            };
            return Ok(
                match self.commit_terminal_response_preparation(preparation, cx) {
                    Ok(()) => ResponseDeliveryProgress::Progressed,
                    Err(error) => ResponseDeliveryProgress::AcceptedTerminal(error),
                },
            );
        }
        let candidate = match self.prepare_nonterminal_response_publication(
            geometry,
            Some(text_admission),
            None,
            None,
            None,
            Some(key),
            None,
            Some(key),
            None,
            None,
        ) {
            Ok(candidate) => candidate,
            Err(error) => return self.settle_terminal_page_response(job, key, error, cx),
        };
        self.commit_nonterminal_response_publication(candidate, cx);
        Ok(
            match self.service_geometry_until_external_boundary(window, cx) {
                Ok(()) => ResponseDeliveryProgress::Progressed,
                Err(error) => ResponseDeliveryProgress::AcceptedTerminal(error),
            },
        )
    }

    pub(in crate::range_widget) fn deliver_geometry_target_object_page_inner(
        &mut self,
        page: crate::ObjectPage,
        credit_spent: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<ResponseDeliveryProgress, RangeTextInputError> {
        let key = page.key();
        let pending = self
            .pending_geometry_object
            .as_ref()
            .ok_or(RangeTextInputError::Stale)?;
        let pending_key = pending.request.key();
        let coalesced = matches!(pending.wait, GeometryObjectWait::Coalesced(wait) if wait == key);
        let detached_job = self.active_geometry != Some(pending.job);
        if coalesced && (pending_key != key || detached_job) {
            if key.purpose() != crate::ObjectPurpose::GeometryTarget
                || !self.dispatched_object_pages.contains(&key)
            {
                return Err(RangeTextInputError::Stale);
            }
            return self.deliver_residency_geometry_object_page(
                page,
                pending.job,
                pending_key,
                detached_job,
                cx,
            );
        }
        if key.purpose() != crate::ObjectPurpose::GeometryTarget
            || pending_key != key
            || self.active_geometry != Some(pending.job)
            || !self.dispatched_object_pages.contains(&key)
        {
            return Err(RangeTextInputError::Stale);
        }
        let job = pending.job;
        let text_page_id = pending.text_page;
        let proofs = match self
            .residency
            .prove_object_page_anchors(self.config.binding, &page)
        {
            Ok(proofs) => proofs,
            Err(_) => {
                let error =
                    RangeTextInputError::Geometry(crate::ExactGeometryError::SourceContract);
                return self.reject_terminal_object_response(job, key, error, cx);
            }
        };
        let object_admission = match self.object_residency.prepare_admit(page, proofs) {
            Ok(admission) => admission,
            Err(crate::ObjectPageAdmissionError::Malformed(_)) => {
                return self.reject_terminal_object_response(
                    job,
                    key,
                    RangeTextInputError::Geometry(crate::ExactGeometryError::SourceContract),
                    cx,
                );
            }
            Err(
                crate::ObjectPageAdmissionError::Stale(_)
                | crate::ObjectPageAdmissionError::Cancelled(_)
                | crate::ObjectPageAdmissionError::Unavailable(_),
            ) => return Err(RangeTextInputError::Stale),
            Err(crate::ObjectPageAdmissionError::LimitExceeded(_)) => {
                return self.settle_terminal_object_response(
                    job,
                    key,
                    RangeTextInputError::SurfaceCapacity,
                    cx,
                );
            }
        };
        if !credit_spent {
            if !self.try_spend_realization_credit(cx) {
                self.defer_geometry_response(
                    DeferredGeometryResponse::TargetObject(object_admission.into_page()),
                    cx,
                )?;
                return Ok(ResponseDeliveryProgress::Progressed);
            }
        }
        let geometry = {
            let text_page = self
                .residency
                .peek_page_by_id(text_page_id)
                .ok_or(RangeTextInputError::Stale)?;
            self.geometry.prepare_target_object_page(
                job,
                text_page,
                object_admission.page(),
                window.text_system(),
                self.target_response_successor()?,
            )
        };
        let geometry = match geometry {
            Ok(geometry) => geometry,
            Err(failure) => {
                self.pending_response_exact_geometry_failure_stage = Some(failure.stage());
                let error = RangeTextInputError::Geometry(failure.error().clone());
                if matches!(
                    failure.error(),
                    crate::ExactGeometryError::Layout(
                        gpui::StreamingLayoutError::CapacityExceeded(_)
                    )
                ) {
                    return self.settle_terminal_object_response(job, key, error, cx);
                }
                if matches!(failure.error(), crate::ExactGeometryError::SourceContract) {
                    return self.reject_terminal_object_response(job, key, error, cx);
                }
                return self.settle_terminal_object_response(job, key, error, cx);
            }
        };
        if geometry.progress() == ExactGeometryProgress::TargetComplete {
            let preparation = match self.prepare_terminal_response_publication(
                geometry,
                None,
                Some(object_admission),
                Some(text_page_id),
                None,
                None,
                Some(key),
            ) {
                Ok(preparation) => preparation,
                Err(RangeTextInputError::SurfaceCapacity) => {
                    return self.settle_terminal_object_response(
                        job,
                        key,
                        RangeTextInputError::SurfaceCapacity,
                        cx,
                    );
                }
                Err(error) => return Err(error),
            };
            return Ok(
                match self.commit_terminal_response_preparation(preparation, cx) {
                    Ok(()) => ResponseDeliveryProgress::Progressed,
                    Err(error) => ResponseDeliveryProgress::AcceptedTerminal(error),
                },
            );
        }
        let candidate = match self.prepare_nonterminal_response_publication(
            geometry,
            None,
            Some(object_admission),
            Some(text_page_id),
            None,
            None,
            Some(key),
            None,
            Some(key),
            None,
        ) {
            Ok(candidate) => candidate,
            Err(error) => return self.settle_terminal_object_response(job, key, error, cx),
        };
        self.commit_nonterminal_response_publication(candidate, cx);
        Ok(
            match self.service_geometry_until_external_boundary(window, cx) {
                Ok(()) => ResponseDeliveryProgress::Progressed,
                Err(error) => ResponseDeliveryProgress::AcceptedTerminal(error),
            },
        )
    }

    fn deliver_residency_geometry_object_page(
        &mut self,
        page: crate::ObjectPage,
        job: crate::GeometryJobKey,
        pending_key: ObjectRequestKey,
        detached_job: bool,
        cx: &mut Context<Self>,
    ) -> Result<ResponseDeliveryProgress, RangeTextInputError> {
        let response_key = page.key();
        let proofs = match self
            .residency
            .prove_object_page_anchors(self.config.binding, &page)
        {
            Ok(proofs) => proofs,
            Err(_) => {
                self.reject_delivered_residency_geometry_object_page(
                    job,
                    pending_key,
                    response_key,
                    detached_job,
                    cx,
                );
                return Ok(ResponseDeliveryProgress::Rejected(
                    RangeTextInputError::Geometry(crate::ExactGeometryError::SourceContract),
                ));
            }
        };
        let admission = match self.object_residency.prepare_admit(page, proofs) {
            Ok(admission) => admission,
            Err(crate::ObjectPageAdmissionError::Malformed(_)) => {
                self.reject_delivered_residency_geometry_object_page(
                    job,
                    pending_key,
                    response_key,
                    detached_job,
                    cx,
                );
                return Ok(ResponseDeliveryProgress::Rejected(
                    RangeTextInputError::Geometry(crate::ExactGeometryError::SourceContract),
                ));
            }
            Err(
                crate::ObjectPageAdmissionError::Stale(_)
                | crate::ObjectPageAdmissionError::Cancelled(_)
                | crate::ObjectPageAdmissionError::Unavailable(_),
            ) => return Err(RangeTextInputError::Stale),
            Err(crate::ObjectPageAdmissionError::LimitExceeded(_)) => {
                let prepared = self.prepare_residency_object_response_failure(
                    job,
                    pending_key,
                    response_key,
                    detached_job,
                    RangeTextInputError::SurfaceCapacity,
                )?;
                let error = self.commit_residency_object_response_failure(prepared, cx);
                return Ok(ResponseDeliveryProgress::AcceptedTerminal(error));
            }
        };
        if self.requests.len() == self.requests.capacity() {
            let prepared = self.prepare_residency_object_response_failure(
                job,
                pending_key,
                response_key,
                detached_job,
                RangeTextInputError::SurfaceCapacity,
            )?;
            let error = self.commit_residency_object_response_failure(prepared, cx);
            return Ok(ResponseDeliveryProgress::AcceptedTerminal(error));
        }
        let charge = admission.page().retained_charge();
        let additional = crate::RangeSurfaceCharge {
            bytes: admission
                .retained_bytes()
                .checked_sub(charge.bytes())
                .ok_or(RangeTextInputError::SurfaceCapacity)?,
            items: admission
                .retained_items()
                .checked_sub(
                    charge
                        .allocated_items()
                        .checked_add(1)
                        .ok_or(RangeTextInputError::SurfaceCapacity)?,
                )
                .ok_or(RangeTextInputError::SurfaceCapacity)?,
        };
        let current = self.current_realization_ownership();
        let peak = crate::RangeSurfaceCharge {
            bytes: current
                .owned_bytes
                .checked_add(additional.bytes)
                .ok_or(RangeTextInputError::SurfaceCapacity)?,
            items: current
                .owned_items
                .checked_add(additional.items)
                .ok_or(RangeTextInputError::SurfaceCapacity)?,
        };
        if peak.bytes > self.config.limits.max_surface_bytes
            || peak.items > self.config.limits.max_surface_items
        {
            let prepared = self.prepare_residency_object_response_failure(
                job,
                pending_key,
                response_key,
                detached_job,
                RangeTextInputError::SurfaceCapacity,
            )?;
            let error = self.commit_residency_object_response_failure(prepared, cx);
            return Ok(ResponseDeliveryProgress::AcceptedTerminal(error));
        }
        self.observe_surface_admission_peak(peak);
        let page_id = admission.page().id();
        self.object_residency.commit_prepared_admit(admission);
        if detached_job {
            self.superseded_geometry_object_responses_settled = self
                .superseded_geometry_object_responses_settled
                .saturating_add(1);
        }
        let matching_pending = self.pending_geometry_object.as_ref().is_some_and(|pending| {
            pending.job == job
                && matches!(pending.wait, GeometryObjectWait::Coalesced(wait) if wait == response_key)
        });
        if matching_pending {
            if detached_job {
                self.pending_geometry_object = None;
            } else if let Some(pending) = self.pending_geometry_object.as_mut() {
                pending.wait = GeometryObjectWait::Resident(page_id);
            }
        }
        assert!(self.dispatched_object_pages.remove(&response_key));
        self.commit_prepared_request(RangeTextInputRequest::ReleaseObjectPage(response_key));
        self.observe_realization_ownership();
        cx.notify();
        Ok(ResponseDeliveryProgress::Progressed)
    }

    fn reject_delivered_residency_geometry_object_page(
        &mut self,
        job: crate::GeometryJobKey,
        pending_key: ObjectRequestKey,
        response_key: ObjectRequestKey,
        detached_job: bool,
        cx: &mut Context<Self>,
    ) {
        let _ = self
            .object_residency
            .settle(response_key, crate::ObjectPageFailure::Malformed);
        if detached_job {
            if self.pending_geometry_object.as_ref().is_some_and(|pending| {
                pending.job == job
                    && matches!(pending.wait, GeometryObjectWait::Coalesced(wait) if wait == response_key)
            }) {
                self.pending_geometry_object = None;
            }
        } else if let Ok(release) = self.geometry.fail_object_page(job, pending_key) {
            self.release_geometry(&release, None, Some(pending_key), Some(cx));
            self.active_geometry = None;
        }
        if self.dispatched_object_pages.remove(&response_key) {
            self.commit_prepared_request(RangeTextInputRequest::ReleaseObjectPage(response_key));
        }
    }

    pub(in crate::range_widget) fn deliver_geometry_page_inner(
        &mut self,
        page: RangePage,
        credit_spent: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<ResponseDeliveryProgress, RangeTextInputError> {
        let key = page.key();
        let job = self.active_geometry.ok_or(RangeTextInputError::Stale)?;
        if key.purpose() != crate::PagePurpose::GeometryIndex
            || !self.dispatched_pages.contains(&key)
        {
            return Err(RangeTextInputError::Stale);
        }
        let text_admission = match self.residency.prepare_admit(page) {
            Ok(admission) => admission,
            Err(crate::PageAdmissionError::Malformed(_)) => {
                return self.reject_terminal_page_response(
                    job,
                    key,
                    RangeTextInputError::Geometry(crate::ExactGeometryError::SourceContract),
                    cx,
                );
            }
            Err(
                crate::PageAdmissionError::Stale(_)
                | crate::PageAdmissionError::Cancelled(_)
                | crate::PageAdmissionError::Unavailable(_),
            ) => return Err(RangeTextInputError::Stale),
            Err(crate::PageAdmissionError::LimitExceeded(_)) => {
                return self.settle_terminal_page_response(
                    job,
                    key,
                    RangeTextInputError::SurfaceCapacity,
                    cx,
                );
            }
        };
        if !credit_spent {
            if !self.try_spend_realization_credit(cx) {
                self.defer_geometry_response(
                    DeferredGeometryResponse::IndexPage(text_admission.into_page()),
                    cx,
                )?;
                return Ok(ResponseDeliveryProgress::Progressed);
            }
        }
        let geometry = self.geometry.prepare_index_page(
            job,
            text_admission.page(),
            window.text_system(),
            self.index_response_successor()?,
        );
        let geometry = match geometry {
            Ok(geometry) => geometry,
            Err(failure) => {
                self.pending_response_exact_geometry_failure_stage = Some(failure.stage());
                let error = RangeTextInputError::Geometry(failure.error().clone());
                if matches!(failure.error(), crate::ExactGeometryError::SourceContract) {
                    return self.reject_terminal_page_response(job, key, error, cx);
                }
                return self.settle_terminal_page_response(job, key, error, cx);
            }
        };
        if geometry.progress() == ExactGeometryProgress::TargetComplete {
            let preparation = match self.prepare_terminal_response_publication(
                geometry,
                Some(text_admission),
                None,
                None,
                None,
                Some(key),
                None,
            ) {
                Ok(preparation) => preparation,
                Err(RangeTextInputError::SurfaceCapacity) => {
                    return Ok(ResponseDeliveryProgress::RetryableTerminalSurfaceCapacity);
                }
                Err(error) => return Err(error),
            };
            return Ok(
                match self.commit_terminal_response_preparation(preparation, cx) {
                    Ok(()) => ResponseDeliveryProgress::Progressed,
                    Err(error) => ResponseDeliveryProgress::AcceptedTerminal(error),
                },
            );
        }
        let index_target = geometry
            .terminal_index()
            .map(|_| self.prepare_index_response_target(&geometry))
            .transpose()?;
        let candidate = match self.prepare_nonterminal_response_publication(
            geometry,
            Some(text_admission),
            None,
            None,
            None,
            Some(key),
            None,
            Some(key),
            None,
            index_target,
        ) {
            Ok(candidate) => candidate,
            Err(error) => return self.settle_terminal_page_response(job, key, error, cx),
        };
        self.commit_nonterminal_response_publication(candidate, cx);
        Ok(
            match self.service_geometry_until_external_boundary(window, cx) {
                Ok(()) => ResponseDeliveryProgress::Progressed,
                Err(error) => ResponseDeliveryProgress::AcceptedTerminal(error),
            },
        )
    }

    pub(super) fn retire_surface_candidate(&mut self) {
        let Some(candidate) = self.surface_candidate.take() else {
            return;
        };
        if self.active_geometry == Some(candidate.job) {
            self.active_geometry = None;
        }
        if let Ok(release) = self.geometry.cancel(candidate.job) {
            self.release_geometry(&release, None, None, None);
        }
    }

    pub(super) fn reject_restoration_geometry(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Result<bool, RangeTextInputError> {
        if self.restoration_seed.is_none() {
            return Ok(false);
        }
        let release = self
            .geometry
            .rebind(self.config.binding, self.config.presentation_generation)?;
        self.release_geometry(&release, None, None, Some(cx));
        Ok(self.reject_restoration_after_geometry_change(cx))
    }

    pub(super) fn reject_restoration_after_geometry_change(
        &mut self,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.restoration_seed.is_none() {
            return false;
        }
        for key in self
            .object_residency
            .rebind(self.config.binding, self.config.presentation_generation)
        {
            self.cancel_object_page_dispatch(key);
        }
        self.active_geometry = None;
        self.pending_geometry_page = None;
        self.pending_geometry_object = None;
        self.surface_candidate = None;
        self.restoration_seed = None;
        self.published_restoration = None;
        let viewport_extent = self.desired.viewport_extent;
        self.desired = DesiredSurface::origin(
            viewport_extent,
            super::bounded_realization_extent(
                viewport_extent,
                self.config.limits.max_realized_block_extent,
            ),
            self.config.overscan,
        );
        cx.emit(RangeTextInputEvent::RestorationRejected);
        cx.notify();
        true
    }

    pub(super) fn release_geometry(
        &mut self,
        release: &crate::ExactGeometryRelease,
        completed_page: Option<crate::PageRequestKey>,
        completed_object_page: Option<crate::ObjectRequestKey>,
        mut cx: Option<&mut Context<Self>>,
    ) {
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
        if self
            .surface_candidate
            .as_ref()
            .is_some_and(|candidate| release.jobs.contains(&candidate.job))
        {
            self.surface_candidate = None;
        }
        for page in &release.pages {
            if Some(*page) == completed_page {
                continue;
            }
            let _ = self.residency.cancel(*page);
            self.cancel_page_dispatch(*page);
        }
        for page in &release.object_pages {
            if Some(*page) == completed_object_page {
                continue;
            }
            let _ = self.object_residency.cancel(*page);
            self.cancel_object_page_dispatch(*page);
        }
        if let Some(cx) = cx.as_mut() {
            cx.notify();
        }
    }

    pub(super) fn cancel_page_dispatch(&mut self, key: crate::PageRequestKey) {
        self.retire_page_response_custody(key);
        if self
            .deferred_geometry_response
            .as_ref()
            .and_then(DeferredGeometryResponse::page_key)
            == Some(key)
        {
            self.deferred_geometry_response = None;
        }
        if let Some(index) = self.requests.iter().position(
            |request| matches!(request, RangeTextInputRequest::Page(page) if page.key() == key),
        ) {
            self.requests.remove(index);
        } else if self.dispatched_pages.remove(&key) {
            self.requests
                .push_back(RangeTextInputRequest::CancelPage(key));
        }
    }
}
