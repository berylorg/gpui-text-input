use gpui::{Context, Window};

use crate::{
    ByteOffset, ByteRange, LogicalExtent, MutationBeginRequest, MutationCursor, MutationError,
    MutationFinishInput, MutationIdentity, MutationKey, MutationKind, MutationLane,
    MutationOutcome, MutationPage, MutationPageItem, MutationPageKey, MutationPageRequest,
    MutationPositions, MutationProposal, MutationSettlement, MutationStreamFinish, ObjectResidency,
    RangeResidency, RangeTextInputError, RangeTextInputEvent, RangeTextInputRequest,
    SegmentationContinuation, SegmentationDirection, SegmentationKind, SourcePosition, SourceRange,
};

use super::RangeTextInput;

#[derive(Debug)]
pub(super) struct PendingLocalMutation {
    pub key: MutationKey,
    pub page: Option<MutationPage>,
    pub finish: MutationFinishInput,
}

#[derive(Clone, Copy, Debug)]
pub(super) enum PendingBoundaryAction {
    Move {
        extend: bool,
        direction: SegmentationDirection,
    },
    Delete {
        direction: SegmentationDirection,
    },
    SelectPointStart {
        origin: ByteOffset,
        kind: SegmentationKind,
    },
    SelectPointEnd {
        start: ByteOffset,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ActiveObjectActivationAttempt {
    NotApplicable,
    Activated,
    Rejected,
}

impl ActiveObjectActivationAttempt {
    pub(super) const fn consumes_key(self) -> bool {
        !matches!(self, Self::NotApplicable)
    }
}

impl RangeTextInput {
    pub fn active_inline_object(&self) -> Option<crate::RealizedInlineObjectAnchor> {
        self.active_object.map(|active| active.anchor)
    }

    pub fn remove_active_inline_object(
        &mut self,
        expected: crate::RealizedInlineObjectAnchor,
        cx: &mut Context<Self>,
    ) -> Result<crate::MutationKey, RangeTextInputError> {
        let active = self.active_object.ok_or(RangeTextInputError::Stale)?;
        if active.anchor != expected {
            return Err(RangeTextInputError::Stale);
        }
        let range = crate::SourceRange::new(active.leading, active.trailing)
            .map_err(|_| RangeTextInputError::Stale)?;
        self.begin_source_replacement(range, String::new(), crate::MutationKind::Edit, cx)
    }

    pub(super) fn active_from_geometry(
        &self,
        object: crate::RealizedInlineObjectGeometry,
    ) -> Option<super::ActiveInlineObject> {
        let surface = self.interactive_surface()?;
        let key = surface.geometry_key();
        let presentation = surface.presentation_for_geometry(object);
        Some(super::ActiveInlineObject {
            anchor: crate::RealizedInlineObjectAnchor {
                binding: surface.binding(),
                object_id: object.id(),
                order: object.order(),
                presentation_generation: key.presentation_generation(),
                layout_epoch: key.epoch(),
                bounds: object.bounds(),
            },
            leading: object.leading(),
            trailing: object.trailing(),
            activation_eligible: presentation.activation_eligible(),
        })
    }

    pub(super) fn clear_active_object(
        &mut self,
        reason: crate::InlineObjectRealizationLossReason,
        cx: &mut Context<Self>,
    ) {
        if self.active_object.is_none() {
            return;
        }
        if let Ok(candidate) = self.prepare_active_object_transition(
            super::transition::ActiveObjectTransition::Clear(reason),
        ) {
            self.commit_active_object_transition(candidate, cx);
        }
    }

    pub(super) fn activate_current_object(
        &mut self,
        key: crate::InlineObjectActivationKey,
        cx: &mut Context<Self>,
    ) -> ActiveObjectActivationAttempt {
        let Some(active) = self.active_object else {
            return ActiveObjectActivationAttempt::NotApplicable;
        };
        let Some(surface) = self.interactive_surface() else {
            self.clear_active_object(crate::InlineObjectRealizationLossReason::Unrealized, cx);
            return if active.activation_eligible {
                ActiveObjectActivationAttempt::Rejected
            } else {
                ActiveObjectActivationAttempt::NotApplicable
            };
        };
        let geometry = surface.geometry_key();
        if surface.binding() != active.anchor.binding
            || geometry.presentation_generation() != active.anchor.presentation_generation
            || geometry.epoch() != active.anchor.layout_epoch
        {
            self.clear_active_object(crate::InlineObjectRealizationLossReason::Unrealized, cx);
            return if active.activation_eligible {
                ActiveObjectActivationAttempt::Rejected
            } else {
                ActiveObjectActivationAttempt::NotApplicable
            };
        }
        let Some(object) = surface.object_selected_by(surface.selection()) else {
            self.clear_active_object(crate::InlineObjectRealizationLossReason::Unrealized, cx);
            return if active.activation_eligible {
                ActiveObjectActivationAttempt::Rejected
            } else {
                ActiveObjectActivationAttempt::NotApplicable
            };
        };
        let Some(current) = self.active_from_geometry(object) else {
            return if active.activation_eligible {
                ActiveObjectActivationAttempt::Rejected
            } else {
                ActiveObjectActivationAttempt::NotApplicable
            };
        };
        if current.anchor != active.anchor
            || current.activation_eligible != active.activation_eligible
        {
            self.clear_active_object(crate::InlineObjectRealizationLossReason::Unrealized, cx);
            return if active.activation_eligible {
                ActiveObjectActivationAttempt::Rejected
            } else {
                ActiveObjectActivationAttempt::NotApplicable
            };
        }
        if !current.activation_eligible {
            return ActiveObjectActivationAttempt::NotApplicable;
        }
        let Ok(candidate) =
            self.prepare_active_object_transition(super::transition::ActiveObjectTransition::Set {
                active: current,
                activation: Some(crate::InlineObjectInputOrigin::Keyboard { key }),
            })
        else {
            return ActiveObjectActivationAttempt::Rejected;
        };
        self.commit_active_object_transition(candidate, cx);
        ActiveObjectActivationAttempt::Activated
    }

    pub fn begin_host_mutation(
        &mut self,
        request: MutationBeginRequest,
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
        let proposal = request.proposal();
        if proposal.kind() != crate::MutationKind::Edit {
            return Err(RangeTextInputError::UnsupportedMutationKind);
        }
        if !self.mutation_queue_has_capacity(proposal.key()) {
            return Err(RangeTextInputError::Busy);
        }
        self.config
            .settlement_coordinator
            .claim_host_operation(proposal.key().operation())?;
        for required in [
            proposal.predecessor().caret(),
            proposal.predecessor().selection_anchor(),
            proposal.predecessor().selection_head(),
            proposal.replacement().start(),
            proposal.replacement().end(),
        ] {
            if !base_positions.contains(&required) {
                return Err(MutationError::MissingPositionProof(required).into());
            }
        }
        self.admit_edit_positions(base_positions, text, objects)?;
        self.edits.begin(request)?;
        self.push_request(RangeTextInputRequest::MutationBegin(request), cx);
        Ok(proposal.key())
    }

    pub fn submit_mutation_page(
        &mut self,
        page: MutationPage,
        cx: &mut Context<Self>,
    ) -> Result<crate::MutationPageAcceptance, RangeTextInputError> {
        if !self.mutation_queue_has_capacity(page.key().key()) {
            return Err(RangeTextInputError::Busy);
        }
        let key = page.key().key();
        let lane = page.key().lane();
        let request = MutationPageRequest::new(page.clone());
        let was_active = self.edits.active_key() == Some(key);
        let acceptance = match self.edits.accept_page(page) {
            Ok(acceptance) => acceptance,
            Err(error) => {
                if was_active && self.edits.is_retired(key) {
                    self.finish_local_mutation(key, MutationOutcome::Error, cx);
                }
                return Err(error.into());
            }
        };
        match lane {
            MutationLane::Source => {
                self.push_request(RangeTextInputRequest::MutationSourcePage(request), cx)
            }
            MutationLane::Proposal => {
                self.push_request(RangeTextInputRequest::MutationProposalPage(request), cx)
            }
        }
        Ok(acceptance)
    }

    pub fn submit_mutation_finish(
        &mut self,
        finish: MutationFinishInput,
        cx: &mut Context<Self>,
    ) -> Result<(), RangeTextInputError> {
        if !self.mutation_queue_has_capacity(finish.key()) {
            return Err(RangeTextInputError::Busy);
        }
        self.edits.finish_input(finish)?;
        self.mutation_positions = Some((finish.key(), finish.intended()));
        self.push_request(RangeTextInputRequest::MutationFinishInput(finish), cx);
        Ok(())
    }

    pub fn admit_edit_positions(
        &mut self,
        positions: &[SourcePosition],
        text: &RangeResidency,
        objects: &ObjectResidency,
    ) -> Result<(), RangeTextInputError> {
        if !matches!(
            self.edits.state(),
            crate::MutationState::Idle | crate::MutationState::Settled
        ) {
            return Err(RangeTextInputError::Busy);
        }
        let max = self
            .config
            .mutation_limits
            .max_page_objects()
            .checked_mul(3)
            .and_then(|count| count.checked_add(2))
            .ok_or(MutationError::PositionProofLimitExceeded)?;
        if positions.len() > max {
            return Err(MutationError::PositionProofLimitExceeded.into());
        }
        let mut proofs = Vec::with_capacity(positions.len());
        for (index, position) in positions.iter().copied().enumerate() {
            if positions[..index].contains(&position) {
                return Err(MutationError::DuplicatePositionProof(position).into());
            }
            proofs.push(
                crate::range_edit::SourcePositionProof::from_admitted_sources(
                    self.config.binding,
                    position,
                    text,
                    objects,
                )?,
            );
        }
        self.admitted_edit_proofs = proofs;
        Ok(())
    }

    pub const fn adopted_mutation_positions(&self) -> Option<MutationPositions> {
        self.adopted_positions
    }

    pub(super) fn proven_no_object_range(
        &self,
        range: ByteRange,
    ) -> Result<(SourceRange, Vec<crate::range_edit::SourcePositionProof>), RangeTextInputError>
    {
        let mut proofs = Vec::with_capacity(2);
        let mut position = |offset| {
            let proof = self
                .admitted_edit_proofs
                .iter()
                .copied()
                .find(|proof| {
                    proof.binding() == self.config.binding
                        && proof.position().byte_offset == offset
                        && proof.position().gap == crate::InlineObjectGap::NoObjects
                })
                .ok_or(MutationError::MissingPositionProof(SourcePosition::new(
                    offset,
                    crate::InlineObjectGap::NoObjects,
                )))?;
            if !proofs.contains(&proof) {
                proofs.push(proof);
            }
            Ok::<_, RangeTextInputError>(proof.position())
        };
        let start = position(range.start())?;
        let end = position(range.end())?;
        let range = SourceRange::new(start, end).map_err(|_| RangeTextInputError::Stale)?;
        Ok((range, proofs))
    }

    pub(super) fn begin_source_replacement(
        &mut self,
        replacement: SourceRange,
        text: String,
        kind: MutationKind,
        cx: &mut Context<Self>,
    ) -> Result<MutationKey, RangeTextInputError> {
        if !self.enabled || self.read_only {
            return Err(RangeTextInputError::ReadOnly);
        }
        if self.pending_local_mutation.is_some() {
            return Err(RangeTextInputError::Busy);
        }
        let surface = self
            .interactive_surface()
            .ok_or(RangeTextInputError::Busy)?;
        let directed = super::RangeSourceSelection {
            anchor: replacement.start(),
            head: replacement.end(),
        };
        let selected_object = surface.object_selected_by(directed);
        if selected_object.is_none()
            && matches!(replacement.start().gap, crate::InlineObjectGap::NoObjects)
            && matches!(replacement.end().gap, crate::InlineObjectGap::NoObjects)
        {
            return self.begin_replacement(
                ByteRange::new(
                    replacement.start().byte_offset,
                    replacement.end().byte_offset,
                )?,
                text,
                kind,
                cx,
            );
        }
        if selected_object.is_none() && !replacement.is_empty() {
            return Err(RangeTextInputError::Pending);
        }
        let required_items = usize::from(!text.is_empty())
            .checked_add(usize::from(selected_object.is_some()))
            .ok_or(RangeTextInputError::SurfaceCapacity)?;
        if required_items > self.config.mutation_limits.max_page_items()
            || text.len() > self.config.mutation_limits.max_page_bytes()
        {
            return Err(RangeTextInputError::SurfaceCapacity);
        }
        let mut proofs = Vec::with_capacity(2);
        for position in [replacement.start(), replacement.end()] {
            if proofs
                .iter()
                .any(|proof: &crate::range_edit::SourcePositionProof| proof.position() == position)
            {
                continue;
            }
            proofs.push(crate::range_edit::SourcePositionProof::from_surface_pages(
                self.config.binding,
                position,
                surface.pages(),
                surface.object_pages(),
            )?);
        }
        let removed = selected_object
            .map(|object| crate::ObjectTarget::new(replacement, object.id(), object.order()))
            .transpose()?;
        let caret = successor_position(replacement, removed, text.len())?;
        let selection = surface.selection();
        let key = MutationKey::new(
            self.config.binding.binding(),
            self.config.binding.revision(),
            self.next_local_operation()?,
        );
        let predecessor = MutationPositions::new(selection.head, selection.anchor, selection.head);
        let proposal = MutationProposal::new(key, kind, predecessor, replacement, 0);
        let mut items = Vec::with_capacity(required_items);
        if let Some(target) = removed {
            items.push(MutationPageItem::Object(crate::ObjectChange::Remove {
                target,
            }));
        }
        if !text.is_empty() {
            items.push(MutationPageItem::Utf8 {
                inserted_offset: 0,
                text: text.into_boxed_str(),
            });
        }
        self.begin_local_mutation(proposal, items, MutationPositions::collapsed(caret), cx)?;
        self.admitted_edit_proofs = proofs;
        Ok(key)
    }

    pub(super) fn begin_replacement_with_lines(
        &mut self,
        range: ByteRange,
        removed_line_breaks: u64,
        text: String,
        kind: MutationKind,
        cx: &mut Context<Self>,
    ) -> Result<MutationKey, RangeTextInputError> {
        if !self.enabled || self.read_only {
            return Err(RangeTextInputError::ReadOnly);
        }
        if self.pending_local_mutation.is_some() {
            return Err(RangeTextInputError::Busy);
        }
        if usize::from(!text.is_empty()) > self.config.mutation_limits.max_page_items()
            || text.len() > self.config.mutation_limits.max_page_bytes()
        {
            return Err(RangeTextInputError::SurfaceCapacity);
        }
        self.config.binding.extent().check_byte_range(range)?;
        let (replacement, proofs) = self.proven_no_object_range(range)?;
        let surface = self
            .interactive_surface()
            .ok_or(RangeTextInputError::Busy)?;
        let selection = surface.selection();
        let key = MutationKey::new(
            self.config.binding.binding(),
            self.config.binding.revision(),
            self.next_local_operation()?,
        );
        let predecessor = MutationPositions::new(selection.head, selection.anchor, selection.head);
        let proposal =
            MutationProposal::new(key, kind, predecessor, replacement, removed_line_breaks);
        let caret = SourcePosition::new(
            ByteOffset::new(range.start().get().saturating_add(text.len() as u64)),
            crate::InlineObjectGap::NoObjects,
        );
        let items = if text.is_empty() {
            Vec::new()
        } else {
            vec![MutationPageItem::Utf8 {
                inserted_offset: 0,
                text: text.into_boxed_str(),
            }]
        };
        self.begin_local_mutation(proposal, items, MutationPositions::collapsed(caret), cx)?;
        self.admitted_edit_proofs = proofs;
        Ok(key)
    }

    pub(super) fn begin_local_mutation(
        &mut self,
        proposal: MutationProposal,
        items: Vec<MutationPageItem>,
        intended: MutationPositions,
        cx: &mut Context<Self>,
    ) -> Result<(), RangeTextInputError> {
        let operation = match self.prepared_local_operation.take() {
            Some(operation) if operation == proposal.key().operation() => operation,
            Some(_) => return Err(RangeTextInputError::Stale),
            None => self.config.settlement_coordinator.allocate_operation()?,
        };
        let proposal = if operation == proposal.key().operation() {
            proposal
        } else {
            MutationProposal::new(
                MutationKey::new(
                    proposal.key().binding(),
                    proposal.key().base_revision(),
                    operation,
                ),
                proposal.kind(),
                proposal.predecessor(),
                proposal.replacement(),
                proposal.replacement_line_breaks(),
            )
        };
        let key = proposal.key();
        let source_cursor = MutationCursor::new(0);
        let proposal_cursor = MutationCursor::new(0);
        let page = if items.is_empty() {
            None
        } else {
            Some(MutationPage::new(
                MutationPageKey::new(
                    key,
                    MutationLane::Proposal,
                    proposal_cursor,
                    0,
                    MutationIdentity::ROOT,
                ),
                MutationCursor::new(1),
                items,
            )?)
        };
        let empty = MutationStreamFinish {
            next_cursor: MutationCursor::new(0),
            next_ordinal: 0,
            cumulative_identity: MutationIdentity::ROOT,
            totals: crate::MutationTotals::default(),
        };
        let proposal_finish = page.as_ref().map_or(empty, |page| MutationStreamFinish {
            next_cursor: page.next_cursor(),
            next_ordinal: 1,
            cumulative_identity: page.cumulative_identity(),
            totals: page.totals(),
        });
        let intended_extent = local_successor_extent(
            self.edits.binding().extent(),
            proposal,
            proposal_finish.totals,
        )?;
        let finish =
            MutationFinishInput::new(key, empty, proposal_finish, intended_extent, intended);
        let begin = MutationBeginRequest::new(proposal, source_cursor, proposal_cursor);
        self.edits.begin(begin)?;
        self.pending_local_mutation = Some(PendingLocalMutation { key, page, finish });
        self.push_request(RangeTextInputRequest::MutationBegin(begin), cx);
        Ok(())
    }

    pub fn accept_mutation_preflight(
        &mut self,
        key: MutationKey,
        cx: &mut Context<Self>,
    ) -> Result<(), RangeTextInputError> {
        if self.edits.active_key() != Some(key) {
            return Err(RangeTextInputError::Stale);
        }
        let queued_records = self.queued_mutation_requests(key);
        let required_records = self
            .pending_local_mutation
            .as_ref()
            .filter(|pending| pending.key == key)
            .map_or(0, |pending| usize::from(pending.page.is_some()) + 1);
        if queued_records
            .checked_add(required_records)
            .is_none_or(|required| required > Self::MAX_QUEUED_MUTATION_REQUESTS)
        {
            return Err(RangeTextInputError::Busy);
        }
        self.edits.accept_preflight(
            key,
            self.active_object
                .map(|active| (active.anchor.object_id, active.anchor.order)),
        )?;
        if let Some(pending) = self
            .pending_local_mutation
            .take()
            .filter(|pending| pending.key == key)
        {
            if let Some(page) = pending.page {
                self.submit_mutation_page(page, cx)?;
            }
            self.submit_mutation_finish(pending.finish, cx)?;
        }
        Ok(())
    }

    pub fn reject_mutation_preflight(
        &mut self,
        key: MutationKey,
        cx: &mut Context<Self>,
    ) -> Result<MutationSettlement, RangeTextInputError> {
        let settlement = self.edits.reject_preflight(key)?;
        self.finish_local_mutation(key, MutationOutcome::Rejected, cx);
        Ok(settlement)
    }

    pub fn reject_mutation_input(
        &mut self,
        key: MutationKey,
        cx: &mut Context<Self>,
    ) -> Result<MutationSettlement, RangeTextInputError> {
        let settlement = self.edits.reject_input(key)?;
        self.finish_local_mutation(key, MutationOutcome::Rejected, cx);
        Ok(settlement)
    }

    pub fn cancel_mutation(
        &mut self,
        key: MutationKey,
        cx: &mut Context<Self>,
    ) -> Result<crate::MutationCancellation, RangeTextInputError> {
        let cancellation = self.edits.cancel(key)?;
        if cancellation == crate::MutationCancellation::Cancelled {
            self.finish_local_mutation(key, MutationOutcome::Cancelled, cx);
            self.push_request(
                RangeTextInputRequest::CancelMutation(crate::MutationCancelRequest::new(key)),
                cx,
            );
        }
        Ok(cancellation)
    }

    pub fn accept_mutation_finish(
        &mut self,
        key: MutationKey,
        cx: &mut Context<Self>,
    ) -> Result<(), RangeTextInputError> {
        if !self.mutation_queue_has_capacity(key) {
            return Err(RangeTextInputError::Busy);
        }
        self.config.settlement_coordinator.reserve_mutation(key)?;
        let request = match self.edits.admit_commit(key) {
            Ok(request) => request,
            Err(error) => {
                self.config.settlement_coordinator.settle_mutation(key);
                return Err(error.into());
            }
        };
        self.push_request(RangeTextInputRequest::MutationCommit(request), cx);
        Ok(())
    }

    fn finish_local_mutation(
        &mut self,
        key: MutationKey,
        outcome: MutationOutcome,
        cx: &mut Context<Self>,
    ) {
        self.requests.retain(|request| {
            !matches!(request,
                RangeTextInputRequest::MutationBegin(begin) if begin.proposal().key() == key
            ) && !matches!(request,
                RangeTextInputRequest::MutationSourcePage(request)
                    | RangeTextInputRequest::MutationProposalPage(request)
                    if request.page().key().key() == key
            ) && !matches!(request,
                RangeTextInputRequest::MutationFinishInput(finish) if finish.key() == key
            ) && !matches!(request,
                RangeTextInputRequest::MutationCommit(request) if request.key() == key
            )
        });
        self.dispatched_mutations.remove(&key);
        self.pending_local_mutation = None;
        if self
            .pending_history
            .is_some_and(|pending| pending.intent().key() == key)
        {
            self.pending_history = None;
        }
        if self
            .mutation_positions
            .is_some_and(|(pending, _)| pending == key)
        {
            self.mutation_positions = None;
        }
        if self
            .mutation_composition
            .is_some_and(|(pending, _, _)| pending == key)
        {
            self.mutation_composition = None;
        }
        cx.emit(RangeTextInputEvent::MutationSettled { key, outcome });
        cx.notify();
    }

    #[allow(clippy::too_many_arguments)]
    pub fn settle_committed_mutation(
        &mut self,
        key: MutationKey,
        binding: crate::RangeBinding,
        positions: MutationPositions,
        text: &RangeResidency,
        objects: &ObjectResidency,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<MutationSettlement, RangeTextInputError> {
        let commit =
            crate::MutationCommit::from_admitted_sources(binding, positions, text, objects)?;
        self.settle_mutation(key, MutationOutcome::Committed(commit), window, cx)
    }

    pub fn settle_mutation(
        &mut self,
        key: MutationKey,
        outcome: MutationOutcome,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<MutationSettlement, RangeTextInputError> {
        if self.edits.active_key() == Some(key) {
            let intended = self
                .mutation_positions
                .filter(|(active, _)| *active == key)
                .map(|(_, positions)| positions)
                .ok_or(RangeTextInputError::Stale)?;
            if let MutationOutcome::Committed(successor) = outcome {
                if successor.positions() != intended {
                    return Err(MutationError::WrongSuccessorPositions.into());
                }
                let positions = successor.positions();
                let composition = self
                    .mutation_composition
                    .filter(|(composition_key, _, _)| *composition_key == key);
                let selection = composition.map_or(
                    super::RangeSourceSelection {
                        anchor: positions.selection_anchor(),
                        head: positions.selection_head(),
                    },
                    |(_, _, selection)| selection,
                );
                let active_loss_reason = match self.edits.active_object_effect() {
                    Some(crate::ActiveObjectEffect::Removed { .. }) => {
                        crate::InlineObjectRealizationLossReason::Removed
                    }
                    Some(crate::ActiveObjectEffect::Replaced { .. }) => {
                        crate::InlineObjectRealizationLossReason::Replaced
                    }
                    None => crate::InlineObjectRealizationLossReason::Superseded,
                };
                let proofs = successor.proofs().as_array().to_vec();
                return self.settle_committed_rebind(
                    key,
                    outcome,
                    successor.binding(),
                    selection,
                    positions,
                    proofs,
                    composition.map(|(_, composition, _)| composition),
                    active_loss_reason,
                    window,
                    cx,
                );
            }
            if !self.config.settlement_coordinator.settle_mutation(key) {
                return Err(RangeTextInputError::Stale);
            }
            let settlement = match self.edits.settle(key, outcome) {
                Ok(settlement) => settlement,
                Err(error) => {
                    self.config.settlement_coordinator.reserve_mutation(key)?;
                    return Err(error.into());
                }
            };
            self.dispatched_mutations.remove(&key);
            self.pending_local_mutation = None;
            if self
                .pending_history
                .is_some_and(|pending| pending.intent().key() == key)
            {
                self.pending_history = None;
            }
            self.mutation_positions = None;
            self.mutation_composition = None;
            cx.emit(RangeTextInputEvent::MutationSettled { key, outcome });
            cx.notify();
            return Ok(settlement);
        }
        if self.edits.is_retired(key) {
            return Err(MutationError::ObsoleteOperation(key).into());
        }
        if !self.config.settlement_coordinator.settle_mutation(key) {
            return Err(RangeTextInputError::Stale);
        }
        self.dispatched_mutations.remove(&key);
        cx.emit(RangeTextInputEvent::MutationSettled { key, outcome });
        cx.notify();
        Ok(MutationSettlement::Obsolete(outcome))
    }

    pub(super) fn insert_text(
        &mut self,
        text: String,
        cx: &mut Context<Self>,
    ) -> Result<(), RangeTextInputError> {
        let surface = self
            .interactive_surface()
            .ok_or(RangeTextInputError::Busy)?;
        let replacement = surface
            .selection()
            .range()
            .map_err(|_| RangeTextInputError::Stale)?;
        self.begin_source_replacement(replacement, text, MutationKind::Edit, cx)?;
        Ok(())
    }

    pub(super) fn select_source_position(
        &mut self,
        position: SourcePosition,
        extend: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(surface) = self.interactive_surface() else {
            return;
        };
        if !surface.overscan().contains_offset(position.byte_offset) {
            return;
        }
        let selection = if extend {
            super::RangeSourceSelection {
                anchor: surface.selection().anchor,
                head: position,
            }
        } else {
            super::RangeSourceSelection::caret(position)
        };
        let selected_object = surface.object_selected_by(selection);
        let _ = self.publish_source_selection(selection, selected_object, None, cx);
        let _ = window;
    }

    pub(super) fn publish_source_selection(
        &mut self,
        selection: super::RangeSourceSelection,
        selected_object: Option<crate::RealizedInlineObjectGeometry>,
        activation: Option<crate::InlineObjectInputOrigin>,
        cx: &mut Context<Self>,
    ) -> Result<(), RangeTextInputError> {
        self.publish_optional_source_selection(Some(selection), selected_object, activation, cx)
    }

    pub(super) fn publish_optional_source_selection(
        &mut self,
        selection: Option<super::RangeSourceSelection>,
        selected_object: Option<crate::RealizedInlineObjectGeometry>,
        activation: Option<crate::InlineObjectInputOrigin>,
        cx: &mut Context<Self>,
    ) -> Result<(), RangeTextInputError> {
        let desired = self.desired_for_source_selection(selection, selected_object, activation)?;
        let candidate = self.prepare_target_transition(desired, None)?;
        self.commit_widget_transition(candidate, Some(cx));
        Ok(())
    }

    pub(super) fn publish_pointer_source_selection(
        &mut self,
        selection: super::RangeSourceSelection,
        selected_object: Option<crate::RealizedInlineObjectGeometry>,
        activation: Option<crate::InlineObjectInputOrigin>,
        pointer_anchor: Option<crate::SourcePosition>,
        cx: &mut Context<Self>,
    ) -> Result<(), RangeTextInputError> {
        let desired =
            self.desired_for_source_selection(Some(selection), selected_object, activation)?;
        let candidate = self.prepare_pointer_target_transition(desired, pointer_anchor)?;
        self.commit_widget_transition(candidate, Some(cx));
        Ok(())
    }

    fn desired_for_source_selection(
        &self,
        selection: Option<super::RangeSourceSelection>,
        selected_object: Option<crate::RealizedInlineObjectGeometry>,
        activation: Option<crate::InlineObjectInputOrigin>,
    ) -> Result<super::DesiredSurface, RangeTextInputError> {
        let mut desired = if selection.is_some() {
            self.desired
        } else {
            super::DesiredSurface::origin(self.config.viewport_extent, self.config.overscan)
        };
        desired.source_selection = selection;
        desired.composition = None;
        desired.reveal_caret = true;
        desired.inline_object_interaction = if let Some(object) = selected_object {
            let surface = self
                .interactive_surface()
                .ok_or(RangeTextInputError::Busy)?;
            let presentation = surface.presentation_for_geometry(object);
            Some(super::DesiredInlineObjectInteraction::Set {
                object_id: object.id(),
                order: object.order(),
                activation_eligible: presentation.activation_eligible(),
                origin: activation,
            })
        } else if self.active_object.is_some() {
            Some(super::DesiredInlineObjectInteraction::Clear(
                crate::InlineObjectRealizationLossReason::SelectionChanged,
            ))
        } else {
            None
        };
        Ok(desired)
    }

    pub(super) fn begin_boundary(
        &mut self,
        kind: SegmentationKind,
        direction: SegmentationDirection,
        action: PendingBoundaryAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), RangeTextInputError> {
        let origin = self
            .interactive_surface()
            .ok_or(RangeTextInputError::Busy)?
            .caret()
            .byte_offset;
        self.begin_boundary_from(origin, kind, direction, action, window, cx)
    }

    pub(super) fn begin_boundary_from(
        &mut self,
        origin: ByteOffset,
        kind: SegmentationKind,
        direction: SegmentationDirection,
        action: PendingBoundaryAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), RangeTextInputError> {
        if !self.enabled {
            return Err(RangeTextInputError::Busy);
        }
        if self.segmentation.is_some() || self.active_geometry.is_some() {
            return Err(RangeTextInputError::Busy);
        }
        self.interactive_surface()
            .ok_or(RangeTextInputError::Busy)?;
        let cap = self.config.segmentation_limits.max_page_bytes();
        let page_direction = match direction {
            SegmentationDirection::Forward => crate::PageDirection::Forward,
            SegmentationDirection::Reverse => crate::PageDirection::Backward,
        };
        let id = crate::PageRequestId::new(self.next_id());
        let key = crate::PageRequestKey::adjacent(
            id,
            self.config.binding.binding(),
            self.config.binding.revision(),
            crate::PagePurpose::Segmentation,
            origin,
            page_direction,
            cap,
        )?;
        match SegmentationContinuation::start(
            self.config.binding.binding(),
            self.config.binding.revision(),
            self.config.binding.extent(),
            kind,
            direction,
            origin,
            self.config.segmentation_limits,
            key,
        )
        .map_err(|_| RangeTextInputError::Stale)?
        {
            crate::SegmentationProgress::Complete(boundary) => {
                self.apply_boundary(boundary.offset(), action, window, cx)
            }
            crate::SegmentationProgress::NeedPage(continuation) => {
                let demand = self
                    .residency
                    .demand(id, crate::PagePurpose::Segmentation, key.demand())
                    .map_err(|_| RangeTextInputError::Busy)?;
                self.segmentation = Some(continuation);
                self.segmentation_action = Some(action);
                let resident = self.accept_page_demand(crate::PageRequest::new(key), demand, cx)?;
                if let Some(page) = resident {
                    self.deliver_segmentation_page(page, window, cx)?;
                }
                Ok(())
            }
        }
    }

    pub(super) fn apply_boundary(
        &mut self,
        offset: ByteOffset,
        action: PendingBoundaryAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), RangeTextInputError> {
        if !self.enabled {
            return Err(RangeTextInputError::Busy);
        }
        match action {
            PendingBoundaryAction::Move { extend, direction } => {
                let surface = self
                    .interactive_surface()
                    .ok_or(RangeTextInputError::Busy)?;
                let anchor = surface.selection().anchor;
                let position = surface
                    .source_position_for_byte(offset, direction)
                    .ok_or(RangeTextInputError::Pending)?;
                let selection = if extend {
                    super::RangeSourceSelection {
                        anchor,
                        head: position,
                    }
                } else {
                    super::RangeSourceSelection::caret(position)
                };
                let selected_object = surface.object_selected_by(selection);
                self.publish_source_selection(selection, selected_object, None, cx)?;
            }
            PendingBoundaryAction::Delete { direction } => {
                let surface = self
                    .interactive_surface()
                    .ok_or(RangeTextInputError::Busy)?;
                let origin = surface.caret();
                let endpoint = surface
                    .source_position_for_byte(offset, direction)
                    .ok_or(RangeTextInputError::Pending)?;
                let range = super::RangeSourceSelection {
                    anchor: origin,
                    head: endpoint,
                }
                .range()
                .map_err(|_| RangeTextInputError::Stale)?;
                self.begin_source_replacement(range, String::new(), MutationKind::Edit, cx)?;
            }
            PendingBoundaryAction::SelectPointStart { origin, kind } => {
                self.begin_boundary_from(
                    origin,
                    kind,
                    SegmentationDirection::Forward,
                    PendingBoundaryAction::SelectPointEnd { start: offset },
                    window,
                    cx,
                )?;
            }
            PendingBoundaryAction::SelectPointEnd { start } => {
                let surface = self
                    .interactive_surface()
                    .ok_or(RangeTextInputError::Busy)?;
                let anchor = surface
                    .source_position_for_byte(start, SegmentationDirection::Forward)
                    .ok_or(RangeTextInputError::Pending)?;
                let head = surface
                    .source_position_for_byte(offset, SegmentationDirection::Reverse)
                    .ok_or(RangeTextInputError::Pending)?;
                let selection = super::RangeSourceSelection { anchor, head };
                let selected_object = surface.object_selected_by(selection);
                self.publish_source_selection(selection, selected_object, None, cx)?;
            }
        }
        let _ = window;
        Ok(())
    }
}

fn local_successor_extent(
    base: LogicalExtent,
    proposal: MutationProposal,
    totals: crate::MutationTotals,
) -> Result<LogicalExtent, MutationError> {
    let bytes = base
        .byte_len()
        .checked_sub(proposal.replacement_bytes().len())
        .and_then(|bytes| bytes.checked_add(totals.inserted_bytes))
        .ok_or(MutationError::IncoherentSuccessor)?;
    let base_breaks = base
        .line_count()
        .checked_sub(u64::from(base.byte_len() != 0))
        .ok_or(MutationError::IncoherentSuccessor)?;
    let breaks = base_breaks
        .checked_sub(proposal.replacement_line_breaks())
        .and_then(|breaks| breaks.checked_add(totals.inserted_line_breaks))
        .ok_or(MutationError::IncoherentSuccessor)?;
    let lines = if bytes == 0 {
        if breaks != 0 {
            return Err(MutationError::IncoherentSuccessor);
        }
        0
    } else {
        breaks
            .checked_add(1)
            .ok_or(MutationError::IncoherentSuccessor)?
    };
    Ok(LogicalExtent::new(bytes, lines))
}

pub(super) fn successor_position(
    replacement: SourceRange,
    removed: Option<crate::ObjectTarget>,
    inserted_bytes: usize,
) -> Result<SourcePosition, RangeTextInputError> {
    let translated = replacement
        .start()
        .byte_offset
        .get()
        .checked_add(inserted_bytes as u64)
        .ok_or(RangeTextInputError::SurfaceCapacity)?;
    let byte_offset = ByteOffset::new(translated);
    if inserted_bytes != 0 {
        let gap = match replacement.end().gap {
            crate::InlineObjectGap::Between { following, .. }
            | crate::InlineObjectGap::Before(following) => {
                crate::InlineObjectGap::Before(following)
            }
            crate::InlineObjectGap::NoObjects | crate::InlineObjectGap::After(_) => {
                crate::InlineObjectGap::NoObjects
            }
        };
        return Ok(SourcePosition::new(byte_offset, gap));
    }
    let Some(_) = removed else {
        return Ok(replacement.end());
    };
    let gap = match (replacement.start().gap, replacement.end().gap) {
        (crate::InlineObjectGap::Before(_), crate::InlineObjectGap::After(_)) => {
            crate::InlineObjectGap::NoObjects
        }
        (crate::InlineObjectGap::Before(_), crate::InlineObjectGap::Between { following, .. }) => {
            crate::InlineObjectGap::Before(following)
        }
        (crate::InlineObjectGap::Between { preceding, .. }, crate::InlineObjectGap::After(_)) => {
            crate::InlineObjectGap::After(preceding)
        }
        (
            crate::InlineObjectGap::Between { preceding, .. },
            crate::InlineObjectGap::Between { following, .. },
        ) => crate::InlineObjectGap::between(preceding, following)
            .map_err(|_| RangeTextInputError::Stale)?,
        _ => return Err(RangeTextInputError::Stale),
    };
    Ok(SourcePosition::new(byte_offset, gap))
}
