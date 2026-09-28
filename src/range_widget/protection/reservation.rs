use super::*;

#[derive(Debug)]
pub struct RangeResidentReservation {
    pub(in crate::range_widget) protection: RangeResidentProtection,
    pub(in crate::range_widget) generation: RangePrepublicationSessionGeneration,
    predecessor: RangeSurfaceCharge,
    combined: RangeSurfaceCharge,
    successor: RangeSurfaceCharge,
}

impl RangeResidentReservation {
    pub fn protection(&self) -> RangeResidentProtection {
        self.protection
    }

    pub fn session_generation(&self) -> RangePrepublicationSessionGeneration {
        self.generation
    }

    pub fn predecessor_charge(&self) -> RangeSurfaceCharge {
        self.predecessor
    }

    pub fn combined_capacity(&self) -> RangeSurfaceCharge {
        self.combined
    }

    pub fn successor_capacity(&self) -> RangeSurfaceCharge {
        self.successor
    }
}

impl RangeTextInput {
    pub fn prepare_resident_successor(
        &self,
        protection: RangeResidentProtection,
        seed: RangeRestorationSeed,
        environment: RangePrepublicationEnvironment,
        combined: RangeSurfaceCharge,
    ) -> Result<(RangePrepublicationSession, RangeResidentReservation), RangePrepublicationFailure>
    {
        if !self.resident_protection_is_current(protection) {
            return Err(RangePrepublicationFailure::Stale);
        }
        let predecessor_seed = protection.seed();
        if seed.caret != predecessor_seed.caret
            || seed.selection != predecessor_seed.selection
            || seed.scroll != predecessor_seed.scroll
        {
            return Err(RangePrepublicationFailure::SourceMismatch);
        }
        let ownership = self.current_realization_ownership();
        let predecessor = RangeSurfaceCharge {
            bytes: ownership.owned_bytes,
            items: ownership.owned_items,
        };
        let successor = RangeSurfaceCharge {
            bytes: combined
                .bytes
                .checked_sub(predecessor.bytes)
                .ok_or(RangePrepublicationFailure::InitialCapacityDenied)?
                .min(environment.config().limits.max_surface_bytes),
            items: combined
                .items
                .checked_sub(predecessor.items)
                .ok_or(RangePrepublicationFailure::InitialCapacityDenied)?
                .min(environment.config().limits.max_surface_items),
        };
        let session =
            RangePrepublicationSession::new_with_admission_capacity(seed, environment, successor)?;
        let reservation = RangeResidentReservation {
            protection,
            generation: session.generation(),
            predecessor,
            combined,
            successor,
        };
        Ok((session, reservation))
    }
}
