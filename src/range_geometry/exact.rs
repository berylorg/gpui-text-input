//! Crate-owned exact streaming geometry and block-target resolution.

use std::collections::VecDeque;

use gpui::{
    Pixels, StreamingLayoutBinding, StreamingLayoutCharge, StreamingLayoutContinuation,
    StreamingLayoutFragment, StreamingLayoutItemCharge,
};
use unicode_segmentation::GraphemeCursor;

use crate::{AtomId, ByteOffset, ByteRange, PageRequestId, PageRequestKey, RangeBinding};

use super::{GeometryJobId, GeometryJobKey, GeometryKey, LayoutEpoch};

mod accounting;
mod admission;
mod checkpoint;
mod lifecycle;
mod owner;
mod scan;
mod target;
mod target_output;
mod types;
mod validation;

pub use types::{
    BlockTarget, BlockTargetPublication, ExactGeometryAdmission, ExactGeometryAggregate,
    ExactGeometryCheckpoint, ExactGeometryCounts, ExactGeometryError, ExactGeometryFailure,
    ExactGeometryFailureStage, ExactGeometryIndex, ExactGeometryLimits, ExactGeometryProgress,
    ExactGeometryRelease, ExactGeometryStart, StreamingGeometryEstimate, StreamingGeometryStyle,
    StreamingOversizePresentation,
};

#[derive(Clone, Debug)]
struct OwnerInputs {
    binding: RangeBinding,
    layout: StreamingLayoutBinding,
    style: StreamingGeometryStyle,
}

#[derive(Clone, Debug)]
struct Scanner {
    cursor: GraphemeCursor,
    cursor_origin: ByteOffset,
    grapheme_start_cursor: GraphemeCursor,
    continuation: StreamingLayoutContinuation,
    continuation_items: usize,
    logical_line: u64,
    segment_text: String,
    segment_start: u64,
    grapheme_text: Option<String>,
    grapheme_start: u64,
    active_atom: Option<Box<ActiveAtom>>,
    checkpoints: VecDeque<ExactGeometryCheckpoint>,
    fragments: Vec<StreamingLayoutFragment>,
    output_charge: StreamingLayoutCharge,
    output_item_charge: StreamingLayoutItemCharge,
    target_line_source: u64,
    target_line_block: Pixels,
    target_source: Option<u64>,
}

/// Compact cross-page geometry continuation for an atom.
///
/// Geometry needs stable identity and the authoritative global range; fallback text remains solely
/// in the borrowed pages and is never duplicated into scanner state.
#[derive(Clone, Copy, Debug)]
struct ActiveAtom {
    id: AtomId,
    global_range: ByteRange,
}

impl Scanner {
    fn origin(binding: &StreamingLayoutBinding, source_len: usize) -> Self {
        let continuation = StreamingLayoutContinuation {
            next_ordinal: 0,
            next_logical_offset: 0,
            inline_offset: Pixels::ZERO,
            block_offset: Pixels::ZERO,
            line_block_extent: binding.line_height,
            visual_lines: 0,
        };
        let cursor = GraphemeCursor::new(0, source_len, true);
        let cursor_origin = ByteOffset::new(0);
        let origin = checkpoint::checkpoint(
            binding,
            continuation,
            0,
            cursor_origin,
            cursor.clone(),
            false,
        )
        .expect("origin checkpoint is coherent");
        Self {
            cursor: cursor.clone(),
            cursor_origin,
            grapheme_start_cursor: cursor,
            continuation,
            continuation_items: 1,
            logical_line: 0,
            segment_text: String::new(),
            segment_start: 0,
            grapheme_text: Some(String::new()),
            grapheme_start: 0,
            active_atom: None,
            checkpoints: VecDeque::from([origin]),
            fragments: Vec::new(),
            output_charge: StreamingLayoutCharge::default(),
            output_item_charge: StreamingLayoutItemCharge::default(),
            target_line_source: 0,
            target_line_block: Pixels::ZERO,
            target_source: None,
        }
    }

    fn from_checkpoint(checkpoint: &ExactGeometryCheckpoint) -> Self {
        Self {
            cursor: checkpoint.grapheme.clone(),
            cursor_origin: checkpoint.grapheme_origin,
            grapheme_start_cursor: checkpoint.grapheme.clone(),
            continuation: checkpoint.continuation,
            continuation_items: 1,
            logical_line: checkpoint.logical_line,
            segment_text: String::new(),
            segment_start: checkpoint.source.get(),
            grapheme_text: Some(String::new()),
            grapheme_start: checkpoint.source.get(),
            active_atom: None,
            checkpoints: VecDeque::new(),
            fragments: Vec::new(),
            output_charge: StreamingLayoutCharge::default(),
            output_item_charge: StreamingLayoutItemCharge::default(),
            target_line_source: checkpoint.source.get(),
            target_line_block: checkpoint.continuation.block_offset,
            target_source: None,
        }
    }
}

#[derive(Clone, Debug)]
enum ActiveKind {
    Index,
    Target {
        target: BlockTarget,
        predecessor: ByteOffset,
    },
}

#[derive(Clone, Copy, Debug)]
enum ActivePageUse {
    Traverse {
        anchor: ByteOffset,
    },
    Context {
        required_end: ByteOffset,
        replay: ByteOffset,
    },
}

#[derive(Clone, Debug)]
struct ActiveJob {
    key: GeometryJobKey,
    kind: ActiveKind,
    page_use: ActivePageUse,
    pending: Option<Box<PageRequestKey>>,
    window_identity: Option<usize>,
    retained_capacity: usize,
    scanner: Scanner,
}

struct AdmissionBudget {
    fixed_bytes: usize,
    fixed_items: usize,
    page_payload_bytes: usize,
    page_items: usize,
    max_bytes: usize,
    max_items: usize,
    peak_bytes: usize,
    peak_items: usize,
    failure_stage: Option<ExactGeometryFailureStage>,
}

impl AdmissionBudget {
    fn observe(
        &mut self,
        active: &ActiveJob,
        transient_bytes: usize,
        transient_items: usize,
    ) -> Result<(), ExactGeometryError> {
        let counts = accounting::active_counts(active);
        let bytes = self
            .fixed_bytes
            .saturating_add(self.page_payload_bytes)
            .saturating_add(counts.total_bytes())
            .saturating_add(transient_bytes);
        let items = self
            .fixed_items
            .saturating_add(self.page_items)
            .saturating_add(counts.total_items())
            .saturating_add(transient_items);
        self.peak_bytes = self.peak_bytes.max(bytes);
        self.peak_items = self.peak_items.max(items);
        if bytes > self.max_bytes || items > self.max_items {
            Err(ExactGeometryError::CapacityExceeded)
        } else {
            Ok(())
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct DesiredTarget {
    key: GeometryJobKey,
    target: BlockTarget,
}

/// Owner of exact streaming index and block-target state.
pub struct ExactGeometryOwner {
    inputs: Option<Box<OwnerInputs>>,
    limits: ExactGeometryLimits,
    key: GeometryKey,
    highest_job: Option<GeometryJobId>,
    highest_request: Option<PageRequestId>,
    active: Option<Box<ActiveJob>>,
    desired_target: Option<Box<DesiredTarget>>,
    index: Option<Box<ExactGeometryIndex>>,
    target: Option<Box<BlockTargetPublication>>,
    high_water_bytes: usize,
    high_water_items: usize,
}
