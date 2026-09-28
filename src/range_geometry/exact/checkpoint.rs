use std::collections::VecDeque;

use gpui::{StreamingLayoutBinding, StreamingLayoutContinuation};
use unicode_segmentation::GraphemeCursor;

use crate::{ByteOffset, ObjectCursor, SourcePosition};

use super::{
    ActiveJob, AdmissionBudget, BlockTarget, ExactGeometryCheckpoint, ExactGeometryError, Scanner,
};

pub(super) fn retain_scanner_checkpoint(
    job: &mut ActiveJob,
    checkpoint: ExactGeometryCheckpoint,
    limit: usize,
    budget: &mut AdmissionBudget,
    transient_bytes: usize,
    transient_items: usize,
) -> Result<(), ExactGeometryError> {
    let checkpoints = &job.scanner.checkpoints;
    let replaces = checkpoints.len() > 1
        && checkpoints.back().is_some_and(|prior| {
            prior.source == checkpoint.source && prior.object_cursor == checkpoint.object_cursor
        });
    let required = if replaces || checkpoints.len() >= limit {
        checkpoints.len()
    } else {
        checkpoints
            .len()
            .checked_add(1)
            .ok_or(ExactGeometryError::CapacityExceeded)?
    };
    let capacity = checkpoints.capacity();
    let replacement = if required > capacity {
        capacity.saturating_mul(2).max(required).min(limit)
    } else {
        0
    };
    let records = replacement
        .checked_add(1)
        .ok_or(ExactGeometryError::CapacityExceeded)?;
    let bytes = records
        .checked_mul(std::mem::size_of::<ExactGeometryCheckpoint>())
        .and_then(|bytes| bytes.checked_add(transient_bytes))
        .ok_or(ExactGeometryError::CapacityExceeded)?;
    let items = records
        .checked_add(transient_items)
        .ok_or(ExactGeometryError::CapacityExceeded)?;
    budget.observe(job, bytes, items)?;
    if replacement != 0 {
        job.scanner
            .checkpoints
            .try_reserve_exact(replacement - job.scanner.checkpoints.len())
            .map_err(|_| ExactGeometryError::CapacityExceeded)?;
    }
    retain_checkpoint(&mut job.scanner.checkpoints, checkpoint, limit);
    budget.observe(job, transient_bytes, transient_items)
}

pub(super) fn checkpoint(
    binding: &StreamingLayoutBinding,
    continuation: StreamingLayoutContinuation,
    logical_line: u64,
    grapheme_origin: ByteOffset,
    grapheme: GraphemeCursor,
    object_cursor: Option<ObjectCursor>,
    terminal: bool,
) -> Result<ExactGeometryCheckpoint, ExactGeometryError> {
    if grapheme_origin
        .get()
        .checked_add(grapheme.cur_cursor() as u64)
        != Some(continuation.next_position.byte_offset)
        || continuation.input_id != binding.input_id
        || continuation.segment_policy_id != binding.segment_policy_id
        || continuation.ended != terminal
    {
        return Err(ExactGeometryError::SourceContract);
    }
    let source = SourcePosition::try_from(continuation.next_position)
        .map_err(|_| ExactGeometryError::SourceContract)?;
    if object_cursor.is_some_and(|cursor| cursor.anchor() > source.byte_offset) {
        return Err(ExactGeometryError::SourceContract);
    }
    Ok(ExactGeometryCheckpoint {
        source,
        object_cursor,
        block_offset: continuation.block_offset,
        visual_lines: continuation.visual_lines,
        logical_line,
        segment: continuation.next_ordinal,
        input_id: binding.input_id,
        segment_policy_id: binding.segment_policy_id,
        terminal,
        continuation,
        grapheme_origin,
        grapheme,
    })
}

pub(super) fn make_checkpoint(
    scanner: &Scanner,
    binding: &StreamingLayoutBinding,
    terminal: bool,
) -> Result<ExactGeometryCheckpoint, ExactGeometryError> {
    checkpoint(
        binding,
        scanner.continuation,
        scanner.logical_line,
        scanner.cursor_origin,
        scanner.cursor.clone(),
        scanner.object_cursor,
        terminal,
    )
}

pub(super) fn retain_checkpoint(
    checkpoints: &mut VecDeque<ExactGeometryCheckpoint>,
    checkpoint: ExactGeometryCheckpoint,
    capacity: usize,
) {
    if checkpoints.len() > 1
        && checkpoints.back().is_some_and(|prior| {
            prior.source == checkpoint.source && prior.object_cursor == checkpoint.object_cursor
        })
    {
        checkpoints.pop_back();
    }
    while checkpoints.len() >= capacity {
        checkpoints.remove(1);
    }
    checkpoints.push_back(checkpoint);
}

pub(super) fn target_scan_ready(
    scanner: &Scanner,
    target: BlockTarget,
    anchor: Option<SourcePosition>,
) -> bool {
    let end = target.block_offset + target.viewport_extent + target.overscan;
    let anchor_reached = anchor.is_none_or(|anchor| {
        SourcePosition::try_from(scanner.continuation.next_position)
            .ok()
            .and_then(|position| position.compare_in_revision(anchor))
            .is_some_and(|ordering| !ordering.is_lt())
    });
    anchor_reached
        && scanner.target_source.is_some()
        && scanner.continuation.block_offset + scanner.continuation.line_block_extent >= end
}
