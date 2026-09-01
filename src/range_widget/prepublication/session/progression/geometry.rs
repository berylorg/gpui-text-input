use super::super::*;

impl RangePrepublicationSession {
    pub(super) fn advance_geometry(
        &mut self,
        text_system: &WindowTextSystem,
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
        if let Some(text_page) = self
            .geometry
            .as_ref()
            .and_then(|geometry| geometry.active_text_page(job))
        {
            let request_id = ObjectRequestId::new(self.next_id()?);
            let request = self
                .geometry
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
                .map_err(classify_geometry_error)?;
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
                    let committed = self
                        .geometry
                        .as_mut()
                        .ok_or(RangePrepublicationFailure::Stale)?
                        .request_object_page(
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
                        .map_err(classify_geometry_error)?;
                    if committed != request
                        || self.object_residency.commit_prepared_demand(prepared)
                            != ObjectDemand::Resident(page)
                    {
                        return Err(RangePrepublicationFailure::Stale);
                    }
                    self.process_geometry_object(job, text_page, page, text_system)?;
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
                        prepared,
                        resident_request,
                        effects,
                    )
                }
                ObjectDemand::Coalesced(_) => Err(RangePrepublicationFailure::Stale),
            }
        } else {
            let request_id = PageRequestId::new(self.next_id()?);
            let request = self
                .geometry
                .as_ref()
                .ok_or(RangePrepublicationFailure::Stale)?
                .preview_page_request(job, request_id)
                .map_err(classify_geometry_error)?;
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
                    let committed = self
                        .geometry
                        .as_mut()
                        .ok_or(RangePrepublicationFailure::Stale)?
                        .request_page(job, request_id)
                        .map_err(classify_geometry_error)?;
                    if committed != request
                        || self.residency.commit_prepared_demand(prepared)
                            != PageDemand::ResidentAdjacent(page)
                    {
                        return Err(RangePrepublicationFailure::Stale);
                    }
                    self.process_geometry_page(job, page, text_system)?;
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

    pub(in crate::range_widget::prepublication::session) fn process_geometry_page(
        &mut self,
        job: GeometryJobKey,
        page_id: PageId,
        text_system: &WindowTextSystem,
    ) -> Result<(), RangePrepublicationFailure> {
        let page = self
            .residency
            .peek_page_by_id(page_id)
            .ok_or(RangePrepublicationFailure::Stale)?;
        let admission = self
            .geometry
            .as_mut()
            .ok_or(RangePrepublicationFailure::Stale)?
            .admit_page(job, page, text_system)
            .map_err(|failure| classify_geometry_error(failure.error().clone()))?;
        self.apply_geometry_progress(admission.progress())
    }

    pub(in crate::range_widget::prepublication::session) fn process_geometry_object(
        &mut self,
        job: GeometryJobKey,
        text_page: PageId,
        object_page: ObjectPageId,
        text_system: &WindowTextSystem,
    ) -> Result<(), RangePrepublicationFailure> {
        let text_page = self
            .residency
            .peek_page_by_id(text_page)
            .ok_or(RangePrepublicationFailure::Stale)?;
        let object_page = self
            .object_residency
            .peek_page_by_id(object_page)
            .ok_or(RangePrepublicationFailure::Stale)?;
        let admission = self
            .geometry
            .as_mut()
            .ok_or(RangePrepublicationFailure::Stale)?
            .admit_object_page(job, text_page, object_page, text_system)
            .map_err(|failure| classify_geometry_error(failure.error().clone()))?;
        self.apply_geometry_progress(admission.progress())
    }

    fn apply_geometry_progress(
        &mut self,
        progress: ExactGeometryProgress,
    ) -> Result<(), RangePrepublicationFailure> {
        match progress {
            ExactGeometryProgress::Scanning | ExactGeometryProgress::NeedObjects => Ok(()),
            ExactGeometryProgress::IndexComplete => {
                self.release_all_resident_custody();
                drop(self.residency.take_resident_pages());
                drop(self.object_residency.take_resident_pages());
                let id = GeometryJobId::new(self.next_id()?);
                let target = BlockTarget::new(
                    Pixels::ZERO,
                    self.environment.config().viewport_extent,
                    self.environment.config().overscan,
                );
                let start = self
                    .geometry
                    .as_mut()
                    .ok_or(RangePrepublicationFailure::Stale)?
                    .request_block_target_anchored(id, target, self.seed.scroll.position)
                    .map_err(classify_geometry_error)?;
                self.geometry_job = Some(start.key());
                self.stage = SessionStage::Target;
                if start.progress() == ExactGeometryProgress::TargetComplete {
                    self.finish_candidate()?;
                } else if start.progress() != ExactGeometryProgress::Scanning {
                    return Err(RangePrepublicationFailure::DeterministicGeometry);
                }
                Ok(())
            }
            ExactGeometryProgress::TargetComplete => self.finish_candidate(),
            ExactGeometryProgress::PendingIndex => {
                Err(RangePrepublicationFailure::DeterministicGeometry)
            }
        }
    }
}
