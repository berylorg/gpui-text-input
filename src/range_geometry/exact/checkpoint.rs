use std::collections::VecDeque;

use gpui::{StreamingLayoutBinding, StreamingLayoutContinuation};
use unicode_segmentation::GraphemeCursor;

use crate::ByteOffset;

use super::{BlockTarget, ExactGeometryCheckpoint, ExactGeometryError, Scanner};

pub(super) fn checkpoint(
    binding: &StreamingLayoutBinding,
    continuation: StreamingLayoutContinuation,
    logical_line: u64,
    grapheme_origin: ByteOffset,
    grapheme: GraphemeCursor,
    terminal: bool,
) -> Result<ExactGeometryCheckpoint, ExactGeometryError> {
    if grapheme_origin
        .get()
        .checked_add(grapheme.cur_cursor() as u64)
        != Some(continuation.next_logical_offset)
    {
        return Err(ExactGeometryError::SourceContract);
    }
    Ok(ExactGeometryCheckpoint {
        source: ByteOffset::new(continuation.next_logical_offset),
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
        terminal,
    )
}

pub(super) fn retain_checkpoint(
    checkpoints: &mut VecDeque<ExactGeometryCheckpoint>,
    checkpoint: ExactGeometryCheckpoint,
    capacity: usize,
) {
    if checkpoints.len() > 1
        && checkpoints
            .back()
            .is_some_and(|prior| prior.source == checkpoint.source)
    {
        checkpoints.pop_back();
    }
    while checkpoints.len() >= capacity {
        checkpoints.remove(1);
    }
    checkpoints.push_back(checkpoint);
}

pub(super) fn target_scan_ready(scanner: &Scanner, target: BlockTarget) -> bool {
    let end = target.block_offset + target.viewport_extent + target.overscan;
    scanner.target_source.is_some()
        && scanner.continuation.block_offset + scanner.continuation.line_block_extent >= end
}
