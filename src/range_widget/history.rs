use gpui::Context;

use crate::{
    LogicalExtent, MutationError, MutationFragment, MutationFragmentPayload, MutationKey,
    MutationKind, MutationPositions, MutationProposal, ObjectResidency, OperationId,
    RangeHistoryIntent, RangeHistoryPlan, RangeResidency, RangeTextInput, RangeTextInputError,
    RangeTextInputRequest, SourcePosition,
};

#[derive(Clone, Copy, Debug)]
struct PlannedHistory {
    proposal: MutationProposal,
    positions: MutationPositions,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct PendingHistory {
    intent: RangeHistoryIntent,
    plan: Option<PlannedHistory>,
}

impl PendingHistory {
    pub const fn intent(self) -> RangeHistoryIntent {
        self.intent
    }
    pub const fn is_planned(self) -> bool {
        self.plan.is_some()
    }
}

impl RangeTextInput {
    pub(super) fn request_history(&mut self, kind: MutationKind, cx: &mut Context<Self>) {
        if !self.enabled
            || self.read_only
            || self.interactive_surface().is_none()
            || self.pending_history.is_some()
            || !matches!(self.edits.state(), crate::MutationState::Idle)
        {
            return;
        }
        let key = MutationKey::new(
            self.config.binding.binding(),
            self.config.binding.revision(),
            OperationId::new(self.next_id()),
        );
        let intent = RangeHistoryIntent::new(key, kind);
        self.pending_history = Some(PendingHistory { intent, plan: None });
        self.push_request(RangeTextInputRequest::HistoryIntent(intent), cx);
    }

    /// Opens the ordinary staged mutation boundary for one exact host-owned history intent.
    pub fn submit_history_plan(
        &mut self,
        plan: RangeHistoryPlan,
        base_positions: &[SourcePosition],
        text: &RangeResidency,
        objects: &ObjectResidency,
        cx: &mut Context<Self>,
    ) -> Result<MutationKey, RangeTextInputError> {
        if !self.mounted {
            return Err(RangeTextInputError::NotMounted);
        }
        if !self.enabled || self.read_only {
            return Err(RangeTextInputError::ReadOnly);
        }
        let pending = self.pending_history.ok_or(RangeTextInputError::Stale)?;
        let intent = plan.intent();
        let proposal = plan.proposal();
        if pending.is_planned()
            || pending.intent() != intent
            || proposal.key() != intent.key()
            || proposal.kind() != intent.kind()
            || !matches!(intent.kind(), MutationKind::Undo | MutationKind::Redo)
            || intent.key().binding() != self.config.binding.binding()
            || intent.key().base_revision() != self.config.binding.revision()
        {
            return Err(RangeTextInputError::Stale);
        }
        for position in crate::range_edit::required_base_positions(proposal, &[]) {
            if !base_positions.contains(&position) {
                return Err(MutationError::MissingPositionProof(position).into());
            }
        }
        self.admit_edit_positions(base_positions, text, objects)?;
        self.edits.begin(proposal)?;
        self.pending_history = Some(PendingHistory {
            intent,
            plan: Some(PlannedHistory {
                proposal,
                positions: plan.positions(),
            }),
        });
        self.push_request(RangeTextInputRequest::MutationPreflight(proposal), cx);
        Ok(proposal.key())
    }

    /// Admits one host-provided history fragment through the shared bounded staging owner.
    pub fn stage_history_fragment(
        &mut self,
        fragment: MutationFragment,
        cx: &mut Context<Self>,
    ) -> Result<(), RangeTextInputError> {
        let pending = self.pending_history.ok_or(RangeTextInputError::Stale)?;
        let plan = pending.plan.ok_or(RangeTextInputError::Stale)?;
        let key = pending.intent().key();
        if fragment.key() != key {
            return Err(MutationError::WrongKey {
                expected: key,
                actual: fragment.key(),
            }
            .into());
        }
        let terminal_positions = match fragment.payload() {
            MutationFragmentPayload::Terminal { intended } => Some(*intended),
            _ => None,
        };
        if let Err(error) = self.edits.stage(fragment.clone()) {
            self.fail_invalid_staging(key, cx);
            return Err(error.into());
        }
        if let Some(positions) = terminal_positions {
            let validation = self
                .expected_history_successor_extent(plan)
                .and_then(|expected| {
                    if positions != plan.positions {
                        return Err(MutationError::WrongSuccessorPositions.into());
                    }
                    for position in [
                        positions.caret(),
                        positions.selection_anchor(),
                        positions.selection_head(),
                    ] {
                        expected.check_byte_range(crate::ByteRange::new(
                            position.byte_offset,
                            position.byte_offset,
                        )?)?;
                    }
                    Ok(())
                });
            if let Err(error) = validation {
                self.fail_invalid_staging(key, cx);
                return Err(error);
            }
        }
        self.push_request(
            RangeTextInputRequest::MutationFragment { key, fragment },
            cx,
        );
        if let Some(positions) = terminal_positions {
            self.mutation_positions = Some((key, positions));
            self.push_request(RangeTextInputRequest::MutationCommit(key), cx);
        }
        Ok(())
    }

    fn expected_history_successor_extent(
        &self,
        plan: PlannedHistory,
    ) -> Result<LogicalExtent, RangeTextInputError> {
        let base = self.config.binding.extent();
        let (inserted_bytes, inserted_breaks) = self.edits.staged_fragments().iter().try_fold(
            (0_u64, 0_u64),
            |(bytes, breaks), fragment| -> Result<_, MutationError> {
                match fragment.payload() {
                    MutationFragmentPayload::Utf8 { text, .. } => Ok((
                        bytes
                            .checked_add(text.len() as u64)
                            .ok_or(MutationError::IncoherentSuccessor)?,
                        breaks
                            .checked_add(text.bytes().filter(|byte| *byte == b'\n').count() as u64)
                            .ok_or(MutationError::IncoherentSuccessor)?,
                    )),
                    MutationFragmentPayload::Atom(_)
                    | MutationFragmentPayload::Object(_)
                    | MutationFragmentPayload::Terminal { .. } => Ok((bytes, breaks)),
                }
            },
        )?;
        let expected_bytes = base
            .byte_len()
            .checked_sub(plan.proposal.replacement_bytes().len())
            .and_then(|bytes| bytes.checked_add(inserted_bytes))
            .ok_or(MutationError::IncoherentSuccessor)?;
        let expected_breaks = base
            .line_count()
            .checked_sub(u64::from(base.byte_len() != 0))
            .and_then(|breaks| breaks.checked_sub(plan.proposal.replacement_line_breaks()))
            .and_then(|breaks| breaks.checked_add(inserted_breaks))
            .ok_or(MutationError::IncoherentSuccessor)?;
        if expected_breaks > expected_bytes || (expected_bytes == 0 && expected_breaks != 0) {
            return Err(MutationError::IncoherentSuccessor.into());
        }
        let expected_lines = if expected_bytes == 0 {
            0
        } else {
            expected_breaks
                .checked_add(1)
                .ok_or(MutationError::IncoherentSuccessor)?
        };
        Ok(LogicalExtent::new(expected_bytes, expected_lines))
    }
}
