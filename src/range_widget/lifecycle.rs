use gpui::{Context, Pixels, Window};

use crate::{
    RangeEditCoordinator, RangeHistoryCommit, RangeHistoryFrontier, RangeHistoryIntent,
    RangeHistoryOutcome, RangeHistorySettlement, RangeRestorationScrollAnchor,
    RangeRestorationSeed, RangeSourceSelection, RangeTextInputError, RangeTextInputEvent,
    RangeTextInputRequest,
};

use super::RangeTextInput;

impl RangeTextInput {
    pub fn settle_history(
        &mut self,
        intent: RangeHistoryIntent,
        outcome: RangeHistoryOutcome,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<RangeHistorySettlement, RangeTextInputError> {
        if let Some(pending) = self.pending_history {
            if pending.intent() == intent && pending.is_admitted() {
                if let RangeHistoryOutcome::Committed(commit) = outcome {
                    self.validate_history_commit(intent, commit)?;
                    return self.settle_committed_history_rebind(intent, commit, window, cx);
                }
                if !self.try_spend_realization_credit(cx) {
                    return Err(RangeTextInputError::Busy);
                }
                if !self.config.settlement_coordinator.settle_history(intent) {
                    self.refund_realization_credit();
                    return Err(RangeTextInputError::Stale);
                }
                self.pending_history = None;
                cx.emit(RangeTextInputEvent::HistorySettled { intent, outcome });
                cx.notify();
                return Ok(RangeHistorySettlement::Current(outcome));
            }
        }
        if !self.try_spend_realization_credit(cx) {
            return Err(RangeTextInputError::Busy);
        }
        if !self.config.settlement_coordinator.settle_history(intent) {
            self.refund_realization_credit();
        }
        Ok(RangeHistorySettlement::Obsolete(outcome))
    }

    fn validate_history_commit(
        &self,
        intent: RangeHistoryIntent,
        commit: RangeHistoryCommit,
    ) -> Result<(), RangeTextInputError> {
        let binding = commit.binding();
        let base = intent.binding();
        if base != self.config.binding
            || base.binding() != intent.key().binding()
            || base.revision() != intent.key().base_revision()
            || intent.frontier().binding() != base
            || intent.frontier() != self.history_frontier()
            || commit.frontier().binding() != binding
            || commit.selection().range().is_err()
            || commit.caret() != commit.selection().head
        {
            return Err(RangeTextInputError::Stale);
        }
        let extent = binding.extent().byte_len();
        if [
            commit.caret(),
            commit.selection().anchor,
            commit.selection().head,
        ]
        .into_iter()
        .any(|position| position.byte_offset.get() > extent)
        {
            return Err(RangeTextInputError::Stale);
        }
        Ok(())
    }

    fn settle_committed_history_rebind(
        &mut self,
        intent: RangeHistoryIntent,
        commit: RangeHistoryCommit,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<RangeHistorySettlement, RangeTextInputError> {
        if self.pending_rebind_intent.as_ref().is_some_and(|pending| {
            matches!(pending, super::realization::PendingRebindIntent::History {
                intent: pending_intent,
                ..
            } if *pending_intent == intent)
        }) {
            return if self.service_pending_rebind_intent(window, cx)? {
                Ok(RangeHistorySettlement::Current(
                    RangeHistoryOutcome::Committed(commit),
                ))
            } else {
                Err(RangeTextInputError::Busy)
            };
        }
        self.retain_pending_rebind_intent(super::realization::PendingRebindIntent::History {
            intent,
            commit,
        })?;
        if self.service_pending_rebind_intent(window, cx)? {
            Ok(RangeHistorySettlement::Current(
                RangeHistoryOutcome::Committed(commit),
            ))
        } else {
            Err(RangeTextInputError::Busy)
        }
    }

    pub(super) fn commit_pending_history_rebind(
        &mut self,
        intent: RangeHistoryIntent,
        commit: RangeHistoryCommit,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<RangeHistorySettlement, RangeTextInputError> {
        if !self.config.settlement_coordinator.contains_history(intent) {
            return Err(RangeTextInputError::Stale);
        }
        let prior_scrollbar_owner = self.scrollbar.owner;
        let next_mount = prior_scrollbar_owner
            .mount_generation
            .get()
            .checked_add(1)
            .ok_or(RangeTextInputError::Stale)?;
        let replacement_scrollbar_owner = gpui_scrollbar::ScrollbarOwnerKey::new(
            prior_scrollbar_owner.owner_id,
            gpui_scrollbar::ScrollbarMountGeneration::new(next_mount),
        );
        let candidate = self.prepare_rebind_transition(
            commit.binding(),
            Some(commit.selection()),
            prior_scrollbar_owner,
            replacement_scrollbar_owner,
            crate::InlineObjectRealizationLossReason::Superseded,
            None,
            None,
            Some((
                crate::MutationPositions::new(
                    commit.caret(),
                    commit.selection().anchor,
                    commit.selection().head,
                ),
                Vec::new(),
            )),
        )?;
        if self.scrollbar.state.current_owner() != Some(prior_scrollbar_owner) {
            return Err(RangeTextInputError::Stale);
        }
        if !self.config.settlement_coordinator.settle_history(intent) {
            return Err(RangeTextInputError::Stale);
        }
        let committed = self.commit_widget_transition_internal(candidate);
        self.history_frontier = commit.frontier();
        assert!(self.scrollbar.state.replace_owner(
            prior_scrollbar_owner,
            replacement_scrollbar_owner,
            window,
            cx,
        ));
        self.flush_widget_transition(committed, Some(cx));
        let outcome = RangeHistoryOutcome::Committed(commit);
        cx.emit(RangeTextInputEvent::HistorySettled { intent, outcome });
        cx.notify();
        Ok(RangeHistorySettlement::Current(outcome))
    }

    pub(super) fn settle_committed_rebind(
        &mut self,
        key: crate::MutationKey,
        outcome: crate::MutationOutcome,
        binding: crate::RangeBinding,
        selection: RangeSourceSelection,
        positions: crate::MutationPositions,
        proofs: Vec<crate::range_edit::SourcePositionProof>,
        composition: Option<crate::ByteRange>,
        active_loss_reason: crate::InlineObjectRealizationLossReason,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<crate::MutationSettlement, RangeTextInputError> {
        if self.pending_rebind_intent.as_ref().is_some_and(|pending| {
            matches!(pending, super::realization::PendingRebindIntent::Mutation {
                key: pending_key,
                binding: pending_binding,
                ..
            } if *pending_key == key && *pending_binding == binding)
        }) {
            return if self.service_pending_rebind_intent(window, cx)? {
                Ok(crate::MutationSettlement::Current(outcome))
            } else {
                Err(RangeTextInputError::Busy)
            };
        }
        self.retain_pending_rebind_intent(super::realization::PendingRebindIntent::Mutation {
            key,
            outcome,
            binding,
            selection,
            positions,
            proofs,
            composition,
            active_loss_reason,
        })?;
        if self.service_pending_rebind_intent(window, cx)? {
            Ok(crate::MutationSettlement::Current(outcome))
        } else {
            Err(RangeTextInputError::Busy)
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn commit_pending_mutation_rebind(
        &mut self,
        key: crate::MutationKey,
        outcome: crate::MutationOutcome,
        binding: crate::RangeBinding,
        selection: RangeSourceSelection,
        positions: crate::MutationPositions,
        proofs: Vec<crate::range_edit::SourcePositionProof>,
        composition: Option<crate::ByteRange>,
        active_loss_reason: crate::InlineObjectRealizationLossReason,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<crate::MutationSettlement, RangeTextInputError> {
        let prior_scrollbar_owner = self.scrollbar.owner;
        let next_mount = prior_scrollbar_owner
            .mount_generation
            .get()
            .checked_add(1)
            .ok_or(RangeTextInputError::Stale)?;
        let replacement_scrollbar_owner = gpui_scrollbar::ScrollbarOwnerKey::new(
            prior_scrollbar_owner.owner_id,
            gpui_scrollbar::ScrollbarMountGeneration::new(next_mount),
        );
        let mut candidate = self.prepare_rebind_transition(
            binding,
            Some(selection),
            prior_scrollbar_owner,
            replacement_scrollbar_owner,
            active_loss_reason,
            Some((key, outcome)),
            composition,
            Some((positions, proofs)),
        )?;
        if self.scrollbar.state.current_owner() != Some(prior_scrollbar_owner) {
            return Err(RangeTextInputError::Stale);
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
        let settled_edits = std::mem::replace(
            &mut self.edits,
            crate::RangeEditCoordinator::new(binding, self.config.mutation_limits),
        );
        candidate.retain_settled_edits(settled_edits);
        let committed = self.commit_widget_transition_internal(candidate);
        self.history_frontier = RangeHistoryFrontier::unavailable(binding);
        assert!(self.scrollbar.state.replace_owner(
            prior_scrollbar_owner,
            replacement_scrollbar_owner,
            window,
            cx,
        ));
        self.flush_widget_transition(committed, Some(cx));
        Ok(settlement)
    }

    /// Rebinds this instance while retaining the prior publication until a successor is coherent.
    pub fn rebind(
        &mut self,
        binding: crate::RangeBinding,
        selection: Option<RangeSourceSelection>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), RangeTextInputError> {
        if !self.mounted {
            return Err(RangeTextInputError::NotMounted);
        }
        if binding == self.config.binding && selection == self.desired.source_selection {
            return Ok(());
        }
        if binding == self.config.binding {
            let selected_object = match selection {
                Some(selection) => self
                    .interactive_surface()
                    .ok_or(RangeTextInputError::Busy)?
                    .object_selected_by(selection),
                None => None,
            };
            return self.publish_optional_source_selection(selection, selected_object, None, cx);
        }
        self.retain_pending_rebind_intent(super::realization::PendingRebindIntent::Direct {
            binding,
            selection,
        })?;
        if self.service_pending_rebind_intent(window, cx)? {
            Ok(())
        } else {
            Err(RangeTextInputError::Busy)
        }
    }

    pub(super) fn commit_pending_direct_rebind(
        &mut self,
        binding: crate::RangeBinding,
        selection: Option<RangeSourceSelection>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), RangeTextInputError> {
        let prior_scrollbar_owner = self.scrollbar.owner;
        let next_mount = prior_scrollbar_owner
            .mount_generation
            .get()
            .checked_add(1)
            .ok_or(RangeTextInputError::Stale)?;
        let replacement_scrollbar_owner = gpui_scrollbar::ScrollbarOwnerKey::new(
            prior_scrollbar_owner.owner_id,
            gpui_scrollbar::ScrollbarMountGeneration::new(next_mount),
        );
        let candidate = self.prepare_rebind_transition(
            binding,
            selection,
            prior_scrollbar_owner,
            replacement_scrollbar_owner,
            crate::InlineObjectRealizationLossReason::Superseded,
            None,
            None,
            None,
        )?;
        // This is deliberately the final fallible/no-change gate. The prepared commit below only
        // moves bounded deltas and publishes effects after the widget is coherent.
        if self.scrollbar.state.current_owner() != Some(prior_scrollbar_owner) {
            return Err(RangeTextInputError::Stale);
        }
        let committed = self.commit_widget_transition_internal(candidate);
        self.history_frontier = RangeHistoryFrontier::unavailable(binding);
        assert!(self.scrollbar.state.replace_owner(
            prior_scrollbar_owner,
            replacement_scrollbar_owner,
            window,
            cx,
        ));
        let progress = self.flush_widget_transition(committed, Some(cx));
        debug_assert_eq!(progress, crate::ExactGeometryProgress::Scanning);
        Ok(())
    }

    /// Exports a text-free restoration seed only after every operation is quiescent.
    pub fn export_restoration(
        &self,
        history: Option<RangeHistoryFrontier>,
    ) -> Result<RangeRestorationSeed, RangeTextInputError> {
        if !self.mounted {
            return Err(RangeTextInputError::NotMounted);
        }
        if !self.is_quiescent() {
            return Err(RangeTextInputError::NotQuiescent);
        }
        let surface = self
            .surface
            .as_ref()
            .ok_or(RangeTextInputError::NotQuiescent)?;
        if history.is_some_and(|frontier| frontier.binding() != surface.binding()) {
            return Err(RangeTextInputError::Stale);
        }
        if surface.composition().is_some() {
            return Err(RangeTextInputError::NotQuiescent);
        }
        if let Some(mut seed) = self.published_restoration
            && seed.binding == surface.binding()
            && seed.caret == surface.caret()
            && seed.selection == surface.selection()
            && seed.scroll.position == surface.scroll_position()
        {
            seed.history = history;
            return Ok(seed);
        }
        let positions = self
            .adopted_positions
            .filter(|positions| {
                positions.caret() == surface.caret()
                    && positions.selection_anchor() == surface.selection().anchor
                    && positions.selection_head() == surface.selection().head
            })
            .or_else(|| {
                let proof_at = |position| {
                    self.admitted_edit_proofs
                        .iter()
                        .find(|proof| {
                            proof.binding() == surface.binding() && proof.position() == position
                        })
                        .map(|proof| proof.position())
                };
                Some(crate::MutationPositions::new(
                    proof_at(surface.caret())?,
                    proof_at(surface.selection().anchor)?,
                    proof_at(surface.selection().head)?,
                ))
            })
            .or_else(|| {
                let proof_at = |position| {
                    crate::range_edit::SourcePositionProof::from_surface_pages(
                        surface.binding(),
                        position,
                        surface.pages(),
                        surface.object_pages(),
                    )
                    .ok()
                    .map(|proof| proof.position())
                };
                Some(crate::MutationPositions::new(
                    proof_at(surface.caret())?,
                    proof_at(surface.selection().anchor)?,
                    proof_at(surface.selection().head)?,
                ))
            })
            .ok_or(RangeTextInputError::IncompleteSurface)?;
        let scroll = self
            .admitted_edit_proofs
            .iter()
            .find(|proof| {
                proof.binding() == surface.binding()
                    && proof.position() == surface.scroll_position()
            })
            .map(|proof| proof.position())
            .or_else(|| {
                (positions.caret() == surface.scroll_position()).then_some(positions.caret())
            })
            .or_else(|| {
                crate::range_edit::SourcePositionProof::from_surface_pages(
                    surface.binding(),
                    surface.scroll_position(),
                    surface.pages(),
                    surface.object_pages(),
                )
                .ok()
                .map(|proof| proof.position())
            })
            .ok_or(RangeTextInputError::IncompleteSurface)?;
        Ok(RangeRestorationSeed {
            binding: surface.binding(),
            caret: positions.caret(),
            selection: RangeSourceSelection {
                anchor: positions.selection_anchor(),
                head: positions.selection_head(),
            },
            scroll: RangeRestorationScrollAnchor {
                position: scroll,
                intra_anchor: surface.scroll_intra_anchor(),
            },
            history,
        })
    }

    /// Validates and realizes a compact seed against newly fetched state only.
    pub fn import_restoration(
        &mut self,
        seed: RangeRestorationSeed,
        cx: &mut Context<Self>,
    ) -> Result<(), RangeTextInputError> {
        if !self.mounted {
            return Err(RangeTextInputError::NotMounted);
        }
        if !self.is_quiescent() {
            return Err(RangeTextInputError::NotQuiescent);
        }
        if seed.binding != self.config.binding
            || seed
                .history
                .is_some_and(|frontier| frontier.binding() != seed.binding)
            || seed.caret != seed.selection.head
            || seed.scroll.intra_anchor < Pixels::ZERO
            || seed.scroll.intra_anchor > self.config.limits.max_intra_anchor
            || seed.selection.range().is_err()
        {
            cx.emit(RangeTextInputEvent::RestorationRejected);
            return Err(RangeTextInputError::MalformedSeed);
        }
        let extent = seed.binding.extent();
        for position in [
            seed.caret,
            seed.selection.anchor,
            seed.selection.head,
            seed.scroll.position,
        ] {
            if position.byte_offset.get() > extent.byte_len() {
                cx.emit(RangeTextInputEvent::RestorationRejected);
                return Err(RangeTextInputError::MalformedSeed);
            }
        }
        for key in self.residency.rebind(self.config.binding) {
            self.cancel_page_dispatch(key);
        }
        for key in self
            .object_residency
            .rebind(self.config.binding, self.config.presentation_generation)
        {
            self.cancel_object_page_dispatch(key);
        }
        self.pending_geometry_object = None;
        let release = self
            .geometry
            .rebind(self.config.binding, self.config.presentation_generation)?;
        self.release_geometry(&release, None, None, Some(cx));
        self.active_geometry = None;
        self.surface_candidate = None;
        self.surface = None;
        self.published_restoration = None;
        self.restoration = Some(super::restoration::RestorationValidation::new(seed));
        self.request_next_restoration_validation(cx)
    }

    /// Cancels and releases every cancellable operation and drops the publication.
    pub fn dispose(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<RangeTextInputRequest> {
        if !self.mounted {
            return Vec::new();
        }
        let active_loss =
            self.active_object
                .take()
                .map(|active| crate::InlineObjectRealizationLoss {
                    anchor: active.anchor,
                    reason: crate::InlineObjectRealizationLossReason::Disposed,
                });
        self.attached_inline_object_surface = None;
        self.mounted = false;
        self.obsolete_realization_continuation();
        self.deferred_geometry_response = None;
        self.response_custody = std::collections::VecDeque::new();
        self.active_response_processing = crate::RangeSurfaceCharge::default();
        self.pending_target_intent = None;
        self.pending_index_intent = false;
        self.pending_layout_intent = None;
        self.pending_presentation_intent = None;
        self.pending_rebind_intent = None;
        self.last_realization_step.remaining = 0;
        self.last_realization_step.reached_external_boundary = true;
        let mut edits = std::mem::replace(
            &mut self.edits,
            RangeEditCoordinator::new(self.config.binding, self.config.mutation_limits),
        );
        if let Some(disposal) = edits.dispose() {
            match disposal {
                crate::MutationDisposal::Cancelled(key) => {
                    self.cancel_mutation_dispatch(key, false)
                }
                crate::MutationDisposal::Detached(key) => {
                    self.cancel_mutation_dispatch(key, true);
                }
            }
        }
        for key in self.residency.dispose() {
            self.cancel_page_dispatch(key);
        }
        for key in self.object_residency.dispose() {
            self.cancel_object_page_dispatch(key);
        }
        self.pending_geometry_object = None;
        self.pending_page_aliases = Vec::new();
        if let Some(cancel) = self.clipboard.dispose() {
            if let Some(page) = cancel.pending_text_page() {
                self.cancel_page_dispatch(page);
            }
            if let Some(page) = cancel.pending_object_page() {
                self.cancel_object_page_dispatch(page);
            }
            if let Some(page) = cancel.pending_provenance_page() {
                self.cancel_clipboard_provenance_dispatch(page);
            }
            if cancel.awaiting_write() {
                self.cancel_clipboard_write_dispatch(cancel.key());
            }
        }
        self.pending_clipboard_page = None;
        self.clipboard_cut_proofs = None;
        let rejected_restoration_validation = self.reject_restoration_validation(cx);
        let rejected_restoration_geometry = self.restoration_seed.is_some();
        let release = self.geometry.dispose();
        self.release_geometry(&release, None, None, Some(cx));
        self.active_geometry = None;
        self.segmentation = None;
        self.segmentation_action = None;
        self.platform = None;
        self.restoration = None;
        self.restoration_seed = None;
        self.published_restoration = None;
        if rejected_restoration_geometry && !rejected_restoration_validation {
            cx.emit(RangeTextInputEvent::RestorationRejected);
        }
        self.replacement = None;
        self.platform_ready = None;
        self.mutation_positions = None;
        self.adopted_positions = None;
        self.admitted_edit_proofs = Vec::new();
        self.mutation_composition = None;
        self.pending_local_mutation = None;
        self.prepared_local_operation = None;
        self.pending_select_all = false;
        self.cancel_history_dispatch();
        self.surface = None;
        self.pointer_anchor = None;
        self.scrollbar.model.set(None);
        let _ = self
            .scrollbar
            .state
            .unmount_viewport(self.scrollbar.owner, window, cx);
        let requests = std::mem::take(&mut self.requests).into_iter().collect();
        self.dispatched_pages.release_backing();
        self.dispatched_object_pages.release_backing();
        self.dispatched_mutations.release_backing();
        if let Some(loss) = active_loss {
            cx.emit(RangeTextInputEvent::InlineObjectRealizationLost(loss));
            cx.notify();
        }
        requests
    }

    pub(super) fn cancel_mutation_dispatch(&mut self, key: crate::MutationKey, detached: bool) {
        self.requests.retain(|request| {
            !matches!(request,
                RangeTextInputRequest::MutationBegin(begin) if begin.proposal().key() == key
            ) && !matches!(request,
                RangeTextInputRequest::MutationSourcePage(page)
                    | RangeTextInputRequest::MutationProposalPage(page)
                    if page.page().key().key() == key
            ) && !matches!(request,
                RangeTextInputRequest::MutationFinishInput(finish) if finish.key() == key
            ) && !matches!(request,
                RangeTextInputRequest::MutationCommit(commit) if commit.key() == key
            )
        });
        if self.dispatched_mutations.remove(&key) {
            self.commit_prepared_request(if detached {
                RangeTextInputRequest::DetachedMutation(key)
            } else {
                RangeTextInputRequest::CancelMutation(crate::MutationCancelRequest::new(key))
            });
        }
    }

    pub(super) fn cancel_object_page_dispatch(&mut self, key: crate::ObjectRequestKey) {
        self.retire_object_response_custody(key);
        if self
            .deferred_geometry_response
            .as_ref()
            .and_then(super::geometry::DeferredGeometryResponse::object_key)
            == Some(key)
        {
            self.deferred_geometry_response = None;
        }
        if let Some(index) = self.requests.iter().position(|request| {
            matches!(request, RangeTextInputRequest::ObjectPage(request) if request.key() == key)
        }) {
            self.requests.remove(index);
        } else if self.dispatched_object_pages.remove(&key) {
            self.requests
                .push_back(RangeTextInputRequest::CancelObjectPage(key));
        }
    }

    pub(super) fn cancel_clipboard_write_dispatch(&mut self, key: crate::ClipboardKey) {
        if let Some(index) = self.requests.iter().position(|request| {
            matches!(request, RangeTextInputRequest::ClipboardWrite(write) if write.key() == key)
        }) {
            self.requests.remove(index);
        } else if self.dispatched_clipboard == Some(super::DispatchedClipboard::Write(key)) {
            self.dispatched_clipboard = None;
            self.requests
                .push_back(RangeTextInputRequest::CancelClipboardWrite(key));
        }
    }

    pub(super) fn cancel_clipboard_provenance_dispatch(
        &mut self,
        key: crate::ClipboardProvenancePageKey,
    ) {
        if let Some(index) = self.requests.iter().position(|request| {
            matches!(request, RangeTextInputRequest::ClipboardProvenancePage(page) if page.key() == key)
        }) {
            self.requests.remove(index);
        } else if self.dispatched_clipboard
            == Some(super::DispatchedClipboard::Provenance(key))
        {
            self.dispatched_clipboard = None;
            self.requests
                .push_back(RangeTextInputRequest::CancelClipboardProvenancePage(key));
        }
    }

    fn cancel_history_dispatch(&mut self) {
        let Some(pending) = self.pending_history.take() else {
            return;
        };
        if pending.is_admitted() {
            debug_assert!(
                self.config
                    .settlement_coordinator
                    .contains_history(pending.intent())
            );
            return;
        }
        let intent = pending.intent();
        if let Some(index) = self.requests.iter().position(|request| {
            matches!(request, RangeTextInputRequest::HistoryIntent(request) if *request == intent)
        }) {
            self.requests.remove(index);
        } else {
            self.requests
                .push_back(RangeTextInputRequest::CancelHistoryIntent(intent));
        }
    }
}
