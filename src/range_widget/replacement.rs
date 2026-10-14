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
    source_range: crate::SourceRange,
    predecessor: crate::MutationPositions,
    proofs: [Option<crate::range_edit::SourcePositionProof>; 2],
}

impl ReplacementScan {
    pub(super) fn payload_charge(&self) -> crate::RangeSurfaceCharge {
        crate::RangeSurfaceCharge {
            bytes: self.text.capacity(),
            items: usize::from(self.text.capacity() != 0),
        }
    }
    pub const fn pending(&self) -> crate::PageRequestKey {
        self.pending
    }
}

impl RangeTextInput {
    fn small_plain_replacement_line_breaks(&self, source_range: crate::SourceRange) -> Option<u64> {
        if source_range.start().gap != crate::InlineObjectGap::NoObjects
            || source_range.end().gap != crate::InlineObjectGap::NoObjects
        {
            return None;
        }
        let range = ByteRange::new(
            source_range.start().byte_offset,
            source_range.end().byte_offset,
        )
        .ok()?;
        if range.len() > self.config.mutation_limits.max_page_bytes() as u64 {
            return None;
        }
        let surface = self.interactive_surface()?;
        let absence = surface.object_pages().iter().chain(self.object_residency.resident_pages()).any(|page| {
            page.key().binding() == self.config.binding.binding()
                && page.key().revision() == self.config.binding.revision()
                && page.complete()
                && matches!(page.key().demand(), crate::ObjectDemandEnvelope::Range { range: envelope, cursor: None, .. } if envelope.contains(range))
                && page.objects().iter().all(|object| object.anchor() < range.start() || object.anchor() > range.end())
        });
        if !absence {
            return None;
        }
        let page = surface
            .pages()
            .iter()
            .chain(self.residency.resident_pages())
            .find(|page| {
                page.key().binding() == self.config.binding.binding()
                    && page.key().revision() == self.config.binding.revision()
                    && page.range().contains(range)
                    && page.atoms().is_empty()
            })?;
        let start = usize::try_from(
            range
                .start()
                .get()
                .checked_sub(page.range().start().get())?,
        )
        .ok()?;
        let end =
            usize::try_from(range.end().get().checked_sub(page.range().start().get())?).ok()?;
        Some(
            page.text()
                .get(start..end)?
                .bytes()
                .filter(|byte| *byte == b'\n')
                .count() as u64,
        )
    }

    pub(super) fn begin_replacement(
        &mut self,
        range: ByteRange,
        text: String,
        kind: MutationKind,
        cx: &mut gpui::Context<Self>,
    ) -> Result<crate::MutationKey, RangeTextInputError> {
        let (source_range, _) = self.proven_no_object_range(range)?;
        self.begin_replacement_inner(source_range, text, kind, None, cx)
    }

    pub(super) fn begin_composite_replacement(
        &mut self,
        range: crate::SourceRange,
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
        let (source_range, _) = self.proven_no_object_range(range)?;
        self.begin_replacement_inner(source_range, text, MutationKind::Edit, Some(selected), cx)
    }

    fn begin_replacement_inner(
        &mut self,
        source_range: crate::SourceRange,
        text: String,
        kind: MutationKind,
        marked_selection: Option<Option<std::ops::Range<usize>>>,
        cx: &mut gpui::Context<Self>,
    ) -> Result<crate::MutationKey, RangeTextInputError> {
        if !self.enabled || self.read_only {
            return Err(RangeTextInputError::ReadOnly);
        }
        let range = ByteRange::new(
            source_range.start().byte_offset,
            source_range.end().byte_offset,
        )?;
        if source_range.is_empty() {
            let marked_state = marked_selection
                .map(|selected| Self::prepare_marked_mutation_state(range, &text, selected));
            let key = self.begin_replacement_with_lines(range, 0, text, kind, cx)?;
            if let Some(state) = marked_state {
                self.record_marked_mutation_state(key, state);
            }
            return Ok(key);
        }
        if self.replacement.is_some() || text.len() > self.config.mutation_limits.max_page_bytes() {
            return Err(RangeTextInputError::Busy);
        }
        self.check_mutation_payload_capacity(crate::RangeSurfaceCharge {
            bytes: text.capacity(),
            items: usize::from(text.capacity() != 0),
        })?;
        self.config.binding.extent().check_byte_range(range)?;
        let surface = self
            .interactive_surface()
            .ok_or(RangeTextInputError::Busy)?;
        let selection = surface.selection();
        let predecessor =
            crate::MutationPositions::new(selection.head, selection.anchor, selection.head);
        let mut proofs = [None, None];
        for (index, position) in [source_range.start(), source_range.end()]
            .into_iter()
            .enumerate()
        {
            proofs[index] = Some(
                self.admitted_edit_proofs
                    .iter()
                    .copied()
                    .find(|proof| {
                        proof.binding() == self.config.binding && proof.position() == position
                    })
                    .or_else(|| {
                        crate::range_edit::SourcePositionProof::from_surface_pages(
                            self.config.binding,
                            position,
                            surface.pages(),
                            surface.object_pages(),
                        )
                        .ok()
                    })
                    .ok_or(crate::MutationError::MissingPositionProof(position))?,
            );
        }
        if let Some(line_breaks) = self.small_plain_replacement_line_breaks(source_range) {
            let marked_state = marked_selection
                .map(|selected| Self::prepare_marked_mutation_state(range, &text, selected));
            let caret = crate::SourcePosition::new(
                ByteOffset::new(
                    range
                        .start()
                        .get()
                        .checked_add(text.len() as u64)
                        .ok_or(RangeTextInputError::SurfaceCapacity)?,
                ),
                crate::InlineObjectGap::NoObjects,
            );
            let key = crate::MutationKey::new(
                self.config.binding.binding(),
                self.config.binding.revision(),
                self.next_local_operation()?,
            );
            let proposal =
                crate::MutationProposal::new(key, kind, predecessor, source_range, line_breaks);
            let items = if text.is_empty() {
                Vec::new()
            } else {
                vec![crate::MutationPageItem::Utf8 {
                    inserted_offset: 0,
                    text: text.into_boxed_str(),
                }]
            };
            self.begin_local_mutation(
                proposal,
                items,
                crate::MutationPositions::collapsed(caret),
                cx,
            )?;
            self.admitted_edit_proofs = proofs.into_iter().flatten().collect();
            if let Some(state) = marked_state {
                self.record_marked_mutation_state(key, state);
            }
            return Ok(key);
        }
        if range.is_empty() {
            return self.begin_paged_source_replacement(
                source_range,
                predecessor,
                proofs,
                0,
                text,
                kind,
                cx,
            );
        }
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
            source_range,
            predecessor,
            proofs,
        });
        if let Err(error) = self.accept_range_continuation_demand(
            crate::PageRequest::new(pending),
            demand_result,
            cx,
        ) {
            self.replacement = None;
            return Err(error);
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
            let marked_state = scan.marked_selection.map(|selected| {
                Self::prepare_marked_mutation_state(scan.range, &scan.text, selected)
            });
            let key = self.begin_paged_source_replacement(
                scan.source_range,
                scan.predecessor,
                scan.proofs,
                scan.line_breaks,
                scan.text,
                scan.kind,
                cx,
            )?;
            if let Some(state) = marked_state {
                self.record_marked_mutation_state(key, state);
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
        if let Err(error) = self.accept_range_continuation_demand(
            crate::PageRequest::new(pending),
            demand_result,
            cx,
        ) {
            self.replacement = None;
            return Err(error);
        }
        Ok(())
    }
}
