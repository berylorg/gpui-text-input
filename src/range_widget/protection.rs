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
    capture: ResidentCapture,
    invalidated: bool,
}

#[derive(Clone, Copy)]
enum ResidentCapture {
    Current,
    Published {
        binding: crate::RangeBinding,
        history: RangeHistoryFrontier,
        visible_history: RangeHistoryFrontier,
    },
}

impl RangeTextInput {
    pub(in crate::range_widget) fn prepare_successor_resident_protection(
        &self,
        mut seed: RangeRestorationSeed,
        cx: &Context<Self>,
    ) -> Option<RangeResidentProtection> {
        let generation = self.protection_generation.checked_add(1)?;
        seed.history = Some(
            seed.history
                .unwrap_or_else(|| RangeHistoryFrontier::unavailable(seed.binding)),
        );
        Some(RangeResidentProtection {
            entity: cx.entity_id(),
            generation,
            seed,
        })
    }

    pub(in crate::range_widget) fn install_successor_resident_protection(
        &mut self,
        cut: RangeResidentProtection,
    ) {
        self.protection_generation = cut.generation;
        self.resident_protection = Some(ProtectedResident {
            cut,
            capture: ResidentCapture::Current,
            invalidated: false,
        });
    }

    pub(super) fn invalidate_resident_protection(&mut self) {
        if let Some(protected) = self.resident_protection.as_mut() {
            protected.invalidated = true;
        }
    }

    pub fn protect_resident(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Result<RangeResidentProtection, RangeTextInputError> {
        self.capture_resident_protection(self.history_frontier(), ResidentCapture::Current, cx)
    }

    pub fn protect_published_resident(
        &mut self,
        history: RangeHistoryFrontier,
        cx: &mut Context<Self>,
    ) -> Result<RangeResidentProtection, RangeTextInputError> {
        let capture = ResidentCapture::Published {
            binding: self.config.binding,
            history: self.history_frontier,
            visible_history: self.history_frontier(),
        };
        self.capture_resident_protection(history, capture, cx)
    }

    fn capture_resident_protection(
        &mut self,
        history: RangeHistoryFrontier,
        capture: ResidentCapture,
        cx: &mut Context<Self>,
    ) -> Result<RangeResidentProtection, RangeTextInputError> {
        if self.enabled || self.resident_protection.is_some() {
            return Err(RangeTextInputError::Busy);
        }
        let seed = self.export_restoration(Some(history))?;
        if matches!(capture, ResidentCapture::Current) && seed.binding != self.config.binding {
            return Err(RangeTextInputError::Stale);
        }
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
            capture,
            invalidated: false,
        });
        Ok(cut)
    }

    pub fn resident_protection_is_current(&self, cut: RangeResidentProtection) -> bool {
        self.mounted
            && self.resident_protection.as_ref().is_some_and(|protected| {
                protected.cut == cut
                    && !protected.invalidated
                    && !self.enabled
                    && match protected.capture {
                        ResidentCapture::Current => {
                            self.config.binding == cut.seed.binding
                                && self.export_restoration(Some(self.history_frontier())).ok()
                                    == Some(cut.seed)
                        }
                        ResidentCapture::Published {
                            binding,
                            history,
                            visible_history,
                        } => {
                            self.config.binding == binding
                                && self.history_frontier == history
                                && self.history_frontier() == visible_history
                                && self.export_restoration(cut.seed.history).ok() == Some(cut.seed)
                        }
                    }
            })
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
