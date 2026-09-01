use super::*;

mod geometry;
mod request_admission;
mod restoration;

impl RangePrepublicationSession {
    pub(super) fn advance_one(
        &mut self,
        text_system: &WindowTextSystem,
        effects: &mut EffectBuffer,
    ) -> Result<bool, RangePrepublicationFailure> {
        if self.delivered.is_some() {
            let coexistence = self
                .response_coexistence_charge()
                .ok_or(RangePrepublicationFailure::Arithmetic)?;
            if !charge_fits(coexistence, self.available) {
                return Ok(false);
            }
            return self.process_delivered(text_system, effects).map(|_| true);
        }
        if self.waiting.is_some() {
            return Ok(false);
        }
        match self.stage {
            SessionStage::Initializing => {
                let Some(cleanup) = self.reserve_cleanup()? else {
                    return Ok(false);
                };
                let request = RangePrepublicationValidationRequest {
                    cleanup,
                    key: RangePrepublicationValidationKey {
                        generation: self.generation,
                        request: self.next_id()?,
                    },
                    binding: self.seed.binding,
                    history: self.seed.history,
                };
                if !self.admit_external_effect(
                    cleanup,
                    effects,
                    RangeSurfaceCharge::default(),
                    RangeSurfaceCharge::default(),
                )? {
                    return Ok(false);
                }
                if !self
                    .environment
                    .cleanup()
                    .bind_request(cleanup, CleanupRequest::Validation(request.key))
                {
                    return Err(RangePrepublicationFailure::Stale);
                }
                self.waiting = Some(Waiting::Validation(request));
                self.stage = SessionStage::Validating;
                effects.push(RangePrepublicationEffect::ValidateOwner(request));
                Ok(true)
            }
            SessionStage::Validating => Ok(false),
            SessionStage::Restoration => self.advance_restoration(effects),
            SessionStage::Index | SessionStage::Target => {
                self.advance_geometry(text_system, effects)
            }
            SessionStage::Ready
            | SessionStage::Cancelled
            | SessionStage::Failed(_)
            | SessionStage::CandidateTaken => Ok(false),
        }
    }
}
