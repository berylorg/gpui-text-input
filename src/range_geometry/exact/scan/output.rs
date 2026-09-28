use super::*;

mod reservation;

pub(super) fn admit_layout(
    job: &mut ActiveJob,
    text_system: &WindowTextSystem,
    binding: &StreamingLayoutBinding,
    limits: ExactGeometryLimits,
    retain_checkpoint: bool,
    budget: &mut AdmissionBudget,
    next_position: gpui::StreamingLayoutPosition,
    transient_runs: usize,
    shared_display: Option<(*const u8, usize)>,
    admit: impl FnOnce(
        &mut gpui::StreamingLayoutSession<'_>,
    ) -> Result<gpui::StreamingLayoutAdmission, gpui::StreamingLayoutError>,
) -> Result<bool, ExactGeometryError> {
    let prior = job.scanner.continuation;
    let transient_run_bytes = transient_runs
        .checked_mul(std::mem::size_of::<gpui::TextRun>())
        .ok_or(ExactGeometryError::CapacityExceeded)?;
    let reserved_binding = reservation::binding(
        job,
        binding,
        next_position,
        transient_run_bytes,
        transient_runs,
        shared_display.map_or(0, |allocation| allocation.1),
        budget,
    )?;
    let (admission, session_item_charge) = {
        let mut session = text_system.resume_streaming_layout_session(reserved_binding, prior)?;
        let admission = admit(&mut session)?;
        let retained_items = session.retained_item_charge();
        (admission, retained_items)
    };
    // The returned admission remains live while its continuation and any retained fragment handles
    // enter scanner state. The prior continuation is replaced, but the admission copy coexists at
    // this peak and is therefore charged.
    let fragment_bytes = super::super::accounting::fragment_record_bytes(admission.fragments.len());
    let full_transient_bytes = admission
        .charge
        .total()?
        .checked_add(fragment_bytes)
        .ok_or(ExactGeometryError::CapacityExceeded)?;
    let full_transient_items = admission
        .item_charge
        .total()?
        .checked_add(admission.fragments.len())
        .ok_or(ExactGeometryError::CapacityExceeded)?;
    let shared_bytes = admission
        .fragments
        .iter()
        .try_fold(0usize, |bytes, fragment| {
            let gpui::StreamingLayoutFragment::InlineObject(fragment) = fragment else {
                return Ok(bytes);
            };
            let allocation = (fragment.presentation.as_ptr(), fragment.presentation.len());
            bytes
                .checked_add(if Some(allocation) == shared_display {
                    allocation.1
                } else {
                    0
                })
                .ok_or(ExactGeometryError::CapacityExceeded)
        })?;
    let prior_display_bytes = budget.output_display_bytes;
    budget.output_display_bytes = prior_display_bytes
        .checked_add(shared_bytes)
        .ok_or(ExactGeometryError::CapacityExceeded)?;
    budget.observe(
        job,
        full_transient_bytes
            .checked_add(transient_run_bytes)
            .ok_or(ExactGeometryError::CapacityExceeded)?,
        full_transient_items
            .checked_add(transient_runs)
            .ok_or(ExactGeometryError::CapacityExceeded)?,
    )?;
    job.scanner.continuation = admission.continuation;
    job.scanner.continuation_items = session_item_charge.total()?;
    let (retained, transient_bytes, transient_items) =
        if matches!(job.kind, ActiveKind::Target { .. }) {
            let ActiveKind::Target { target, anchor, .. } = job.kind else {
                unreachable!();
            };
            let retained_count = admission
                .fragments
                .iter()
                .filter(|fragment| {
                    super::super::target_output::fragment_intersects_target(
                        fragment,
                        prior,
                        target,
                        anchor,
                        binding.line_height,
                    )
                })
                .count();
            let capacity = collection_capacity(
                job.scanner.fragments.capacity(),
                job.scanner.fragments.len(),
                retained_count,
            )?;
            let backing_bytes = if capacity > job.scanner.fragments.capacity() {
                capacity
                    .checked_mul(std::mem::size_of::<gpui::StreamingLayoutFragment>())
                    .ok_or(ExactGeometryError::CapacityExceeded)?
            } else {
                0
            };
            budget.observe(
                job,
                full_transient_bytes
                    .checked_add(backing_bytes)
                    .ok_or(ExactGeometryError::CapacityExceeded)?,
                full_transient_items,
            )?;
            if backing_bytes != 0 {
                job.scanner
                    .fragments
                    .try_reserve_exact(capacity - job.scanner.fragments.len())
                    .map_err(|_| ExactGeometryError::CapacityExceeded)?;
            }
            super::super::target_output::resolve_source_anchor(job, &admission.fragments);
            super::super::target_output::update_target_source(
                job,
                &admission.fragments,
                admission.continuation,
            );
            if retained_count != 0 {
                job.scanner.output_charge = super::super::accounting::add_fragment_charge(
                    job.scanner.output_charge,
                    admission.charge,
                )?;
                job.scanner.output_item_charge =
                    super::super::accounting::add_fragment_item_charge(
                        job.scanner.output_item_charge,
                        admission.item_charge,
                    )?;
                job.scanner.fragments.extend(
                    admission
                        .fragments
                        .iter()
                        .filter(|fragment| {
                            super::super::target_output::fragment_intersects_target(
                                fragment,
                                prior,
                                target,
                                anchor,
                                binding.line_height,
                            )
                        })
                        .cloned(),
                );
                // Fragment clones share GPUI's immutable payload Arcs. Only the second initialized
                // enum records coexist; the payload charge remains single-counted in scanner output.
                (
                    true,
                    fragment_bytes
                        .saturating_add(std::mem::size_of::<StreamingLayoutContinuation>()),
                    admission.fragments.len().saturating_add(1),
                )
            } else {
                (false, full_transient_bytes, full_transient_items)
            }
        } else {
            (false, full_transient_bytes, full_transient_items)
        };
    budget.observe(job, transient_bytes, transient_items)?;
    if retain_checkpoint && matches!(job.kind, ActiveKind::Index) {
        let checkpoint = super::super::checkpoint::make_checkpoint(&job.scanner, binding, false)
            .map_err(|error| {
                budget.failure_stage = Some(super::super::ExactGeometryFailureStage::Checkpoint);
                error
            })?;
        super::super::checkpoint::retain_scanner_checkpoint(
            job,
            checkpoint,
            limits.max_checkpoints,
            budget,
            transient_bytes,
            transient_items,
        )
        .map_err(|error| {
            budget.failure_stage = Some(super::super::ExactGeometryFailureStage::Checkpoint);
            error
        })?;
    }
    drop(admission);
    if !retained {
        budget.output_display_bytes = prior_display_bytes;
    }
    budget.observe(job, 0, 0)?;
    Ok(retained)
}

fn collection_capacity(
    current: usize,
    len: usize,
    additional: usize,
) -> Result<usize, ExactGeometryError> {
    let required = len
        .checked_add(additional)
        .ok_or(ExactGeometryError::CapacityExceeded)?;
    Ok(if required <= current {
        current
    } else {
        current.saturating_mul(2).max(required).max(4)
    })
}

pub(super) fn reserve_presentation(
    job: &mut ActiveJob,
    budget: &mut AdmissionBudget,
) -> Result<(), ExactGeometryError> {
    let capacity = collection_capacity(
        job.scanner.object_presentations.capacity(),
        job.scanner.object_presentations.len(),
        1,
    )?;
    if capacity > job.scanner.object_presentations.capacity() {
        let bytes = capacity
            .checked_mul(std::mem::size_of::<
                super::super::TargetInlineObjectPresentation,
            >())
            .ok_or(ExactGeometryError::CapacityExceeded)?;
        budget.observe(job, bytes, capacity)?;
        job.scanner
            .object_presentations
            .try_reserve_exact(capacity - job.scanner.object_presentations.len())
            .map_err(|_| ExactGeometryError::CapacityExceeded)?;
    }
    Ok(())
}
