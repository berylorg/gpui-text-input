//! Bounded Unicode segmentation over revision-bound range pages.

use crate::range_source::{
    BindingId, ByteOffset, LogicalExtent, PageDemandEnvelope, PageDirection, PageEdgeFact,
    PagePurpose, PageRequestKey, RangePage, SourceRevision,
};
use unicode_segmentation::{WordCursor, WordCursorError, WordCursorResult};

mod grapheme;
mod line;
mod types;
use grapheme::GraphemeEngine;
use line::{CursorOutcome, line_outcome};
pub use types::{
    AdjacentPageEdge, AdjacentPageRequest, ResolvedBoundary, SegmentationCancellation,
    SegmentationCounts, SegmentationDirection, SegmentationError, SegmentationKind,
    SegmentationLimits, SegmentationProgress, SegmentationResume,
};

#[derive(Clone, Debug)]
enum Engine {
    Grapheme(GraphemeEngine),
    Word(WordCursor),
    LogicalLine,
}

/// Fixed-state continuation for one exact range-backed boundary traversal.
///
/// The continuation retains fixed protocol state and an exact request key, never page text. Each
/// supplied page is admitted only when its complete key and returned range match the pending
/// demand.
#[derive(Debug)]
pub struct SegmentationContinuation {
    binding: BindingId,
    revision: SourceRevision,
    extent: ByteOffset,
    kind: SegmentationKind,
    direction: SegmentationDirection,
    origin: ByteOffset,
    limits: SegmentationLimits,
    request: PageRequestKey,
    engine: Engine,
}

impl SegmentationContinuation {
    /// Starts a strict boundary traversal using `first_request` as the exact first demand.
    ///
    /// `choose_adjacent` passed to [`Self::resume`] is the only mechanism that chooses subsequent
    /// page sizes. The segmenter constrains their exact edge but never guesses a source page size.
    pub fn start(
        binding: BindingId,
        revision: SourceRevision,
        extent: LogicalExtent,
        kind: SegmentationKind,
        direction: SegmentationDirection,
        origin: ByteOffset,
        limits: SegmentationLimits,
        first_request: PageRequestKey,
    ) -> Result<SegmentationProgress, SegmentationError> {
        let extent = ByteOffset::new(extent.byte_len());
        if origin.get() > extent.get() {
            return Err(SegmentationError::InvalidOrigin);
        }
        if direction == SegmentationDirection::Forward && origin == extent {
            Self::validate_edge_request(
                binding,
                revision,
                origin,
                direction,
                limits,
                &first_request,
            )?;
            return Ok(SegmentationProgress::Complete(Self::resolved(
                binding, revision, kind, direction, origin, extent, true,
            )));
        }
        if direction == SegmentationDirection::Reverse && origin.get() == 0 {
            Self::validate_edge_request(
                binding,
                revision,
                origin,
                direction,
                limits,
                &first_request,
            )?;
            return Ok(SegmentationProgress::Complete(Self::resolved(
                binding, revision, kind, direction, origin, origin, true,
            )));
        }
        Self::validate_common_request(binding, revision, extent, limits, &first_request)?;
        let required_direction = match direction {
            SegmentationDirection::Forward => PageDirection::Forward,
            SegmentationDirection::Reverse => PageDirection::Backward,
        };
        if first_request.demand()
            != (PageDemandEnvelope::Adjacent {
                anchor: origin,
                direction: required_direction,
                max_payload_bytes: first_request.max_payload_bytes(),
            })
        {
            return Err(SegmentationError::InvalidRequest);
        }
        let offset =
            usize::try_from(origin.get()).map_err(|_| SegmentationError::OffsetOutOfRange)?;
        let len = usize::try_from(extent.get()).map_err(|_| SegmentationError::OffsetOutOfRange)?;
        let engine = match kind {
            SegmentationKind::Grapheme => Engine::Grapheme(GraphemeEngine::new(offset, len)),
            SegmentationKind::Word => Engine::Word(
                WordCursor::new(offset, len).map_err(|_| SegmentationError::CursorContract)?,
            ),
            SegmentationKind::LogicalLine => Engine::LogicalLine,
        };
        Ok(SegmentationProgress::NeedPage(Self {
            binding,
            revision,
            extent,
            kind,
            direction,
            origin,
            limits,
            request: first_request,
            engine,
        }))
    }

    /// Returns the exact page request currently bound to this continuation.
    pub fn pending_request(&self) -> &PageRequestKey {
        &self.request
    }

    /// Returns the segmentation kind carried by this continuation.
    pub fn kind(&self) -> SegmentationKind {
        self.kind
    }

    /// Returns the traversal direction carried by this continuation.
    pub fn direction(&self) -> SegmentationDirection {
        self.direction
    }

    /// Returns the exact traversal origin.
    pub fn origin(&self) -> ByteOffset {
        self.origin
    }

    /// Returns the hard limits carried by this continuation.
    pub const fn limits(&self) -> SegmentationLimits {
        self.limits
    }

    /// Reports fixed continuation residency without counting the caller-owned borrowed page.
    pub const fn counts(&self) -> SegmentationCounts {
        SegmentationCounts {
            continuations: 1,
            pending_pages: 1,
            resident_pages: 0,
            resident_page_bytes: 0,
        }
    }

    /// Cancels this traversal and releases its one exact pending request identity.
    pub fn cancel(self) -> SegmentationCancellation {
        SegmentationCancellation {
            request: self.request,
        }
    }

    /// Admits `page`, advances without retaining it, and binds any adjacent demand.
    ///
    /// The callback is invoked only if another page is genuinely required. Its returned request
    /// must use the supplied binding, revision, segmentation purpose, and exact adjacent edge.
    pub fn resume<F>(
        &mut self,
        page: &RangePage,
        mut choose_adjacent: F,
    ) -> Result<SegmentationResume, SegmentationError>
    where
        F: FnMut(AdjacentPageRequest) -> PageRequestKey,
    {
        self.admit(page)?;
        let mut candidate_engine = self.engine.clone();
        let page_start = usize::try_from(page.range().start().get())
            .map_err(|_| SegmentationError::OffsetOutOfRange)?;
        let page_end = usize::try_from(page.range().end().get())
            .map_err(|_| SegmentationError::OffsetOutOfRange)?;
        let cursor = match &self.engine {
            Engine::Grapheme(engine) => engine.cursor(),
            Engine::Word(cursor) => cursor.cur_cursor(),
            Engine::LogicalLine => usize::try_from(self.origin.get())
                .map_err(|_| SegmentationError::OffsetOutOfRange)?,
        };
        let text = page.text();
        if page_start <= cursor && cursor <= page_end && !text.is_char_boundary(cursor - page_start)
        {
            return Err(SegmentationError::InvalidOrigin);
        }
        let (chunk, chunk_start) = if page_end <= cursor || page_start >= cursor {
            // Context and replay requests sit wholly on one side of the cursor. Both streaming
            // cursor protocols require the exact adjacent chunk, not a slice at the origin.
            (text, page_start)
        } else {
            match self.direction {
                SegmentationDirection::Forward => (&text[cursor - page_start..], cursor),
                SegmentationDirection::Reverse => (&text[..cursor - page_start], page_start),
            }
        };

        let mut outcome = match &mut candidate_engine {
            Engine::Grapheme(engine) => {
                let origin = usize::try_from(self.origin.get())
                    .map_err(|_| SegmentationError::OffsetOutOfRange)?;
                engine.resume(
                    page,
                    self.direction,
                    origin,
                    self.limits.max_grapheme_steps_per_resume(),
                )?
            }
            Engine::Word(cursor) => {
                let result = match self.direction {
                    SegmentationDirection::Forward => cursor.next_boundary(chunk, chunk_start),
                    SegmentationDirection::Reverse => cursor.prev_boundary(chunk, chunk_start),
                }
                .map_err(map_word_error)?;
                match result {
                    WordCursorResult::Boundary(offset) => CursorOutcome::Boundary(offset),
                    WordCursorResult::End => CursorOutcome::DocumentEdge,
                    WordCursorResult::NextChunk(offset) => CursorOutcome::Need(
                        AdjacentPageEdge::NextChunk(ByteOffset::new(offset as u64)),
                    ),
                    WordCursorResult::PrevChunk(offset) => CursorOutcome::Need(
                        AdjacentPageEdge::PrevChunk(ByteOffset::new(offset as u64)),
                    ),
                    WordCursorResult::PreContext(offset) => CursorOutcome::Need(
                        AdjacentPageEdge::PreContext(ByteOffset::new(offset as u64)),
                    ),
                    WordCursorResult::PostContext(offset) => CursorOutcome::Need(
                        AdjacentPageEdge::NextChunk(ByteOffset::new(offset as u64)),
                    ),
                }
            }
            Engine::LogicalLine => line_outcome(self.direction, self.origin, chunk, chunk_start)?,
        };
        if let CursorOutcome::Need(edge) = outcome {
            let at_document_edge = match edge {
                AdjacentPageEdge::NextChunk(offset) => {
                    offset == self.extent && page.following() == PageEdgeFact::DocumentBoundary
                }
                AdjacentPageEdge::PrevChunk(offset) | AdjacentPageEdge::PreContext(offset) => {
                    offset.get() == 0 && page.preceding() == PageEdgeFact::DocumentBoundary
                }
                AdjacentPageEdge::Replay(_) => false,
            };
            if at_document_edge {
                outcome = CursorOutcome::DocumentEdge;
            }
        }
        let (resume, successor) = self.finish_outcome(outcome, &mut choose_adjacent)?;
        self.engine = candidate_engine;
        if let Some(request) = successor {
            self.request = request;
        }
        Ok(resume)
    }

    fn finish_outcome<F>(
        &self,
        outcome: CursorOutcome,
        choose_adjacent: &mut F,
    ) -> Result<(SegmentationResume, Option<PageRequestKey>), SegmentationError>
    where
        F: FnMut(AdjacentPageRequest) -> PageRequestKey,
    {
        match outcome {
            CursorOutcome::Boundary(offset) => {
                let offset =
                    u64::try_from(offset).map_err(|_| SegmentationError::OffsetOutOfRange)?;
                let offset = ByteOffset::new(offset);
                let document_edge = offset.get() == 0 || offset == self.extent;
                Ok((
                    SegmentationResume::Complete(Self::resolved(
                        self.binding,
                        self.revision,
                        self.kind,
                        self.direction,
                        self.origin,
                        offset,
                        document_edge,
                    )),
                    None,
                ))
            }
            CursorOutcome::DocumentEdge => {
                let offset = match self.direction {
                    SegmentationDirection::Forward => self.extent,
                    SegmentationDirection::Reverse => ByteOffset::new(0),
                };
                Ok((
                    SegmentationResume::Complete(Self::resolved(
                        self.binding,
                        self.revision,
                        self.kind,
                        self.direction,
                        self.origin,
                        offset,
                        true,
                    )),
                    None,
                ))
            }
            CursorOutcome::Need(edge) => {
                let adjacent = AdjacentPageRequest {
                    binding: self.binding,
                    revision: self.revision,
                    kind: self.kind,
                    edge,
                };
                let request = choose_adjacent(adjacent);
                Self::validate_common_request(
                    self.binding,
                    self.revision,
                    self.extent,
                    self.limits,
                    &request,
                )?;
                if request.id() == self.request.id() {
                    return Err(SegmentationError::InvalidRequest);
                }
                let expected = match edge {
                    AdjacentPageEdge::NextChunk(anchor) => (anchor, PageDirection::Forward),
                    AdjacentPageEdge::PrevChunk(anchor) | AdjacentPageEdge::PreContext(anchor) => {
                        (anchor, PageDirection::Backward)
                    }
                    AdjacentPageEdge::Replay(anchor) => match self.direction {
                        SegmentationDirection::Forward => (anchor, PageDirection::Forward),
                        SegmentationDirection::Reverse => (anchor, PageDirection::Backward),
                    },
                };
                if request.demand()
                    != (PageDemandEnvelope::Adjacent {
                        anchor: expected.0,
                        direction: expected.1,
                        max_payload_bytes: request.max_payload_bytes(),
                    })
                {
                    return Err(SegmentationError::NonAdjacentRequest);
                }
                Ok((SegmentationResume::NeedPage, Some(request)))
            }
        }
    }

    fn admit(&self, page: &RangePage) -> Result<(), SegmentationError> {
        if page.key() != self.request {
            return Err(SegmentationError::ObsoletePage);
        }
        if page.range().end() > self.extent {
            return Err(SegmentationError::MalformedPage);
        }
        let starts_document = page.range().start().get() == 0;
        let ends_document = page.range().end() == self.extent;
        if (page.preceding() == PageEdgeFact::DocumentBoundary) != starts_document
            || (page.following() == PageEdgeFact::DocumentBoundary) != ends_document
            || page.end_of_source() != ends_document
        {
            return Err(SegmentationError::MalformedPage);
        }
        Ok(())
    }

    fn validate_common_request(
        binding: BindingId,
        revision: SourceRevision,
        extent: ByteOffset,
        limits: SegmentationLimits,
        request: &PageRequestKey,
    ) -> Result<(), SegmentationError> {
        if request.binding() != binding
            || request.revision() != revision
            || request.purpose() != PagePurpose::Segmentation
        {
            return Err(SegmentationError::InvalidRequest);
        }
        let PageDemandEnvelope::Adjacent { anchor, .. } = request.demand() else {
            return Err(SegmentationError::InvalidRequest);
        };
        if anchor.get() > extent.get() {
            return Err(SegmentationError::InvalidRequest);
        }
        if request.max_payload_bytes() > limits.max_page_bytes() {
            return Err(SegmentationError::PageRangeLimitExceeded);
        }
        Ok(())
    }

    fn validate_edge_request(
        binding: BindingId,
        revision: SourceRevision,
        origin: ByteOffset,
        direction: SegmentationDirection,
        limits: SegmentationLimits,
        request: &PageRequestKey,
    ) -> Result<(), SegmentationError> {
        let PageDemandEnvelope::Adjacent {
            anchor,
            direction: page_direction,
            ..
        } = request.demand()
        else {
            return Err(SegmentationError::InvalidRequest);
        };
        let required_direction = match direction {
            SegmentationDirection::Forward => PageDirection::Forward,
            SegmentationDirection::Reverse => PageDirection::Backward,
        };
        if request.binding() != binding
            || request.revision() != revision
            || request.purpose() != PagePurpose::Segmentation
            || anchor != origin
            || page_direction != required_direction
        {
            return Err(SegmentationError::InvalidRequest);
        }
        if request.max_payload_bytes() > limits.max_page_bytes() {
            return Err(SegmentationError::PageRangeLimitExceeded);
        }
        Ok(())
    }

    fn resolved(
        binding: BindingId,
        revision: SourceRevision,
        kind: SegmentationKind,
        direction: SegmentationDirection,
        origin: ByteOffset,
        offset: ByteOffset,
        document_edge: bool,
    ) -> ResolvedBoundary {
        ResolvedBoundary {
            binding,
            revision,
            kind,
            direction,
            origin,
            offset,
            document_edge,
        }
    }
}

fn map_word_error(_: WordCursorError) -> SegmentationError {
    SegmentationError::CursorContract
}
