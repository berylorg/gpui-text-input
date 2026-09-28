use super::*;

pub(super) fn binding(
    job: &ActiveJob,
    binding: &StreamingLayoutBinding,
    next_position: gpui::StreamingLayoutPosition,
    transient_bytes: usize,
    transient_items: usize,
    budget: &mut AdmissionBudget,
) -> Result<StreamingLayoutBinding, ExactGeometryError> {
    budget.observe(job, 0, 0)?;
    let counts = super::super::super::accounting::active_counts(job);
    let continuation_items = super::super::super::accounting::continuation_items(next_position);
    let occupied_bytes = budget
        .fixed_bytes
        .checked_add(budget.page_payload_bytes)
        .and_then(|bytes| bytes.checked_add(transient_bytes))
        .and_then(|bytes| bytes.checked_add(counts.total_bytes()))
        .and_then(|bytes| bytes.checked_add(std::mem::size_of::<gpui::StreamingLayoutFragment>()))
        .ok_or(ExactGeometryError::CapacityExceeded)?;
    let occupied_items = budget
        .fixed_items
        .checked_add(budget.page_items)
        .and_then(|items| items.checked_add(transient_items))
        .and_then(|items| items.checked_add(counts.total_items()))
        .and_then(|items| items.checked_sub(job.scanner.continuation_items))
        .and_then(|items| items.checked_add(continuation_items.max(job.scanner.continuation_items)))
        .and_then(|items| items.checked_add(1))
        .ok_or(ExactGeometryError::CapacityExceeded)?;
    budget.admit_layout_startup(
        (occupied_bytes, occupied_items),
        job.scanner.continuation.next_position,
    )?;
    let (bytes, items) = budget.remaining_capacity(occupied_bytes, occupied_items)?;
    let mut reserved = binding.clone();
    reserved.limits.retained_bytes = reserved.limits.retained_bytes.min(bytes);
    reserved.limits.retained_items = reserved.limits.retained_items.min(items);
    Ok(reserved)
}
