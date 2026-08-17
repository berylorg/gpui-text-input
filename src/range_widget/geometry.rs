mod response_commit;
mod response_preparation;

use std::{collections::VecDeque, mem::size_of};

use gpui::{Context, Window};

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
    requests: VecDeque<RangeTextInputRequest>,
    release_request: Option<RangeTextInputRequest>,
    completed_page: Option<PageRequestKey>,
    completed_object_page: Option<ObjectRequestKey>,
    admission_charge: crate::RangeSurfaceCharge,
    next_id: Option<u64>,
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct TerminalResponseItemComponents {
    pub(super) prior_surface: usize,
    pub(super) current_request_storage: usize,
    pub(super) candidate_record: usize,
    pub(super) geometry: usize,
    pub(super) page_admission_allocation: usize,
    pub(super) resident_payload: usize,
    pub(super) publication_allocation: usize,
    pub(super) destination_request_storage: usize,
    pub(super) release_records: usize,
    pub(super) deferred_events: usize,
}

#[cfg(test)]
impl TerminalResponseItemComponents {
    pub(super) fn checked_total(self) -> Option<usize> {
        [
            self.prior_surface,
            self.current_request_storage,
            self.candidate_record,
            self.geometry,
            self.page_admission_allocation,
            self.resident_payload,
            self.publication_allocation,
            self.destination_request_storage,
            self.release_records,
            self.deferred_events,
        ]
        .into_iter()
        .try_fold(0usize, usize::checked_add)
    }
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
    requests: VecDeque<RangeTextInputRequest>,
    effects: [Option<RangeTextInputRequest>; 2],
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
        self.next_id
            .checked_add(1)
            .ok_or(RangeTextInputError::Busy)?;
        Ok(crate::range_geometry::TargetResponseSuccessor {
            target_job_id: crate::GeometryJobId::new(self.next_id),
            page_id: PageRequestId::new(self.next_id),
            object_id: ObjectRequestId::new(self.next_id),
            max_objects: self.config.object_residency_limits.max_resident_objects(),
            max_object_bytes: self.config.object_residency_limits.max_resident_bytes(),
            target: self.desired.target(),
            anchor: None,
            select_all: false,
        })
    }

    fn index_response_successor(
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
        Ok(crate::range_geometry::TargetResponseSuccessor {
            target_job_id: crate::GeometryJobId::new(self.next_id),
            page_id: PageRequestId::new(page_id),
            object_id: ObjectRequestId::new(page_id),
            max_objects: self.config.object_residency_limits.max_resident_objects(),
            max_object_bytes: self.config.object_residency_limits.max_resident_bytes(),
            target: self.desired.target(),
            anchor: restoration.map(|seed| seed.scroll.position),
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

    pub(super) fn start_target(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Result<(), RangeTextInputError> {
        let desired = self.desired;
        if self.reject_restoration_task(cx)? {
            self.desired = desired;
            return self.start_index();
        }
        self.start_target_for(Some(cx))
    }

    fn start_target_for(
        &mut self,
        cx: Option<&mut Context<Self>>,
    ) -> Result<(), RangeTextInputError> {
        let candidate = self.prepare_target_transition(self.desired, None)?;
        match self.commit_widget_transition(candidate, cx) {
            ExactGeometryProgress::TargetComplete => Ok(()),
            ExactGeometryProgress::Scanning | ExactGeometryProgress::PendingIndex => Ok(()),
            _ => Err(RangeTextInputError::Stale),
        }
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
        let result = self
            .service_geometry_object(window, cx)
            .and_then(|()| self.service_geometry_page_inner(window, cx));
        if result.is_err() {
            self.reject_restoration_geometry(cx)?;
        }
        result
    }

    pub(super) fn service_geometry_until_external_boundary(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), RangeTextInputError> {
        let limit = self
            .config
            .residency_limits
            .max_resident_pages()
            .saturating_add(self.config.object_residency_limits.max_resident_objects())
            .saturating_add(2);
        for _ in 0..limit {
            let before = self.geometry_service_marker();
            self.service_geometry_page(window, cx)?;
            if self.geometry_service_marker() == before {
                return Ok(());
            }
        }
        Err(RangeTextInputError::Busy)
    }

    pub(super) fn service_admitted_geometry_for_prepaint(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), RangeTextInputError> {
        if !self.requests.is_empty() {
            return Ok(());
        }
        let limit = self
            .config
            .residency_limits
            .max_resident_pages()
            .saturating_add(self.config.object_residency_limits.max_resident_objects())
            .saturating_add(2);
        for _ in 0..limit {
            if !self.requests.is_empty() {
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
                return Ok(());
            };
            result.expect("resident geometry wait was present")?;
            if self.geometry_service_marker() == before {
                cx.defer_in(window, |input, window, cx| {
                    let _ = input.service_geometry_until_external_boundary(window, cx);
                });
                return Ok(());
            }
        }
        Ok(())
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
            let candidate = match self.prepare_terminal_response_publication(
                geometry,
                None,
                None,
                Some(text_page_id),
                Some(object_page_id),
                None,
                Some(key),
                None,
            ) {
                Ok(candidate) => candidate,
                Err(error) => return Some(Err(error)),
            };
            self.commit_terminal_response_publication(candidate, cx);
            return Some(Ok(()));
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
        let index_target = match geometry
            .terminal_index()
            .map(|_| self.prepare_index_response_target(&geometry))
            .transpose()
        {
            Ok(index_target) => index_target,
            Err(error) => return Some(Err(error)),
        };
        if geometry.progress() == ExactGeometryProgress::TargetComplete {
            let candidate = match self.prepare_terminal_response_publication(
                geometry,
                None,
                None,
                Some(text_page_id),
                Some(object_page_id),
                None,
                Some(key),
                index_target,
            ) {
                Ok(candidate) => candidate,
                Err(error) => return Some(Err(error)),
            };
            self.commit_terminal_response_publication(candidate, cx);
            return Some(Ok(()));
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

    pub(super) fn deliver_geometry_object_page(
        &mut self,
        page: crate::ObjectPage,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), RangeTextInputError> {
        let key = page.key();
        let pending = self
            .pending_geometry_object
            .as_ref()
            .ok_or(RangeTextInputError::Stale)?;
        if key.purpose() != crate::ObjectPurpose::GeometryIndex
            || pending.request.key() != key
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
                self.reject_delivered_geometry_object_page(key, cx);
                return Err(error);
            }
        };
        let object_admission = match self.object_residency.prepare_admit(page, proofs) {
            Ok(admission) => admission,
            Err(crate::ObjectPageAdmissionError::Malformed(_)) => {
                self.reject_delivered_geometry_object_page(key, cx);
                return Err(RangeTextInputError::Geometry(
                    crate::ExactGeometryError::SourceContract,
                ));
            }
            Err(
                crate::ObjectPageAdmissionError::Stale(_)
                | crate::ObjectPageAdmissionError::Cancelled(_)
                | crate::ObjectPageAdmissionError::Unavailable(_)
                | crate::ObjectPageAdmissionError::LimitExceeded(_),
            ) => return Err(RangeTextInputError::Stale),
        };
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
                let error = RangeTextInputError::Geometry(failure.error().clone());
                if matches!(failure.error(), crate::ExactGeometryError::SourceContract) {
                    self.reject_delivered_geometry_object_page(key, cx);
                }
                return Err(error);
            }
        };
        let index_target = geometry
            .terminal_index()
            .map(|_| self.prepare_index_response_target(&geometry))
            .transpose()?;
        if geometry.progress() == ExactGeometryProgress::TargetComplete {
            let candidate = self.prepare_terminal_response_publication(
                geometry,
                None,
                Some(object_admission),
                Some(text_page_id),
                None,
                None,
                Some(key),
                index_target,
            )?;
            self.commit_terminal_response_publication(candidate, cx);
            return Ok(());
        }
        let candidate = self.prepare_nonterminal_response_publication(
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
        )?;
        self.commit_nonterminal_response_publication(candidate, cx);
        self.service_geometry_until_external_boundary(window, cx)
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

    fn reject_delivered_geometry_object_page(
        &mut self,
        key: ObjectRequestKey,
        cx: &mut Context<Self>,
    ) {
        let _ = self.fail_geometry_object_page(key, crate::ObjectPageFailure::Malformed, cx);
        if self.dispatched_object_pages.remove(&key) {
            self.requests
                .push_back(RangeTextInputRequest::ReleaseObjectPage(key));
        }
    }

    fn reject_delivered_geometry_page(&mut self, key: PageRequestKey, cx: &mut Context<Self>) {
        let _ = self.residency.settle(key, crate::PageFailure::Malformed);
        if let Some(job) = self.active_geometry
            && let Ok(release) = self.geometry.fail_page(job, key)
        {
            self.release_geometry(&release, Some(key), None, Some(cx));
            self.active_geometry = None;
        }
        if self.dispatched_pages.remove(&key) {
            self.requests
                .push_back(RangeTextInputRequest::ReleasePage(key));
        }
    }

    pub(super) fn deliver_geometry_page(
        &mut self,
        page: RangePage,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), RangeTextInputError> {
        self.deliver_geometry_page_inner(page, window, cx)
    }

    pub(super) fn deliver_geometry_target_page(
        &mut self,
        page: RangePage,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), RangeTextInputError> {
        let key = page.key();
        let job = self.active_geometry.ok_or(RangeTextInputError::Stale)?;
        if key.purpose() != crate::PagePurpose::GeometryTarget
            || !self.dispatched_pages.contains(&key)
        {
            return Err(RangeTextInputError::Stale);
        }
        let geometry = self.geometry.prepare_target_page(
            job,
            &page,
            window.text_system(),
            self.target_response_successor()?,
        );
        let geometry = match geometry {
            Ok(geometry) => geometry,
            Err(failure) => {
                let error = RangeTextInputError::Geometry(failure.error().clone());
                if matches!(failure.error(), crate::ExactGeometryError::SourceContract) {
                    self.reject_delivered_geometry_page(key, cx);
                }
                return Err(error);
            }
        };
        let text_admission = match self.residency.prepare_admit(page) {
            Ok(admission) => admission,
            Err(crate::PageAdmissionError::Malformed(_)) => {
                self.reject_delivered_geometry_page(key, cx);
                return Err(RangeTextInputError::Geometry(
                    crate::ExactGeometryError::SourceContract,
                ));
            }
            Err(
                crate::PageAdmissionError::Stale(_)
                | crate::PageAdmissionError::Cancelled(_)
                | crate::PageAdmissionError::Unavailable(_)
                | crate::PageAdmissionError::LimitExceeded(_),
            ) => return Err(RangeTextInputError::Stale),
        };
        if geometry.progress() == ExactGeometryProgress::TargetComplete {
            let candidate = self.prepare_terminal_response_publication(
                geometry,
                Some(text_admission),
                None,
                None,
                None,
                Some(key),
                None,
                None,
            )?;
            self.commit_terminal_response_publication(candidate, cx);
            return Ok(());
        }
        let candidate = self.prepare_nonterminal_response_publication(
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
        )?;
        self.commit_nonterminal_response_publication(candidate, cx);
        self.service_geometry_until_external_boundary(window, cx)
    }

    pub(super) fn deliver_geometry_target_object_page(
        &mut self,
        page: crate::ObjectPage,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), RangeTextInputError> {
        let key = page.key();
        let pending = self
            .pending_geometry_object
            .as_ref()
            .ok_or(RangeTextInputError::Stale)?;
        if key.purpose() != crate::ObjectPurpose::GeometryTarget
            || pending.request.key() != key
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
                self.reject_delivered_geometry_object_page(key, cx);
                return Err(error);
            }
        };
        let object_admission = match self.object_residency.prepare_admit(page, proofs) {
            Ok(admission) => admission,
            Err(crate::ObjectPageAdmissionError::Malformed(_)) => {
                self.reject_delivered_geometry_object_page(key, cx);
                return Err(RangeTextInputError::Geometry(
                    crate::ExactGeometryError::SourceContract,
                ));
            }
            Err(
                crate::ObjectPageAdmissionError::Stale(_)
                | crate::ObjectPageAdmissionError::Cancelled(_)
                | crate::ObjectPageAdmissionError::Unavailable(_)
                | crate::ObjectPageAdmissionError::LimitExceeded(_),
            ) => return Err(RangeTextInputError::Stale),
        };
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
                let error = RangeTextInputError::Geometry(failure.error().clone());
                if matches!(failure.error(), crate::ExactGeometryError::SourceContract) {
                    self.reject_delivered_geometry_object_page(key, cx);
                }
                return Err(error);
            }
        };
        if geometry.progress() == ExactGeometryProgress::TargetComplete {
            let candidate = self.prepare_terminal_response_publication(
                geometry,
                None,
                Some(object_admission),
                Some(text_page_id),
                None,
                None,
                Some(key),
                None,
            )?;
            self.commit_terminal_response_publication(candidate, cx);
            return Ok(());
        }
        let candidate = self.prepare_nonterminal_response_publication(
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
        )?;
        self.commit_nonterminal_response_publication(candidate, cx);
        self.service_geometry_until_external_boundary(window, cx)
    }

    fn deliver_geometry_page_inner(
        &mut self,
        page: RangePage,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), RangeTextInputError> {
        let key = page.key();
        let job = self.active_geometry.ok_or(RangeTextInputError::Stale)?;
        if key.purpose() != crate::PagePurpose::GeometryIndex
            || !self.dispatched_pages.contains(&key)
        {
            return Err(RangeTextInputError::Stale);
        }
        let geometry = self.geometry.prepare_index_page(
            job,
            &page,
            window.text_system(),
            self.index_response_successor()?,
        );
        let geometry = match geometry {
            Ok(geometry) => geometry,
            Err(failure) => {
                let error = RangeTextInputError::Geometry(failure.error().clone());
                if matches!(failure.error(), crate::ExactGeometryError::SourceContract) {
                    self.reject_delivered_geometry_page(key, cx);
                }
                return Err(error);
            }
        };
        let text_admission = match self.residency.prepare_admit(page) {
            Ok(admission) => admission,
            Err(crate::PageAdmissionError::Malformed(_)) => {
                self.reject_delivered_geometry_page(key, cx);
                return Err(RangeTextInputError::Geometry(
                    crate::ExactGeometryError::SourceContract,
                ));
            }
            Err(
                crate::PageAdmissionError::Stale(_)
                | crate::PageAdmissionError::Cancelled(_)
                | crate::PageAdmissionError::Unavailable(_)
                | crate::PageAdmissionError::LimitExceeded(_),
            ) => return Err(RangeTextInputError::Stale),
        };
        let index_target = geometry
            .terminal_index()
            .map(|_| self.prepare_index_response_target(&geometry))
            .transpose()?;
        if geometry.progress() == ExactGeometryProgress::TargetComplete {
            let candidate = self.prepare_terminal_response_publication(
                geometry,
                Some(text_admission),
                None,
                None,
                None,
                Some(key),
                None,
                index_target,
            )?;
            self.commit_terminal_response_publication(candidate, cx);
            return Ok(());
        }
        let candidate = self.prepare_nonterminal_response_publication(
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
        )?;
        self.commit_nonterminal_response_publication(candidate, cx);
        self.service_geometry_until_external_boundary(window, cx)
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
        self.desired = DesiredSurface::origin(self.config.viewport_extent, self.config.overscan);
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
