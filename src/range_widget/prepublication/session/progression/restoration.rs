use super::super::*;

impl RangePrepublicationSession {
    pub(super) fn advance_restoration(
        &mut self,
        effects: &mut EffectBuffer,
    ) -> Result<bool, RangePrepublicationFailure> {
        match self.validation.next() {
            RestorationValidationNext::Text(candidate) => {
                let id = PageRequestId::new(self.next_id()?);
                let prepared = self
                    .residency
                    .prepare_demand_after_retirement(
                        id,
                        PagePurpose::Restoration,
                        crate::PageDemandEnvelope::Validation {
                            candidate,
                            max_payload_bytes: self.environment.config().limits.page_bytes,
                        },
                        &[],
                    )
                    .map_err(|_| RangePrepublicationFailure::TerminalCapacity)?;
                match prepared.outcome() {
                    PageDemand::ResidentValidation {
                        candidate_is_boundary,
                        ..
                    } => {
                        let _ = self.residency.commit_prepared_demand(prepared);
                        self.validation
                            .accept_resident_text_boundary(candidate_is_boundary)
                            .map_err(classify_widget_error)?;
                        Ok(true)
                    }
                    PageDemand::Requested(request) => {
                        self.admit_restoration_page_request(prepared, request, effects)
                    }
                    PageDemand::ResidentAdjacent(_) | PageDemand::Coalesced(_) => {
                        Err(RangePrepublicationFailure::Stale)
                    }
                }
            }
            RestorationValidationNext::Object { position, cursor } => {
                let id = ObjectRequestId::new(self.next_id()?);
                let demand = crate::ObjectDemandEnvelope::anchor(
                    position.byte_offset,
                    cursor,
                    crate::ObjectDirection::Forward,
                    self.environment
                        .config()
                        .object_residency_limits
                        .max_resident_objects(),
                    self.environment
                        .config()
                        .object_residency_limits
                        .max_resident_bytes(),
                )
                .map_err(|_| RangePrepublicationFailure::InvalidEnvironment)?;
                let prepared = self
                    .object_residency
                    .prepare_demand_after_retirement_from(
                        id,
                        ObjectPurpose::Restoration,
                        demand,
                        &[],
                        self.object_residency.resident_page_iter(),
                    )
                    .map_err(|_| RangePrepublicationFailure::TerminalCapacity)?;
                match prepared.outcome() {
                    ObjectDemand::Resident(page_id) => {
                        let _ = self.object_residency.commit_prepared_demand(prepared);
                        let page = self
                            .object_residency
                            .peek_page_by_id(page_id)
                            .ok_or(RangePrepublicationFailure::Stale)?;
                        self.validation
                            .accept_resident_object(page)
                            .map_err(classify_widget_error)?;
                        Ok(true)
                    }
                    ObjectDemand::Requested(request) => {
                        self.admit_restoration_object_request(prepared, request, effects)
                    }
                    ObjectDemand::Coalesced(_) => Err(RangePrepublicationFailure::Stale),
                }
            }
            RestorationValidationNext::Complete => {
                self.release_all_resident_custody();
                drop(self.residency.take_resident_pages());
                drop(self.object_residency.take_resident_pages());
                let id = GeometryJobId::new(self.next_id()?);
                let start = self
                    .geometry
                    .as_mut()
                    .ok_or(RangePrepublicationFailure::Stale)?
                    .start_index(id)
                    .map_err(classify_geometry_error)?;
                if start.progress() != ExactGeometryProgress::Scanning {
                    return Err(RangePrepublicationFailure::DeterministicGeometry);
                }
                self.geometry_job = Some(start.key());
                self.stage = SessionStage::Index;
                Ok(true)
            }
        }
    }
}
