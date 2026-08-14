use gpui::{
    SharedString, StreamingLayoutBinding, StreamingLayoutContinuation, StreamingOversizeAtom,
    StreamingTextSegment, WindowTextSystem,
};
use unicode_segmentation::GraphemeCursor;

use crate::{ByteOffset, ByteRange, RangePage};

use super::{
    ActiveAtom, ActiveJob, ActiveKind, AdmissionBudget, ExactGeometryCheckpoint,
    ExactGeometryError, ExactGeometryLimits, StreamingGeometryStyle,
};

mod text;

#[derive(Clone, Copy, Debug)]
pub(super) enum PageScan {
    Complete,
    NeedContext {
        required_end: ByteOffset,
        replay: ByteOffset,
    },
}

pub(super) fn process_page(
    job: &mut ActiveJob,
    page: &RangePage,
    text_system: &WindowTextSystem,
    binding: &StreamingLayoutBinding,
    style: &StreamingGeometryStyle,
    limits: ExactGeometryLimits,
    source_len: u64,
    budget: &mut AdmissionBudget,
) -> Result<PageScan, ExactGeometryError> {
    let page_start = page.range().start().get();
    let page_end = page.range().end().get();
    let mut position = page_start;
    let mut atoms = page.atoms().iter().peekable();
    if let Some(active) = job.scanner.active_atom.as_deref().copied() {
        let atom = atoms.next().ok_or(ExactGeometryError::SourceContract)?;
        if atom.id() != active.id || atom.global_range() != active.global_range {
            return Err(ExactGeometryError::SourceContract);
        }
        let range = active.global_range;
        position = range.end().get().min(page_end);
        if position == range.end().get() {
            let end_cursor = cursor_at(position, source_len)?;
            admit_compact_atom(
                job,
                range,
                end_cursor,
                text_system,
                binding,
                style,
                limits,
                true,
                false,
                budget,
            )?;
            job.scanner.active_atom = None;
        } else {
            return Ok(PageScan::Complete);
        }
    }
    for atom in atoms {
        let atom_start = atom.global_range().start().get();
        if atom_start < position || atom.fragment_range().start().get() < position {
            return Err(ExactGeometryError::SourceContract);
        }
        if atom_start > position {
            if let Some(need) = text::process_text_region(
                job,
                page,
                position,
                atom_start,
                true,
                text_system,
                binding,
                style,
                limits,
                source_len,
                budget,
            )? {
                return Ok(need);
            }
        }
        let range = atom.global_range();
        position = range.end().get().min(page_end);
        if range.end().get() <= page_end {
            let end_cursor = cursor_at(range.end().get(), source_len)?;
            admit_compact_atom(
                job,
                range,
                end_cursor,
                text_system,
                binding,
                style,
                limits,
                true,
                false,
                budget,
            )?;
        } else {
            job.scanner.active_atom = Some(Box::new(ActiveAtom {
                id: atom.id(),
                global_range: range,
            }));
            budget.observe(job, 0, 0)?;
            return Ok(PageScan::Complete);
        }
    }
    if position < page_end {
        if let Some(need) = text::process_text_region(
            job,
            page,
            position,
            page_end,
            false,
            text_system,
            binding,
            style,
            limits,
            source_len,
            budget,
        )? {
            return Ok(need);
        }
    }
    Ok(PageScan::Complete)
}

fn complete_grapheme(
    job: &mut ActiveJob,
    end: u64,
    text_system: &WindowTextSystem,
    binding: &StreamingLayoutBinding,
    style: &StreamingGeometryStyle,
    limits: ExactGeometryLimits,
    budget: &mut AdmissionBudget,
) -> Result<(), ExactGeometryError> {
    let start = job.scanner.grapheme_start;
    let grapheme = job.scanner.grapheme_text.take();
    let end_cursor = job.scanner.cursor.clone();
    if grapheme.as_deref() == Some("\n") {
        job.scanner.logical_line = job
            .scanner
            .logical_line
            .checked_add(1)
            .ok_or(ExactGeometryError::SourceContract)?;
        job.scanner.cursor = end_cursor.clone();
        job.scanner.grapheme_start = end;
        job.scanner.grapheme_start_cursor = end_cursor.clone();
        admit_text_segment(job, text_system, binding, style, limits, true, end, budget)?;
    } else if let Some(grapheme) = grapheme {
        if job
            .scanner
            .segment_text
            .len()
            .checked_add(grapheme.len())
            .is_none_or(|bytes| bytes > binding.limits.segment_bytes)
        {
            job.scanner.cursor = job.scanner.grapheme_start_cursor.clone();
            admit_text_segment(
                job,
                text_system,
                binding,
                style,
                limits,
                false,
                start,
                budget,
            )?;
            job.scanner.cursor = end_cursor.clone();
            job.scanner.segment_start = start;
        }
        job.scanner.segment_text.push_str(&grapheme);
        budget.observe(job, 0, 0)?;
    } else {
        job.scanner.cursor = job.scanner.grapheme_start_cursor.clone();
        admit_text_segment(
            job,
            text_system,
            binding,
            style,
            limits,
            false,
            start,
            budget,
        )?;
        job.scanner.cursor = end_cursor.clone();
        job.scanner.segment_start = start;
        admit_compact_atom(
            job,
            ByteRange::from_u64(start, end).map_err(|_| ExactGeometryError::SourceContract)?,
            end_cursor.clone(),
            text_system,
            binding,
            style,
            limits,
            false,
            false,
            budget,
        )?;
        job.scanner.segment_start = end;
    }
    job.scanner.cursor = end_cursor;
    job.scanner.grapheme_start = end;
    job.scanner.grapheme_start_cursor = job.scanner.cursor.clone();
    job.scanner.grapheme_text = Some(String::new());
    budget.observe(job, 0, 0)?;
    Ok(())
}

fn admit_text_segment(
    job: &mut ActiveJob,
    text_system: &WindowTextSystem,
    binding: &StreamingLayoutBinding,
    style: &StreamingGeometryStyle,
    limits: ExactGeometryLimits,
    ends_line: bool,
    next_offset: u64,
    budget: &mut AdmissionBudget,
) -> Result<(), ExactGeometryError> {
    if job.scanner.segment_text.is_empty() && !ends_line {
        return Ok(());
    }
    let text = std::mem::take(&mut job.scanner.segment_text);
    let end = job
        .scanner
        .segment_start
        .checked_add(text.len() as u64)
        .ok_or(ExactGeometryError::SourceContract)?;
    let runs = if text.is_empty() {
        Vec::new()
    } else {
        let mut run = style.text_run.clone();
        run.len = text.len();
        vec![run]
    };
    let segment = StreamingTextSegment {
        input_id: binding.input_id,
        segment_policy_id: binding.segment_policy_id,
        ordinal: job.scanner.continuation.next_ordinal,
        logical_range: job.scanner.segment_start..end,
        next_logical_offset: next_offset,
        text: SharedString::new(text),
        runs,
        ends_logical_line: ends_line,
    };
    job.scanner.segment_start = next_offset;
    admit_layout(job, text_system, binding, limits, budget, |session| {
        session.admit_text(segment)
    })?;
    Ok(())
}

fn admit_compact_atom(
    job: &mut ActiveJob,
    range: ByteRange,
    end_cursor: GraphemeCursor,
    text_system: &WindowTextSystem,
    binding: &StreamingLayoutBinding,
    style: &StreamingGeometryStyle,
    limits: ExactGeometryLimits,
    imposes_grapheme_boundary: bool,
    ends_line: bool,
    budget: &mut AdmissionBudget,
) -> Result<(), ExactGeometryError> {
    job.scanner.cursor = job.scanner.grapheme_start_cursor.clone();
    admit_text_segment(
        job,
        text_system,
        binding,
        style,
        limits,
        false,
        range.start().get(),
        budget,
    )?;
    job.scanner.cursor = end_cursor.clone();
    if imposes_grapheme_boundary {
        job.scanner.cursor_origin = range.end();
    }
    let presentation = &style.oversize;
    let atom = StreamingOversizeAtom {
        input_id: binding.input_id,
        segment_policy_id: binding.segment_policy_id,
        ordinal: job.scanner.continuation.next_ordinal,
        logical_range: range.start().get()..range.end().get(),
        next_logical_offset: range.end().get(),
        presentation: presentation.presentation.clone(),
        runs: presentation.runs.clone(),
        width: presentation.width,
        height: presentation.height,
        baseline: presentation.baseline,
        background: presentation.background,
        ends_logical_line: ends_line,
    };
    job.scanner.segment_start = range.end().get();
    job.scanner.grapheme_start = range.end().get();
    job.scanner.grapheme_start_cursor = end_cursor.clone();
    admit_layout(job, text_system, binding, limits, budget, |session| {
        session.admit_oversize_atom(atom)
    })?;
    Ok(())
}

fn admit_layout(
    job: &mut ActiveJob,
    text_system: &WindowTextSystem,
    binding: &StreamingLayoutBinding,
    limits: ExactGeometryLimits,
    budget: &mut AdmissionBudget,
    admit: impl FnOnce(
        &mut gpui::StreamingLayoutSession<'_>,
    ) -> Result<gpui::StreamingLayoutAdmission, gpui::StreamingLayoutError>,
) -> Result<(), ExactGeometryError> {
    let prior = job.scanner.continuation;
    let (admission, session_item_charge) = {
        let mut session = text_system.resume_streaming_layout_session(binding.clone(), prior)?;
        let admission = admit(&mut session)?;
        let retained_items = session.retained_item_charge();
        (admission, retained_items)
    };
    // The returned admission remains live while its continuation and any retained fragment handles
    // enter scanner state. The prior continuation is replaced, but the admission copy coexists at
    // this peak and is therefore charged.
    job.scanner.continuation = admission.continuation;
    job.scanner.continuation_items = session_item_charge.total()?;
    let admission_items = admission.fragments.len();
    let full_transient_bytes = admission.charge.total()?;
    let full_transient_items = admission.item_charge.total()?;
    let (transient_bytes, transient_items) = if let ActiveKind::Target { target, .. } = job.kind {
        super::target_output::update_target_source(
            job,
            &admission.fragments,
            admission.continuation,
        );
        if super::target_output::admission_intersects_target(
            &admission.fragments,
            prior,
            target,
            binding.line_height,
        ) {
            job.scanner.output_charge = super::accounting::add_fragment_charge(
                job.scanner.output_charge,
                admission.charge,
            )?;
            job.scanner.output_item_charge = super::accounting::add_fragment_item_charge(
                job.scanner.output_item_charge,
                admission.item_charge,
            )?;
            job.scanner
                .fragments
                .extend(admission.fragments.iter().cloned());
            // Fragment clones share GPUI's immutable payload Arcs. Only the second initialized
            // enum records coexist; the payload charge remains single-counted in scanner output.
            (
                super::accounting::fragment_record_bytes(admission_items)
                    .saturating_add(std::mem::size_of::<StreamingLayoutContinuation>()),
                admission_items.saturating_add(1),
            )
        } else {
            (full_transient_bytes, full_transient_items)
        }
    } else {
        (full_transient_bytes, full_transient_items)
    };
    budget.observe(job, transient_bytes, transient_items)?;
    if matches!(job.kind, ActiveKind::Index) {
        let checkpoint =
            super::checkpoint::make_checkpoint(&job.scanner, binding, false).map_err(|error| {
                budget.failure_stage = Some(super::ExactGeometryFailureStage::Checkpoint);
                error
            })?;
        budget
            .observe(
                job,
                transient_bytes.saturating_add(std::mem::size_of::<ExactGeometryCheckpoint>()),
                transient_items,
            )
            .map_err(|error| {
                budget.failure_stage = Some(super::ExactGeometryFailureStage::Checkpoint);
                error
            })?;
        super::checkpoint::retain_checkpoint(
            &mut job.scanner.checkpoints,
            checkpoint,
            limits.max_checkpoints,
        );
        budget
            .observe(job, transient_bytes, transient_items)
            .map_err(|error| {
                budget.failure_stage = Some(super::ExactGeometryFailureStage::Checkpoint);
                error
            })?;
    }
    budget.observe(job, 0, 0)?;
    Ok(())
}

pub(super) fn finalize_source(
    job: &mut ActiveJob,
    text_system: &WindowTextSystem,
    binding: &StreamingLayoutBinding,
    style: &StreamingGeometryStyle,
    limits: ExactGeometryLimits,
    source_end: u64,
    budget: &mut AdmissionBudget,
) -> Result<(), ExactGeometryError> {
    if job.scanner.active_atom.is_some() {
        return Err(ExactGeometryError::SourceContract);
    }
    if job.scanner.grapheme_start < source_end {
        complete_grapheme(job, source_end, text_system, binding, style, limits, budget)?;
    }
    admit_text_segment(
        job,
        text_system,
        binding,
        style,
        limits,
        true,
        source_end,
        budget,
    )?;
    super::target_output::finish_target_source(job);
    Ok(())
}

fn cursor_at(offset: u64, source_len: u64) -> Result<GraphemeCursor, ExactGeometryError> {
    let remaining = source_len
        .checked_sub(offset)
        .ok_or(ExactGeometryError::SourceContract)?;
    let remaining = usize::try_from(remaining).map_err(|_| ExactGeometryError::SourceContract)?;
    Ok(GraphemeCursor::new(0, remaining, true))
}
