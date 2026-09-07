use crate::{
    MutationEvidenceAcknowledgement, MutationFinishInput, MutationKey, MutationOutcome,
    MutationPage, MutationPageAcceptance, MutationPageRequest, MutationPass, RangeTextInput,
    RangeTextInputError, RangeTextInputRequest,
};
use gpui::Context;

impl RangeTextInput {
    pub(super) fn local_mutation_payload_charge(
        &self,
    ) -> Result<crate::RangeSurfaceCharge, RangeTextInputError> {
        self.pending_local_mutation
            .as_ref()
            .and_then(|local| local.page.as_ref())
            .map_or(Ok(crate::RangeSurfaceCharge::default()), |page| {
                let (bytes, items) = page
                    .payload_allocation_charge()
                    .ok_or(RangeTextInputError::SurfaceCapacity)?;
                Ok(crate::RangeSurfaceCharge { bytes, items })
            })
    }

    pub(super) fn check_mutation_payload_capacity(
        &self,
        payload: crate::RangeSurfaceCharge,
    ) -> Result<(), RangeTextInputError> {
        let current = self.current_realization_ownership();
        if current
            .owned_bytes
            .checked_add(payload.bytes)
            .is_none_or(|bytes| bytes > self.config.limits.max_surface_bytes)
            || current
                .owned_items
                .checked_add(payload.items)
                .is_none_or(|items| items > self.config.limits.max_surface_items)
        {
            return Err(RangeTextInputError::SurfaceCapacity);
        }
        Ok(())
    }

    fn check_mutation_request_capacity(
        &self,
        request: &RangeTextInputRequest,
    ) -> Result<(), RangeTextInputError> {
        if self.requests.len() == self.requests.capacity() {
            return Err(RangeTextInputError::SurfaceCapacity);
        }
        let payload = super::transition::queued_request_payload_charge(
            std::iter::once(request),
            self.clipboard.current_provenance_page(),
        )?;
        self.check_mutation_payload_capacity(payload)
    }

    pub fn request_mutation_evidence(
        &mut self,
        key: MutationKey,
    ) -> Result<MutationPass, RangeTextInputError> {
        Ok(self.edits.request_evidence(key)?)
    }

    pub fn local_mutation_evidence(
        &self,
        pass: MutationPass,
    ) -> Result<(Option<&MutationPage>, MutationFinishInput), RangeTextInputError> {
        self.edits.validate_evidence_pass(pass)?;
        let local = self
            .pending_local_mutation
            .as_ref()
            .filter(|local| local.key == pass.key())
            .ok_or(crate::MutationError::EvidenceUnavailable)?;
        Ok((local.page.as_ref(), local.finish))
    }

    pub fn submit_mutation_evidence_page(
        &mut self,
        pass: MutationPass,
        page: MutationPage,
        cx: &mut Context<Self>,
    ) -> Result<MutationEvidenceAcknowledgement, RangeTextInputError> {
        let was_active = self.edits.active_key() == Some(pass.key());
        let result = self.edits.submit_evidence_page(pass, page);
        if result.is_err() && was_active && self.edits.is_retired(pass.key()) {
            self.finish_local_mutation(pass.key(), MutationOutcome::Error, cx);
        }
        Ok(result?)
    }

    pub fn acknowledge_mutation_evidence_page(
        &mut self,
        acknowledgement: MutationEvidenceAcknowledgement,
    ) -> Result<(), RangeTextInputError> {
        Ok(self.edits.acknowledge_evidence_page(acknowledgement)?)
    }

    pub fn submit_mutation_evidence_finish(
        &mut self,
        pass: MutationPass,
        finish: MutationFinishInput,
    ) -> Result<(), RangeTextInputError> {
        Ok(self.edits.finish_evidence(pass, finish)?)
    }

    pub fn mutation_restart(&self, key: MutationKey) -> Result<MutationPass, RangeTextInputError> {
        Ok(self.edits.mutation_restart(key)?)
    }

    pub fn acknowledge_mutation_restart(
        &mut self,
        pass: MutationPass,
        cx: &mut Context<Self>,
    ) -> Result<(), RangeTextInputError> {
        let required = self
            .pending_local_mutation
            .as_ref()
            .filter(|local| local.key == pass.key())
            .map_or(0, |local| usize::from(local.page.is_some()) + 1);
        if self
            .queued_mutation_requests(pass.key())
            .checked_add(required)
            .is_none_or(|count| count > Self::MAX_QUEUED_MUTATION_REQUESTS)
        {
            return Err(RangeTextInputError::Busy);
        }
        if self
            .requests
            .len()
            .checked_add(required)
            .is_none_or(|count| count > self.requests.capacity())
        {
            return Err(RangeTextInputError::SurfaceCapacity);
        }
        self.check_mutation_payload_capacity(crate::RangeSurfaceCharge::default())?;
        self.edits.acknowledge_restart(pass)?;
        if let Some(local) = self.pending_local_mutation.take() {
            if let Some(page) = local.page {
                self.submit_mutation_pass_page(pass, page, cx)?;
            }
            self.submit_mutation_pass_finish(pass, local.finish, cx)?;
        }
        Ok(())
    }

    pub fn submit_mutation_pass_page(
        &mut self,
        pass: MutationPass,
        page: MutationPage,
        cx: &mut Context<Self>,
    ) -> Result<MutationPageAcceptance, RangeTextInputError> {
        if !self.mutation_queue_has_capacity(pass.key()) {
            return Err(RangeTextInputError::Busy);
        }
        let request = MutationPageRequest::new(page.clone()).with_pass(pass);
        let request = match request.page().key().lane() {
            crate::MutationLane::Source => RangeTextInputRequest::MutationSourcePage(request),
            crate::MutationLane::Proposal => RangeTextInputRequest::MutationProposalPage(request),
        };
        self.check_mutation_request_capacity(&request)?;
        let was_active = self.edits.active_key() == Some(pass.key());
        let result = self.edits.accept_pass_page(pass, page);
        if result.is_err() && was_active && self.edits.is_retired(pass.key()) {
            self.finish_local_mutation(pass.key(), MutationOutcome::Error, cx);
        }
        let acceptance = result?;
        self.push_request(request, cx)?;
        Ok(acceptance)
    }

    pub fn submit_mutation_pass_finish(
        &mut self,
        pass: MutationPass,
        finish: MutationFinishInput,
        cx: &mut Context<Self>,
    ) -> Result<(), RangeTextInputError> {
        if !self.mutation_queue_has_capacity(pass.key()) {
            return Err(RangeTextInputError::Busy);
        }
        self.check_mutation_request_capacity(&RangeTextInputRequest::MutationFinishInput(finish))?;
        let was_active = self.edits.active_key() == Some(pass.key());
        let result = self.edits.finish_pass_input(pass, finish);
        if result.is_err() && was_active && self.edits.is_retired(pass.key()) {
            self.finish_local_mutation(pass.key(), MutationOutcome::Rejected, cx);
        }
        result?;
        self.mutation_positions = Some((finish.key(), finish.intended()));
        self.push_request(RangeTextInputRequest::MutationFinishInput(finish), cx)?;
        Ok(())
    }
}
