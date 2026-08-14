use gpui::{StreamingLayoutBinding, WindowTextSystem};
use unicode_segmentation::{GraphemeCursor, GraphemeIncomplete};

use crate::{ByteOffset, RangePage};

use super::super::{
    ActiveJob, AdmissionBudget, ExactGeometryError, ExactGeometryLimits, Scanner,
    StreamingGeometryStyle,
};
use super::{PageScan, complete_grapheme};

#[allow(clippy::too_many_arguments)]
pub(super) fn process_text_region(
    job: &mut ActiveJob,
    page: &RangePage,
    start: u64,
    end: u64,
    forced_boundary: bool,
    text_system: &WindowTextSystem,
    binding: &StreamingLayoutBinding,
    style: &StreamingGeometryStyle,
    limits: ExactGeometryLimits,
    source_len: u64,
    budget: &mut AdmissionBudget,
) -> Result<Option<PageScan>, ExactGeometryError> {
    let page_start = page.range().start().get();
    let local_start =
        usize::try_from(start - page_start).map_err(|_| ExactGeometryError::SourceContract)?;
    let local_end =
        usize::try_from(end - page_start).map_err(|_| ExactGeometryError::SourceContract)?;
    let text = page
        .text()
        .get(local_start..local_end)
        .ok_or(ExactGeometryError::SourceContract)?;
    let cursor_origin = job.scanner.cursor_origin.get();
    let chunk_start = usize::try_from(
        start
            .checked_sub(cursor_origin)
            .ok_or(ExactGeometryError::SourceContract)?,
    )
    .map_err(|_| ExactGeometryError::SourceContract)?;
    let mut consumed = 0usize;
    loop {
        match job.scanner.cursor.next_boundary(text, chunk_start) {
            Ok(Some(boundary)) => {
                let local = boundary
                    .checked_sub(chunk_start)
                    .ok_or(ExactGeometryError::SourceContract)?;
                append_grapheme(
                    &mut job.scanner,
                    &text[consumed..local],
                    binding.limits.segment_bytes,
                );
                budget.observe(job, 0, 0)?;
                complete_grapheme(
                    job,
                    cursor_origin
                        .checked_add(boundary as u64)
                        .ok_or(ExactGeometryError::SourceContract)?,
                    text_system,
                    binding,
                    style,
                    limits,
                    budget,
                )?;
                consumed = local;
            }
            Ok(None) | Err(GraphemeIncomplete::NextChunk) => {
                append_grapheme(
                    &mut job.scanner,
                    &text[consumed..],
                    binding.limits.segment_bytes,
                );
                budget.observe(job, 0, 0)?;
                break;
            }
            Err(GraphemeIncomplete::PreContext(required)) => {
                if required > chunk_start {
                    let context_end = required
                        .checked_sub(chunk_start)
                        .ok_or(ExactGeometryError::SourceContract)?;
                    let context = text
                        .get(..context_end)
                        .ok_or(ExactGeometryError::SourceContract)?;
                    job.scanner.cursor.provide_context(context, chunk_start);
                    continue;
                }
                let replay = chunk_start
                    .checked_add(consumed)
                    .ok_or(ExactGeometryError::SourceContract)?;
                return Ok(Some(PageScan::NeedContext {
                    required_end: ByteOffset::new(
                        cursor_origin
                            .checked_add(required as u64)
                            .ok_or(ExactGeometryError::SourceContract)?,
                    ),
                    replay: ByteOffset::new(
                        cursor_origin
                            .checked_add(replay as u64)
                            .ok_or(ExactGeometryError::SourceContract)?,
                    ),
                }));
            }
            Err(_) => return Err(ExactGeometryError::SourceContract),
        }
    }
    if forced_boundary && end > job.scanner.grapheme_start {
        job.scanner.cursor = GraphemeCursor::new(
            usize::try_from(end - cursor_origin).map_err(|_| ExactGeometryError::SourceContract)?,
            usize::try_from(source_len - cursor_origin)
                .map_err(|_| ExactGeometryError::SourceContract)?,
            true,
        );
        complete_grapheme(job, end, text_system, binding, style, limits, budget)?;
    }
    Ok(None)
}

fn append_grapheme(scanner: &mut Scanner, piece: &str, cap: usize) {
    let Some(buffer) = scanner.grapheme_text.as_mut() else {
        return;
    };
    if buffer
        .len()
        .checked_add(piece.len())
        .is_none_or(|bytes| bytes > cap)
    {
        scanner.grapheme_text = None;
    } else {
        buffer.push_str(piece);
    }
}
