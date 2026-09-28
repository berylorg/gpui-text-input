use super::super::*;
use crate::range_geometry::{
    PreparedTargetResponse, PreparedTargetSuccessor, TargetResponseSuccessor,
};

impl RangePrepublicationSession {
    pub(super) fn advance_geometry(
        &mut self,
        _text_system: &WindowTextSystem,
        effects: &mut EffectBuffer,
    ) -> Result<bool, RangePrepublicationFailure> {
        if matches!(self.stage, SessionStage::Target)
            && self
                .geometry
                .as_ref()
                .is_some_and(|geometry| geometry.target().is_some())
        {
            self.finish_candidate()?;
            return Ok(!self.ledger_blocked);
        }
        let job = self.geometry_job.ok_or(RangePrepublicationFailure::Stale)?;
        let successor = self
            .geometry
            .as_ref()
            .ok_or(RangePrepublicationFailure::Stale)?
            .pending_response_successor(job)
            .map_err(classify_geometry_error)?;
        let pending_exists = successor.is_some();
        if let Some(text_page) = self
            .geometry
            .as_ref()
            .and_then(|geometry| geometry.active_text_page(job))
        {
            let request_id = match successor {
                Some(PreparedTargetSuccessor::Object { request, .. }) => request.key().id(),
                _ => ObjectRequestId::new(self.next_id()?),
            };
            let request = if let Some(PreparedTargetSuccessor::Object { request, .. }) = successor {
                request
            } else {
                self.geometry
                    .as_ref()
                    .ok_or(RangePrepublicationFailure::Stale)?
                    .preview_object_page_request(
                        job,
                        request_id,
                        self.environment
                            .config()
                            .object_residency_limits
                            .max_resident_objects(),
                        self.environment
                            .config()
                            .object_residency_limits
                            .max_resident_bytes(),
                    )
                    .map_err(classify_geometry_error)?
            };
            let prepared = self
                .object_residency
                .prepare_demand_after_retirement_from(
                    request.key().id(),
                    request.key().purpose(),
                    request.key().demand(),
                    &[],
                    self.object_residency.resident_page_iter(),
                )
                .map_err(|_| RangePrepublicationFailure::TerminalCapacity)?;
            match prepared.outcome() {
                ObjectDemand::Resident(page) => {
                    let Some(capacity) = self.admit_resident_geometry_request(
                        std::mem::size_of::<crate::ObjectRequestKey>(),
                        pending_exists,
                        RangeSurfaceCharge {
                            bytes: prepared.retained_bytes(),
                            items: prepared.retained_items(),
                        },
                        effects,
                    )?
                    else {
                        return Ok(false);
                    };
                    let committed = if pending_exists {
                        request
                    } else {
                        self.geometry
                            .as_mut()
                            .ok_or(RangePrepublicationFailure::Stale)?
                            .request_object_page_with_capacity(
                                job,
                                request_id,
                                self.environment
                                    .config()
                                    .object_residency_limits
                                    .max_resident_objects(),
                                self.environment
                                    .config()
                                    .object_residency_limits
                                    .max_resident_bytes(),
                                capacity.bytes,
                                capacity.items,
                            )
                            .map_err(classify_geometry_error)?
                    };
                    if committed != request
                        || self.object_residency.commit_prepared_demand(prepared)
                            != ObjectDemand::Resident(page)
                    {
                        return Err(RangePrepublicationFailure::Stale);
                    }
                    self.retain_geometry_response(GeometryResponseInput::Object {
                        job,
                        text_page,
                        page,
                        resident: true,
                    })?;
                    Ok(true)
                }
                ObjectDemand::Requested(resident_request) => {
                    if resident_request.key() != request.key() {
                        return Err(RangePrepublicationFailure::Stale);
                    }
                    self.admit_geometry_object_request(
                        job,
                        text_page,
                        request_id,
                        request,
                        pending_exists,
                        prepared,
                        resident_request,
                        effects,
                    )
                }
                ObjectDemand::Coalesced(_) => Err(RangePrepublicationFailure::Stale),
            }
        } else {
            let request_id = match successor {
                Some(PreparedTargetSuccessor::Page(request)) => request.key().id(),
                _ => PageRequestId::new(self.next_id()?),
            };
            let request = if let Some(PreparedTargetSuccessor::Page(request)) = successor {
                request
            } else {
                self.geometry
                    .as_ref()
                    .ok_or(RangePrepublicationFailure::Stale)?
                    .preview_page_request(job, request_id)
                    .map_err(classify_geometry_error)?
            };
            let prepared = self
                .residency
                .prepare_demand_after_retirement(
                    request.key().id(),
                    request.key().purpose(),
                    request.key().demand(),
                    &[],
                )
                .map_err(|_| RangePrepublicationFailure::TerminalCapacity)?;
            match prepared.outcome() {
                PageDemand::ResidentAdjacent(page) => {
                    let Some(capacity) = self.admit_resident_geometry_request(
                        std::mem::size_of::<crate::PageRequestKey>(),
                        pending_exists,
                        RangeSurfaceCharge {
                            bytes: prepared.retained_bytes(),
                            items: prepared.retained_items(),
                        },
                        effects,
                    )?
                    else {
                        return Ok(false);
                    };
                    let committed = if pending_exists {
                        request
                    } else {
                        self.geometry
                            .as_mut()
                            .ok_or(RangePrepublicationFailure::Stale)?
                            .request_page_with_capacity(
                                job,
                                request_id,
                                capacity.bytes,
                                capacity.items,
                            )
                            .map_err(classify_geometry_error)?
                    };
                    if committed != request
                        || self.residency.commit_prepared_demand(prepared)
                            != PageDemand::ResidentAdjacent(page)
                    {
                        return Err(RangePrepublicationFailure::Stale);
                    }
                    self.retain_geometry_response(GeometryResponseInput::Page {
                        job,
                        page,
                        resident: true,
                    })?;
                    Ok(true)
                }
                PageDemand::Requested(resident_request) => {
                    if resident_request.key() != request.key() {
                        return Err(RangePrepublicationFailure::Stale);
                    }
                    self.admit_geometry_page_request(
                        job,
                        request_id,
                        request,
                        pending_exists,
                        prepared,
                        resident_request,
                        effects,
                    )
                }
                PageDemand::ResidentValidation { .. } | PageDemand::Coalesced(_) => {
                    Err(RangePrepublicationFailure::Stale)
                }
            }
        }
    }

    pub(super) fn advance_admitted_geometry(
        &mut self,
        text_system: &WindowTextSystem,
    ) -> Result<bool, RangePrepublicationFailure> {
        let charge = self
            .current_charge()
            .ok_or(RangePrepublicationFailure::Arithmetic)?;
        if !charge_fits(charge, configured_capacity(self.environment.config())) {
            return Err(RangePrepublicationFailure::TerminalCapacity);
        }
        if !charge_fits(charge, self.available) {
            return Ok(false);
        }
        match self
            .admitted_geometry
            .ok_or(RangePrepublicationFailure::Stale)?
            .input
        {
            GeometryResponseInput::Page {
                job,
                page,
                resident,
            } => {
                self.process_geometry_page(job, page, text_system, resident)?;
            }
            GeometryResponseInput::Object {
                job,
                text_page,
                page,
                resident,
            } => {
                self.process_geometry_object(job, text_page, page, text_system, resident)?;
            }
        }
        self.admitted_geometry = None;
        self.waiting = None;
        Ok(true)
    }

    pub(in crate::range_widget::prepublication::session) fn process_geometry_page(
        &mut self,
        job: GeometryJobKey,
        page_id: PageId,
        text_system: &WindowTextSystem,
        resident: bool,
    ) -> Result<(), RangePrepublicationFailure> {
        let successor = self.geometry_response_successor()?;
        let page = self
            .residency
            .peek_page_by_id(page_id)
            .ok_or(RangePrepublicationFailure::Stale)?;
        let geometry = self
            .geometry
            .as_ref()
            .ok_or(RangePrepublicationFailure::Stale)?;
        let prepared = match (matches!(self.stage, SessionStage::Index), resident) {
            (true, true) => geometry.prepare_index_resident_page(job, page, text_system, successor),
            (true, false) => geometry.prepare_index_page(job, page, text_system, successor),
            (false, true) => {
                geometry.prepare_target_resident_page(job, page, text_system, successor)
            }
            (false, false) => geometry.prepare_target_page(job, page, text_system, successor),
        }
        .map_err(|failure| classify_geometry_error(failure.error().clone()))?;
        self.commit_geometry_response(prepared)
    }

    pub(in crate::range_widget::prepublication::session) fn process_geometry_object(
        &mut self,
        job: GeometryJobKey,
        text_page: PageId,
        object_page: ObjectPageId,
        text_system: &WindowTextSystem,
        resident: bool,
    ) -> Result<(), RangePrepublicationFailure> {
        let successor = self.geometry_response_successor()?;
        let text_page = self
            .residency
            .peek_page_by_id(text_page)
            .ok_or(RangePrepublicationFailure::Stale)?;
        let object_page = self
            .object_residency
            .peek_page_by_id(object_page)
            .ok_or(RangePrepublicationFailure::Stale)?;
        let geometry = self
            .geometry
            .as_ref()
            .ok_or(RangePrepublicationFailure::Stale)?;
        let prepared = match (matches!(self.stage, SessionStage::Index), resident) {
            (true, true) => geometry.prepare_index_resident_object_page(
                job,
                text_page,
                object_page,
                text_system,
                successor,
            ),
            (true, false) => geometry.prepare_index_object_page(
                job,
                text_page,
                object_page,
                text_system,
                successor,
            ),
            (false, true) => geometry.prepare_target_resident_object_page(
                job,
                text_page,
                object_page,
                text_system,
                successor,
            ),
            (false, false) => geometry.prepare_target_object_page(
                job,
                text_page,
                object_page,
                text_system,
                successor,
            ),
        }
        .map_err(|failure| classify_geometry_error(failure.error().clone()))?;
        self.commit_geometry_response(prepared)
    }

    fn geometry_response_successor(
        &self,
    ) -> Result<TargetResponseSuccessor, RangePrepublicationFailure> {
        let [job, page, object] = self
            .admitted_geometry
            .ok_or(RangePrepublicationFailure::Stale)?
            .successor_ids;
        Ok(TargetResponseSuccessor {
            target_job_id: GeometryJobId::new(job),
            page_id: PageRequestId::new(page),
            object_id: ObjectRequestId::new(object),
            max_objects: self
                .environment
                .config()
                .object_residency_limits
                .max_resident_objects(),
            max_object_bytes: self
                .environment
                .config()
                .object_residency_limits
                .max_resident_bytes(),
            target: BlockTarget::new(
                Pixels::ZERO,
                self.environment.config().viewport_extent,
                self.environment.config().overscan,
            ),
            anchor: Some(crate::range_widget::restoration::restoration_layout_anchor(
                self.seed,
            )),
            select_all: false,
        })
    }

    fn commit_geometry_response(
        &mut self,
        prepared: PreparedTargetResponse,
    ) -> Result<(), RangePrepublicationFailure> {
        let index_complete = prepared.terminal_index().is_some();
        let key = prepared.key();
        let progress = self
            .geometry
            .as_mut()
            .ok_or(RangePrepublicationFailure::Stale)?
            .commit_prepared_target_response(prepared)
            .progress();
        self.geometry_job = Some(key);
        if index_complete {
            self.release_all_resident_custody();
            drop(self.residency.take_resident_pages());
            drop(self.object_residency.take_resident_pages());
            self.stage = SessionStage::Target;
        }
        match progress {
            ExactGeometryProgress::Scanning | ExactGeometryProgress::NeedObjects => Ok(()),
            ExactGeometryProgress::TargetComplete => self.finish_candidate(),
            ExactGeometryProgress::PendingIndex | ExactGeometryProgress::IndexComplete => {
                Err(RangePrepublicationFailure::DeterministicGeometry)
            }
        }
    }
}
