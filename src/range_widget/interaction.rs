use gpui::{Context, Window};

use crate::{
    ByteOffset, ByteRange, MutationFragment, MutationFragmentPayload, MutationKey, MutationKind,
    MutationOutcome, MutationProposal, MutationSettlement, OperationId, RangeTextInputError,
    RangeTextInputEvent, RangeTextInputRequest, SegmentationContinuation, SegmentationDirection,
    SegmentationKind,
};

use super::{RangeSelection, RangeTextInput};

#[derive(Clone, Copy, Debug)]
pub(super) enum PendingBoundaryAction {
    Move {
        extend: bool,
    },
    Delete,
    SelectPointStart {
        origin: ByteOffset,
        kind: SegmentationKind,
    },
    SelectPointEnd {
        start: ByteOffset,
    },
}

impl RangeTextInput {
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
        let key = MutationKey::new(
            self.config.binding.binding(),
            self.config.binding.revision(),
            OperationId::new(self.next_id()),
        );
        let proposal = MutationProposal::new(key, kind, range, removed_line_breaks);
        self.edits.begin(proposal)?;
        let caret = ByteOffset::new(range.start().get().saturating_add(text.len() as u64));
        self.pending_insert = Some((key, text, caret));
        self.push_request(RangeTextInputRequest::MutationPreflight(proposal), cx);
        Ok(key)
    }

    /// Accepts preflight and emits the exact bounded staging stream.
    pub fn accept_mutation_preflight(
        &mut self,
        key: MutationKey,
        cx: &mut Context<Self>,
    ) -> Result<(), RangeTextInputError> {
        let is_insert = self
            .pending_insert
            .as_ref()
            .is_some_and(|(pending, _, _)| *pending == key);
        let is_history = self
            .pending_history
            .is_some_and(|pending| pending.intent().key() == key && pending.is_planned());
        if !is_insert && !is_history {
            return Err(RangeTextInputError::Stale);
        }
        self.edits.accept_preflight(key)?;
        if is_history {
            return Ok(());
        }
        let (pending_key, text, caret) = self
            .pending_insert
            .take()
            .ok_or(RangeTextInputError::Stale)?;
        debug_assert_eq!(pending_key, key);
        let cap = self.config.mutation_limits.max_staged_bytes().max(1);
        let mut ordinal = 0;
        let mut start = 0;
        while start < text.len() {
            let mut end = start.saturating_add(cap).min(text.len());
            while end > start && !text.is_char_boundary(end) {
                end -= 1;
            }
            if end == start {
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
            self.edits.stage(fragment.clone())?;
            self.push_request(
                RangeTextInputRequest::MutationFragment { key, fragment },
                cx,
            );
            ordinal += 1;
            start = end;
        }
        let terminal = MutationFragment::new(key, ordinal, MutationFragmentPayload::Terminal);
        self.edits.stage(terminal.clone())?;
        self.push_request(
            RangeTextInputRequest::MutationFragment {
                key,
                fragment: terminal,
            },
            cx,
        );
        self.mutation_selection = Some((key, RangeSelection::caret(caret)));
        self.push_request(RangeTextInputRequest::MutationCommit(key), cx);
        Ok(())
    }

    /// Rejects a preflight without changing the coherent publication.
    pub fn reject_mutation_preflight(
        &mut self,
        key: MutationKey,
        cx: &mut Context<Self>,
    ) -> Result<MutationSettlement, RangeTextInputError> {
        let settlement = self.edits.reject_preflight(key)?;
        self.finish_rejected_mutation(key, cx);
        Ok(settlement)
    }

    /// Rejects a host-inspected staged proposal or fragment without publication.
    pub fn reject_mutation_staging(
        &mut self,
        key: MutationKey,
        cx: &mut Context<Self>,
    ) -> Result<MutationSettlement, RangeTextInputError> {
        let settlement = self.edits.reject_staging(key)?;
        self.finish_rejected_mutation(key, cx);
        Ok(settlement)
    }

    /// Records that the host admitted the exact staged commit.
    pub fn admit_mutation_commit(&mut self, key: MutationKey) -> Result<(), RangeTextInputError> {
        if self.detached_edits.len() >= self.config.limits.max_detached_edits {
            return Err(RangeTextInputError::DetachedCapacity);
        }
        self.edits.admit_commit(key)?;
        Ok(())
    }

    fn finish_rejected_mutation(&mut self, key: MutationKey, cx: &mut Context<Self>) {
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
            .is_some_and(|(pending, _, _)| *pending == key)
        {
            self.pending_insert = None;
        }
        if self
            .pending_history
            .is_some_and(|pending| pending.intent().key() == key)
        {
            self.pending_history = None;
        }
        if self
            .mutation_selection
            .is_some_and(|(pending, _)| pending == key)
        {
            self.mutation_selection = None;
        }
        if self
            .mutation_composition
            .is_some_and(|(pending, _, _)| pending == key)
        {
            self.mutation_composition = None;
        }
        cx.emit(RangeTextInputEvent::MutationSettled {
            key,
            outcome: MutationOutcome::Rejected,
        });
        cx.notify();
    }

    /// Delivers one exact terminal mutation settlement.
    pub fn settle_mutation(
        &mut self,
        key: MutationKey,
        outcome: MutationOutcome,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<MutationSettlement, RangeTextInputError> {
        if self.edits.active_key() == Some(key) {
            let selection = self
                .mutation_selection
                .filter(|(active, _)| *active == key)
                .map(|(_, selection)| selection)
                .unwrap_or_else(|| RangeSelection::caret(ByteOffset::new(0)));
            if let MutationOutcome::Committed(successor) = outcome {
                successor.extent().check_byte_range(selection.range())?;
            }
            let settlement = self.edits.settle(key, outcome)?;
            self.dispatched_mutations.remove(&key);
            self.pending_insert = None;
            if self
                .pending_history
                .is_some_and(|pending| pending.intent().key() == key)
            {
                self.pending_history = None;
            }
            if let MutationOutcome::Committed(successor) = outcome {
                self.mutation_selection = None;
                let composition = self.mutation_composition.take();
                self.rebind(successor, Some(selection), window, cx)?;
                if let Some((composition_key, composition, selection)) = composition {
                    if composition_key == key {
                        self.desired.composition = Some(composition);
                        self.desired.selection = selection;
                        if self.geometry.index().is_some() {
                            self.start_target()?;
                        }
                    }
                }
            } else {
                self.mutation_selection = None;
                self.mutation_composition = None;
            }
            cx.emit(RangeTextInputEvent::MutationSettled { key, outcome });
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
        self.begin_replacement(surface.selection().range(), text, MutationKind::Edit, cx)?;
        Ok(())
    }

    pub(super) fn select_offset(
        &mut self,
        offset: ByteOffset,
        extend: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(surface) = self.interactive_surface() else {
            return;
        };
        if !surface.overscan().contains_offset(offset) {
            return;
        }
        let selection = if extend {
            RangeSelection {
                anchor: surface.selection().anchor,
                head: offset,
            }
        } else {
            RangeSelection::caret(offset)
        };
        self.desired.selection = selection;
        self.desired.composition = None;
        self.desired.reveal_caret = true;
        let _ = self.start_target();
        let _ = window;
        cx.notify();
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
            .caret();
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
            PendingBoundaryAction::Move { extend } => {
                let anchor = self
                    .interactive_surface()
                    .ok_or(RangeTextInputError::Busy)?
                    .selection()
                    .anchor;
                self.desired.selection = if extend {
                    RangeSelection {
                        anchor,
                        head: offset,
                    }
                } else {
                    RangeSelection::caret(offset)
                };
                self.desired.composition = None;
                self.desired.reveal_caret = true;
                self.start_target()?;
            }
            PendingBoundaryAction::Delete => {
                let origin = self
                    .interactive_surface()
                    .ok_or(RangeTextInputError::Busy)?
                    .caret();
                let range = ByteRange::new(origin.min(offset), origin.max(offset))?;
                self.begin_replacement(range, String::new(), MutationKind::Edit, cx)?;
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
                self.desired.selection = RangeSelection {
                    anchor: start,
                    head: offset,
                };
                self.desired.composition = None;
                self.desired.reveal_caret = true;
                self.start_target()?;
            }
        }
        let _ = window;
        Ok(())
    }
}
