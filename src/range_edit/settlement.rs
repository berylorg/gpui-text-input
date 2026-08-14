use super::*;

impl RangeEditCoordinator {
    /// Admits the host's one exact terminal outcome and validates a committed successor extent.
    pub fn settle(
        &mut self,
        key: MutationKey,
        outcome: MutationOutcome,
    ) -> Result<MutationSettlement, MutationError> {
        let active = self.active_for_key(key)?;
        if !matches!(
            active.state,
            MutationState::CommitPending | MutationState::DetachedCommit
        ) {
            return Err(MutationError::WrongState {
                expected: MutationState::CommitPending,
                actual: active.state,
            });
        }
        if let MutationOutcome::Committed(successor) = outcome {
            let expected_bytes = active
                .base_extent
                .byte_len()
                .checked_sub(active.proposal.replacement().len())
                .and_then(|bytes| bytes.checked_add(active.inserted_bytes))
                .ok_or(MutationError::IncoherentSuccessor)?;
            let base_breaks = active
                .base_extent
                .line_count()
                .checked_sub(u64::from(active.base_extent.byte_len() != 0))
                .ok_or(MutationError::IncoherentSuccessor)?;
            let expected_breaks = base_breaks
                .checked_sub(active.proposal.replacement_line_breaks())
                .and_then(|breaks| breaks.checked_add(active.inserted_line_breaks))
                .ok_or(MutationError::IncoherentSuccessor)?;
            let expected_lines = if expected_bytes == 0 {
                if expected_breaks != 0 {
                    return Err(MutationError::IncoherentSuccessor);
                }
                0
            } else {
                expected_breaks
                    .checked_add(1)
                    .ok_or(MutationError::IncoherentSuccessor)?
            };
            if successor.binding() != key.binding()
                || successor.revision() == key.base_revision()
                || successor.extent().byte_len() != expected_bytes
                || successor.extent().line_count() != expected_lines
            {
                return Err(MutationError::IncoherentSuccessor);
            }
        }
        let detached = active.detached;
        Ok(self.finish(key, outcome, detached))
    }
}
