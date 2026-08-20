use std::ops::Range;

use crate::{ByteOffset, PageRequestKey, RangePage};

#[derive(Debug)]
pub(super) enum PlatformReplayKind {
    Query,
    Replace {
        text: String,
        marked: Option<PlatformMarkedText>,
    },
}

#[derive(Debug)]
pub(super) struct PlatformMarkedText {
    pub selected: Option<Range<usize>>,
}

#[derive(Debug)]
pub(super) struct PlatformReplay {
    pub utf16: Range<usize>,
    pub kind: PlatformReplayKind,
    pending: PageRequestKey,
    utf16_cursor: usize,
    byte_start: Option<ByteOffset>,
    byte_end: Option<ByteOffset>,
    output: String,
    removed_line_breaks: u64,
    max_output_bytes: usize,
}

pub(super) enum ReplayProgress {
    Continue(ByteOffset),
    QueryReady(String),
    ReplaceReady {
        bytes: std::ops::Range<u64>,
        text: String,
        marked: Option<PlatformMarkedText>,
        removed_line_breaks: u64,
    },
}

impl PlatformReplay {
    pub fn new(
        utf16: Range<usize>,
        kind: PlatformReplayKind,
        pending: PageRequestKey,
        max_output_bytes: usize,
    ) -> Self {
        Self {
            utf16,
            kind,
            pending,
            utf16_cursor: 0,
            byte_start: None,
            byte_end: None,
            output: String::new(),
            removed_line_breaks: 0,
            max_output_bytes,
        }
    }

    pub const fn pending_key(&self) -> PageRequestKey {
        self.pending
    }
    pub fn set_pending(&mut self, pending: PageRequestKey) {
        self.pending = pending;
    }

    pub fn admit(mut self, page: &RangePage) -> Result<(Self, Option<ReplayProgress>), ()> {
        if page.key() != self.pending {
            return Err(());
        }
        let page_start = page.range().start().get();
        for (local, ch) in page.text().char_indices() {
            let global = ByteOffset::new(page_start + local as u64);
            if self.utf16_cursor == self.utf16.start {
                self.byte_start = Some(global);
            }
            if self.utf16_cursor == self.utf16.end {
                self.byte_end = Some(global);
            }
            let next_utf16 = self.utf16_cursor.checked_add(ch.len_utf16()).ok_or(())?;
            if self.utf16.start > self.utf16_cursor && self.utf16.start < next_utf16
                || self.utf16.end > self.utf16_cursor && self.utf16.end < next_utf16
            {
                return Err(());
            }
            if self.utf16_cursor >= self.utf16.start && next_utf16 <= self.utf16.end {
                if matches!(self.kind, PlatformReplayKind::Query) {
                    let next = self.output.len().checked_add(ch.len_utf8()).ok_or(())?;
                    if next > self.max_output_bytes {
                        return Err(());
                    }
                    self.output.push(ch);
                }
                if ch == '\n' {
                    self.removed_line_breaks = self.removed_line_breaks.checked_add(1).ok_or(())?;
                }
            }
            self.utf16_cursor = next_utf16;
        }
        let page_end = page.range().end();
        if self.utf16_cursor == self.utf16.start {
            self.byte_start = Some(page_end);
        }
        if self.utf16_cursor == self.utf16.end {
            self.byte_end = Some(page_end);
        }
        if let (Some(start), Some(end)) = (self.byte_start, self.byte_end) {
            let range = start.get()..end.get();
            let progress = match std::mem::replace(&mut self.kind, PlatformReplayKind::Query) {
                PlatformReplayKind::Query => {
                    ReplayProgress::QueryReady(std::mem::take(&mut self.output))
                }
                PlatformReplayKind::Replace { text, marked } => ReplayProgress::ReplaceReady {
                    bytes: range,
                    text,
                    marked,
                    removed_line_breaks: self.removed_line_breaks,
                },
            };
            return Ok((self, Some(progress)));
        }
        if page.end_of_source() {
            return Err(());
        }
        Ok((self, Some(ReplayProgress::Continue(page_end))))
    }
}

impl super::RangeTextInput {
    fn request_platform_page(
        &mut self,
        start: crate::ByteOffset,
        cx: &mut gpui::Context<Self>,
    ) -> Result<(crate::PageRequestKey, Option<crate::RangePage>), crate::RangeTextInputError> {
        let extent = self.config.binding.extent().byte_len();
        if start.get() >= extent && extent != 0 {
            return Err(crate::RangeTextInputError::Stale);
        }
        let id = crate::PageRequestId::new(self.next_id());
        let demand = self
            .residency
            .demand(
                id,
                crate::PagePurpose::PlatformRange,
                crate::PageDemandEnvelope::Adjacent {
                    anchor: start,
                    direction: crate::PageDirection::Forward,
                    max_payload_bytes: self.config.limits.platform_bytes,
                },
            )
            .map_err(|_| crate::RangeTextInputError::Busy)?;
        let request = crate::PageRequest::new(crate::PageRequestKey::adjacent(
            id,
            self.config.binding.binding(),
            self.config.binding.revision(),
            crate::PagePurpose::PlatformRange,
            start,
            crate::PageDirection::Forward,
            self.config.limits.platform_bytes,
        )?);
        let key = request.key();
        let resident = self.accept_page_demand(request, demand, cx)?;
        Ok((key, resident))
    }

    /// Begins or retrieves one exact nonresident platform UTF-16 query.
    pub fn platform_text_for_range(
        &mut self,
        range: std::ops::Range<usize>,
        cx: &mut gpui::Context<Self>,
    ) -> Result<crate::PlatformRangeResult, crate::RangeTextInputError> {
        self.interactive_surface()
            .ok_or(crate::RangeTextInputError::Busy)?;
        if let Some((ready_range, text)) = self.platform_ready.take() {
            if ready_range == range {
                return Ok(crate::PlatformRangeResult::Ready(text));
            }
            self.platform_ready = Some((ready_range, text));
        }
        if let Some(replay) = &self.platform {
            if replay.utf16 == range {
                return Ok(crate::PlatformRangeResult::Pending(replay.pending_key()));
            }
            return Err(crate::RangeTextInputError::Busy);
        }
        let (key, resident) = self.request_platform_page(crate::ByteOffset::new(0), cx)?;
        self.platform = Some(PlatformReplay::new(
            range,
            PlatformReplayKind::Query,
            key,
            usize::try_from(self.config.limits.platform_bytes)
                .map_err(|_| crate::RangeTextInputError::SurfaceCapacity)?,
        ));
        cx.notify();
        if let Some(page) = resident {
            self.deliver_platform_page(page, cx)?;
        }
        Ok(crate::PlatformRangeResult::Pending(key))
    }

    /// Begins an exact UTF-16 replacement without substituting the current selection.
    pub fn replace_platform_range(
        &mut self,
        range: std::ops::Range<usize>,
        text: String,
        cx: &mut gpui::Context<Self>,
    ) -> Result<crate::PageRequestKey, crate::RangeTextInputError> {
        self.interactive_surface()
            .ok_or(crate::RangeTextInputError::Busy)?;
        if !self.enabled || self.read_only {
            return Err(crate::RangeTextInputError::ReadOnly);
        }
        if self.platform.is_some() {
            return Err(crate::RangeTextInputError::Busy);
        }
        if text.len() > self.config.mutation_limits.max_page_bytes() {
            return Err(crate::RangeTextInputError::SurfaceCapacity);
        }
        let (key, resident) = self.request_platform_page(crate::ByteOffset::new(0), cx)?;
        self.platform = Some(PlatformReplay::new(
            range,
            PlatformReplayKind::Replace { text, marked: None },
            key,
            usize::try_from(self.config.limits.platform_bytes)
                .map_err(|_| crate::RangeTextInputError::SurfaceCapacity)?,
        ));
        cx.notify();
        if let Some(page) = resident {
            self.deliver_platform_page(page, cx)?;
        }
        Ok(key)
    }

    pub(super) fn replace_and_mark_platform_range(
        &mut self,
        range: std::ops::Range<usize>,
        text: String,
        selected: Option<std::ops::Range<usize>>,
        cx: &mut gpui::Context<Self>,
    ) -> Result<crate::PageRequestKey, crate::RangeTextInputError> {
        self.interactive_surface()
            .ok_or(crate::RangeTextInputError::Busy)?;
        if !self.enabled || self.read_only {
            return Err(crate::RangeTextInputError::ReadOnly);
        }
        if self.platform.is_some() {
            return Err(crate::RangeTextInputError::Busy);
        }
        if text.len() > self.config.mutation_limits.max_page_bytes() {
            return Err(crate::RangeTextInputError::SurfaceCapacity);
        }
        let (key, resident) = self.request_platform_page(crate::ByteOffset::new(0), cx)?;
        self.platform = Some(PlatformReplay::new(
            range,
            PlatformReplayKind::Replace {
                text,
                marked: Some(PlatformMarkedText { selected }),
            },
            key,
            usize::try_from(self.config.limits.platform_bytes)
                .map_err(|_| crate::RangeTextInputError::SurfaceCapacity)?,
        ));
        cx.notify();
        if let Some(page) = resident {
            self.deliver_platform_page(page, cx)?;
        }
        Ok(key)
    }

    pub(super) fn deliver_platform_page(
        &mut self,
        page: crate::RangePage,
        cx: &mut gpui::Context<Self>,
    ) -> Result<(), crate::RangeTextInputError> {
        if !self
            .platform
            .as_ref()
            .is_some_and(|replay| replay.pending_key() == page.key())
        {
            return Err(crate::RangeTextInputError::Stale);
        }
        let replay = self
            .platform
            .take()
            .ok_or(crate::RangeTextInputError::Stale)?;
        let utf16 = replay.utf16.clone();
        let admitted = replay.admit(&page);
        let _ = self
            .residency
            .settle(page.key(), crate::PageFailure::Cancelled);
        let (mut replay, progress) = admitted.map_err(|_| crate::RangeTextInputError::Stale)?;
        match progress.ok_or(crate::RangeTextInputError::Stale)? {
            ReplayProgress::Continue(start) => {
                let (key, resident) = self.request_platform_page(start, cx)?;
                replay.set_pending(key);
                self.platform = Some(replay);
                if let Some(page) = resident {
                    self.deliver_platform_page(page, cx)?;
                }
            }
            ReplayProgress::QueryReady(text) => {
                self.platform_ready = Some((utf16, text));
            }
            ReplayProgress::ReplaceReady {
                bytes,
                text,
                marked,
                removed_line_breaks,
            } => {
                let range = crate::ByteRange::from_u64(bytes.start, bytes.end)?;
                let key = self.begin_replacement_with_lines(
                    range,
                    removed_line_breaks,
                    text.clone(),
                    crate::MutationKind::Edit,
                    cx,
                )?;
                if let Some(marked) = marked {
                    self.record_marked_mutation(key, range, &text, marked.selected);
                }
            }
        }
        cx.notify();
        Ok(())
    }
}
