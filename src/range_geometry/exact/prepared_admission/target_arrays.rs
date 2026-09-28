use super::*;

pub(super) fn prepare_array<T: Clone>(
    current: &[T],
    added: &[T],
    publication: (usize, usize),
    release: &ExactGeometryRelease,
    delta: &ActiveJob,
    budget: &mut AdmissionBudget,
) -> Result<Arc<[T]>, ExactGeometryFailure> {
    let count = current
        .len()
        .checked_add(added.len())
        .ok_or_else(|| prepared_capacity_failure(budget))?;
    let retained_bytes = release_storage_bytes(release)
        .ok()
        .and_then(|bytes| bytes.checked_add(publication.0))
        .ok_or_else(|| prepared_capacity_failure(budget))?;
    let retained_items = release_storage_items(release)
        .ok()
        .and_then(|items| items.checked_add(publication.1))
        .ok_or_else(|| prepared_capacity_failure(budget))?;
    let mut admit = |capacity: usize| {
        let bytes = capacity
            .checked_mul(size_of::<T>())
            .and_then(|bytes| bytes.checked_add(retained_bytes))
            .ok_or_else(|| prepared_capacity_failure(budget))?;
        let items = capacity
            .checked_add(retained_items)
            .ok_or_else(|| prepared_capacity_failure(budget))?;
        observe_prepared(budget, delta, bytes, items)
    };
    // Reserve the complete publication while vector and immutable-array backing coexist.
    admit(count)?;
    let mut records = Vec::new();
    records
        .try_reserve_exact(count)
        .map_err(|_| prepared_capacity_failure(budget))?;
    let bytes = records
        .capacity()
        .checked_mul(size_of::<T>())
        .and_then(|bytes| bytes.checked_add(retained_bytes))
        .ok_or_else(|| prepared_capacity_failure(budget))?;
    let items = records
        .capacity()
        .checked_add(retained_items)
        .ok_or_else(|| prepared_capacity_failure(budget))?;
    observe_prepared(budget, delta, bytes, items)?;
    records.extend_from_slice(current);
    records.extend_from_slice(added);
    Ok(Arc::from(records))
}
