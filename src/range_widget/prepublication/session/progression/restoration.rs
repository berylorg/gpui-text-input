use super::super::*;
use crate::range_geometry::{CapacityRefusal, ResponseCapacity};

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
                let charge = add_charge(
                    self.current_charge()
                        .ok_or(RangePrepublicationFailure::Arithmetic)?,
                    multiply_charge(
                        RangeSurfaceCharge {
                            bytes: std::mem::size_of::<RangePrepublicationEffect>(),
                            items: 1,
                        },
                        effects.len(),
                    )
                    .ok_or(RangePrepublicationFailure::Arithmetic)?,
                )?;
                let next_id = self
                    .next_id
                    .checked_add(1)
                    .ok_or(RangePrepublicationFailure::Arithmetic)?;
                let prepared = self
                    .geometry
                    .as_ref()
                    .ok_or(RangePrepublicationFailure::Stale)?
                    .prepare_initial_index(
                        GeometryJobId::new(self.next_id),
                        ResponseCapacity::Enclosing {
                            current: (charge.bytes, charge.items),
                            limit: (self.available.bytes, self.available.items),
                        },
                    );
                let prepared = match prepared {
                    Ok(prepared) => {
                        let (bytes, items) = prepared.enclosing_peak();
                        self.observe_charge(RangeSurfaceCharge { bytes, items });
                        prepared
                    }
                    Err(failure) => {
                        if let Some((bytes, items)) = failure.enclosing_peak() {
                            let peak = RangeSurfaceCharge { bytes, items };
                            self.observe_charge(peak);
                            if !charge_fits(peak, configured_capacity(self.environment.config())) {
                                return Err(RangePrepublicationFailure::TerminalCapacity);
                            }
                            if failure.capacity_refusal() == Some(CapacityRefusal::Enclosing) {
                                self.ledger_blocked = true;
                                return Ok(false);
                            }
                        }
                        return Err(classify_geometry_error(failure.error().clone()));
                    }
                };
                let start = self
                    .geometry
                    .as_mut()
                    .unwrap()
                    .commit_initial_index(prepared);
                self.next_id = next_id;
                self.release_all_resident_custody();
                self.residency.discard_resident_pages();
                self.object_residency.discard_resident_pages();
                self.geometry_job = Some(start.key());
                self.stage = SessionStage::Index;
                Ok(true)
            }
        }
    }
}
