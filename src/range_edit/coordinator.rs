use super::*;

impl RangeEditCoordinator {
    pub const fn new(binding: RangeBinding, limits: MutationLimits) -> Self {
        Self {
            binding,
            limits,
            active: None,
            last_terminal: None,
            released: MutationCounts {
                fragments: 0,
                staged_bytes: 0,
                objects: 0,
                object_bytes: 0,
                presentation_bytes: 0,
                proofs: 0,
                source_pages: 0,
                transactions: 0,
            },
        }
    }

    pub const fn binding(&self) -> RangeBinding {
        self.binding
    }

    pub fn state(&self) -> MutationState {
        self.active
            .as_ref()
            .map_or(MutationState::Idle, |active| active.state)
    }

    pub fn active_key(&self) -> Option<MutationKey> {
        self.active.as_ref().map(|active| active.proposal.key())
    }

    pub fn counts(&self) -> MutationCounts {
        self.active
            .as_ref()
            .map_or(MutationCounts::default(), ActiveMutation::counts)
    }

    pub const fn released_counts(&self) -> MutationCounts {
        self.released
    }

    pub fn staged_fragments(&self) -> &[MutationFragment] {
        self.active
            .as_ref()
            .map_or(&[], |active| active.fragments.as_slice())
    }

    pub fn reserve_source_positions(
        &mut self,
        key: MutationKey,
        positions: &[SourcePosition],
        text: &RangeResidency,
        objects: &ObjectResidency,
    ) -> Result<(), MutationError> {
        let max_proofs = max_position_proofs(self.limits)?;
        if positions.len() > max_proofs {
            return Err(MutationError::PositionProofLimitExceeded);
        }
        let active = self.active_for_key(key)?;
        if active.state != MutationState::Staging {
            return Err(MutationError::WrongState {
                expected: MutationState::Staging,
                actual: active.state,
            });
        }
        let binding = RangeBinding::new(key.binding(), key.base_revision(), active.base_extent);
        let mut proofs = Vec::with_capacity(positions.len());
        for (index, position) in positions.iter().copied().enumerate() {
            if positions[..index].contains(&position) {
                return Err(MutationError::DuplicatePositionProof(position));
            }
            let proof =
                SourcePositionProof::from_admitted_sources(binding, position, text, objects)?;
            proofs.push(proof);
        }
        self.reserve_owned_source_proofs(key, proofs)
    }

    pub(crate) fn reserve_owned_source_proofs(
        &mut self,
        key: MutationKey,
        proofs: Vec<SourcePositionProof>,
    ) -> Result<(), MutationError> {
        if proofs.len() > max_position_proofs(self.limits)? {
            return Err(MutationError::PositionProofLimitExceeded);
        }
        let active = self.active_mut(key, MutationState::Staging)?;
        if proofs.is_empty() {
            return Err(MutationError::MissingPositionProof(
                active.proposal.replacement().start(),
            ));
        }
        if active.proof_count != 0 {
            return Err(MutationError::DuplicatePositionProof(
                proofs
                    .first()
                    .map_or(active.proposal.replacement().start(), |proof| {
                        proof.position()
                    }),
            ));
        }
        let mut text_pages = Vec::with_capacity(proofs.len());
        let mut object_pages = Vec::with_capacity(proofs.len());
        for (index, proof) in proofs.iter().copied().enumerate() {
            if proof.binding().binding() != key.binding()
                || proof.binding().revision() != key.base_revision()
                || proof.binding().extent() != active.base_extent
            {
                return Err(MutationError::StalePositionProof);
            }
            if proofs[..index]
                .iter()
                .any(|prior| prior.position() == proof.position())
            {
                return Err(MutationError::DuplicatePositionProof(proof.position()));
            }
            if let Some(page) = proof.text_page()
                && !text_pages.contains(&page)
            {
                text_pages.push(page);
            }
            if !object_pages.contains(&proof.object_page()) {
                object_pages.push(proof.object_page());
            }
        }
        active.proof_count = proofs.len();
        active.source_page_count = text_pages.len() + object_pages.len();
        active.source_proofs = proofs;
        Ok(())
    }

    pub fn admit_commit(&mut self, key: MutationKey) -> Result<(), MutationError> {
        let active = self.active_mut(key, MutationState::Staging)?;
        if !active.terminal_seen {
            return Err(MutationError::MissingTerminalFragment);
        }
        let required = required_base_positions(active.proposal, &active.fragments);
        for position in required.iter().copied() {
            if !active
                .source_proofs
                .iter()
                .any(|proof| proof.position() == position)
            {
                return Err(MutationError::MissingPositionProof(position));
            }
        }
        for proof in active.source_proofs.iter().copied() {
            if !required.contains(&proof.position()) {
                return Err(MutationError::UnexpectedPositionProof(proof.position()));
            }
        }
        let expected_bytes = active
            .base_extent
            .byte_len()
            .checked_sub(active.proposal.replacement_bytes().len())
            .and_then(|bytes| bytes.checked_add(active.inserted_bytes))
            .ok_or(MutationError::IncoherentSuccessor)?;
        let intended = active
            .intended
            .ok_or(MutationError::MissingTerminalFragment)?;
        if [
            intended.caret(),
            intended.selection_anchor(),
            intended.selection_head(),
        ]
        .iter()
        .any(|position| position.byte_offset.get() > expected_bytes)
        {
            return Err(MutationError::IncoherentSuccessor);
        }
        for fragment in &active.fragments {
            let object = match fragment.payload() {
                MutationFragmentPayload::Object(ObjectChange::Insert { object, .. })
                | MutationFragmentPayload::Object(ObjectChange::Replace { object, .. })
                | MutationFragmentPayload::Object(ObjectChange::Move { object, .. }) => object,
                _ => continue,
            };
            if object.anchor().get() > expected_bytes {
                return Err(MutationError::SuccessorObjectOutsideExtent);
            }
        }
        active.state = MutationState::CommitPending;
        Ok(())
    }

    pub fn cancel(&mut self, key: MutationKey) -> Result<MutationCancellation, MutationError> {
        let state = self.active_for_key(key)?.state;
        match state {
            MutationState::Preflight | MutationState::Staging => {
                self.finish(key, MutationOutcome::Cancelled, false);
                Ok(MutationCancellation::Cancelled)
            }
            MutationState::CommitPending | MutationState::DetachedCommit => {
                Ok(MutationCancellation::AwaitingHostSettlement)
            }
            MutationState::Idle => Err(MutationError::NoActive),
        }
    }

    pub(super) fn check_key(&self, key: MutationKey) -> Result<(), MutationError> {
        let expected = MutationKey::new(
            self.binding.binding(),
            self.binding.revision(),
            key.operation(),
        );
        if key.binding() != expected.binding() || key.base_revision() != expected.base_revision() {
            return Err(MutationError::WrongKey {
                expected,
                actual: key,
            });
        }
        Ok(())
    }

    pub(super) fn active_for_key(
        &self,
        key: MutationKey,
    ) -> Result<&ActiveMutation, MutationError> {
        let Some(active) = &self.active else {
            return if self.last_terminal == Some(key) {
                Err(MutationError::ObsoleteOperation(key))
            } else {
                Err(MutationError::NoActive)
            };
        };
        if active.proposal.key() != key {
            return Err(MutationError::WrongKey {
                expected: active.proposal.key(),
                actual: key,
            });
        }
        Ok(active)
    }

    pub(super) fn active_mut(
        &mut self,
        key: MutationKey,
        expected: MutationState,
    ) -> Result<&mut ActiveMutation, MutationError> {
        let active = self.active_for_key(key)?;
        if active.state != expected {
            return Err(MutationError::WrongState {
                expected,
                actual: active.state,
            });
        }
        Ok(self.active.as_mut().expect("active checked"))
    }

    pub(super) fn finish(
        &mut self,
        key: MutationKey,
        outcome: MutationOutcome,
        obsolete: bool,
    ) -> MutationSettlement {
        let active = self.active.take().expect("active transaction checked");
        self.record_release(active.counts());
        self.last_terminal = Some(key);
        if !obsolete {
            if let MutationOutcome::Committed(successor) = outcome {
                self.binding = successor.binding();
            }
            MutationSettlement::Current(outcome)
        } else {
            MutationSettlement::Obsolete(outcome)
        }
    }

    pub(super) fn record_release(&mut self, released: MutationCounts) {
        self.released = self
            .released
            .checked_add(released)
            .expect("bounded mutation release accounting cannot overflow");
    }
}

pub(crate) fn required_base_positions(
    proposal: MutationProposal,
    fragments: &[MutationFragment],
) -> Vec<SourcePosition> {
    let mut positions =
        Vec::with_capacity(2_usize.saturating_add(fragments.len().saturating_mul(3)));
    let mut push = |position| {
        if !positions.contains(&position) {
            positions.push(position);
        }
    };
    push(proposal.replacement().start());
    push(proposal.replacement().end());
    for fragment in fragments {
        let MutationFragmentPayload::Object(change) = fragment.payload() else {
            continue;
        };
        match change {
            ObjectChange::Insert { at, .. } => push(*at),
            ObjectChange::Remove { target } | ObjectChange::Replace { target, .. } => {
                push(target.range().start());
                push(target.range().end());
            }
            ObjectChange::Move { target, to, .. } => {
                push(target.range().start());
                push(target.range().end());
                push(*to);
            }
        }
    }
    positions
}

fn max_position_proofs(limits: MutationLimits) -> Result<usize, MutationError> {
    limits
        .max_objects()
        .checked_mul(3)
        .and_then(|count| count.checked_add(2))
        .ok_or(MutationError::PositionProofLimitExceeded)
}

impl ActiveMutation {
    pub(super) fn counts(&self) -> MutationCounts {
        MutationCounts {
            fragments: self.fragment_count,
            staged_bytes: self.staged_bytes,
            objects: self.object_count,
            object_bytes: self.object_bytes,
            presentation_bytes: self.presentation_bytes,
            proofs: self.proof_count,
            source_pages: self.source_page_count,
            transactions: 1,
        }
    }

    pub(super) fn release_staging(&mut self) -> MutationCounts {
        let counts = MutationCounts {
            fragments: self.fragment_count,
            staged_bytes: self.staged_bytes,
            objects: self.object_count,
            object_bytes: self.object_bytes,
            presentation_bytes: self.presentation_bytes,
            proofs: self.proof_count,
            source_pages: self.source_page_count,
            transactions: 0,
        };
        self.fragment_count = 0;
        self.staged_bytes = 0;
        self.object_count = 0;
        self.object_bytes = 0;
        self.presentation_bytes = 0;
        self.proof_count = 0;
        self.source_page_count = 0;
        self.fragments.clear();
        self.source_proofs.clear();
        counts
    }
}
