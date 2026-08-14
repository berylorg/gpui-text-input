//! Bounded exact clipboard collection and settlement.

use gpui::Context;

use super::{RangeTextInput, RangeTextInputError, RangeTextInputRequest};
use crate::{PageDemand, PageFailure, PagePurpose, PageRequestId, PageRequestKey, RangePage};

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
        let resident = page
            .clone_for_request(pending.request)
            .map_err(|_| RangeTextInputError::Stale)?;
        let progress = self
            .clipboard
            .admit_page(resident)
            .map_err(|_| RangeTextInputError::Stale)?;
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
        let progress = self
            .clipboard
            .settle_page(pending.request, failure)
            .map_err(|_| RangeTextInputError::Stale)?;
        self.advance_clipboard(progress, cx)
    }

    pub(super) fn deliver_clipboard_page(
        &mut self,
        page: RangePage,
        cx: &mut Context<Self>,
    ) -> Result<(), RangeTextInputError> {
        let key = page.key();
        let progress = self
            .clipboard
            .admit_page(page)
            .map_err(|_| RangeTextInputError::Stale)?;
        let _ = self.residency.settle(key, PageFailure::Cancelled);
        self.advance_clipboard(progress, cx)
    }

    pub(super) fn advance_clipboard(
        &mut self,
        progress: crate::ClipboardProgress,
        cx: &mut Context<Self>,
    ) -> Result<(), RangeTextInputError> {
        match progress {
            crate::ClipboardProgress::NeedPage { key, next_offset } => {
                let _ = next_offset;
                let id = PageRequestId::new(self.next_id());
                let request = self
                    .clipboard
                    .request_page(key, id)
                    .map_err(|_| RangeTextInputError::Busy)?;
                let demand = self
                    .residency
                    .demand(id, PagePurpose::Clipboard, request.key().demand())
                    .map_err(|_| RangeTextInputError::Busy)?;
                match demand {
                    PageDemand::Requested(expected) if expected.key() == request.key() => {
                        self.push_request(RangeTextInputRequest::Page(request), cx);
                    }
                    PageDemand::ResidentAdjacent(page) => {
                        let resident = self
                            .residency
                            .page_by_id(page)
                            .and_then(|page| page.clone_for_request(request.key()).ok())
                            .ok_or(RangeTextInputError::Stale)?;
                        let progress = self
                            .clipboard
                            .admit_page(resident)
                            .map_err(|_| RangeTextInputError::Stale)?;
                        return self.advance_clipboard(progress, cx);
                    }
                    PageDemand::Coalesced(existing) => {
                        self.pending_clipboard_page = Some(PendingClipboardPage {
                            request: request.key(),
                            wait: ClipboardPageWait::Coalesced(existing),
                        });
                        cx.notify();
                    }
                    _ => return Err(RangeTextInputError::Stale),
                }
            }
            crate::ClipboardProgress::Write(write) => {
                self.push_request(RangeTextInputRequest::ClipboardWrite(write), cx);
            }
            crate::ClipboardProgress::Terminal(_) => {
                cx.notify();
            }
        }
        Ok(())
    }

    /// Starts bounded exact copy or cut collection for the current coherent selection.
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
        let selection = self
            .interactive_surface()
            .ok_or(RangeTextInputError::Busy)?
            .selection()
            .range();
        let id = crate::ClipboardId::new(self.next_id());
        let progress = self
            .clipboard
            .begin(id, kind, selection)
            .map_err(|_| RangeTextInputError::Busy)?;
        let key = crate::ClipboardKey::new(
            id,
            self.config.binding.binding(),
            self.config.binding.revision(),
            selection,
        );
        self.advance_clipboard(progress, cx)?;
        Ok(key)
    }

    /// Delivers the exact result of the platform clipboard write.
    pub fn settle_clipboard_write(
        &mut self,
        key: crate::ClipboardKey,
        outcome: crate::ClipboardWriteOutcome,
        cx: &mut Context<Self>,
    ) -> Result<crate::ClipboardCompletion, RangeTextInputError> {
        let completion = self
            .clipboard
            .acknowledge_write(key, outcome)
            .map_err(|_| RangeTextInputError::Stale)?;
        if let crate::ClipboardCompletion::Delete(deletion) = completion {
            let mutation = deletion.proposal(crate::OperationId::new(self.next_id()));
            self.edits.begin(mutation)?;
            self.pending_insert = Some((
                mutation.key(),
                String::new(),
                mutation.replacement().start(),
            ));
            self.push_request(RangeTextInputRequest::MutationPreflight(mutation), cx);
        }
        Ok(completion)
    }
}
