use crate::{
    MutationEvidenceAcknowledgement, MutationFinishInput, MutationKey, MutationOutcome,
    MutationPage, MutationPageAcceptance, MutationPageRequest, MutationPass, RangeTextInput,
    RangeTextInputError, RangeTextInputRequest,
};
use gpui::Context;

#[derive(Debug)]
pub enum LocalMutationEvidence<'a> {
    Pending,
    Page(&'a MutationPage),
    Finish(MutationFinishInput),
}

impl RangeTextInput {
    pub(super) fn local_mutation_payload_charge(
        &self,
    ) -> Result<crate::RangeSurfaceCharge, RangeTextInputError> {
        let page = self
            .pending_local_mutation
            .as_ref()
            .and_then(|local| local.page.as_ref())
            .map_or(
                Ok::<_, RangeTextInputError>(crate::RangeSurfaceCharge::default()),
                |page| {
                    let (bytes, items) = page
                        .payload_allocation_charge()
                        .ok_or(RangeTextInputError::SurfaceCapacity)?;
                    Ok(crate::RangeSurfaceCharge { bytes, items })
                },
            )?;
        let producer = self
            .pending_local_mutation
            .as_ref()
            .and_then(|local| local.producer.as_ref())
            .map_or(crate::RangeSurfaceCharge::default(), |producer| {
                crate::RangeSurfaceCharge {
                    bytes: producer.text.capacity(),
                    items: usize::from(producer.text.capacity() != 0),
                }
            });
        let scan = self
            .replacement
            .as_ref()
            .map_or(crate::RangeSurfaceCharge::default(), |scan| {
                scan.payload_charge()
            });
        Ok(crate::RangeSurfaceCharge {
            bytes: page
                .bytes
                .checked_add(producer.bytes)
                .and_then(|bytes| bytes.checked_add(scan.bytes))
                .ok_or(RangeTextInputError::SurfaceCapacity)?,
            items: page
                .items
                .checked_add(producer.items)
                .and_then(|items| items.checked_add(scan.items))
                .ok_or(RangeTextInputError::SurfaceCapacity)?,
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
        let pass = self.edits.request_evidence(key)?;
        if let Some(local) = self
            .pending_local_mutation
            .as_mut()
            .filter(|local| local.key == key)
        {
            local.evidence_page_acknowledged = false;
            if let Some(producer) = local.producer.as_mut() {
                producer.restart(super::source_replacement::ProducerMode::Evidence(pass));
            }
        }
        Ok(pass)
    }

    pub fn local_mutation_evidence(
        &mut self,
        pass: MutationPass,
        cx: &mut Context<Self>,
    ) -> Result<LocalMutationEvidence<'_>, RangeTextInputError> {
        self.edits.validate_evidence_pass(pass)?;
        self.prepare_local_replacement(cx)?;
        let local = self
            .pending_local_mutation
            .as_ref()
            .filter(|local| local.key == pass.key())
            .ok_or(crate::MutationError::EvidenceUnavailable)?;
        if let Some(producer) = local.producer.as_ref() {
            if !matches!(producer.mode, super::source_replacement::ProducerMode::Evidence(current) if current == pass)
            {
                return Err(RangeTextInputError::Stale);
            }
            return Ok(if let Some(page) = local.page.as_ref() {
                LocalMutationEvidence::Page(page)
            } else if producer.pending.is_none() && producer.is_complete() {
                LocalMutationEvidence::Finish(producer.finish(local.finish))
            } else {
                LocalMutationEvidence::Pending
            });
        }
        Ok(if !local.evidence_page_acknowledged {
            local.page.as_ref().map_or(
                LocalMutationEvidence::Finish(local.finish),
                LocalMutationEvidence::Page,
            )
        } else {
            LocalMutationEvidence::Finish(local.finish)
        })
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
        self.edits.acknowledge_evidence_page(acknowledgement)?;
        if let Some(local) = self.pending_local_mutation.as_mut() {
            local.evidence_page_acknowledged = true;
            if local.producer.is_some() {
                local.page = None;
            }
        }
        Ok(())
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
        if let Some(local) = self
            .pending_local_mutation
            .as_mut()
            .filter(|local| local.key == pass.key())
            && let Some(producer) = local.producer.as_mut()
        {
            local.page = None;
            producer.restart(super::source_replacement::ProducerMode::Staging(Some(pass)));
            return self.drive_local_replacement(cx);
        }
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
