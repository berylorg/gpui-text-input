use gpui::{Context, Window};

use super::response_custody::ResponseDeliveryProgress;
use super::{RangeTextInput, RangeTextInputError, RangeTextInputRequest, SurfaceCandidateKind};
use crate::{
    ObjectPage, ObjectPageFailure, ObjectRequestId, PageDemand, PageFailure, PagePurpose,
    PageRequestId, PageRequestKey, RangePage, SourceRange,
};

pub(super) struct PendingClipboardPage {
    request: PageRequestKey,
    wait: ClipboardPageWait,
}

enum ClipboardPageWait {
    Coalesced(PageRequestKey),
}

impl RangeTextInput {
    pub fn clipboard_counts(&self) -> crate::ClipboardCounts {
        self.clipboard.counts()
    }

    pub(super) fn clipboard_waits_on(&self, key: PageRequestKey) -> bool {
        self.pending_clipboard_page.as_ref().is_some_and(|pending| {
            matches!(pending.wait, ClipboardPageWait::Coalesced(existing) if existing == key)
        })
    }

    pub(super) fn service_coalesced_clipboard_page(
        &mut self,
        page: &RangePage,
        cx: &mut Context<Self>,
    ) -> Result<(), RangeTextInputError> {
        let pending = self
            .pending_clipboard_page
            .as_ref()
            .ok_or(RangeTextInputError::Stale)?;
        if !matches!(pending.wait, ClipboardPageWait::Coalesced(existing) if existing == page.key())
        {
            return Err(RangeTextInputError::Stale);
        }
        let request = pending.request;
        self.admit_borrowed_page_clone(page, request, cx)?;
        self.pending_clipboard_page = None;
        Ok(())
    }

    pub(super) fn fail_coalesced_clipboard_page(
        &mut self,
        key: PageRequestKey,
        failure: PageFailure,
        cx: &mut Context<Self>,
    ) -> Result<(), RangeTextInputError> {
        let pending = self
            .pending_clipboard_page
            .take()
            .ok_or(RangeTextInputError::Stale)?;
        if !matches!(pending.wait, ClipboardPageWait::Coalesced(existing) if existing == key) {
            self.pending_clipboard_page = Some(pending);
            return Err(RangeTextInputError::Stale);
        }
        let progress = match self.clipboard.settle_text_page(pending.request, failure) {
            Ok(progress) => progress,
            Err(_) => {
                self.abort_clipboard_text_request(pending.request, cx);
                return Err(RangeTextInputError::Stale);
            }
        };
        self.advance_clipboard(progress, cx)
    }

    pub(super) fn deliver_clipboard_page(
        &mut self,
        page: RangePage,
        cx: &mut Context<Self>,
    ) -> Result<(), RangeTextInputError> {
        let key = page.key();
        let step = match self.clipboard.prepare_text_page(&page) {
            Ok(step) => step,
            Err(_) => {
                let _ = self.residency.settle(key, PageFailure::Cancelled);
                self.abort_clipboard_text_request(key, cx);
                return Err(RangeTextInputError::Stale);
            }
        };
        self.admit_clipboard_prepared_step(&step)?;
        self.commit_prepared_clipboard_page(page, step, cx)
    }

    pub(super) fn commit_prepared_clipboard_page(
        &mut self,
        page: RangePage,
        step: crate::ClipboardPreparedStep,
        cx: &mut Context<Self>,
    ) -> Result<(), RangeTextInputError> {
        let commit = self
            .clipboard
            .commit_text_page(page, step)
            .map_err(RangeTextInputError::Clipboard)?;
        self.active_response_processing = Default::default();
        self.drive_clipboard_prepared(commit, cx)
    }

    pub(super) fn admit_clipboard_prepared_step(
        &mut self,
        step: &crate::ClipboardPreparedStep,
    ) -> Result<(), RangeTextInputError> {
        let current = self.current_realization_ownership();
        let old = self.clipboard.ownership_charge();
        let transfer = if step.transfers_response() {
            self.active_response_processing
        } else {
            Default::default()
        };
        let projected = crate::RangeSurfaceCharge {
            bytes: current
                .owned_bytes
                .checked_sub(old.bytes())
                .and_then(|value| value.checked_sub(transfer.bytes))
                .and_then(|value| value.checked_add(step.peak_ownership().bytes()))
                .ok_or(RangeTextInputError::SurfaceCapacity)?,
            items: current
                .owned_items
                .checked_sub(old.items())
                .and_then(|value| value.checked_sub(transfer.items))
                .and_then(|value| value.checked_add(step.peak_ownership().items()))
                .ok_or(RangeTextInputError::SurfaceCapacity)?,
        };
        let peak = crate::RangeSurfaceCharge {
            bytes: current.owned_bytes.max(projected.bytes),
            items: current.owned_items.max(projected.items),
        };
        if peak.bytes > self.config.limits.max_surface_bytes
            || peak.items > self.config.limits.max_surface_items
        {
            return Err(RangeTextInputError::SurfaceCapacity);
        }
        self.observe_surface_admission_peak(peak);
        Ok(())
    }

    fn admit_clipboard_prepared_begin(
        &mut self,
        prepared: &crate::ClipboardPreparedBegin,
    ) -> Result<(), RangeTextInputError> {
        let current = self.current_realization_ownership();
        debug_assert_eq!(self.clipboard.ownership_charge(), Default::default());
        let projected = crate::RangeSurfaceCharge {
            bytes: current
                .owned_bytes
                .checked_add(prepared.peak_ownership().bytes())
                .ok_or(RangeTextInputError::SurfaceCapacity)?,
            items: current
                .owned_items
                .checked_add(prepared.peak_ownership().items())
                .ok_or(RangeTextInputError::SurfaceCapacity)?,
        };
        if projected.bytes > self.config.limits.max_surface_bytes
            || projected.items > self.config.limits.max_surface_items
        {
            return Err(RangeTextInputError::SurfaceCapacity);
        }
        self.observe_surface_admission_peak(projected);
        Ok(())
    }

    fn drive_clipboard_prepared(
        &mut self,
        mut commit: crate::ClipboardPreparedCommit,
        cx: &mut Context<Self>,
    ) -> Result<(), RangeTextInputError> {
        loop {
            if let Some(key) = commit.released_text_page() {
                let _ = self.residency.settle(key, PageFailure::Cancelled);
                self.dispatched_pages.remove(&key);
                self.commit_prepared_request(RangeTextInputRequest::ReleasePage(key));
            }
            if let Some(key) = commit.released_object_page() {
                self.dispatched_object_pages.remove(&key);
                self.commit_prepared_request(RangeTextInputRequest::ReleaseObjectPage(key));
            }
            if let Some(progress) = commit.into_progress() {
                return self.advance_clipboard(progress, cx);
            }
            let step = self
                .clipboard
                .prepare_next()
                .map_err(RangeTextInputError::Clipboard)?;
            self.admit_clipboard_prepared_step(&step)?;
            commit = self
                .clipboard
                .commit_prepared(step)
                .map_err(RangeTextInputError::Clipboard)?;
        }
    }

    pub(super) fn resume_clipboard_prepared(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Result<(), RangeTextInputError> {
        let step = self
            .clipboard
            .prepare_next()
            .map_err(RangeTextInputError::Clipboard)?;
        self.admit_clipboard_prepared_step(&step)?;
        let commit = self
            .clipboard
            .commit_prepared(step)
            .map_err(RangeTextInputError::Clipboard)?;
        self.drive_clipboard_prepared(commit, cx)
    }

    pub(super) fn advance_clipboard(
        &mut self,
        progress: crate::ClipboardProgress,
        cx: &mut Context<Self>,
    ) -> Result<(), RangeTextInputError> {
        match progress {
            crate::ClipboardProgress::NeedTextPage {
                key,
                next_offset: _,
                target: _,
            } => {
                let id = PageRequestId::new(self.next_id());
                let request = self
                    .clipboard
                    .request_text_page(key, id)
                    .map_err(|_| RangeTextInputError::Busy)?;
                let demand =
                    match self
                        .residency
                        .demand(id, PagePurpose::Clipboard, request.key().demand())
                    {
                        Ok(demand) => demand,
                        Err(_) => {
                            self.abort_clipboard_text_request(request.key(), cx);
                            return Err(RangeTextInputError::Busy);
                        }
                    };
                match demand {
                    PageDemand::Requested(expected) if expected.key() == request.key() => {
                        if let Err(error) =
                            self.push_request(RangeTextInputRequest::Page(request), cx)
                        {
                            self.abort_clipboard_text_request(expected.key(), cx);
                            return Err(error);
                        }
                    }
                    PageDemand::ResidentAdjacent(page) => {
                        if let Err(error) = self.admit_resident_page_clone(page, request.key(), cx)
                        {
                            self.abort_clipboard_text_request(request.key(), cx);
                            return Err(error);
                        }
                    }
                    PageDemand::Coalesced(existing) => {
                        self.pending_clipboard_page = Some(PendingClipboardPage {
                            request: request.key(),
                            wait: ClipboardPageWait::Coalesced(existing),
                        });
                        cx.notify();
                    }
                    unexpected => {
                        if let PageDemand::Requested(unexpected) = unexpected {
                            let _ = self.residency.cancel(unexpected.key());
                        }
                        self.abort_clipboard_text_request(request.key(), cx);
                        return Err(RangeTextInputError::Stale);
                    }
                }
            }
            crate::ClipboardProgress::NeedObjectPage { key, cursor: _ } => {
                let id = ObjectRequestId::new(self.next_id());
                let request = self
                    .clipboard
                    .request_object_page(key, id)
                    .map_err(|_| RangeTextInputError::Busy)?;
                if let Err(error) =
                    self.push_request(RangeTextInputRequest::ObjectPage(request), cx)
                {
                    let _ = self.clipboard.cancel(key);
                    self.clipboard_cut_proofs = None;
                    self.observe_realization_ownership();
                    return Err(error);
                }
            }
            crate::ClipboardProgress::ProvenancePage(page) => {
                let key = page.key().clipboard();
                if let Err(error) =
                    self.push_request(RangeTextInputRequest::ClipboardProvenancePage(page), cx)
                {
                    let _ = self.clipboard.cancel(key);
                    self.clipboard_cut_proofs = None;
                    return Err(error);
                }
            }
            crate::ClipboardProgress::Write(write) => {
                let key = write.key();
                if let Err(error) =
                    self.push_request(RangeTextInputRequest::ClipboardWrite(write), cx)
                {
                    let _ = self.clipboard.cancel(key);
                    self.clipboard_cut_proofs = None;
                    return Err(error);
                }
            }
            crate::ClipboardProgress::Terminal(crate::ClipboardCompletion::Propagate(kind)) => {
                self.clipboard_cut_proofs = None;
                let command = match kind {
                    crate::ClipboardKind::Copy => crate::TextInputCommand::Copy,
                    crate::ClipboardKind::Cut => crate::TextInputCommand::Cut,
                };
                cx.emit(crate::RangeTextInputEvent::CommandPropagated(command));
                cx.notify();
            }
            crate::ClipboardProgress::Terminal(_) => {
                self.clipboard_cut_proofs = None;
                cx.notify();
            }
        }
        Ok(())
    }

    fn abort_clipboard_text_request(&mut self, request: PageRequestKey, cx: &mut Context<Self>) {
        if self
            .pending_clipboard_page
            .as_ref()
            .is_some_and(|pending| pending.request == request)
        {
            self.pending_clipboard_page = None;
        }
        let _ = self.residency.cancel(request);
        let settled = self
            .clipboard
            .settle_text_page(request, PageFailure::Cancelled);
        debug_assert!(matches!(
            settled,
            Ok(crate::ClipboardProgress::Terminal(
                crate::ClipboardCompletion::Cancelled
            ))
        ));
        self.clipboard_cut_proofs = None;
        cx.notify();
    }

    pub fn begin_clipboard(
        &mut self,
        kind: crate::ClipboardKind,
        cx: &mut Context<Self>,
    ) -> Result<crate::ClipboardKey, RangeTextInputError> {
        if !self.enabled {
            return Err(RangeTextInputError::Busy);
        }
        if kind == crate::ClipboardKind::Cut && (!self.enabled || self.read_only) {
            return Err(RangeTextInputError::ReadOnly);
        }
        let surface = self
            .surface
            .as_ref()
            .filter(|surface| {
                let surface_geometry = surface.geometry_key();
                let candidate_eligible = self.surface_candidate.as_ref().is_none_or(|candidate| {
                    candidate.kind == SurfaceCandidateKind::IndexRefinement
                        || (candidate.kind == SurfaceCandidateKind::Replacement
                            && candidate.binding == self.config.binding
                            && candidate.job.geometry() == surface_geometry
                            && self.active_geometry == Some(candidate.job))
                });
                self.mounted
                    && surface.binding() == self.config.binding
                    && surface_geometry.binding() == self.config.binding.binding()
                    && surface_geometry.revision() == self.config.binding.revision()
                    && surface_geometry.presentation_generation()
                        == self.config.presentation_generation
                    && surface_geometry.epoch() == self.geometry.key().epoch()
                    && self.pending_history.is_none()
                    && self.pending_layout_intent.is_none()
                    && self.pending_presentation_intent.is_none()
                    && self.pending_rebind_intent.is_none()
                    && candidate_eligible
            })
            .ok_or(RangeTextInputError::Busy)?;
        let selection = surface
            .selection()
            .range()
            .map_err(|_| RangeTextInputError::Stale)?;
        let directed = surface.selection();
        let predecessor =
            crate::MutationPositions::new(directed.head, directed.anchor, directed.head);
        self.begin_composite_clipboard_with_proofs(kind, selection, predecessor, Vec::new(), cx)
    }

    pub fn begin_composite_clipboard(
        &mut self,
        kind: crate::ClipboardKind,
        selection: SourceRange,
        predecessor: crate::MutationPositions,
        text: &crate::RangeResidency,
        objects: &crate::ObjectResidency,
        cx: &mut Context<Self>,
    ) -> Result<crate::ClipboardKey, RangeTextInputError> {
        let mut proofs = Vec::with_capacity(2);
        for position in [selection.start(), selection.end()] {
            let proof = crate::range_edit::SourcePositionProof::from_admitted_sources(
                self.config.binding,
                position,
                text,
                objects,
            )?;
            if !proofs.contains(&proof) {
                proofs.push(proof);
            }
        }
        self.begin_composite_clipboard_with_proofs(kind, selection, predecessor, proofs, cx)
    }

    fn begin_composite_clipboard_with_proofs(
        &mut self,
        kind: crate::ClipboardKind,
        selection: SourceRange,
        predecessor: crate::MutationPositions,
        proofs: Vec<crate::range_edit::SourcePositionProof>,
        cx: &mut Context<Self>,
    ) -> Result<crate::ClipboardKey, RangeTextInputError> {
        if !self.mounted {
            return Err(RangeTextInputError::NotMounted);
        }
        if !self.enabled {
            return Err(RangeTextInputError::Busy);
        }
        if kind == crate::ClipboardKind::Cut && self.read_only {
            return Err(RangeTextInputError::ReadOnly);
        }
        let id = crate::ClipboardId::new(self.next_id());
        let prepared = self
            .clipboard
            .prepare_begin(id, kind, selection, predecessor)
            .map_err(|_| RangeTextInputError::Busy)?;
        let key = prepared.key();
        self.admit_clipboard_prepared_begin(&prepared)?;
        let progress = self
            .clipboard
            .commit_begin(prepared)
            .map_err(RangeTextInputError::Clipboard)?;
        self.clipboard_cut_proofs = (kind == crate::ClipboardKind::Cut).then_some((key, proofs));
        self.advance_clipboard(progress, cx)?;
        Ok(key)
    }

    pub fn deliver_object_page(
        &mut self,
        page: ObjectPage,
        cx: &mut Context<Self>,
    ) -> Result<(), RangeTextInputError> {
        if matches!(
            page.key().purpose(),
            crate::ObjectPurpose::GeometryIndex | crate::ObjectPurpose::GeometryTarget
        ) {
            return Err(RangeTextInputError::ObjectResponseRejected(page));
        }
        let key = page.key();
        if !self.dispatched_object_pages.contains(&key) {
            return Err(RangeTextInputError::ObjectResponseRejected(page));
        }
        self.admit_response_custody(super::response_custody::RangeResponseCustody::Object(page))
            .map_err(|response| match response {
                super::response_custody::RangeResponseCustody::Object(page) => {
                    RangeTextInputError::ObjectResponseCapacity(page)
                }
                super::response_custody::RangeResponseCustody::Page(_)
                | super::response_custody::RangeResponseCustody::PageNoAliases(_)
                | super::response_custody::RangeResponseCustody::ResidentPage(_)
                | super::response_custody::RangeResponseCustody::AliasFanout(_) => unreachable!(),
            })?;
        match self.service_object_response_custody(cx) {
            super::response_custody::ResponseCustodyProgress::Idle
            | super::response_custody::ResponseCustodyProgress::Progressed
            | super::response_custody::ResponseCustodyProgress::AcceptedTerminal
            | super::response_custody::ResponseCustodyProgress::RetryableTerminalSurfaceCapacity
            | super::response_custody::ResponseCustodyProgress::RetryableClipboardPreparationCapacity => Ok(()),
            super::response_custody::ResponseCustodyProgress::Rejected(error) => Err(error),
        }
    }

    pub(super) fn deliver_custodied_object_page(
        &mut self,
        page: ObjectPage,
        window: Option<&mut Window>,
        cx: &mut Context<Self>,
    ) -> Result<ResponseDeliveryProgress, RangeTextInputError> {
        let key = page.key();
        if matches!(
            key.purpose(),
            crate::ObjectPurpose::GeometryIndex | crate::ObjectPurpose::GeometryTarget
        ) {
            let Some(window) = window else {
                return Ok(ResponseDeliveryProgress::Rejected(
                    RangeTextInputError::Busy,
                ));
            };
            return match key.purpose() {
                crate::ObjectPurpose::GeometryTarget => {
                    self.deliver_geometry_target_object_page_inner(page, true, window, cx)
                }
                crate::ObjectPurpose::GeometryIndex => {
                    self.deliver_geometry_object_page_inner(page, true, window, cx)
                }
                _ => unreachable!(),
            };
        }
        if !self.dispatched_object_pages.contains(&key) {
            self.commit_prepared_request(RangeTextInputRequest::ReleaseObjectPage(key));
            return Ok(ResponseDeliveryProgress::Rejected(
                RangeTextInputError::Stale,
            ));
        }
        let purpose = key.purpose();
        let result = match purpose {
            crate::ObjectPurpose::Clipboard => {
                let step = self
                    .clipboard
                    .prepare_object_page(&page)
                    .map_err(|_| RangeTextInputError::Stale)?;
                self.admit_clipboard_prepared_step(&step)?;
                self.commit_prepared_clipboard_object_page(page, step, cx)
            }
            crate::ObjectPurpose::Restoration => {
                self.dispatched_object_pages.remove(&key);
                self.deliver_restoration_object_page(page, cx)
            }
            crate::ObjectPurpose::GeometryIndex | crate::ObjectPurpose::GeometryTarget => {
                unreachable!("geometry object purposes were routed before generic delivery")
            }
            _ => Err(RangeTextInputError::Stale),
        };
        if self.clipboard.pending_object_page() != Some(key) {
            self.dispatched_object_pages.remove(&key);
        }
        Ok(match result {
            Ok(()) => ResponseDeliveryProgress::Progressed,
            Err(error) => ResponseDeliveryProgress::Rejected(error),
        })
    }

    pub(super) fn commit_prepared_clipboard_object_page(
        &mut self,
        page: ObjectPage,
        step: crate::ClipboardPreparedStep,
        cx: &mut Context<Self>,
    ) -> Result<(), RangeTextInputError> {
        let commit = self
            .clipboard
            .commit_object_page(page, step)
            .map_err(RangeTextInputError::Clipboard)?;
        self.active_response_processing = Default::default();
        self.drive_clipboard_prepared(commit, cx)
    }

    pub fn deliver_object_page_in_window(
        &mut self,
        page: crate::ObjectPage,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), RangeTextInputError> {
        if !self.dispatched_object_pages.contains(&page.key()) {
            return Err(RangeTextInputError::ObjectResponseRejected(page));
        }
        self.admit_response_custody(super::response_custody::RangeResponseCustody::Object(page))
            .map_err(|response| match response {
                super::response_custody::RangeResponseCustody::Object(page) => {
                    RangeTextInputError::ObjectResponseCapacity(page)
                }
                super::response_custody::RangeResponseCustody::Page(_)
                | super::response_custody::RangeResponseCustody::PageNoAliases(_)
                | super::response_custody::RangeResponseCustody::ResidentPage(_)
                | super::response_custody::RangeResponseCustody::AliasFanout(_) => unreachable!(),
            })?;
        if let super::response_custody::ResponseCustodyProgress::Rejected(error) =
            self.service_response_custody(window, cx)
        {
            return Err(error);
        }
        self.service_geometry_until_external_boundary(window, cx)
    }

    pub fn fail_object_page(
        &mut self,
        key: crate::ObjectRequestKey,
        failure: ObjectPageFailure,
        cx: &mut Context<Self>,
    ) -> Result<(), RangeTextInputError> {
        if !self.dispatched_object_pages.remove(&key) {
            return Err(RangeTextInputError::Stale);
        }
        match key.purpose() {
            crate::ObjectPurpose::Clipboard => {
                let progress = self
                    .clipboard
                    .settle_object_page(key, failure)
                    .map_err(|_| RangeTextInputError::Stale)?;
                self.advance_clipboard(progress, cx)
            }
            crate::ObjectPurpose::Restoration => {
                self.reject_restoration(cx);
                Ok(())
            }
            crate::ObjectPurpose::GeometryIndex | crate::ObjectPurpose::GeometryTarget => {
                self.fail_geometry_object_page(key, failure, cx)
            }
            _ => Err(RangeTextInputError::Stale),
        }
    }

    pub fn settle_clipboard_write(
        &mut self,
        key: crate::ClipboardKey,
        outcome: crate::ClipboardWriteOutcome,
        cx: &mut Context<Self>,
    ) -> Result<crate::ClipboardCompletion, RangeTextInputError> {
        if self.dispatched_clipboard != Some(super::DispatchedClipboard::Write(key)) {
            return Err(RangeTextInputError::Stale);
        }
        let completion = self
            .clipboard
            .acknowledge_write(key, outcome)
            .map_err(|_| RangeTextInputError::Stale)?;
        self.dispatched_clipboard = None;
        if let crate::ClipboardCompletion::Delete(deletion) = completion {
            let replacement = deletion.selection();
            let (_, mut proofs) = self
                .clipboard_cut_proofs
                .take()
                .filter(|(proof_key, _)| *proof_key == key)
                .ok_or(RangeTextInputError::Stale)?;
            if proofs.is_empty() {
                let surface = self
                    .surface
                    .as_ref()
                    .filter(|surface| self.mounted && surface.binding() == self.config.binding)
                    .ok_or(RangeTextInputError::Stale)?;
                for position in [replacement.start(), replacement.end()] {
                    let proof = crate::range_edit::SourcePositionProof::from_surface_pages(
                        self.config.binding,
                        position,
                        surface.pages(),
                        surface.object_pages(),
                    )
                    .or_else(|_| {
                        self.admitted_edit_proofs
                            .iter()
                            .copied()
                            .find(|proof| {
                                proof.binding() == self.config.binding
                                    && proof.position() == position
                            })
                            .ok_or(crate::MutationError::InvalidObjectGapProof)
                    })?;
                    if !proofs.contains(&proof) {
                        proofs.push(proof);
                    }
                }
            }
            let mutation =
                deletion.proposal(crate::OperationId::new(self.next_id()), replacement)?;
            let removed = selected_object_neighbor(replacement)
                .map(|object| crate::ObjectTarget::new(replacement, object.id(), object.order()))
                .transpose()?;
            let caret = if removed.is_some() {
                super::interaction::successor_position(replacement, removed, 0)?
            } else {
                replacement.start()
            };
            let items = removed
                .map(|target| {
                    crate::MutationPageItem::Object(crate::ObjectChange::Remove { target })
                })
                .into_iter()
                .collect();
            self.begin_local_mutation(
                mutation,
                items,
                crate::MutationPositions::collapsed(caret),
                cx,
            )?;
            self.admitted_edit_proofs = proofs;
        } else {
            self.clipboard_cut_proofs = None;
        }
        Ok(completion)
    }

    pub fn acknowledge_clipboard_provenance_page(
        &mut self,
        page: crate::ClipboardProvenancePage,
        cx: &mut Context<Self>,
    ) -> Result<(), RangeTextInputError> {
        if self.dispatched_clipboard != Some(super::DispatchedClipboard::Provenance(page.key())) {
            return Err(RangeTextInputError::Stale);
        }
        let step = match self.clipboard.acknowledge_provenance_page(page) {
            Ok(step) => step,
            Err(crate::ClipboardError::ProvenancePageCollision(_)) => {
                self.dispatched_clipboard = None;
                self.clipboard_cut_proofs = None;
                self.observe_realization_ownership();
                return Err(RangeTextInputError::Stale);
            }
            Err(_) => return Err(RangeTextInputError::Stale),
        };
        self.dispatched_clipboard = None;
        self.admit_clipboard_prepared_step(&step)?;
        let commit = self
            .clipboard
            .commit_prepared(step)
            .map_err(RangeTextInputError::Clipboard)?;
        self.drive_clipboard_prepared(commit, cx)
    }
}

fn selected_object_neighbor(range: SourceRange) -> Option<crate::InlineObjectNeighbor> {
    let leading = match range.start().gap {
        crate::InlineObjectGap::Before(first) => Some(first),
        crate::InlineObjectGap::Between { following, .. } => Some(following),
        crate::InlineObjectGap::NoObjects | crate::InlineObjectGap::After(_) => None,
    };
    let trailing = match range.end().gap {
        crate::InlineObjectGap::After(last) => Some(last),
        crate::InlineObjectGap::Between { preceding, .. } => Some(preceding),
        crate::InlineObjectGap::NoObjects | crate::InlineObjectGap::Before(_) => None,
    };
    (leading == trailing).then_some(leading).flatten()
}
