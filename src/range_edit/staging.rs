use super::*;

impl RangeEditCoordinator {
    pub fn accept_page(
        &mut self,
        page: MutationPage,
    ) -> Result<MutationPageAcceptance, MutationError> {
        let key = page.key().key();
        let result = self.accept_page_validated(&page);
        if matches!(result, Err(MutationError::PageCollision)) {
            let _ = self.finish(key, MutationOutcome::Error, false);
        }
        result
    }

    fn accept_page_validated(
        &mut self,
        page: &MutationPage,
    ) -> Result<MutationPageAcceptance, MutationError> {
        let limits = self.limits;
        let key = page.key();
        let state = self.active_for_key(key.key())?.state;
        if matches!(
            state,
            MutationState::FinishPending | MutationState::CommitPending
        ) {
            return Err(MutationError::PostFinishInput);
        }
        let active = self.active_mut(key.key(), MutationState::InputStreaming)?;
        let lane = active.lane(key.lane());

        if key.ordinal() < lane.next_ordinal {
            let Some(last) = lane.last_page else {
                return Err(MutationError::ObsoleteOperation(key.key()));
            };
            if key == last.key {
                return if page.page_identity() == last.page_identity
                    && page.cumulative_identity() == last.cumulative_identity
                {
                    Ok(MutationPageAcceptance::Replay)
                } else {
                    Err(MutationError::PageCollision)
                };
            }
            return Err(MutationError::ObsoleteOperation(key.key()));
        }
        if key.ordinal() != lane.next_ordinal {
            return Err(MutationError::OrdinalMismatch {
                expected: lane.next_ordinal,
                actual: key.ordinal(),
            });
        }
        if key.cursor() != lane.next_cursor {
            return Err(MutationError::CursorMismatch);
        }
        if key.prior() != lane.cumulative_identity {
            return Err(MutationError::PriorIdentityMismatch);
        }
        if page.items().len() > limits.max_page_items() {
            return Err(MutationError::PageItemLimitExceeded);
        }
        if page.totals().retained_bytes > limits.max_page_bytes() as u64 {
            return Err(MutationError::PageByteLimitExceeded);
        }
        if page.totals().objects > limits.max_page_objects() as u64 {
            return Err(MutationError::ObjectLimitExceeded);
        }
        if page.totals().object_bytes > limits.max_page_object_bytes() as u64 {
            return Err(MutationError::ObjectByteLimitExceeded);
        }
        if page.totals().presentation_bytes > limits.max_page_presentation_bytes() as u64 {
            return Err(MutationError::PresentationByteLimitExceeded);
        }

        let mut proposal_candidate = ProposalPageCandidate {
            sequence: active.sequence,
            active_object_effect: active.active_object_effect,
        };
        if key.lane() == MutationLane::Proposal {
            validate_proposal_page(
                active.proposal,
                active.tracked_active_object,
                &mut proposal_candidate,
                page.items(),
            )?;
        }
        let next_totals = lane
            .totals
            .checked_add(page.totals())
            .ok_or(MutationError::CumulativeOverflow)?;
        let next_ordinal = lane
            .next_ordinal
            .checked_add(1)
            .ok_or(MutationError::CumulativeOverflow)?;
        if key.lane() == MutationLane::Proposal {
            active.sequence = proposal_candidate.sequence;
            active.active_object_effect = proposal_candidate.active_object_effect;
        }
        let lane = active.lane_mut(key.lane());
        lane.totals = next_totals;
        lane.next_cursor = page.next_cursor();
        lane.next_ordinal = next_ordinal;
        lane.cumulative_identity = page.cumulative_identity();
        lane.last_page = Some(PageReceipt {
            key,
            page_identity: page.page_identity(),
            cumulative_identity: page.cumulative_identity(),
        });
        Ok(MutationPageAcceptance::Accepted {
            next_cursor: lane.next_cursor,
            next_ordinal: lane.next_ordinal,
            cumulative_identity: lane.cumulative_identity,
            totals: lane.totals,
        })
    }

    pub fn finish_input(&mut self, finish: MutationFinishInput) -> Result<(), MutationError> {
        let active = self.active_mut(finish.key(), MutationState::InputStreaming)?;
        if active.source.finish() != finish.source()
            || active.proposal_lane.finish() != finish.proposal()
            || expected_successor_extent(active)? != finish.intended_extent()
        {
            return Err(MutationError::FinishMismatch);
        }
        validate_intended(active, finish.intended_extent(), finish.intended())?;
        active.intended = Some(finish.intended());
        active.intended_extent = Some(finish.intended_extent());
        active.state = MutationState::FinishPending;
        Ok(())
    }

    pub fn admit_commit(
        &mut self,
        key: MutationKey,
    ) -> Result<MutationCommitRequest, MutationError> {
        let active = self.active_mut(key, MutationState::FinishPending)?;
        let intended = active.intended.ok_or(MutationError::MissingFinishInput)?;
        let intended_extent = active
            .intended_extent
            .ok_or(MutationError::MissingFinishInput)?;
        validate_intended(active, intended_extent, intended)?;
        active.state = MutationState::CommitPending;
        let finish_identity = finish_identity(active);
        Ok(MutationCommitRequest::new(key, finish_identity))
    }
}

fn validate_proposal_page(
    proposal: MutationProposal,
    tracked_active_object: Option<(InlineObjectId, InlineObjectOrder)>,
    candidate: &mut ProposalPageCandidate,
    items: &[MutationPageItem],
) -> Result<(), MutationError> {
    for item in items {
        match item {
            MutationPageItem::Utf8 {
                inserted_offset,
                text,
            } => {
                if *inserted_offset != candidate.sequence.inserted_bytes {
                    return Err(MutationError::InsertOffsetMismatch {
                        expected: candidate.sequence.inserted_bytes,
                        actual: *inserted_offset,
                    });
                }
                candidate.sequence.inserted_bytes = candidate
                    .sequence
                    .inserted_bytes
                    .checked_add(text.len() as u64)
                    .ok_or(MutationError::CumulativeOverflow)?;
                candidate.sequence.inserted_line_breaks = candidate
                    .sequence
                    .inserted_line_breaks
                    .checked_add(text.bytes().filter(|byte| *byte == b'\n').count() as u64)
                    .ok_or(MutationError::CumulativeOverflow)?;
            }
            MutationPageItem::Atom(change) => {
                validate_atom_change(proposal, &mut candidate.sequence, change)?
            }
            MutationPageItem::Object(change) => {
                validate_object_change(proposal, tracked_active_object, candidate, *change)?
            }
        }
    }
    Ok(())
}

fn validate_atom_change(
    proposal: MutationProposal,
    sequence: &mut MutationSequenceState,
    change: &AtomChange,
) -> Result<(), MutationError> {
    match change {
        AtomChange::Insert {
            id, inserted_range, ..
        } => {
            if inserted_range.is_empty() || inserted_range.end().get() > sequence.inserted_bytes {
                return Err(MutationError::MalformedAtomChange);
            }
            if let Some((prior_id, prior_range)) = sequence.last_inserted_atom {
                if *id == prior_id || inserted_range.start() < prior_range.end() {
                    return Err(MutationError::InsertedAtomRangeOutOfOrder {
                        previous: prior_range,
                        actual: *inserted_range,
                    });
                }
            }
            sequence.last_inserted_atom = Some((*id, *inserted_range));
        }
        AtomChange::Remove { id, source_range } => {
            if source_range.is_empty() || !proposal.replacement_bytes().contains(*source_range) {
                return Err(MutationError::MalformedAtomChange);
            }
            if let Some((prior_id, prior_range)) = sequence.last_removed_atom {
                if *id == prior_id || source_range.start() < prior_range.end() {
                    return Err(MutationError::RemovedAtomRangeOutOfOrder {
                        previous: prior_range,
                        actual: *source_range,
                    });
                }
            }
            sequence.last_removed_atom = Some((*id, *source_range));
        }
    }
    Ok(())
}

fn validate_object_change(
    proposal: MutationProposal,
    tracked_active_object: Option<(InlineObjectId, InlineObjectOrder)>,
    candidate: &mut ProposalPageCandidate,
    change: ObjectChange,
) -> Result<(), MutationError> {
    let replacement = proposal.replacement();
    let (target, successor) = match change {
        ObjectChange::Insert { object } => (None, Some(object)),
        ObjectChange::Remove { target } => (Some(target), None),
        ObjectChange::Replace { target, object } | ObjectChange::Move { target, object } => {
            (Some(target), Some(object))
        }
    };
    if target.is_some_and(|target| !source_range_contains(replacement, target.range())) {
        return Err(MutationError::ObjectChangeOutsideReplacement);
    }
    if let ObjectChange::Move { target, object } = change
        && target.id() != object.id()
    {
        return Err(MutationError::MalformedObjectChange);
    }
    if let Some(target) = target {
        if let Some(previous) = candidate.sequence.last_object_target
            && (!matches!(
                positions_cmp(previous.range().end(), target.range().start()),
                Some(std::cmp::Ordering::Less | std::cmp::Ordering::Equal)
            ) || previous.id() == target.id())
        {
            return Err(MutationError::MalformedObjectChange);
        }
        candidate.sequence.last_object_target = Some(target);
        if tracked_active_object == Some((target.id(), target.order())) {
            candidate.active_object_effect = match change {
                ObjectChange::Remove { .. } => Some(ActiveObjectEffect::Removed {
                    id: target.id(),
                    order: target.order(),
                }),
                ObjectChange::Replace { .. } => Some(ActiveObjectEffect::Replaced {
                    id: target.id(),
                    order: target.order(),
                }),
                _ => candidate.active_object_effect,
            };
        }
    }
    if let Some(successor) = successor {
        if let Some(previous) = candidate.sequence.last_successor_object {
            let previous_key = (previous.anchor(), previous.order(), previous.id());
            let actual_key = (successor.anchor(), successor.order(), successor.id());
            if previous.anchor() == successor.anchor() && previous.order() == successor.order() {
                return Err(MutationError::DuplicateSuccessorObjectOrder {
                    anchor: successor.anchor(),
                    order: successor.order(),
                });
            }
            if previous_key >= actual_key {
                return Err(MutationError::SuccessorObjectsOutOfOrder);
            }
        }
        candidate.sequence.last_successor_object = Some(successor);
    }
    Ok(())
}

fn validate_intended(
    active: &ActiveMutation,
    intended_extent: LogicalExtent,
    intended: MutationPositions,
) -> Result<(), MutationError> {
    let expected_bytes = intended_extent.byte_len();
    for position in [
        intended.caret(),
        intended.selection_anchor(),
        intended.selection_head(),
    ] {
        if position.byte_offset.get() > expected_bytes {
            return Err(MutationError::IncoherentSuccessor);
        }
    }
    if active
        .sequence
        .last_successor_object
        .is_some_and(|object| object.anchor().get() > expected_bytes)
    {
        return Err(MutationError::SuccessorObjectOutsideExtent);
    }
    Ok(())
}

pub(super) fn expected_successor_bytes(active: &ActiveMutation) -> Result<u64, MutationError> {
    active
        .base_extent
        .byte_len()
        .checked_sub(active.proposal.replacement_bytes().len())
        .and_then(|bytes| bytes.checked_add(active.sequence.inserted_bytes))
        .ok_or(MutationError::IncoherentSuccessor)
}

pub(super) fn expected_successor_extent(
    active: &ActiveMutation,
) -> Result<LogicalExtent, MutationError> {
    let expected_bytes = expected_successor_bytes(active)?;
    let base_breaks = active
        .base_extent
        .line_count()
        .checked_sub(u64::from(active.base_extent.byte_len() != 0))
        .ok_or(MutationError::IncoherentSuccessor)?;
    let expected_breaks = base_breaks
        .checked_sub(active.proposal.replacement_line_breaks())
        .and_then(|breaks| breaks.checked_add(active.sequence.inserted_line_breaks))
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
    Ok(LogicalExtent::new(expected_bytes, expected_lines))
}

pub(super) fn finish_identity(active: &ActiveMutation) -> MutationIdentity {
    canonical_finish_identity(
        active.proposal,
        active.base_extent,
        active.initial_source_cursor,
        active.initial_proposal_cursor,
        active.source.finish(),
        active.proposal_lane.finish(),
        active
            .intended_extent
            .expect("finish input fixes intended extent"),
        active
            .intended
            .expect("finish input fixes intended positions"),
        active
            .source
            .totals
            .checked_add(active.proposal_lane.totals)
            .expect("accepted cumulative totals were checked"),
    )
}

fn source_range_contains(outer: SourceRange, inner: SourceRange) -> bool {
    matches!(
        positions_cmp(outer.start(), inner.start()),
        Some(std::cmp::Ordering::Less | std::cmp::Ordering::Equal)
    ) && matches!(
        positions_cmp(inner.end(), outer.end()),
        Some(std::cmp::Ordering::Less | std::cmp::Ordering::Equal)
    )
}

fn positions_cmp(left: SourcePosition, right: SourcePosition) -> Option<std::cmp::Ordering> {
    left.compare_in_revision(right)
}
