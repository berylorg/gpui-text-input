//! Crate-owned exact streaming geometry and block-target resolution.

use std::collections::VecDeque;

use gpui::{
    Pixels, StreamingLayoutBinding, StreamingLayoutCharge, StreamingLayoutContinuation,
    StreamingLayoutFragment, StreamingLayoutItemCharge,
};
use unicode_segmentation::GraphemeCursor;

use crate::{
    AtomId, ByteOffset, ByteRange, InlineObjectFact, ObjectCursor, ObjectRequestKey, PageId,
    PageRequestId, PageRequestKey, PresentationGeneration, RangeBinding, SourcePosition,
};

use super::{GeometryJobId, GeometryJobKey, GeometryKey, LayoutEpoch};

mod accounting;
mod admission;
mod checkpoint;
mod lifecycle;
mod owner;
mod prepared_admission;
mod scan;
mod target;
mod target_output;
mod transition;
mod types;
mod validation;

pub(crate) use prepared_admission::{
    PreparedTargetResponse, PreparedTargetSuccessor, TargetResponseSuccessor,
};
pub(crate) use transition::PreparedGeometryTransition;

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
    presentation_generation: PresentationGeneration,
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
    read_position: u64,
    active_atom: Option<Box<ActiveAtom>>,
    checkpoints: VecDeque<ExactGeometryCheckpoint>,
    fragments: Vec<StreamingLayoutFragment>,
    output_charge: StreamingLayoutCharge,
    output_item_charge: StreamingLayoutItemCharge,
    target_line_position: SourcePosition,
    target_line_block: Pixels,
    target_source: Option<SourcePosition>,
    first_object_cursor: Option<ObjectCursor>,
    object_cursor: Option<ObjectCursor>,
    deferred_object: Option<Box<DeferredObject>>,
}

#[derive(Clone, Debug)]
struct DeferredObject {
    binding: RangeBinding,
    presentation_generation: PresentationGeneration,
    fact: InlineObjectFact,
}

#[derive(Clone, Copy, Debug)]
struct ActiveTextPage {
    id: PageId,
    range: ByteRange,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PendingInput {
    Text(PageRequestKey),
    Object(ObjectRequestKey),
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
            input_id: binding.input_id,
            segment_policy_id: binding.segment_policy_id,
            next_ordinal: 0,
            next_position: binding.start_position,
            inline_offset: Pixels::ZERO,
            line_has_content: false,
            block_offset: Pixels::ZERO,
            line_block_extent: binding.line_height,
            visual_lines: 0,
            finalized_logical_lines: 0,
            line_finalized: false,
            ended: false,
        };
        let cursor = GraphemeCursor::new(0, source_len, true);
        let cursor_origin = ByteOffset::new(0);
        let origin = checkpoint::checkpoint(
            binding,
            continuation,
            0,
            cursor_origin,
            cursor.clone(),
            None,
            false,
        )
        .expect("origin checkpoint is coherent");
        Self {
            cursor: cursor.clone(),
            cursor_origin,
            grapheme_start_cursor: cursor,
            continuation,
            continuation_items: accounting::ordinary_continuation_items(),
            logical_line: 0,
            segment_text: String::new(),
            segment_start: 0,
            grapheme_text: Some(String::new()),
            grapheme_start: 0,
            read_position: 0,
            active_atom: None,
            checkpoints: VecDeque::from([origin]),
            fragments: Vec::new(),
            output_charge: StreamingLayoutCharge::default(),
            output_item_charge: StreamingLayoutItemCharge::default(),
            target_line_position: SourcePosition::try_from(binding.start_position)
                .expect("validated origin position"),
            target_line_block: Pixels::ZERO,
            target_source: None,
            first_object_cursor: None,
            object_cursor: None,
            deferred_object: None,
        }
    }

    fn from_checkpoint(checkpoint: &ExactGeometryCheckpoint) -> Self {
        Self {
            cursor: checkpoint.grapheme.clone(),
            cursor_origin: checkpoint.grapheme_origin,
            grapheme_start_cursor: checkpoint.grapheme.clone(),
            continuation: checkpoint.continuation,
            continuation_items: accounting::ordinary_continuation_items(),
            logical_line: checkpoint.logical_line,
            segment_text: String::new(),
            segment_start: checkpoint.source.byte_offset.get(),
            grapheme_text: Some(String::new()),
            grapheme_start: checkpoint.source.byte_offset.get(),
            read_position: checkpoint.source.byte_offset.get(),
            active_atom: None,
            checkpoints: VecDeque::new(),
            fragments: Vec::new(),
            output_charge: StreamingLayoutCharge::default(),
            output_item_charge: StreamingLayoutItemCharge::default(),
            target_line_position: checkpoint.source,
            target_line_block: checkpoint.continuation.block_offset,
            target_source: None,
            first_object_cursor: None,
            object_cursor: checkpoint.object_cursor,
            deferred_object: None,
        }
    }
}

#[derive(Clone, Debug)]
enum ActiveKind {
    Index,
    Target {
        target: BlockTarget,
        predecessor: SourcePosition,
        anchor: Option<SourcePosition>,
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
    pending: Option<Box<PendingInput>>,
    text_page: Option<ActiveTextPage>,
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
            .checked_add(self.page_payload_bytes)
            .and_then(|bytes| bytes.checked_add(counts.total_bytes()))
            .and_then(|bytes| bytes.checked_add(transient_bytes));
        let items = self
            .fixed_items
            .checked_add(self.page_items)
            .and_then(|items| items.checked_add(counts.total_items()))
            .and_then(|items| items.checked_add(transient_items));
        let (Some(bytes), Some(items)) = (bytes, items) else {
            self.peak_bytes = usize::MAX;
            self.peak_items = usize::MAX;
            return Err(ExactGeometryError::CapacityExceeded);
        };
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
    anchor: Option<SourcePosition>,
}

/// Owner of exact streaming index and block-target state.
pub struct ExactGeometryOwner {
    inputs: Option<Box<OwnerInputs>>,
    limits: ExactGeometryLimits,
    key: GeometryKey,
    highest_job: Option<GeometryJobId>,
    highest_request: Option<PageRequestId>,
    highest_object_request: Option<crate::ObjectRequestId>,
    active: Option<Box<ActiveJob>>,
    desired_target: Option<Box<DesiredTarget>>,
    index: Option<Box<ExactGeometryIndex>>,
    target: Option<Box<BlockTargetPublication>>,
    high_water_bytes: usize,
    high_water_items: usize,
}

#[cfg(test)]
mod tests {
    use gpui::{
        SharedString, StreamingLayoutBinding, StreamingLayoutLimits, StreamingLayoutPosition,
        TextRun, black, font, px,
    };

    use super::*;
    use crate::{
        BindingId, LogicalExtent, ObjectRequestId, PageId, PresentationGeneration, SourceRevision,
    };

    fn object_pending_owner() -> (ExactGeometryOwner, GeometryJobKey) {
        let binding = RangeBinding::new(
            BindingId::new(1),
            SourceRevision::new(1),
            LogicalExtent::new(0, 0),
        );
        let run = TextRun {
            len: 0,
            font: font(".SystemUIFont"),
            color: black(),
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let style = StreamingGeometryStyle::new(
            run.clone(),
            StreamingOversizePresentation::new(
                SharedString::new_static(""),
                Vec::new(),
                px(10.),
                px(14.),
                px(0.),
                None,
            ),
        );
        let layout = StreamingLayoutBinding {
            input_id: 1,
            segment_policy_id: 1,
            start_position: StreamingLayoutPosition::at(0),
            wrap_width: px(100.),
            font_size: px(10.),
            line_height: px(14.),
            limits: StreamingLayoutLimits {
                segment_bytes: 8,
                runs: 8,
                decorations: 8,
                glyphs: 32,
                wraps: 8,
                maps: 33,
                fragments: 1,
                retained_items: 1024,
                retained_bytes: 64 * 1024,
            },
        };
        let mut owner = ExactGeometryOwner::new(
            binding,
            PresentationGeneration::new(1),
            layout,
            style,
            ExactGeometryLimits::new(256, 4, 256 * 1024, usize::MAX).unwrap(),
        )
        .unwrap();
        let start = owner.start_index(GeometryJobId::new(1)).unwrap();
        owner.active.as_deref_mut().unwrap().text_page = Some(ActiveTextPage {
            id: PageId::new(1),
            range: ByteRange::from_u64(0, 0).unwrap(),
        });
        (owner, start.key())
    }

    #[test]
    fn pending_object_request_item_cap_is_exact_and_atomic() {
        let (mut exact, exact_job) = object_pending_owner();
        let required = exact.counts().total_items() + 1;
        exact.limits.max_retained_items = required;
        exact
            .request_object_page(exact_job, ObjectRequestId::new(1), 1, 4096)
            .unwrap();
        assert_eq!(exact.counts().total_items(), required);

        let (mut under, under_job) = object_pending_owner();
        under.limits.max_retained_items = required - 1;
        let before = under.counts();
        assert_eq!(
            under.request_object_page(under_job, ObjectRequestId::new(1), 1, 4096),
            Err(ExactGeometryError::CapacityExceeded)
        );
        assert_eq!(under.counts(), before);
    }
}
