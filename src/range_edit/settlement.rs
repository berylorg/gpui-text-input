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
        if !matches!(
            active.state,
            MutationState::CommitPending | MutationState::DetachedCommit
        ) {
            return Err(MutationError::WrongState {
                expected: MutationState::CommitPending,
                actual: active.state,
            });
        }
        if let MutationOutcome::Committed(commit) = outcome {
            let expected_bytes = active
                .base_extent
                .byte_len()
                .checked_sub(active.proposal.replacement_bytes().len())
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
            let successor = commit.binding();
            if successor.binding() != key.binding()
                || successor.revision() == key.base_revision()
                || successor.extent().byte_len() != expected_bytes
                || successor.extent().line_count() != expected_lines
            {
                return Err(MutationError::IncoherentSuccessor);
            }
            if active.intended != Some(commit.positions()) {
                return Err(MutationError::WrongSuccessorPositions);
            }
            for position in [
                commit.positions().caret(),
                commit.positions().selection_anchor(),
                commit.positions().selection_head(),
            ] {
                let point = ByteRange::new(position.byte_offset, position.byte_offset)
                    .map_err(|_| MutationError::IncoherentSuccessor)?;
                successor
                    .extent()
                    .check_byte_range(point)
                    .map_err(|_| MutationError::IncoherentSuccessor)?;
            }
            let proofs = commit.proofs().as_array();
            if proofs.iter().any(|proof| proof.binding() != successor) {
                return Err(MutationError::StalePositionProof);
            }
        }
        let detached = active.detached;
        if let MutationOutcome::Committed(commit) = outcome {
            let proofs = commit.proofs().as_array();
            let mut text_pages = Vec::with_capacity(3);
            let mut object_pages = Vec::with_capacity(3);
            for proof in proofs {
                if let Some(page) = proof.text_page()
                    && !text_pages.contains(&page)
                {
                    text_pages.push(page);
                }
                if !object_pages.contains(&proof.object_page()) {
                    object_pages.push(proof.object_page());
                }
            }
            self.record_release(MutationCounts {
                proofs: 3,
                source_pages: text_pages.len() + object_pages.len(),
                ..MutationCounts::default()
            });
        }
        Ok(self.finish(key, outcome, detached))
    }
}
