use super::*;

impl RangeEditCoordinator {
    pub fn begin(&mut self, proposal: MutationProposal) -> Result<(), MutationError> {
        if let Some(active) = &self.active {
            return Err(MutationError::Busy(active.proposal.key()));
        }
        if self.last_terminal == Some(proposal.key()) {
            return Err(MutationError::ObsoleteOperation(proposal.key()));
        }
        self.check_key(proposal.key())?;
        if self
            .binding
            .extent()
            .check_byte_range(proposal.replacement_bytes())
            .is_err()
        {
            return Err(MutationError::ReplacementOutsideExtent);
        }
        let base_extent = self.binding.extent();
        let base_line_breaks = match (base_extent.byte_len(), base_extent.line_count()) {
            (0, 0) => 0,
            (0, _) | (_, 0) => return Err(MutationError::MalformedBaseExtent),
            (bytes, lines) => {
                let breaks = lines - 1;
                if breaks > bytes {
                    return Err(MutationError::MalformedBaseExtent);
                }
                breaks
            }
        };
        let removed_breaks = proposal.replacement_line_breaks();
        let replacement = proposal.replacement_bytes();
        let replaces_whole_source =
            replacement.start().get() == 0 && replacement.end().get() == base_extent.byte_len();
        if removed_breaks > replacement.len()
            || removed_breaks > base_line_breaks
            || (replacement.is_empty() && removed_breaks != 0)
            || (replaces_whole_source && removed_breaks != base_line_breaks)
        {
            return Err(MutationError::MalformedReplacementLineBreaks);
        }
        self.active = Some(ActiveMutation {
            proposal,
            base_extent,
            state: MutationState::Preflight,
            next_ordinal: 0,
            inserted_bytes: 0,
            inserted_line_breaks: 0,
            fragment_count: 0,
            staged_bytes: 0,
            object_count: 0,
            object_bytes: 0,
            presentation_bytes: 0,
            proof_count: 0,
            source_page_count: 0,
            terminal_seen: false,
            intended: None,
            detached: false,
            fragments: Vec::with_capacity(self.limits.max_fragments),
            source_proofs: Vec::new(),
        });
        Ok(())
    }

    pub fn accept_preflight(&mut self, key: MutationKey) -> Result<(), MutationError> {
        let active = self.active_mut(key, MutationState::Preflight)?;
        active.state = MutationState::Staging;
        Ok(())
    }

    pub fn reject_preflight(
        &mut self,
        key: MutationKey,
    ) -> Result<MutationSettlement, MutationError> {
        self.active_mut(key, MutationState::Preflight)?;
        Ok(self.finish(key, MutationOutcome::Rejected, false))
    }

    pub fn reject_staging(
        &mut self,
        key: MutationKey,
    ) -> Result<MutationSettlement, MutationError> {
        self.active_mut(key, MutationState::Staging)?;
        Ok(self.finish(key, MutationOutcome::Rejected, false))
    }

    pub(crate) fn fail_precommit(
        &mut self,
        key: MutationKey,
    ) -> Result<MutationSettlement, MutationError> {
        let state = self.active_for_key(key)?.state;
        if !matches!(state, MutationState::Preflight | MutationState::Staging) {
            return Err(MutationError::WrongState {
                expected: MutationState::Staging,
                actual: state,
            });
        }
        Ok(self.finish(key, MutationOutcome::Error, false))
    }
}
