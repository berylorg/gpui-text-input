use gpui::{Context, Window};

use super::{RangePrepublicationCandidate, adoption::checked_adoption_config, types::*};
use crate::range_widget::{
    RangeResidentProtection, RangeResidentReservation, RangeTextInput,
    transition::PreparedResidentPublication,
};

impl RangeTextInput {
    pub fn adopt_protected_resident_successor(
        &mut self,
        reservation: RangeResidentReservation,
        environment: &RangePrepublicationEnvironment,
        candidate: RangePrepublicationCandidate,
        current: RangePrepublicationCurrent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<RangeResidentProtection, RangePrepublicationAdoptionError> {
        let successor = self
            .prepare_successor_resident_protection(candidate.seed, cx)
            .ok_or(RangePrepublicationAdoptionError::ProtectionUnavailable)?;
        self.adopt_resident_successor(reservation, environment, candidate, current, window, cx)?;
        self.install_successor_resident_protection(successor);
        Ok(successor)
    }

    pub fn adopt_resident_successor(
        &mut self,
        reservation: RangeResidentReservation,
        environment: &RangePrepublicationEnvironment,
        mut candidate: RangePrepublicationCandidate,
        current: RangePrepublicationCurrent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), RangePrepublicationAdoptionError> {
        let protection = reservation.protection();
        let predecessor = protection.seed();
        if !self.resident_protection_is_current(protection)
            || self.enabled
            || self.export_restoration(Some(self.history_frontier())).ok() != Some(predecessor)
        {
            return Err(RangePrepublicationAdoptionError::PredecessorMismatch);
        }
        if candidate.generation != reservation.session_generation()
            || candidate.seed.caret != predecessor.caret
            || candidate.seed.selection != predecessor.selection
            || candidate.seed.scroll != predecessor.scroll
        {
            return Err(RangePrepublicationAdoptionError::ReservationMismatch);
        }
        let ownership = self.current_realization_ownership();
        let old = crate::RangeSurfaceCharge {
            bytes: ownership.owned_bytes,
            items: ownership.owned_items,
        };
        let combined = reservation.combined_capacity();
        if old != reservation.predecessor_charge()
            || !charge_fits(current.available_capacity, reservation.successor_capacity())
            || old
                .bytes
                .checked_add(current.available_capacity.bytes)
                .is_none_or(|bytes| bytes > combined.bytes)
            || old
                .items
                .checked_add(current.available_capacity.items)
                .is_none_or(|items| items > combined.items)
        {
            return Err(RangePrepublicationAdoptionError::CapacityMismatch);
        }
        if !self
            .text_system
            .upgrade()
            .is_some_and(|system| std::sync::Arc::ptr_eq(&system, window.text_system()))
        {
            return Err(RangePrepublicationAdoptionError::EnvironmentMismatch);
        }
        let config = checked_adoption_config(environment, &candidate, current, window)?;
        let prepared = PreparedResidentPublication::prepare(self, config)?;
        if candidate.geometry.is_none() || candidate.surface.is_none() {
            return Err(RangePrepublicationAdoptionError::CandidateConsumed);
        }
        // Transfer is the final fallible gate; the resident remains untouched on refusal.
        let custody = candidate
            .take_adopted_cleanup()
            .ok_or(RangePrepublicationAdoptionError::CandidateConsumed)?;
        let owners = crate::range_widget::AdoptedPrepublicationOwners {
            geometry: candidate
                .geometry
                .take()
                .expect("checked candidate geometry"),
            surface: candidate.surface.take().expect("checked candidate surface"),
            seed: candidate.seed,
            surface_charge: candidate.surface_charge,
            history: current.history,
            next_id: candidate.next_id,
        };
        self.commit_prepared_resident_publication(prepared, owners, custody, window, cx);
        Ok(())
    }
}
