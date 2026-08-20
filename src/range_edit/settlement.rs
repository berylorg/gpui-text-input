use super::staging::expected_successor_extent;
use super::*;

impl RangeEditCoordinator {
    pub fn settle_committed(
        &mut self,
        key: MutationKey,
        binding: RangeBinding,
        positions: MutationPositions,
        text: &RangeResidency,
        objects: &ObjectResidency,
    ) -> Result<MutationSettlement, MutationError> {
        let commit = MutationCommit::from_admitted_sources(binding, positions, text, objects)?;
        self.settle(key, MutationOutcome::Committed(commit))
    }

    pub fn settle(
        &mut self,
        key: MutationKey,
        outcome: MutationOutcome,
    ) -> Result<MutationSettlement, MutationError> {
        let active = self.active_for_key(key)?;
        if active.state != MutationState::CommitPending {
            return Err(MutationError::WrongState {
                expected: MutationState::CommitPending,
                actual: active.state,
            });
        }
        if let MutationOutcome::Committed(commit) = outcome {
            validate_commit(active, key, commit)?;
        }
        let detached = active.detached;
        Ok(self.finish(key, outcome, detached))
    }
}

fn validate_commit(
    active: &ActiveMutation,
    key: MutationKey,
    commit: MutationCommit,
) -> Result<(), MutationError> {
    let expected_extent = expected_successor_extent(active)?;
    let successor = commit.binding();
    if successor.binding() != key.binding()
        || successor.revision() == key.base_revision()
        || successor.extent() != expected_extent
    {
        return Err(MutationError::IncoherentSuccessor);
    }
    if active.intended != Some(commit.positions()) {
        return Err(MutationError::WrongSuccessorPositions);
    }
    if commit
        .proofs()
        .as_array()
        .iter()
        .any(|proof| proof.binding() != successor)
    {
        return Err(MutationError::StalePositionProof);
    }
    Ok(())
}
