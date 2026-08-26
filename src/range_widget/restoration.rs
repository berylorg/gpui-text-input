use crate::{
    ByteOffset, ObjectCursor, ObjectDemandEnvelope, ObjectDirection, ObjectPage,
    ObjectPageEdgeFact, ObjectPurpose, ObjectRequest, ObjectRequestId, ObjectRequestKey,
    PagePurpose, PageRequest, PageRequestId, PageRequestKey, RangeRestorationSeed, RangeTextInput,
    RangeTextInputError, RangeTextInputEvent, RangeTextInputRequest, SourcePosition,
};

#[derive(Debug)]
pub(super) struct RestorationValidation {
    seed: RangeRestorationSeed,
    text_offsets: Vec<ByteOffset>,
    text_next: usize,
    object_positions: Vec<SourcePosition>,
    object_next: usize,
    pub(super) pending_text: Option<PageRequestKey>,
    pending_object: Option<ObjectRequestKey>,
    object_cursor: Option<ObjectCursor>,
    prior_object: Option<ObjectCursor>,
    object_seen: bool,
    gap_proven: bool,
}

impl RestorationValidation {
    pub fn new(seed: RangeRestorationSeed) -> Self {
        let positions = [
            seed.caret,
            seed.selection.anchor,
            seed.selection.head,
            seed.scroll.position,
        ];
        let mut text_offsets = positions.map(|position| position.byte_offset).to_vec();
        text_offsets.sort();
        text_offsets.dedup();
        let mut object_positions = Vec::with_capacity(positions.len());
        for position in positions {
            if !object_positions.contains(&position) {
                object_positions.push(position);
            }
        }
        Self {
            seed,
            text_offsets,
            text_next: 0,
            object_positions,
            object_next: 0,
            pending_text: None,
            pending_object: None,
            object_cursor: None,
            prior_object: None,
            object_seen: false,
            gap_proven: false,
        }
    }

    pub const fn pending_text(&self) -> Option<PageRequestKey> {
        self.pending_text
    }
    pub const fn pending_object(&self) -> Option<ObjectRequestKey> {
        self.pending_object
    }

    #[cfg(test)]
    pub(super) fn complete_for_test(seed: RangeRestorationSeed) -> Self {
        let mut validation = Self::new(seed);
        validation.text_next = validation.text_offsets.len();
        validation.object_next = validation.object_positions.len();
        validation
    }

    fn is_complete(&self) -> bool {
        self.text_next == self.text_offsets.len()
            && self.object_next == self.object_positions.len()
            && self.pending_text.is_none()
            && self.pending_object.is_none()
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
        if validation.text_next < validation.text_offsets.len() {
            let candidate = validation.text_offsets[validation.text_next];
            let id = PageRequestId::new(self.next_id());
            let key = PageRequestKey::validation(
                id,
                self.config.binding.binding(),
                self.config.binding.revision(),
                PagePurpose::Restoration,
                candidate,
                self.config.limits.page_bytes,
            )?;
            validation.pending_text = Some(key);
            self.restoration = Some(validation);
            self.push_request(RangeTextInputRequest::Page(PageRequest::new(key)), cx)?;
            return Ok(());
        }
        if validation.object_next < validation.object_positions.len() {
            let position = validation.object_positions[validation.object_next];
            let id = ObjectRequestId::new(self.next_id());
            let demand = ObjectDemandEnvelope::anchor(
                position.byte_offset,
                validation.object_cursor,
                ObjectDirection::Forward,
                self.config.clipboard_limits.max_object_page_objects(),
                self.config
                    .clipboard_limits
                    .max_object_page_retained_bytes(),
            )
            .map_err(|_| RangeTextInputError::InvalidLimits)?;
            let key = ObjectRequestKey::new(
                id,
                self.config.binding.binding(),
                self.config.binding.revision(),
                self.config.presentation_generation,
                ObjectPurpose::Restoration,
                demand,
            )
            .map_err(|_| RangeTextInputError::InvalidLimits)?;
            validation.pending_object = Some(key);
            self.restoration = Some(validation);
            self.push_request(
                RangeTextInputRequest::ObjectPage(ObjectRequest::new(key)),
                cx,
            )?;
            return Ok(());
        }
        self.finish_restoration_validation(validation, cx)
    }

    pub(super) fn deliver_restoration_page(
        &mut self,
        page: crate::RangePage,
        cx: &mut gpui::Context<Self>,
    ) -> Result<(), RangeTextInputError> {
        let mut validation = self.restoration.take().ok_or(RangeTextInputError::Stale)?;
        if validation.pending_text != Some(page.key()) {
            self.restoration = Some(validation);
            return Err(RangeTextInputError::Stale);
        }
        if page.retained_bytes() > self.config.residency_limits.max_resident_bytes() {
            cx.emit(RangeTextInputEvent::RestorationRejected);
            cx.notify();
            return Err(RangeTextInputError::SurfaceCapacity);
        }
        if page.candidate_is_boundary() != Some(true) {
            self.restoration = None;
            cx.emit(RangeTextInputEvent::RestorationRejected);
            return Err(RangeTextInputError::MalformedSeed);
        }
        validation.pending_text = None;
        validation.text_next += 1;
        self.restoration = Some(validation);
        self.request_next_restoration_validation(cx)
    }

    pub(super) fn deliver_restoration_object_page(
        &mut self,
        page: ObjectPage,
        cx: &mut gpui::Context<Self>,
    ) -> Result<(), RangeTextInputError> {
        let mut validation = self.restoration.take().ok_or(RangeTextInputError::Stale)?;
        if validation.pending_object != Some(page.key()) {
            self.restoration = Some(validation);
            return Err(RangeTextInputError::Stale);
        }
        let position = validation.object_positions[validation.object_next];
        for object in page.objects() {
            validation.object_seen = true;
            let cursor = object.cursor();
            let leading = validation.prior_object.map_or_else(
                || crate::InlineObjectGap::before(cursor.neighbor()),
                |prior| {
                    crate::InlineObjectGap::between(prior.neighbor(), cursor.neighbor())
                        .expect("strict page order creates a valid gap")
                },
            );
            validation.gap_proven |= position.gap == leading;
            validation.prior_object = Some(cursor);
        }
        if page.complete() {
            if let Some(last) = validation.prior_object {
                validation.gap_proven |=
                    position.gap == crate::InlineObjectGap::after(last.neighbor());
            } else if position.gap == crate::InlineObjectGap::NoObjects {
                validation.gap_proven = true;
            }
            if !validation.gap_proven
                || (position.gap == crate::InlineObjectGap::NoObjects && validation.object_seen)
            {
                self.restoration = None;
                cx.emit(RangeTextInputEvent::RestorationRejected);
                return Err(RangeTextInputError::MalformedSeed);
            }
            validation.object_next += 1;
            validation.object_cursor = None;
            validation.prior_object = None;
            validation.object_seen = false;
            validation.gap_proven = false;
        } else {
            let Some(cursor) = page.continuation() else {
                self.restoration = None;
                cx.emit(RangeTextInputEvent::RestorationRejected);
                return Err(RangeTextInputError::MalformedSeed);
            };
            if page.following() != ObjectPageEdgeFact::Continues(cursor) {
                self.restoration = None;
                cx.emit(RangeTextInputEvent::RestorationRejected);
                return Err(RangeTextInputError::MalformedSeed);
            }
            validation.object_cursor = Some(cursor);
        }
        validation.pending_object = None;
        self.restoration = Some(validation);
        self.request_next_restoration_validation(cx)
    }

    fn finish_restoration_validation(
        &mut self,
        validation: RestorationValidation,
        cx: &mut gpui::Context<Self>,
    ) -> Result<(), RangeTextInputError> {
        self.restoration = Some(validation);
        let _ = self.service_pending_restoration_completion(cx)?;
        Ok(())
    }

    pub(super) fn service_pending_restoration_completion(
        &mut self,
        cx: &mut gpui::Context<Self>,
    ) -> Result<bool, RangeTextInputError> {
        if !self
            .restoration
            .as_ref()
            .is_some_and(RestorationValidation::is_complete)
        {
            return Ok(false);
        }
        if !self.try_spend_realization_credit(cx) {
            return Ok(false);
        }
        let validation = self
            .restoration
            .take()
            .expect("complete restoration validation exists");
        let seed = validation.seed;
        let mut desired = self.desired;
        desired.source_selection = Some(seed.selection);
        desired.composition = None;
        desired.scroll = super::RangeScrollAnchor {
            source: seed.scroll.position.byte_offset,
            intra_anchor: seed.scroll.intra_anchor,
        };
        desired.viewport_extent = self.config.viewport_extent;
        desired.overscan = self.config.overscan;
        desired.target_block = gpui::Pixels::ZERO;
        desired.realization_anchor_block = gpui::Pixels::ZERO;
        desired.preserve_scroll_anchor = true;
        desired.reveal_caret = false;
        let candidate = match self.prepare_restoration_index_transition(desired) {
            Ok(candidate) => candidate,
            Err(error) => {
                self.restoration = Some(validation);
                self.refund_realization_credit();
                self.schedule_realization_continuation(cx);
                return Err(error);
            }
        };
        let progress = self.commit_widget_transition(candidate, None);
        debug_assert_eq!(progress, crate::ExactGeometryProgress::Scanning);
        self.restoration_seed = Some(seed);
        cx.notify();
        Ok(true)
    }

    pub(super) fn reject_restoration_task(
        &mut self,
        cx: &mut gpui::Context<Self>,
    ) -> Result<bool, RangeTextInputError> {
        if self.reject_restoration_validation(cx) {
            return Ok(true);
        }
        self.reject_restoration_geometry(cx)
    }

    pub(super) fn reject_restoration_validation(&mut self, cx: &mut gpui::Context<Self>) -> bool {
        let Some(validation) = self.restoration.take() else {
            return false;
        };
        if let Some(key) = validation.pending_text() {
            let _ = self.residency.cancel(key);
            self.cancel_page_dispatch(key);
        }
        if let Some(key) = validation.pending_object() {
            self.cancel_object_page_dispatch(key);
        }
        self.retire_surface_candidate();
        self.restoration_seed = None;
        self.published_restoration = None;
        cx.emit(RangeTextInputEvent::RestorationRejected);
        cx.notify();
        true
    }

    pub(super) fn reject_restoration(&mut self, cx: &mut gpui::Context<Self>) {
        let rejected = self.reject_restoration_validation(cx);
        debug_assert!(rejected);
    }
}
