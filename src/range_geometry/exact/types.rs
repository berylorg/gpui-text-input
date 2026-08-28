use std::sync::Arc;

use gpui::{
    Hsla, Pixels, SharedString, StreamingLayoutCharge, StreamingLayoutContinuation,
    StreamingLayoutError, StreamingLayoutFragment, StreamingLayoutItemCharge, TextRun,
};
use unicode_segmentation::GraphemeCursor;

use crate::{
    ByteOffset, InlineObjectPresentation, ObjectCursor, ObjectRequestKey, PageRequestKey,
    RangeSourceSelection, SourcePosition,
};

use super::super::{GeometryJobKey, GeometryQuality};

mod counts;
pub use counts::ExactGeometryCounts;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExactGeometryLimits {
    pub(super) max_page_bytes: u64,
    pub(super) max_checkpoints: usize,
    pub(super) max_retained_bytes: usize,
    pub(super) max_retained_items: usize,
}

impl ExactGeometryLimits {
    pub fn new(
        max_page_bytes: u64,
        max_checkpoints: usize,
        max_retained_bytes: usize,
        max_retained_items: usize,
    ) -> Result<Self, ExactGeometryError> {
        if max_page_bytes < 4
            || max_checkpoints < 2
            || max_retained_bytes == 0
            || max_retained_items == 0
        {
            return Err(ExactGeometryError::InvalidLimits);
        }
        Ok(Self {
            max_page_bytes,
            max_checkpoints,
            max_retained_bytes,
            max_retained_items,
        })
    }

    pub const fn max_page_bytes(self) -> u64 {
        self.max_page_bytes
    }

    pub const fn max_checkpoints(self) -> usize {
        self.max_checkpoints
    }

    pub const fn max_retained_bytes(self) -> usize {
        self.max_retained_bytes
    }

    pub const fn max_retained_items(self) -> usize {
        self.max_retained_items
    }
}

#[derive(Clone, Debug)]
pub struct StreamingOversizePresentation {
    pub(super) presentation: SharedString,
    pub(super) runs: Vec<TextRun>,
    pub(super) width: Pixels,
    pub(super) height: Pixels,
    pub(super) baseline: Pixels,
    pub(super) background: Option<Hsla>,
}

impl StreamingOversizePresentation {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        presentation: SharedString,
        runs: Vec<TextRun>,
        width: Pixels,
        height: Pixels,
        baseline: Pixels,
        background: Option<Hsla>,
    ) -> Self {
        Self {
            presentation,
            runs,
            width,
            height,
            baseline,
            background,
        }
    }
}

#[derive(Clone, Debug)]
pub struct StreamingGeometryStyle {
    pub(super) text_run: TextRun,
    pub(super) oversize: StreamingOversizePresentation,
}

impl StreamingGeometryStyle {
    pub fn new(text_run: TextRun, oversize: StreamingOversizePresentation) -> Self {
        Self { text_run, oversize }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ExactGeometryAggregate {
    pub(super) visual_lines: u64,
    pub(super) content_height: Pixels,
}

impl ExactGeometryAggregate {
    pub const fn quality(self) -> GeometryQuality {
        GeometryQuality::Exact
    }

    pub const fn visual_lines(self) -> u64 {
        self.visual_lines
    }

    pub const fn content_height(self) -> Pixels {
        self.content_height
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StreamingGeometryEstimate {
    pub(super) scanned_source: SourcePosition,
    pub(super) visual_lines_lower_bound: u64,
    pub(super) content_height_lower_bound: Pixels,
}

impl StreamingGeometryEstimate {
    pub const fn quality(self) -> GeometryQuality {
        GeometryQuality::Estimated
    }

    pub const fn scanned_source(self) -> SourcePosition {
        self.scanned_source
    }

    pub const fn visual_lines_lower_bound(self) -> u64 {
        self.visual_lines_lower_bound
    }

    pub const fn content_height_lower_bound(self) -> Pixels {
        self.content_height_lower_bound
    }
}

#[derive(Clone, Debug)]
pub struct ExactGeometryCheckpoint {
    pub(super) source: SourcePosition,
    pub(super) object_cursor: Option<ObjectCursor>,
    pub(super) block_offset: Pixels,
    pub(super) visual_lines: u64,
    pub(super) logical_line: u64,
    pub(super) segment: u64,
    pub(super) input_id: u64,
    pub(super) segment_policy_id: u64,
    pub(super) terminal: bool,
    pub(super) continuation: StreamingLayoutContinuation,
    pub(super) grapheme_origin: ByteOffset,
    pub(super) grapheme: GraphemeCursor,
}

impl ExactGeometryCheckpoint {
    pub const fn source(&self) -> SourcePosition {
        self.source
    }

    pub const fn object_cursor(&self) -> Option<ObjectCursor> {
        self.object_cursor
    }

    pub const fn block_offset(&self) -> Pixels {
        self.block_offset
    }

    pub fn resume_block_offset(&self) -> Pixels {
        self.continuation.block_offset + self.continuation.line_block_extent
    }

    pub const fn visual_lines(&self) -> u64 {
        self.visual_lines
    }

    pub const fn logical_line(&self) -> u64 {
        self.logical_line
    }

    pub const fn segment(&self) -> u64 {
        self.segment
    }

    pub const fn input_id(&self) -> u64 {
        self.input_id
    }

    pub const fn segment_policy_id(&self) -> u64 {
        self.segment_policy_id
    }

    pub fn cursor_offset(&self) -> usize {
        usize::try_from(self.grapheme_origin.get())
            .unwrap_or(usize::MAX)
            .saturating_add(self.grapheme.cur_cursor())
    }

    pub const fn is_terminal(&self) -> bool {
        self.terminal
    }
}

#[derive(Clone, Debug)]
pub struct ExactGeometryIndex {
    pub(super) key: GeometryJobKey,
    pub(super) checkpoints: Arc<[ExactGeometryCheckpoint]>,
    pub(super) aggregate: ExactGeometryAggregate,
    pub(super) document_selection: RangeSourceSelection,
}

impl ExactGeometryIndex {
    pub const fn key(&self) -> GeometryJobKey {
        self.key
    }

    pub fn checkpoints(&self) -> &[ExactGeometryCheckpoint] {
        &self.checkpoints
    }

    pub const fn aggregate(&self) -> ExactGeometryAggregate {
        self.aggregate
    }

    pub(crate) const fn document_selection(&self) -> RangeSourceSelection {
        self.document_selection
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BlockTarget {
    pub(super) block_offset: Pixels,
    pub(super) viewport_extent: Pixels,
    pub(super) overscan: Pixels,
}

impl BlockTarget {
    pub const fn new(block_offset: Pixels, viewport_extent: Pixels, overscan: Pixels) -> Self {
        Self {
            block_offset,
            viewport_extent,
            overscan,
        }
    }

    pub const fn block_offset(self) -> Pixels {
        self.block_offset
    }

    pub const fn viewport_extent(self) -> Pixels {
        self.viewport_extent
    }

    pub const fn overscan(self) -> Pixels {
        self.overscan
    }
}

#[derive(Debug)]
pub(crate) struct TargetInlineObjectPresentation {
    cursor: ObjectCursor,
    presentation: InlineObjectPresentation,
}

impl Clone for TargetInlineObjectPresentation {
    fn clone(&self) -> Self {
        Self {
            cursor: self.cursor,
            presentation: self.presentation.shared_clone(),
        }
    }
}

impl TargetInlineObjectPresentation {
    pub(crate) fn new(cursor: ObjectCursor, presentation: InlineObjectPresentation) -> Self {
        Self {
            cursor,
            presentation,
        }
    }

    pub(crate) const fn cursor(&self) -> ObjectCursor {
        self.cursor
    }

    pub(crate) const fn presentation(&self) -> &InlineObjectPresentation {
        &self.presentation
    }

    pub(crate) fn presentation_allocation(&self) -> (*const u8, usize) {
        self.presentation.display_allocation()
    }
}

#[derive(Clone, Debug)]
pub struct BlockTargetPublication {
    pub(super) key: GeometryJobKey,
    pub(super) predecessor: SourcePosition,
    pub(super) target_source: SourcePosition,
    pub(super) source_end: SourcePosition,
    pub(super) predecessor_checkpoint: ExactGeometryCheckpoint,
    pub(super) visual_lines_lower_bound: u64,
    pub(super) content_height_lower_bound: Pixels,
    pub(super) fragments: Arc<[StreamingLayoutFragment]>,
    pub(super) object_presentations: Arc<[TargetInlineObjectPresentation]>,
    pub(super) charge: StreamingLayoutCharge,
    pub(super) item_charge: StreamingLayoutItemCharge,
}

impl BlockTargetPublication {
    pub(crate) fn presentation_overlap_bytes<'a>(
        &self,
        pages: impl Iterator<Item = &'a crate::ObjectPage> + Clone,
    ) -> Option<usize> {
        presentation_overlap_bytes(&self.object_presentations, pages)
    }
    pub const fn key(&self) -> GeometryJobKey {
        self.key
    }

    pub const fn predecessor(&self) -> SourcePosition {
        self.predecessor
    }

    pub const fn target_source(&self) -> SourcePosition {
        self.target_source
    }

    pub const fn source_end(&self) -> SourcePosition {
        self.source_end
    }

    pub(crate) const fn predecessor_checkpoint(&self) -> &ExactGeometryCheckpoint {
        &self.predecessor_checkpoint
    }

    pub(crate) const fn visual_lines_lower_bound(&self) -> u64 {
        self.visual_lines_lower_bound
    }

    pub(crate) const fn content_height_lower_bound(&self) -> Pixels {
        self.content_height_lower_bound
    }

    pub fn fragments(&self) -> &[StreamingLayoutFragment] {
        &self.fragments
    }

    pub(crate) fn object_presentations(&self) -> &[TargetInlineObjectPresentation] {
        &self.object_presentations
    }

    pub(crate) fn output_record_bytes(&self) -> Option<usize> {
        self.fragments
            .len()
            .checked_mul(std::mem::size_of::<StreamingLayoutFragment>())?
            .checked_add(
                self.object_presentations
                    .len()
                    .checked_mul(std::mem::size_of::<TargetInlineObjectPresentation>())?,
            )
    }

    pub(crate) fn object_presentation_items(&self) -> usize {
        self.object_presentations.len()
    }

    pub const fn charge(&self) -> StreamingLayoutCharge {
        self.charge
    }

    pub const fn item_charge(&self) -> StreamingLayoutItemCharge {
        self.item_charge
    }
}

pub(super) fn presentation_overlap_bytes<'a>(
    presentations: &[TargetInlineObjectPresentation],
    pages: impl Iterator<Item = &'a crate::ObjectPage> + Clone,
) -> Option<usize> {
    presentations.iter().try_fold(0usize, |total, target| {
        let allocation = target.presentation_allocation();
        let aliased = pages
            .clone()
            .flat_map(crate::ObjectPage::presentation_allocations)
            .any(|candidate| candidate == allocation);
        total.checked_add(if aliased { allocation.1 } else { 0 })
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExactGeometryProgress {
    PendingIndex,
    Scanning,
    NeedObjects,
    IndexComplete,
    TargetComplete,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExactGeometryStart {
    pub(super) key: GeometryJobKey,
    pub(super) progress: ExactGeometryProgress,
    pub(super) release: ExactGeometryRelease,
    pub(super) admission_required_bytes: usize,
    pub(super) admission_required_items: usize,
}

impl ExactGeometryStart {
    pub const fn key(&self) -> GeometryJobKey {
        self.key
    }

    pub const fn progress(&self) -> ExactGeometryProgress {
        self.progress
    }

    pub const fn release(&self) -> &ExactGeometryRelease {
        &self.release
    }

    pub const fn admission_required_bytes(&self) -> usize {
        self.admission_required_bytes
    }

    pub const fn admission_required_items(&self) -> usize {
        self.admission_required_items
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExactGeometryAdmission {
    pub(super) progress: ExactGeometryProgress,
    pub(super) release: ExactGeometryRelease,
    pub(super) admission_required_bytes: usize,
    pub(super) admission_required_items: usize,
}

impl ExactGeometryAdmission {
    pub const fn progress(&self) -> ExactGeometryProgress {
        self.progress
    }

    pub const fn release(&self) -> &ExactGeometryRelease {
        &self.release
    }

    pub const fn admission_required_bytes(&self) -> usize {
        self.admission_required_bytes
    }

    pub const fn admission_required_items(&self) -> usize {
        self.admission_required_items
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ExactGeometryRelease {
    pub jobs: Vec<GeometryJobKey>,
    pub pages: Vec<PageRequestKey>,
    pub object_pages: Vec<ObjectRequestKey>,
    pub counts: ExactGeometryCounts,
}

#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum ExactGeometryError {
    InvalidLimits,
    InvalidMetric,
    Disposed,
    EpochExhausted,
    IdNotMonotonic,
    Busy,
    IndexIncomplete,
    NoActiveJob,
    ObsoleteJob(GeometryJobKey),
    PageAlreadyPending,
    WrongPage(PageRequestKey),
    WrongObjectPage(ObjectRequestKey),
    WrongInputKind,
    NoncontiguousPage {
        expected: ByteOffset,
        actual: ByteOffset,
    },
    PageTooLarge,
    SourceContract,
    CapacityExceeded,
    Layout(StreamingLayoutError),
}

#[derive(Clone, Debug, PartialEq)]
pub struct ExactGeometryFailure {
    pub(super) error: ExactGeometryError,
    pub(super) stage: ExactGeometryFailureStage,
    pub(super) release: ExactGeometryRelease,
    pub(super) admission_required_bytes: usize,
    pub(super) admission_required_items: usize,
}

impl ExactGeometryFailure {
    pub const fn error(&self) -> &ExactGeometryError {
        &self.error
    }

    pub const fn stage(&self) -> ExactGeometryFailureStage {
        self.stage
    }

    pub const fn release(&self) -> &ExactGeometryRelease {
        &self.release
    }

    pub const fn admission_required_bytes(&self) -> usize {
        self.admission_required_bytes
    }

    pub const fn admission_required_items(&self) -> usize {
        self.admission_required_items
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExactGeometryFailureStage {
    Validation,
    PageCoexistence,
    WindowIdentity,
    Scan,
    Finalize,
    Checkpoint,
    Publication,
}

impl std::fmt::Display for ExactGeometryFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}", self.error)
    }
}

impl std::error::Error for ExactGeometryFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.error)
    }
}

impl std::fmt::Display for ExactGeometryError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "exact streaming geometry rejected: {self:?}")
    }
}

impl std::error::Error for ExactGeometryError {}

impl From<StreamingLayoutError> for ExactGeometryError {
    fn from(value: StreamingLayoutError) -> Self {
        Self::Layout(value)
    }
}
