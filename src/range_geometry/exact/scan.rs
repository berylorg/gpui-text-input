use gpui::{
    SharedString, StreamingEndOfSource, StreamingInlineObject, StreamingLayoutBinding,
    StreamingLayoutContinuation, StreamingLineFinalization, StreamingOversizeAtom,
    StreamingTextSegment, WindowTextSystem,
};
use unicode_segmentation::GraphemeCursor;

use crate::{
    ByteOffset, ByteRange, InlineObjectFact, InlineObjectGap, ObjectPage, RangePage, SourcePosition,
};

use super::{
    ActiveAtom, ActiveJob, ActiveKind, AdmissionBudget, DeferredObject, ExactGeometryCheckpoint,
    ExactGeometryError, ExactGeometryLimits, OwnerInputs, StreamingGeometryStyle,
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

#[allow(clippy::too_many_arguments)]
pub(super) fn process_page_range(
    job: &mut ActiveJob,
    page: &RangePage,
    start: u64,
    end: u64,
    end_position: SourcePosition,
    text_system: &WindowTextSystem,
    binding: &StreamingLayoutBinding,
    style: &StreamingGeometryStyle,
    limits: ExactGeometryLimits,
    source_len: u64,
    budget: &mut AdmissionBudget,
) -> Result<PageScan, ExactGeometryError> {
    let page_end = page.range().end().get();
    if start != job.scanner.read_position
        || start > end
        || end > page_end
        || start < page.range().start().get()
    {
        return Err(ExactGeometryError::SourceContract);
    }
    let mut position = start;
    let mut atoms = page.atoms().iter().peekable();
    if let Some(active) = job.scanner.active_atom.as_deref().copied() {
        let atom = atoms.next().ok_or(ExactGeometryError::SourceContract)?;
        if atom.id() != active.id || atom.global_range() != active.global_range {
            return Err(ExactGeometryError::SourceContract);
        }
        let range = active.global_range;
        position = range.end().get().min(end);
        if position == range.end().get() {
            let end_cursor = cursor_at(position, source_len)?;
            admit_compact_atom(
                job,
                range,
                end_cursor,
                if position == end {
                    end_position
                } else {
                    ordinary_position(position)
                },
                text_system,
                binding,
                style,
                limits,
                true,
                budget,
            )?;
            job.scanner.active_atom = None;
        } else if end == page.range().end().get() {
            job.scanner.read_position = end;
            return Ok(PageScan::Complete);
        } else {
            return Err(ExactGeometryError::SourceContract);
        }
    }
    for atom in atoms {
        if atom.global_range().end().get() <= start {
            continue;
        }
        let atom_start = atom.global_range().start().get();
        if atom_start < position || atom.fragment_range().start().get() < position {
            return Err(ExactGeometryError::SourceContract);
        }
        if atom_start >= end {
            break;
        }
        if atom_start > position {
            if let Some(need) = text::process_text_region(
                job,
                page,
                position,
                atom_start,
                ordinary_position(atom_start),
                true,
                text_system,
                binding,
                style,
                limits,
                source_len,
                budget,
            )? {
                if let PageScan::NeedContext { replay, .. } = need {
                    job.scanner.read_position = replay.get();
                }
                return Ok(need);
            }
        }
        let range = atom.global_range();
        position = range.end().get().min(end);
        if range.end().get() <= end {
            let end_cursor = cursor_at(range.end().get(), source_len)?;
            admit_compact_atom(
                job,
                range,
                end_cursor,
                if range.end().get() == end {
                    end_position
                } else {
                    ordinary_position(range.end().get())
                },
                text_system,
                binding,
                style,
                limits,
                true,
                budget,
            )?;
        } else if end == page.range().end().get() {
            job.scanner.active_atom = Some(Box::new(ActiveAtom {
                id: atom.id(),
                global_range: range,
            }));
            budget.observe(job, 0, 0)?;
            job.scanner.read_position = end;
            return Ok(PageScan::Complete);
        } else {
            return Err(ExactGeometryError::SourceContract);
        }
    }
    if position < end {
        if let Some(need) = text::process_text_region(
            job,
            page,
            position,
            end,
            end_position,
            !matches!(end_position.gap, InlineObjectGap::NoObjects),
            text_system,
            binding,
            style,
            limits,
            source_len,
            budget,
        )? {
            if let PageScan::NeedContext { replay, .. } = need {
                job.scanner.read_position = replay.get();
            }
            return Ok(need);
        }
    }
    job.scanner.read_position = end;
    Ok(PageScan::Complete)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn process_object_page(
    job: &mut ActiveJob,
    text_page: &RangePage,
    object_page: &ObjectPage,
    text_system: &WindowTextSystem,
    inputs: &OwnerInputs,
    limits: ExactGeometryLimits,
    source_len: u64,
    budget: &mut AdmissionBudget,
) -> Result<PageScan, ExactGeometryError> {
    let objects = object_page.objects();
    let mut index = 0usize;
    if let Some(deferred) = job.scanner.deferred_object.take() {
        if deferred.binding != inputs.binding
            || deferred.presentation_generation != inputs.presentation_generation
        {
            return Err(ExactGeometryError::SourceContract);
        }
        let next = objects.first().ok_or(ExactGeometryError::SourceContract)?;
        admit_inline_object(
            job,
            text_page,
            &deferred.fact,
            Some(next),
            text_system,
            &inputs.layout,
            &inputs.style,
            limits,
            source_len,
            budget,
        )?;
    }
    while index < objects.len() {
        let object = &objects[index];
        if job.scanner.first_object_cursor.is_none() {
            job.scanner.first_object_cursor = Some(object.cursor());
        }
        let is_deferred_tail = !object_page.complete() && index + 1 == objects.len();
        if is_deferred_tail {
            if job.scanner.deferred_object.is_some() {
                return Err(ExactGeometryError::SourceContract);
            }
            job.scanner.deferred_object = Some(Box::new(DeferredObject {
                binding: inputs.binding,
                presentation_generation: inputs.presentation_generation,
                fact: object.clone(),
            }));
            job.scanner.object_cursor = Some(object.cursor());
            budget.observe(job, 0, 0)?;
            break;
        }
        let next = objects.get(index + 1);
        admit_inline_object(
            job,
            text_page,
            object,
            next,
            text_system,
            &inputs.layout,
            &inputs.style,
            limits,
            source_len,
            budget,
        )?;
        index += 1;
    }
    if !object_page.complete() {
        if object_page.continuation() != job.scanner.object_cursor {
            return Err(ExactGeometryError::SourceContract);
        }
        return Ok(PageScan::Complete);
    }
    if job.scanner.deferred_object.is_some() {
        return Err(ExactGeometryError::SourceContract);
    }
    if let Some(last) = objects.last() {
        job.scanner.object_cursor = Some(last.cursor());
    }
    // Text bytes can be retained in the bounded segment/grapheme scanner after the last GPUI
    // admission. The explicit read cursor proves exactly which resident byte follows that retained
    // prefix; it is deliberately distinct from the composite GPUI continuation.
    let start = job.scanner.read_position;
    let end = text_page.range().end().get();
    let end_position = if start == end {
        SourcePosition::try_from(job.scanner.continuation.next_position)
            .map_err(|_| ExactGeometryError::SourceContract)?
    } else {
        ordinary_position(end)
    };
    process_page_range(
        job,
        text_page,
        start,
        end,
        end_position,
        text_system,
        &inputs.layout,
        &inputs.style,
        limits,
        source_len,
        budget,
    )
}

#[allow(clippy::too_many_arguments)]
fn admit_inline_object(
    job: &mut ActiveJob,
    page: &RangePage,
    object: &InlineObjectFact,
    next: Option<&InlineObjectFact>,
    text_system: &WindowTextSystem,
    binding: &StreamingLayoutBinding,
    style: &StreamingGeometryStyle,
    limits: ExactGeometryLimits,
    source_len: u64,
    budget: &mut AdmissionBudget,
) -> Result<(), ExactGeometryError> {
    let leading = SourcePosition::try_from(job.scanner.continuation.next_position)
        .map_err(|_| ExactGeometryError::SourceContract)?;
    if leading.byte_offset > object.anchor() {
        return Err(ExactGeometryError::SourceContract);
    }
    let pristine_object_origin =
        leading.byte_offset == object.anchor() && matches!(leading.gap, InlineObjectGap::NoObjects);
    let expected_leading = if pristine_object_origin {
        SourcePosition::new(
            object.anchor(),
            InlineObjectGap::before(object.cursor().neighbor()),
        )
    } else if leading.byte_offset == object.anchor() {
        leading
    } else {
        SourcePosition::new(
            object.anchor(),
            InlineObjectGap::before(object.cursor().neighbor()),
        )
    };
    if leading.byte_offset < object.anchor() {
        process_page_range(
            job,
            page,
            job.scanner.read_position,
            object.anchor().get(),
            expected_leading,
            text_system,
            binding,
            style,
            limits,
            source_len,
            budget,
        )?;
        admit_text_segment(
            job,
            expected_leading,
            text_system,
            binding,
            style,
            limits,
            true,
            budget,
        )?;
    }
    if pristine_object_origin {
        job.scanner.continuation.next_position = expected_leading.into();
        for checkpoint in job
            .scanner
            .checkpoints
            .iter_mut()
            .filter(|checkpoint| checkpoint.source == leading)
        {
            checkpoint.source = expected_leading;
            checkpoint.continuation.next_position = expected_leading.into();
        }
        if job.scanner.target_line_position == leading {
            job.scanner.target_line_position = expected_leading;
        }
        if job.scanner.target_source == Some(leading) {
            job.scanner.target_source = Some(expected_leading);
        }
    }
    if SourcePosition::try_from(job.scanner.continuation.next_position)
        .map_err(|_| ExactGeometryError::SourceContract)?
        != expected_leading
    {
        return Err(ExactGeometryError::SourceContract);
    }
    let trailing_gap = match next.filter(|next| next.anchor() == object.anchor()) {
        Some(next) => {
            InlineObjectGap::between(object.cursor().neighbor(), next.cursor().neighbor())
                .map_err(|_| ExactGeometryError::SourceContract)?
        }
        None => InlineObjectGap::after(object.cursor().neighbor()),
    };
    let trailing = SourcePosition::new(object.anchor(), trailing_gap);
    let presentation = object.presentation();
    let runs = if presentation.display().is_empty() {
        Vec::new()
    } else {
        let mut run = style.text_run.clone();
        run.len = presentation.display().len();
        vec![run]
    };
    let input = StreamingInlineObject {
        input_id: binding.input_id,
        segment_policy_id: binding.segment_policy_id,
        ordinal: job.scanner.continuation.next_ordinal,
        id: object.id().into(),
        order: object.order().into(),
        leading: expected_leading.into(),
        trailing: trailing.into(),
        presentation: presentation.display().clone(),
        runs,
        width: presentation.width(),
        height: presentation.height(),
        baseline: presentation.baseline(),
        background: presentation.background(),
    };
    // Checkpoints retained by the admission must pair the post-object composite continuation with
    // the exact object-source cursor that produced it.
    job.scanner.object_cursor = Some(object.cursor());
    let fragment_start = job.scanner.fragments.len();
    let retained = admit_layout(job, text_system, binding, limits, true, budget, |session| {
        session.admit_inline_object(input)
    })?;
    if retained {
        let matching_fragments = job.scanner.fragments[fragment_start..]
            .iter()
            .filter(|fragment| match fragment {
                gpui::StreamingLayoutFragment::InlineObject(fragment) => {
                    fragment.id == object.id().into()
                        && fragment.order == object.order().into()
                        && fragment.leading == expected_leading.into()
                }
                _ => false,
            })
            .count();
        if matching_fragments != 1 {
            return Err(ExactGeometryError::SourceContract);
        }
        job.scanner
            .object_presentations
            .push(super::TargetInlineObjectPresentation::new(
                object.cursor(),
                object.presentation().clone(),
            ));
        budget.observe(job, 0, 0)?;
    }
    Ok(())
}

fn complete_grapheme(
    job: &mut ActiveJob,
    end: u64,
    end_position: SourcePosition,
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
        job.scanner.cursor = job.scanner.grapheme_start_cursor.clone();
        admit_text_segment(
            job,
            ordinary_position(start),
            text_system,
            binding,
            style,
            limits,
            false,
            budget,
        )?;
        job.scanner.cursor = end_cursor.clone();
        job.scanner.grapheme_start = end;
        job.scanner.grapheme_start_cursor = end_cursor.clone();
        job.scanner.logical_line = job
            .scanner
            .logical_line
            .checked_add(1)
            .ok_or(ExactGeometryError::SourceContract)?;
        let delimiter_start = job.scanner.continuation.next_position;
        job.scanner.segment_start = end;
        let finalization = StreamingLineFinalization {
            input_id: binding.input_id,
            segment_policy_id: binding.segment_policy_id,
            ordinal: job.scanner.continuation.next_ordinal,
            delimiter_range: Some(delimiter_start..end_position.into()),
            next_position: end_position.into(),
        };
        admit_layout(job, text_system, binding, limits, true, budget, |session| {
            session.finalize_logical_line(finalization)
        })?;
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
                ordinary_position(start),
                text_system,
                binding,
                style,
                limits,
                true,
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
            ordinary_position(start),
            text_system,
            binding,
            style,
            limits,
            true,
            budget,
        )?;
        job.scanner.cursor = end_cursor.clone();
        job.scanner.segment_start = start;
        admit_compact_atom(
            job,
            ByteRange::from_u64(start, end).map_err(|_| ExactGeometryError::SourceContract)?,
            end_cursor.clone(),
            end_position,
            text_system,
            binding,
            style,
            limits,
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
    end_position: SourcePosition,
    text_system: &WindowTextSystem,
    binding: &StreamingLayoutBinding,
    style: &StreamingGeometryStyle,
    limits: ExactGeometryLimits,
    retain_checkpoint: bool,
    budget: &mut AdmissionBudget,
) -> Result<(), ExactGeometryError> {
    if job.scanner.segment_text.is_empty() {
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
        logical_range: job.scanner.continuation.next_position..end_position.into(),
        text: SharedString::new(text),
        runs,
    };
    job.scanner.segment_start = end;
    admit_layout(
        job,
        text_system,
        binding,
        limits,
        retain_checkpoint,
        budget,
        |session| session.admit_text(segment),
    )?;
    Ok(())
}

fn admit_compact_atom(
    job: &mut ActiveJob,
    range: ByteRange,
    end_cursor: GraphemeCursor,
    end_position: SourcePosition,
    text_system: &WindowTextSystem,
    binding: &StreamingLayoutBinding,
    style: &StreamingGeometryStyle,
    limits: ExactGeometryLimits,
    imposes_grapheme_boundary: bool,
    budget: &mut AdmissionBudget,
) -> Result<(), ExactGeometryError> {
    job.scanner.cursor = job.scanner.grapheme_start_cursor.clone();
    admit_text_segment(
        job,
        SourcePosition::new(range.start(), InlineObjectGap::NoObjects),
        text_system,
        binding,
        style,
        limits,
        true,
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
        logical_range: job.scanner.continuation.next_position..end_position.into(),
        presentation: presentation.presentation.clone(),
        runs: presentation.runs.clone(),
        width: presentation.width,
        height: presentation.height,
        baseline: presentation.baseline,
        background: presentation.background,
    };
    job.scanner.segment_start = range.end().get();
    job.scanner.grapheme_start = range.end().get();
    job.scanner.grapheme_start_cursor = end_cursor.clone();
    admit_layout(job, text_system, binding, limits, true, budget, |session| {
        session.admit_oversize_atom(atom)
    })?;
    Ok(())
}

fn admit_layout(
    job: &mut ActiveJob,
    text_system: &WindowTextSystem,
    binding: &StreamingLayoutBinding,
    limits: ExactGeometryLimits,
    retain_checkpoint: bool,
    budget: &mut AdmissionBudget,
    admit: impl FnOnce(
        &mut gpui::StreamingLayoutSession<'_>,
    ) -> Result<gpui::StreamingLayoutAdmission, gpui::StreamingLayoutError>,
) -> Result<bool, ExactGeometryError> {
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
    let (retained, transient_bytes, transient_items) =
        if matches!(job.kind, ActiveKind::Target { .. }) {
            super::target_output::resolve_source_anchor(job, &admission.fragments);
            let ActiveKind::Target { target, anchor, .. } = job.kind else {
                unreachable!();
            };
            super::target_output::update_target_source(
                job,
                &admission.fragments,
                admission.continuation,
            );
            if super::target_output::admission_intersects_target(
                &admission.fragments,
                prior,
                target,
                anchor,
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
                    true,
                    super::accounting::fragment_record_bytes(admission_items)
                        .saturating_add(std::mem::size_of::<StreamingLayoutContinuation>()),
                    admission_items.saturating_add(1),
                )
            } else {
                (false, full_transient_bytes, full_transient_items)
            }
        } else {
            (false, full_transient_bytes, full_transient_items)
        };
    budget.observe(job, transient_bytes, transient_items)?;
    if retain_checkpoint && matches!(job.kind, ActiveKind::Index) {
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
    Ok(retained)
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
        complete_grapheme(
            job,
            source_end,
            ordinary_position(source_end),
            text_system,
            binding,
            style,
            limits,
            budget,
        )?;
    }
    let terminal = if job.scanner.continuation.next_position.byte_offset == source_end {
        SourcePosition::try_from(job.scanner.continuation.next_position)
            .map_err(|_| ExactGeometryError::SourceContract)?
    } else {
        ordinary_position(source_end)
    };
    admit_text_segment(
        job,
        terminal,
        text_system,
        binding,
        style,
        limits,
        false,
        budget,
    )?;
    let end = StreamingEndOfSource {
        input_id: binding.input_id,
        segment_policy_id: binding.segment_policy_id,
        ordinal: job.scanner.continuation.next_ordinal,
        source_extent: source_end,
        position: job.scanner.continuation.next_position,
    };
    admit_layout(
        job,
        text_system,
        binding,
        limits,
        false,
        budget,
        |session| session.end_source(end),
    )?;
    super::target_output::finish_target_source(job);
    Ok(())
}

fn ordinary_position(offset: u64) -> SourcePosition {
    SourcePosition::new(ByteOffset::new(offset), InlineObjectGap::NoObjects)
}

fn cursor_at(offset: u64, source_len: u64) -> Result<GraphemeCursor, ExactGeometryError> {
    let remaining = source_len
        .checked_sub(offset)
        .ok_or(ExactGeometryError::SourceContract)?;
    let remaining = usize::try_from(remaining).map_err(|_| ExactGeometryError::SourceContract)?;
    Ok(GraphemeCursor::new(0, remaining, true))
}
