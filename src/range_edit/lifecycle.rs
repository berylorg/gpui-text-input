use super::*;

impl RangeEditCoordinator {
    pub fn cancel(&mut self, key: MutationKey) -> Result<MutationCancellation, MutationError> {
        let state = self.active_for_key(key)?.state;
        match state {
            MutationState::PreflightPending
            | MutationState::InputStreaming
            | MutationState::FinishPending => {
                self.finish(key, MutationOutcome::Cancelled, false);
                Ok(MutationCancellation::Cancelled)
            }
            MutationState::CommitPending => Ok(MutationCancellation::AwaitingHostSettlement),
            MutationState::Idle | MutationState::Settled => Err(MutationError::NoActive),
        }
    }

    pub fn rebind(&mut self, binding: RangeBinding) -> Option<MutationDisposal> {
        let binding_changed = self.binding.binding() != binding.binding()
            || self.binding.revision() != binding.revision();
        let disposal = self.active.as_mut().map(|active| {
            if active.state == MutationState::CommitPending {
                active.detached = true;
                MutationDisposal::Detached(active.proposal.key())
            } else {
                MutationDisposal::Cancelled(active.proposal.key())
            }
        });
        if matches!(disposal, Some(MutationDisposal::Cancelled(_))) {
            let active = self
                .active
                .take()
                .expect("cancelled active mutation exists");
            let key = active.proposal.key();
            self.record_release(active.counts());
            self.last_terminal = Some(key);
        }
        self.binding = binding;
        if binding_changed {
            self.operation_high_water = None;
            self.high_water_begin_identity = None;
        }
        disposal
    }

    pub fn dispose(&mut self) -> Option<MutationDisposal> {
        let active = self.active.as_mut()?;
        let key = active.proposal.key();
        if active.state == MutationState::CommitPending {
            active.detached = true;
            return Some(MutationDisposal::Detached(key));
        }
        let active = self.active.take().expect("active mutation exists");
        self.record_release(active.counts());
        self.last_terminal = Some(key);
        Some(MutationDisposal::Cancelled(key))
    }
}
