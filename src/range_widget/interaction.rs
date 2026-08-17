use gpui::{Context, Window};

use crate::{
    ByteOffset, ByteRange, MutationError, MutationFragment, MutationFragmentPayload, MutationKey,
    MutationKind, MutationOutcome, MutationPositions, MutationProposal, MutationSettlement,
    ObjectResidency, OperationId, RangeResidency, RangeTextInputError, RangeTextInputEvent,
    RangeTextInputRequest, SegmentationContinuation, SegmentationDirection, SegmentationKind,
    SourcePosition, SourceRange,
};

use super::RangeTextInput;

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

    pub fn propose_host_mutation(
        &mut self,
        proposal: MutationProposal,
        fragments: Vec<MutationFragment>,
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
        if fragments.len() > self.config.mutation_limits.max_fragments() {
            return Err(MutationError::FragmentLimitExceeded.into());
        }
        self.edits.begin(proposal)?;
        self.edits.accept_preflight(proposal.key())?;
        let required = crate::range_edit::required_base_positions(proposal, &fragments);
        if let Err(error) =
            self.edits
                .reserve_source_positions(proposal.key(), &required, text, objects)
        {
            let _ = self.edits.fail_precommit(proposal.key());
            return Err(error.into());
        }
        let mut intended = None;
        for fragment in &fragments {
            if let MutationFragmentPayload::Terminal {
                intended: positions,
            } = fragment.payload()
            {
                intended = Some(*positions);
            }
            if let Err(error) = self.edits.stage(fragment.clone()) {
                let _ = self.edits.fail_precommit(proposal.key());
                return Err(error.into());
            }
        }
        let Some(intended) = intended else {
            let _ = self.edits.fail_precommit(proposal.key());
            return Err(MutationError::MissingTerminalFragment.into());
        };
        self.mutation_positions = Some((proposal.key(), intended));
        self.push_request(RangeTextInputRequest::MutationPreflight(proposal), cx);
        for fragment in fragments {
            self.push_request(
                RangeTextInputRequest::MutationFragment {
                    key: proposal.key(),
                    fragment,
                },
                cx,
            );
        }
        self.push_request(RangeTextInputRequest::MutationCommit(proposal.key()), cx);
        Ok(proposal.key())
    }

    pub fn admit_edit_positions(
        &mut self,
        positions: &[SourcePosition],
        text: &RangeResidency,
        objects: &ObjectResidency,
    ) -> Result<(), RangeTextInputError> {
        if !matches!(self.edits.state(), crate::MutationState::Idle) {
            return Err(RangeTextInputError::Busy);
        }
        let max = self
            .config
            .mutation_limits
            .max_objects()
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
        if self.pending_insert.is_some() || self.pending_object_remove.is_some() {
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
        let required_fragments = 1usize
            .checked_add(usize::from(!text.is_empty()))
            .and_then(|count| count.checked_add(usize::from(selected_object.is_some())))
            .ok_or(RangeTextInputError::SurfaceCapacity)?;
        if required_fragments > self.config.mutation_limits.max_fragments()
            || text.len() > self.config.mutation_limits.max_staged_bytes()
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
        let key = MutationKey::new(
            self.config.binding.binding(),
            self.config.binding.revision(),
            OperationId::new(self.next_id()),
        );
        let proposal = MutationProposal::new(key, kind, replacement, 0);
        self.edits.begin(proposal)?;
        self.pending_insert = Some((key, text, caret, proofs));
        self.pending_object_remove = removed.map(|target| (key, target));
        self.push_request(RangeTextInputRequest::MutationPreflight(proposal), cx);
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
        if self.pending_insert.is_some() {
            return Err(RangeTextInputError::Busy);
        }
        if !text.is_empty() && self.config.mutation_limits.max_fragments() < 2 {
            return Err(RangeTextInputError::SurfaceCapacity);
        }
        self.config.binding.extent().check_byte_range(range)?;
        let (replacement, proofs) = self.proven_no_object_range(range)?;
        let key = MutationKey::new(
            self.config.binding.binding(),
            self.config.binding.revision(),
            OperationId::new(self.next_id()),
        );
        let proposal = MutationProposal::new(key, kind, replacement, removed_line_breaks);
        self.edits.begin(proposal)?;
        let caret = SourcePosition::new(
            ByteOffset::new(range.start().get().saturating_add(text.len() as u64)),
            crate::InlineObjectGap::NoObjects,
        );
        self.pending_insert = Some((key, text, caret, proofs));
        self.push_request(RangeTextInputRequest::MutationPreflight(proposal), cx);
        Ok(key)
    }

    pub fn accept_mutation_preflight(
        &mut self,
        key: MutationKey,
        cx: &mut Context<Self>,
    ) -> Result<(), RangeTextInputError> {
        let is_insert = self
            .pending_insert
            .as_ref()
            .is_some_and(|(pending, _, _, _)| *pending == key);
        let is_history = self
            .pending_history
            .is_some_and(|pending| pending.intent().key() == key && pending.is_planned());
        if !is_insert && !is_history {
            return Err(RangeTextInputError::Stale);
        }
        self.edits.accept_preflight(key)?;
        if is_history {
            if let Err(error) = self
                .edits
                .reserve_owned_source_proofs(key, self.admitted_edit_proofs.clone())
            {
                self.fail_invalid_staging(key, cx);
                return Err(error.into());
            }
            return Ok(());
        }
        let (pending_key, text, caret, proofs) = self
            .pending_insert
            .take()
            .ok_or(RangeTextInputError::Stale)?;
        debug_assert_eq!(pending_key, key);
        if let Err(error) = self.edits.reserve_owned_source_proofs(key, proofs) {
            self.fail_invalid_staging(key, cx);
            return Err(error.into());
        }
        let cap = self.config.mutation_limits.max_staged_bytes().max(1);
        let mut ordinal = 0;
        if let Some((object_key, target)) = self.pending_object_remove.take() {
            if object_key != key {
                self.fail_invalid_staging(key, cx);
                return Err(RangeTextInputError::Stale);
            }
            let fragment = MutationFragment::new(
                key,
                ordinal,
                MutationFragmentPayload::Object(crate::ObjectChange::Remove { target }),
            );
            if let Err(error) = self.edits.stage(fragment.clone()) {
                self.fail_invalid_staging(key, cx);
                return Err(error.into());
            }
            self.push_request(
                RangeTextInputRequest::MutationFragment { key, fragment },
                cx,
            );
            ordinal += 1;
        }
        let mut start = 0;
        while start < text.len() {
            let mut end = start.saturating_add(cap).min(text.len());
            while end > start && !text.is_char_boundary(end) {
                end -= 1;
            }
            if end == start {
                self.fail_invalid_staging(key, cx);
                return Err(RangeTextInputError::SurfaceCapacity);
            }
            let fragment = MutationFragment::new(
                key,
                ordinal,
                MutationFragmentPayload::Utf8 {
                    inserted_offset: start as u64,
                    text: text[start..end].to_owned(),
                },
            );
            if let Err(error) = self.edits.stage(fragment.clone()) {
                self.fail_invalid_staging(key, cx);
                return Err(error.into());
            }
            self.push_request(
                RangeTextInputRequest::MutationFragment { key, fragment },
                cx,
            );
            ordinal += 1;
            start = end;
        }
        let terminal = MutationFragment::new(
            key,
            ordinal,
            MutationFragmentPayload::Terminal {
                intended: MutationPositions::new(caret, caret, caret),
            },
        );
        if let Err(error) = self.edits.stage(terminal.clone()) {
            self.fail_invalid_staging(key, cx);
            return Err(error.into());
        }
        self.push_request(
            RangeTextInputRequest::MutationFragment {
                key,
                fragment: terminal,
            },
            cx,
        );
        self.mutation_positions = Some((key, MutationPositions::collapsed(caret)));
        self.push_request(RangeTextInputRequest::MutationCommit(key), cx);
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

    pub fn reject_mutation_staging(
        &mut self,
        key: MutationKey,
        cx: &mut Context<Self>,
    ) -> Result<MutationSettlement, RangeTextInputError> {
        let settlement = self.edits.reject_staging(key)?;
        self.finish_local_mutation(key, MutationOutcome::Rejected, cx);
        Ok(settlement)
    }

    pub fn admit_mutation_commit(&mut self, key: MutationKey) -> Result<(), RangeTextInputError> {
        if self.detached_edits.len() >= self.config.limits.max_detached_edits {
            return Err(RangeTextInputError::DetachedCapacity);
        }
        self.edits.admit_commit(key)?;
        Ok(())
    }

    pub(super) fn fail_invalid_staging(&mut self, key: MutationKey, cx: &mut Context<Self>) {
        let terminalized = match self.edits.fail_precommit(key) {
            Ok(_) => true,
            Err(MutationError::ObsoleteOperation(obsolete)) if obsolete == key => true,
            Err(_) => false,
        };
        if terminalized {
            self.cancel_mutation_dispatch(key, false);
            self.finish_local_mutation(key, MutationOutcome::Error, cx);
        }
    }

    fn finish_local_mutation(
        &mut self,
        key: MutationKey,
        outcome: MutationOutcome,
        cx: &mut Context<Self>,
    ) {
        self.requests.retain(|request| {
            !matches!(request,
                RangeTextInputRequest::MutationPreflight(proposal) if proposal.key() == key
            ) && !matches!(request,
                RangeTextInputRequest::MutationFragment { key: request_key, .. }
                    | RangeTextInputRequest::MutationCommit(request_key) if *request_key == key
            )
        });
        self.dispatched_mutations.remove(&key);
        if self
            .pending_insert
            .as_ref()
            .is_some_and(|(pending, _, _, _)| *pending == key)
        {
            self.pending_insert = None;
            self.pending_object_remove = None;
        }
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
                let active_loss_reason = self.active_object.map_or(
                    crate::InlineObjectRealizationLossReason::Superseded,
                    |active| {
                        self.edits
                            .staged_fragments()
                            .iter()
                            .find_map(|fragment| match fragment.payload() {
                                MutationFragmentPayload::Object(crate::ObjectChange::Remove {
                                    target,
                                }) if target.id() == active.anchor.object_id
                                    && target.order() == active.anchor.order =>
                                {
                                    Some(crate::InlineObjectRealizationLossReason::Removed)
                                }
                                MutationFragmentPayload::Object(crate::ObjectChange::Replace {
                                    target,
                                    ..
                                }) if target.id() == active.anchor.object_id
                                    && target.order() == active.anchor.order =>
                                {
                                    Some(crate::InlineObjectRealizationLossReason::Replaced)
                                }
                                _ => None,
                            })
                            .unwrap_or(crate::InlineObjectRealizationLossReason::Superseded)
                    },
                );
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
            let settlement = self.edits.settle(key, outcome)?;
            self.dispatched_mutations.remove(&key);
            self.pending_insert = None;
            self.pending_object_remove = None;
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
        let Some(index) = self
            .detached_edits
            .iter()
            .position(|owner| owner.active_key() == Some(key))
        else {
            return Err(RangeTextInputError::Stale);
        };
        let settlement = self.detached_edits[index].settle(key, outcome)?;
        self.dispatched_mutations.remove(&key);
        self.detached_edits.remove(index);
        cx.emit(RangeTextInputEvent::MutationSettled { key, outcome });
        cx.notify();
        Ok(settlement)
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
