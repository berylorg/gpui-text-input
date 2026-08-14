use gpui::{Context, Pixels, Window};

use crate::{
    ByteOffset, RangeEditCoordinator, RangeHistoryFrontier, RangeRestorationSeed,
    RangeScrollAnchor, RangeSelection, RangeTextInputError, RangeTextInputEvent,
    RangeTextInputRequest,
};

use super::RangeTextInput;

impl RangeTextInput {
    /// Rebinds this instance while retaining the prior publication until a successor is coherent.
    pub fn rebind(
        &mut self,
        binding: crate::RangeBinding,
        selection: Option<RangeSelection>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), RangeTextInputError> {
        if !self.mounted {
            return Err(RangeTextInputError::NotMounted);
        }
        if self.detached_edits.len() >= self.config.limits.max_detached_edits
            && matches!(
                self.edits.state(),
                crate::MutationState::CommitPending | crate::MutationState::DetachedCommit
            )
        {
            return Err(RangeTextInputError::Busy);
        }
        let mut prior_edits = std::mem::replace(
            &mut self.edits,
            RangeEditCoordinator::new(binding, self.config.mutation_limits),
        );
        if let Some(disposal) = prior_edits.dispose() {
            match disposal {
                crate::MutationDisposal::Cancelled(key) => {
                    self.cancel_mutation_dispatch(key, false)
                }
                crate::MutationDisposal::Detached(key) => {
                    self.cancel_mutation_dispatch(key, true);
                    self.detached_edits.push(prior_edits);
                }
            }
        }
        self.pending_insert = None;
        self.mutation_selection = None;
        self.mutation_composition = None;
        self.cancel_history_dispatch();
        for key in self.residency.rebind(binding) {
            self.cancel_page_dispatch(key);
        }
        self.pending_page_aliases.clear();
        if let Some(cancellation) = self.clipboard.rebind(binding)
            && let Some(page) = cancellation.pending_page()
        {
            self.cancel_page_dispatch(page);
        }
        self.pending_clipboard_page = None;
        let release = self.geometry.rebind(binding)?;
        self.release_geometry(&release, None, Some(cx));
        self.surface_candidate = None;
        self.config.binding = binding;
        self.active_geometry = None;
        self.surface_candidate = None;
        self.segmentation = None;
        self.segmentation_action = None;
        self.platform = None;
        self.restoration = None;
        self.replacement = None;
        self.platform_ready = None;
        self.desired.selection =
            selection.unwrap_or_else(|| RangeSelection::caret(ByteOffset::new(0)));
        self.desired.composition = None;
        self.desired.scroll.source = self.desired.selection.head;
        self.desired.scroll.intra_anchor = Pixels::ZERO;
        self.desired.target_block = Pixels::ZERO;
        self.desired.preserve_scroll_anchor = false;
        self.desired.reveal_caret = true;
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
        if !self.scrollbar.state.replace_owner(
            prior_scrollbar_owner,
            replacement_scrollbar_owner,
            window,
            cx,
        ) {
            return Err(RangeTextInputError::Stale);
        }
        self.scrollbar.owner = replacement_scrollbar_owner;
        self.scrollbar.model.set(None);
        self.start_index()?;
        cx.notify();
        Ok(())
    }

    /// Exports a text-free restoration seed only after every operation is quiescent.
    pub fn export_restoration(
        &self,
        history: Option<RangeHistoryFrontier>,
    ) -> Result<RangeRestorationSeed, RangeTextInputError> {
        if !self.is_quiescent() {
            return Err(RangeTextInputError::NotQuiescent);
        }
        let surface = self
            .surface
            .as_ref()
            .ok_or(RangeTextInputError::NotQuiescent)?;
        if surface.composition().is_some() {
            return Err(RangeTextInputError::NotQuiescent);
        }
        Ok(RangeRestorationSeed {
            binding: surface.binding(),
            caret: surface.caret(),
            selection: surface.selection(),
            scroll: RangeScrollAnchor {
                source: surface.scroll_source(),
                intra_anchor: surface.scroll_intra_anchor(),
            },
            viewport: surface.viewport(),
            overscan: surface.overscan(),
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
            || seed.caret != seed.selection.head
            || seed.scroll.intra_anchor < Pixels::ZERO
            || seed.scroll.intra_anchor > self.config.limits.max_intra_anchor
            || !seed.overscan.contains(seed.viewport)
        {
            cx.emit(RangeTextInputEvent::RestorationRejected);
            return Err(RangeTextInputError::MalformedSeed);
        }
        let extent = seed.binding.extent();
        for range in [seed.selection.range(), seed.viewport, seed.overscan] {
            if extent.check_byte_range(range).is_err() {
                cx.emit(RangeTextInputEvent::RestorationRejected);
                return Err(RangeTextInputError::MalformedSeed);
            }
        }
        for offset in [
            seed.caret,
            seed.selection.anchor,
            seed.selection.head,
            seed.scroll.source,
            seed.viewport.start(),
            seed.viewport.end(),
            seed.overscan.start(),
            seed.overscan.end(),
        ] {
            if offset.get() > extent.byte_len() {
                cx.emit(RangeTextInputEvent::RestorationRejected);
                return Err(RangeTextInputError::MalformedSeed);
            }
        }
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
        self.mounted = false;
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
                    self.detached_edits.push(edits);
                }
            }
        }
        for key in self.residency.dispose() {
            self.cancel_page_dispatch(key);
        }
        self.pending_page_aliases.clear();
        if let Some(cancel) = self.clipboard.dispose()
            && let Some(page) = cancel.pending_page()
        {
            self.cancel_page_dispatch(page);
        }
        self.pending_clipboard_page = None;
        let release = self.geometry.dispose();
        self.release_geometry(&release, None, Some(cx));
        self.active_geometry = None;
        self.segmentation = None;
        self.segmentation_action = None;
        self.platform = None;
        self.restoration = None;
        self.replacement = None;
        self.platform_ready = None;
        self.mutation_selection = None;
        self.mutation_composition = None;
        self.cancel_history_dispatch();
        self.surface = None;
        self.pointer_anchor = None;
        self.scrollbar.model.set(None);
        let _ = self
            .scrollbar
            .state
            .unmount_viewport(self.scrollbar.owner, window, cx);
        self.requests.drain(..).collect()
    }

    fn cancel_mutation_dispatch(&mut self, key: crate::MutationKey, detached: bool) {
        self.requests.retain(|request| {
            !matches!(request,
                RangeTextInputRequest::MutationPreflight(proposal) if proposal.key() == key
            ) && !matches!(request,
                RangeTextInputRequest::MutationFragment { key: request_key, .. }
                    | RangeTextInputRequest::MutationCommit(request_key) if *request_key == key
            )
        });
        if self.dispatched_mutations.remove(&key) {
            self.requests.push_back(if detached {
                RangeTextInputRequest::DetachedMutation(key)
            } else {
                RangeTextInputRequest::CancelMutation(key)
            });
        }
    }

    fn cancel_history_dispatch(&mut self) {
        let Some(pending) = self.pending_history.take() else {
            return;
        };
        if pending.is_planned() {
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
