use crate::{
    ByteOffset, ByteRange, MutationKind, PageDemandEnvelope, PageDirection, PageFailure,
    PagePurpose, PageRequestId, RangePage, RangeTextInput, RangeTextInputError,
};

#[derive(Debug)]
pub(super) struct ReplacementScan {
    range: ByteRange,
    text: String,
    kind: MutationKind,
    next: ByteOffset,
    line_breaks: u64,
    pending: crate::PageRequestKey,
    marked_selection: Option<Option<std::ops::Range<usize>>>,
}

impl ReplacementScan {
    pub const fn pending(&self) -> crate::PageRequestKey {
        self.pending
    }
}

impl RangeTextInput {
    pub(super) fn begin_replacement(
        &mut self,
        range: ByteRange,
        text: String,
        kind: MutationKind,
        cx: &mut gpui::Context<Self>,
    ) -> Result<crate::MutationKey, RangeTextInputError> {
        self.begin_replacement_inner(range, text, kind, None, cx)
    }

    pub(super) fn begin_marked_replacement(
        &mut self,
        range: ByteRange,
        text: String,
        selected: Option<std::ops::Range<usize>>,
        cx: &mut gpui::Context<Self>,
    ) -> Result<crate::MutationKey, RangeTextInputError> {
        self.begin_replacement_inner(range, text, MutationKind::Edit, Some(selected), cx)
    }

    fn begin_replacement_inner(
        &mut self,
        range: ByteRange,
        text: String,
        kind: MutationKind,
        marked_selection: Option<Option<std::ops::Range<usize>>>,
        cx: &mut gpui::Context<Self>,
    ) -> Result<crate::MutationKey, RangeTextInputError> {
        if !self.enabled || self.read_only {
            return Err(RangeTextInputError::ReadOnly);
        }
        if range.is_empty() {
            let marked_text = marked_selection.is_some().then(|| text.clone());
            let key = self.begin_replacement_with_lines(range, 0, text, kind, cx)?;
            if let (Some(text), Some(selected)) = (marked_text, marked_selection) {
                self.record_marked_mutation(key, range, &text, selected);
            }
            return Ok(key);
        }
        if self.replacement.is_some() || text.len() > self.config.mutation_limits.max_staged_bytes()
        {
            return Err(RangeTextInputError::Busy);
        }
        self.config.binding.extent().check_byte_range(range)?;
        let id = PageRequestId::new(self.next_id());
        let demand = PageDemandEnvelope::Adjacent {
            anchor: range.start(),
            direction: PageDirection::Forward,
            max_payload_bytes: self.config.limits.page_bytes,
        };
        let demand_result = self
            .residency
            .demand(id, PagePurpose::Selection, demand)
            .map_err(|_| RangeTextInputError::Busy)?;
        let pending = crate::PageRequestKey::adjacent(
            id,
            self.config.binding.binding(),
            self.config.binding.revision(),
            PagePurpose::Selection,
            range.start(),
            PageDirection::Forward,
            self.config.limits.page_bytes,
        )?;
        self.replacement = Some(ReplacementScan {
            range,
            text,
            kind,
            next: range.start(),
            line_breaks: 0,
            pending,
            marked_selection,
        });
        let resident =
            self.accept_page_demand(crate::PageRequest::new(pending), demand_result, cx)?;
        if let Some(page) = resident {
            self.deliver_replacement_page(page, cx)?;
        }
        Err(RangeTextInputError::Pending)
    }

    pub(super) fn deliver_replacement_page(
        &mut self,
        page: RangePage,
        cx: &mut gpui::Context<Self>,
    ) -> Result<(), RangeTextInputError> {
        if !self
            .replacement
            .as_ref()
            .is_some_and(|scan| scan.pending() == page.key())
        {
            return Err(RangeTextInputError::Stale);
        }
        let mut scan = self.replacement.take().ok_or(RangeTextInputError::Stale)?;
        if page.key() != scan.pending || page.range().start() != scan.next {
            let _ = self.residency.settle(page.key(), PageFailure::Malformed);
            return Err(RangeTextInputError::Stale);
        }
        let part_end = page.range().end().min(scan.range.end());
        let local_end = usize::try_from(part_end.get() - page.range().start().get())
            .map_err(|_| RangeTextInputError::Stale)?;
        if !page.text().is_char_boundary(local_end) {
            let _ = self.residency.settle(page.key(), PageFailure::Malformed);
            return Err(RangeTextInputError::Stale);
        }
        scan.line_breaks = scan
            .line_breaks
            .checked_add(
                page.text()[..local_end]
                    .bytes()
                    .filter(|byte| *byte == b'\n')
                    .count() as u64,
            )
            .ok_or(RangeTextInputError::SurfaceCapacity)?;
        let _ = self.residency.settle(page.key(), PageFailure::Cancelled);
        scan.next = part_end;
        if part_end == scan.range.end() {
            let marked_text = scan.marked_selection.is_some().then(|| scan.text.clone());
            let key = self.begin_replacement_with_lines(
                scan.range,
                scan.line_breaks,
                scan.text,
                scan.kind,
                cx,
            )?;
            if let (Some(text), Some(selected)) = (marked_text, scan.marked_selection) {
                self.record_marked_mutation(key, scan.range, &text, selected);
            }
            return Ok(());
        }
        let id = PageRequestId::new(self.next_id());
        let demand = PageDemandEnvelope::Adjacent {
            anchor: scan.next,
            direction: PageDirection::Forward,
            max_payload_bytes: self.config.limits.page_bytes,
        };
        let demand_result = self
            .residency
            .demand(id, PagePurpose::Selection, demand)
            .map_err(|_| RangeTextInputError::Busy)?;
        scan.pending = crate::PageRequestKey::adjacent(
            id,
            self.config.binding.binding(),
            self.config.binding.revision(),
            PagePurpose::Selection,
            scan.next,
            PageDirection::Forward,
            self.config.limits.page_bytes,
        )?;
        let pending = scan.pending;
        self.replacement = Some(scan);
        let resident =
            self.accept_page_demand(crate::PageRequest::new(pending), demand_result, cx)?;
        if let Some(page) = resident {
            self.deliver_replacement_page(page, cx)?;
        }
        Ok(())
    }
}
