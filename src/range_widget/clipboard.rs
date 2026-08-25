use gpui::{Context, Window};

use super::{RangeTextInput, RangeTextInputError, RangeTextInputRequest};
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
            .take()
            .ok_or(RangeTextInputError::Stale)?;
        if !matches!(pending.wait, ClipboardPageWait::Coalesced(existing) if existing == page.key())
        {
            self.pending_clipboard_page = Some(pending);
            return Err(RangeTextInputError::Stale);
        }
        let resident = match page.clone_for_request(pending.request) {
            Ok(resident) => resident,
            Err(_) => {
                self.abort_clipboard_text_request(pending.request, cx);
                return Err(RangeTextInputError::Stale);
            }
        };
        let progress = match self.clipboard.admit_text_page(resident) {
            Ok(progress) => progress,
            Err(_) => {
                self.abort_clipboard_text_request(pending.request, cx);
                return Err(RangeTextInputError::Stale);
            }
        };
        self.advance_clipboard(progress, cx)
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
        let progress = match self.clipboard.admit_text_page(page) {
            Ok(progress) => progress,
            Err(_) => {
                let _ = self.residency.settle(key, PageFailure::Cancelled);
                self.abort_clipboard_text_request(key, cx);
                return Err(RangeTextInputError::Stale);
            }
        };
        let _ = self.residency.settle(key, PageFailure::Cancelled);
        self.advance_clipboard(progress, cx)
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
                        self.push_request(RangeTextInputRequest::Page(request), cx);
                    }
                    PageDemand::ResidentAdjacent(page) => {
                        let resident = match self
                            .residency
                            .page_by_id(page)
                            .and_then(|page| page.clone_for_request(request.key()).ok())
                        {
                            Some(resident) => resident,
                            None => {
                                self.abort_clipboard_text_request(request.key(), cx);
                                return Err(RangeTextInputError::Stale);
                            }
                        };
                        let progress = match self.clipboard.admit_text_page(resident) {
                            Ok(progress) => progress,
                            Err(_) => {
                                self.abort_clipboard_text_request(request.key(), cx);
                                return Err(RangeTextInputError::Stale);
                            }
                        };
                        return self.advance_clipboard(progress, cx);
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
                self.push_request(RangeTextInputRequest::ObjectPage(request), cx);
            }
            crate::ClipboardProgress::Write(write) => {
                self.push_request(RangeTextInputRequest::ClipboardWrite(write), cx);
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
            .interactive_surface()
            .ok_or(RangeTextInputError::Busy)?;
        let selection = surface
            .selection()
            .range()
            .map_err(|_| RangeTextInputError::Stale)?;
        let directed = surface.selection();
        let predecessor =
            crate::MutationPositions::new(directed.head, directed.anchor, directed.head);
        let mut proofs = Vec::with_capacity(2);
        for position in [selection.start(), selection.end()] {
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
                        proof.binding() == self.config.binding && proof.position() == position
                    })
                    .ok_or(crate::MutationError::InvalidObjectGapProof)
            })?;
            if !proofs.contains(&proof) {
                proofs.push(proof);
            }
        }
        self.begin_composite_clipboard_with_proofs(kind, selection, predecessor, proofs, cx)
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
        let progress = self
            .clipboard
            .begin(id, kind, selection, predecessor)
            .map_err(|_| RangeTextInputError::Busy)?;
        let key = crate::ClipboardKey::new(
            id,
            self.config.binding.binding(),
            self.config.binding.revision(),
            selection,
            predecessor,
        );
        self.clipboard_cut_proofs = (kind == crate::ClipboardKind::Cut).then_some((key, proofs));
        self.advance_clipboard(progress, cx)?;
        Ok(key)
    }

    pub fn deliver_object_page(
        &mut self,
        page: ObjectPage,
        cx: &mut Context<Self>,
    ) -> Result<(), RangeTextInputError> {
        let key = page.key();
        if matches!(
            key.purpose(),
            crate::ObjectPurpose::GeometryIndex | crate::ObjectPurpose::GeometryTarget
        ) {
            if !self.dispatched_object_pages.contains(&key) {
                self.requests
                    .push_back(RangeTextInputRequest::ReleaseObjectPage(key));
            }
            return Err(RangeTextInputError::Stale);
        }
        if !self.dispatched_object_pages.contains(&key) {
            self.requests
                .push_back(RangeTextInputRequest::ReleaseObjectPage(key));
            return Err(RangeTextInputError::Stale);
        }
        let purpose = key.purpose();
        self.dispatched_object_pages.remove(&key);
        let result = match purpose {
            crate::ObjectPurpose::Clipboard => {
                let progress = self
                    .clipboard
                    .admit_object_page(page)
                    .map_err(|_| RangeTextInputError::Stale)?;
                self.advance_clipboard(progress, cx)
            }
            crate::ObjectPurpose::Restoration => self.deliver_restoration_object_page(page, cx),
            crate::ObjectPurpose::GeometryIndex | crate::ObjectPurpose::GeometryTarget => {
                unreachable!("geometry object purposes were routed before generic delivery")
            }
            _ => Err(RangeTextInputError::Stale),
        };
        self.requests
            .push_back(RangeTextInputRequest::ReleaseObjectPage(key));
        result
    }

    pub fn deliver_object_page_in_window(
        &mut self,
        page: crate::ObjectPage,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), RangeTextInputError> {
        if matches!(
            page.key().purpose(),
            crate::ObjectPurpose::GeometryIndex | crate::ObjectPurpose::GeometryTarget
        ) && !self.dispatched_object_pages.contains(&page.key())
        {
            self.requests
                .push_back(RangeTextInputRequest::ReleaseObjectPage(page.key()));
            return Err(RangeTextInputError::Stale);
        }
        match page.key().purpose() {
            crate::ObjectPurpose::GeometryTarget => {
                return self.deliver_geometry_target_object_page(page, window, cx);
            }
            crate::ObjectPurpose::GeometryIndex => {
                return self.deliver_geometry_object_page(page, window, cx);
            }
            _ => {}
        }
        self.deliver_object_page(page, cx)?;
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
        if self.dispatched_clipboard_write != Some(key) {
            return Err(RangeTextInputError::Stale);
        }
        let completion = self
            .clipboard
            .acknowledge_write(key, outcome)
            .map_err(|_| RangeTextInputError::Stale)?;
        self.dispatched_clipboard_write = None;
        if let crate::ClipboardCompletion::Delete(deletion) = completion {
            let replacement = deletion.selection();
            let (_, proofs) = self
                .clipboard_cut_proofs
                .take()
                .filter(|(proof_key, _)| *proof_key == key)
                .ok_or(RangeTextInputError::Stale)?;
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
