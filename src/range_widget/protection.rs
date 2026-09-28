use super::*;

mod reservation;
pub use reservation::RangeResidentReservation;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RangeResidentProtection {
    entity: gpui::EntityId,
    generation: u64,
    seed: RangeRestorationSeed,
}

impl RangeResidentProtection {
    pub fn seed(self) -> RangeRestorationSeed {
        self.seed
    }
}

pub(super) struct ProtectedResident {
    cut: RangeResidentProtection,
    invalidated: bool,
}

impl RangeTextInput {
    pub(super) fn invalidate_resident_protection(&mut self) {
        if let Some(protected) = self.resident_protection.as_mut() {
            protected.invalidated = true;
        }
    }

    pub fn protect_resident(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Result<RangeResidentProtection, RangeTextInputError> {
        if self.enabled || self.resident_protection.is_some() {
            return Err(RangeTextInputError::Busy);
        }
        let seed = self.export_restoration(Some(self.history_frontier()))?;
        let generation = self
            .protection_generation
            .checked_add(1)
            .ok_or(RangeTextInputError::Busy)?;
        let cut = RangeResidentProtection {
            entity: cx.entity_id(),
            generation,
            seed,
        };
        self.protection_generation = generation;
        self.resident_protection = Some(ProtectedResident {
            cut,
            invalidated: false,
        });
        Ok(cut)
    }

    pub fn resident_protection_is_current(&self, cut: RangeResidentProtection) -> bool {
        self.mounted
            && self
                .resident_protection
                .as_ref()
                .is_some_and(|protected| protected.cut == cut && !protected.invalidated)
    }

    pub fn release_resident_protection(
        &mut self,
        cut: RangeResidentProtection,
        cx: &mut Context<Self>,
    ) -> Result<(), RangeTextInputError> {
        if !self.mounted {
            return Err(RangeTextInputError::NotMounted);
        }
        if !self
            .resident_protection
            .as_ref()
            .is_some_and(|protected| protected.cut == cut)
        {
            return Err(RangeTextInputError::Stale);
        }
        self.resident_protection = None;
        cx.notify();
        Ok(())
    }

    pub(super) fn require_unprotected_resident(&self) -> Result<(), RangeTextInputError> {
        if self.resident_protection.is_some() {
            Err(RangeTextInputError::Busy)
        } else {
            Ok(())
        }
    }

    pub(super) fn reject_protected_environment_change(
        &mut self,
    ) -> Result<(), RangeTextInputError> {
        if let Some(protected) = self.resident_protection.as_mut() {
            protected.invalidated = true;
            Err(RangeTextInputError::Busy)
        } else {
            Ok(())
        }
    }
}
