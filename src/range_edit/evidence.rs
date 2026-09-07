use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MutationProducerIdentity(u64);

impl MutationProducerIdentity {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }
    pub const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MutationPassKind {
    Evidence,
    Staging,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MutationPass {
    key: MutationKey,
    producer: MutationProducerIdentity,
    kind: MutationPassKind,
}

impl MutationPass {
    pub const fn key(self) -> MutationKey {
        self.key
    }
    pub const fn producer(self) -> MutationProducerIdentity {
        self.producer
    }
    pub const fn kind(self) -> MutationPassKind {
        self.kind
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MutationEvidenceAcknowledgement {
    pass: MutationPass,
    page: MutationPageKey,
    identity: MutationIdentity,
    acceptance: MutationPageAcceptance,
    request: u64,
}

impl MutationEvidenceAcknowledgement {
    pub const fn pass(self) -> MutationPass {
        self.pass
    }
    pub const fn page(self) -> MutationPageKey {
        self.page
    }
    pub const fn identity(self) -> MutationIdentity {
        self.identity
    }
    pub const fn acceptance(self) -> MutationPageAcceptance {
        self.acceptance
    }
    pub const fn request(self) -> u64 {
        self.request
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum EvidencePhase {
    Unavailable,
    Streaming,
    Complete,
    RestartPending,
    Staging,
}

#[derive(Debug)]
pub(super) struct EvidenceState {
    pub(super) phase: EvidencePhase,
    pub(super) closure: Option<MutationFinishInput>,
    outstanding: Option<MutationEvidenceAcknowledgement>,
    next_request: u64,
}

impl RangeEditCoordinator {
    pub(crate) fn validate_evidence_pass(&self, pass: MutationPass) -> Result<(), MutationError> {
        self.check_pass(pass, EvidencePhase::Streaming)
    }

    pub fn request_evidence(&mut self, key: MutationKey) -> Result<MutationPass, MutationError> {
        let active = self.active_mut(key, MutationState::PreflightPending)?;
        if active.evidence.is_some() {
            return Err(MutationError::EvidenceRequired);
        }
        active.evidence = Some(EvidenceState {
            phase: if active.producer.is_some() {
                EvidencePhase::Streaming
            } else {
                EvidencePhase::Unavailable
            },
            closure: None,
            outstanding: None,
            next_request: 0,
        });
        let producer = active.producer.ok_or(MutationError::EvidenceUnavailable)?;
        Ok(MutationPass {
            key,
            producer,
            kind: MutationPassKind::Evidence,
        })
    }

    fn check_pass(&self, pass: MutationPass, phase: EvidencePhase) -> Result<(), MutationError> {
        let active = self.active_for_key(pass.key)?;
        let expected_kind = if phase == EvidencePhase::Streaming {
            MutationPassKind::Evidence
        } else {
            MutationPassKind::Staging
        };
        if active.producer != Some(pass.producer)
            || pass.kind != expected_kind
            || active
                .evidence
                .as_ref()
                .is_none_or(|evidence| evidence.phase != phase)
        {
            return Err(MutationError::WrongMutationPass);
        }
        Ok(())
    }

    pub fn submit_evidence_page(
        &mut self,
        pass: MutationPass,
        page: MutationPage,
    ) -> Result<MutationEvidenceAcknowledgement, MutationError> {
        self.check_pass(pass, EvidencePhase::Streaming)?;
        if page.key().key() != pass.key {
            return Err(MutationError::WrongMutationPass);
        }
        if self
            .active_for_key(pass.key)?
            .evidence
            .as_ref()
            .unwrap()
            .outstanding
            .is_some()
        {
            return Err(MutationError::EvidenceAcknowledgementPending);
        }
        let page_key = page.key();
        let identity = page.page_identity();
        let request = self
            .active_for_key(pass.key)?
            .evidence
            .as_ref()
            .unwrap()
            .next_request;
        let next_request = request
            .checked_add(1)
            .ok_or(MutationError::CumulativeOverflow)?;
        let acceptance = self.accept_page_in_state(page, MutationState::PreflightPending)?;
        let acknowledgement = MutationEvidenceAcknowledgement {
            pass,
            page: page_key,
            identity,
            acceptance,
            request,
        };
        let evidence = self.active.as_mut().unwrap().evidence.as_mut().unwrap();
        evidence.outstanding = Some(acknowledgement);
        evidence.next_request = next_request;
        Ok(acknowledgement)
    }

    pub fn acknowledge_evidence_page(
        &mut self,
        acknowledgement: MutationEvidenceAcknowledgement,
    ) -> Result<(), MutationError> {
        self.check_pass(acknowledgement.pass, EvidencePhase::Streaming)?;
        let evidence = self.active.as_mut().unwrap().evidence.as_mut().unwrap();
        if evidence.outstanding != Some(acknowledgement) {
            return Err(MutationError::EvidenceAcknowledgementMismatch);
        }
        evidence.outstanding = None;
        Ok(())
    }

    pub fn finish_evidence(
        &mut self,
        pass: MutationPass,
        finish: MutationFinishInput,
    ) -> Result<(), MutationError> {
        self.check_pass(pass, EvidencePhase::Streaming)?;
        if finish.key() != pass.key {
            return Err(MutationError::WrongMutationPass);
        }
        if self
            .active_for_key(pass.key)?
            .evidence
            .as_ref()
            .unwrap()
            .outstanding
            .is_some()
        {
            return Err(MutationError::EvidenceAcknowledgementPending);
        }
        self.finish_input_in_state(finish, MutationState::PreflightPending)?;
        let evidence = self.active.as_mut().unwrap().evidence.as_mut().unwrap();
        evidence.closure = Some(finish);
        evidence.phase = EvidencePhase::Complete;
        Ok(())
    }

    pub fn mutation_restart(&self, key: MutationKey) -> Result<MutationPass, MutationError> {
        let active = self.active_for_key(key)?;
        if active
            .evidence
            .as_ref()
            .is_none_or(|evidence| evidence.phase != EvidencePhase::RestartPending)
        {
            return Err(MutationError::WrongMutationPass);
        }
        Ok(MutationPass {
            key,
            producer: active.producer.unwrap(),
            kind: MutationPassKind::Staging,
        })
    }

    pub fn acknowledge_restart(&mut self, pass: MutationPass) -> Result<(), MutationError> {
        self.check_pass(pass, EvidencePhase::RestartPending)?;
        let active = self.active_mut(pass.key, MutationState::InputStreaming)?;
        active.source = LaneState::new(active.initial_source_cursor);
        active.proposal_lane = LaneState::new(active.initial_proposal_cursor);
        active.sequence = MutationSequenceState::default();
        active.active_object_effect = None;
        active.intended = None;
        active.intended_extent = None;
        active.evidence.as_mut().unwrap().phase = EvidencePhase::Staging;
        Ok(())
    }

    pub fn accept_pass_page(
        &mut self,
        pass: MutationPass,
        page: MutationPage,
    ) -> Result<MutationPageAcceptance, MutationError> {
        self.check_pass(pass, EvidencePhase::Staging)?;
        if page.key().key() != pass.key {
            return Err(MutationError::WrongMutationPass);
        }
        self.accept_page_in_state(page, MutationState::InputStreaming)
    }

    pub fn finish_pass_input(
        &mut self,
        pass: MutationPass,
        finish: MutationFinishInput,
    ) -> Result<(), MutationError> {
        self.check_pass(pass, EvidencePhase::Staging)?;
        if finish.key() != pass.key {
            return Err(MutationError::WrongMutationPass);
        }
        if self
            .active_for_key(pass.key)?
            .evidence
            .as_ref()
            .unwrap()
            .closure
            != Some(finish)
        {
            self.finish(pass.key, MutationOutcome::Rejected, false);
            return Err(MutationError::ReplayMismatch);
        }
        if let Err(error) = self.finish_input_in_state(finish, MutationState::InputStreaming) {
            self.finish(pass.key, MutationOutcome::Rejected, false);
            return Err(error);
        }
        Ok(())
    }
}
