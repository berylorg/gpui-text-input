use super::*;

impl RangeEditCoordinator {
    pub fn begin(&mut self, request: MutationBeginRequest) -> Result<(), MutationError> {
        let proposal = request.proposal();
        if let Some(active) = &self.active {
            return Err(MutationError::Busy(active.proposal.key()));
        }
        self.check_key(proposal.key())?;
        let base_extent = self.binding.extent();
        let begin_identity = canonical_begin_identity(
            proposal,
            base_extent,
            request.source_cursor(),
            request.proposal_cursor(),
        );
        if let Some(high_water) = self.operation_high_water {
            match proposal.key().operation().cmp(&high_water) {
                std::cmp::Ordering::Less => {
                    return Err(MutationError::ObsoleteOperation(proposal.key()));
                }
                std::cmp::Ordering::Equal => {
                    return if self.high_water_begin_identity == Some(begin_identity) {
                        Err(MutationError::ObsoleteOperation(proposal.key()))
                    } else {
                        Err(MutationError::OperationCollision)
                    };
                }
                std::cmp::Ordering::Greater => {}
            }
        }
        if self
            .binding
            .extent()
            .check_byte_range(proposal.replacement_bytes())
            .is_err()
        {
            return Err(MutationError::ReplacementOutsideExtent);
        }
        validate_base_extent(base_extent)?;
        validate_replacement_line_breaks(proposal, base_extent)?;
        for position in [
            proposal.predecessor().caret(),
            proposal.predecessor().selection_anchor(),
            proposal.predecessor().selection_head(),
            proposal.replacement().start(),
            proposal.replacement().end(),
        ] {
            let point = ByteRange::new(position.byte_offset, position.byte_offset)
                .map_err(|_| MutationError::PositionOutsideExtent)?;
            base_extent
                .check_byte_range(point)
                .map_err(|_| MutationError::PositionOutsideExtent)?;
        }
        self.active = Some(ActiveMutation {
            proposal,
            base_extent,
            state: MutationState::PreflightPending,
            source: LaneState::new(request.source_cursor()),
            proposal_lane: LaneState::new(request.proposal_cursor()),
            intended: None,
            intended_extent: None,
            initial_source_cursor: request.source_cursor(),
            initial_proposal_cursor: request.proposal_cursor(),
            detached: false,
            sequence: MutationSequenceState::default(),
            tracked_active_object: None,
            active_object_effect: None,
        });
        self.operation_high_water = Some(proposal.key().operation());
        self.high_water_begin_identity = Some(begin_identity);
        self.ever_started = true;
        Ok(())
    }

    pub fn accept_preflight(
        &mut self,
        key: MutationKey,
        active_object: Option<(InlineObjectId, InlineObjectOrder)>,
    ) -> Result<(), MutationError> {
        let active = self.active_mut(key, MutationState::PreflightPending)?;
        active.tracked_active_object = active_object;
        active.state = MutationState::InputStreaming;
        Ok(())
    }

    pub fn reject_preflight(
        &mut self,
        key: MutationKey,
    ) -> Result<MutationSettlement, MutationError> {
        self.active_mut(key, MutationState::PreflightPending)?;
        Ok(self.finish(key, MutationOutcome::Rejected, false))
    }

    pub fn reject_input(&mut self, key: MutationKey) -> Result<MutationSettlement, MutationError> {
        let state = self.active_for_key(key)?.state;
        if !matches!(
            state,
            MutationState::InputStreaming | MutationState::FinishPending
        ) {
            return Err(MutationError::WrongState {
                expected: MutationState::InputStreaming,
                actual: state,
            });
        }
        Ok(self.finish(key, MutationOutcome::Rejected, false))
    }
}

fn validate_base_extent(base_extent: LogicalExtent) -> Result<(), MutationError> {
    match (base_extent.byte_len(), base_extent.line_count()) {
        (0, 0) => Ok(()),
        (0, _) | (_, 0) => Err(MutationError::MalformedBaseExtent),
        (bytes, lines) if lines - 1 <= bytes => Ok(()),
        _ => Err(MutationError::MalformedBaseExtent),
    }
}

fn validate_replacement_line_breaks(
    proposal: MutationProposal,
    base_extent: LogicalExtent,
) -> Result<(), MutationError> {
    let base_breaks = base_extent
        .line_count()
        .checked_sub(u64::from(base_extent.byte_len() != 0))
        .ok_or(MutationError::MalformedBaseExtent)?;
    let removed_breaks = proposal.replacement_line_breaks();
    let replacement = proposal.replacement_bytes();
    let replaces_whole_source =
        replacement.start().get() == 0 && replacement.end().get() == base_extent.byte_len();
    if removed_breaks > replacement.len()
        || removed_breaks > base_breaks
        || (replacement.is_empty() && removed_breaks != 0)
        || (replaces_whole_source && removed_breaks != base_breaks)
    {
        return Err(MutationError::MalformedReplacementLineBreaks);
    }
    Ok(())
}
