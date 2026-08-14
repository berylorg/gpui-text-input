use crate::{
    ByteOffset, PageDemandEnvelope, PageFailure, PagePurpose, PageRequestId, RangePage,
    RangeRestorationSeed, RangeTextInput, RangeTextInputError, RangeTextInputEvent,
};

#[derive(Debug)]
pub(super) struct RestorationValidation {
    seed: RangeRestorationSeed,
    offsets: Vec<ByteOffset>,
    next: usize,
    pending: Option<crate::PageRequestKey>,
}

impl RestorationValidation {
    pub fn new(seed: RangeRestorationSeed) -> Self {
        let mut offsets = vec![
            seed.caret,
            seed.selection.anchor,
            seed.selection.head,
            seed.scroll.source,
            seed.viewport.start(),
            seed.viewport.end(),
            seed.overscan.start(),
            seed.overscan.end(),
        ];
        offsets.sort();
        offsets.dedup();
        Self {
            seed,
            offsets,
            next: 0,
            pending: None,
        }
    }

    pub const fn pending(&self) -> Option<crate::PageRequestKey> {
        self.pending
    }
}

impl RangeTextInput {
    pub(super) fn request_next_restoration_validation(
        &mut self,
        cx: &mut gpui::Context<Self>,
    ) -> Result<(), RangeTextInputError> {
        let Some(mut validation) = self.restoration.take() else {
            return Err(RangeTextInputError::Stale);
        };
        if validation.next == validation.offsets.len() {
            let seed = validation.seed;
            self.desired.selection = seed.selection;
            self.desired.composition = None;
            self.desired.scroll = seed.scroll;
            self.desired.viewport_extent = self.config.viewport_extent;
            self.desired.overscan = self.config.overscan;
            self.desired.preserve_scroll_anchor = true;
            self.desired.reveal_caret = false;
            let checkpoint = self
                .geometry
                .index()
                .and_then(|index| {
                    index
                        .checkpoints()
                        .iter()
                        .rev()
                        .find(|checkpoint| checkpoint.source() <= seed.scroll.source)
                })
                .ok_or(RangeTextInputError::Stale)?;
            self.desired.target_block = checkpoint.block_offset();
            self.start_restoration_target(seed)?;
            cx.notify();
            return Ok(());
        }
        let candidate = validation.offsets[validation.next];
        let id = PageRequestId::new(self.next_id());
        let demand = PageDemandEnvelope::Validation {
            candidate,
            max_payload_bytes: self.config.limits.page_bytes,
        };
        let demand_result = self
            .residency
            .demand(id, PagePurpose::Restoration, demand)
            .map_err(|_| RangeTextInputError::Busy)?;
        let key = crate::PageRequestKey::validation(
            id,
            self.config.binding.binding(),
            self.config.binding.revision(),
            PagePurpose::Restoration,
            candidate,
            self.config.limits.page_bytes,
        )?;
        validation.pending = Some(key);
        self.restoration = Some(validation);
        let resident = self.accept_page_demand(crate::PageRequest::new(key), demand_result, cx)?;
        cx.notify();
        if let Some(page) = resident {
            self.deliver_restoration_page(page, cx)?;
        }
        Ok(())
    }

    pub(super) fn deliver_restoration_page(
        &mut self,
        page: RangePage,
        cx: &mut gpui::Context<Self>,
    ) -> Result<(), RangeTextInputError> {
        if !self
            .restoration
            .as_ref()
            .is_some_and(|validation| validation.pending() == Some(page.key()))
        {
            return Err(RangeTextInputError::Stale);
        }
        let mut validation = self.restoration.take().ok_or(RangeTextInputError::Stale)?;
        if validation.pending != Some(page.key()) || page.candidate_is_boundary() != Some(true) {
            let _ = self.residency.settle(page.key(), PageFailure::Malformed);
            cx.emit(RangeTextInputEvent::RestorationRejected);
            return Err(RangeTextInputError::MalformedSeed);
        }
        let _ = self.residency.settle(page.key(), PageFailure::Cancelled);
        validation.pending = None;
        validation.next += 1;
        self.restoration = Some(validation);
        self.request_next_restoration_validation(cx)
    }

    pub(super) fn reject_restoration(&mut self, cx: &mut gpui::Context<Self>) {
        self.restoration = None;
        cx.emit(RangeTextInputEvent::RestorationRejected);
        cx.notify();
    }
}
