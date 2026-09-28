use super::*;

pub(super) enum ReleaseKey {
    Job(crate::GeometryJobKey),
    Page(PageRequestKey),
    ObjectPage(ObjectRequestKey),
}

pub(super) fn push_release_key(
    release: &mut ExactGeometryRelease,
    key: ReleaseKey,
    publication: (usize, usize),
    delta: &ActiveJob,
    budget: &mut AdmissionBudget,
) -> Result<(), ExactGeometryFailure> {
    let storage = (
        release_storage_bytes(release).map_err(|_| prepared_capacity_failure(budget))?,
        release_storage_items(release).map_err(|_| prepared_capacity_failure(budget))?,
    );
    let retained = (
        storage
            .0
            .checked_add(publication.0)
            .ok_or_else(|| prepared_capacity_failure(budget))?,
        storage
            .1
            .checked_add(publication.1)
            .ok_or_else(|| prepared_capacity_failure(budget))?,
    );
    match key {
        ReleaseKey::Job(key) => push_record(&mut release.jobs, key, retained, delta, budget),
        ReleaseKey::Page(key) => push_record(&mut release.pages, key, retained, delta, budget),
        ReleaseKey::ObjectPage(key) => {
            push_record(&mut release.object_pages, key, retained, delta, budget)
        }
    }
}

fn push_record<T>(
    records: &mut Vec<T>,
    key: T,
    retained: (usize, usize),
    delta: &ActiveJob,
    budget: &mut AdmissionBudget,
) -> Result<(), ExactGeometryFailure> {
    let old_capacity = records.capacity();
    if records.len() == old_capacity {
        let capacity = records
            .len()
            .checked_add(1)
            .ok_or_else(|| prepared_capacity_failure(budget))?;
        let bytes = capacity
            .checked_mul(size_of::<T>())
            .and_then(|bytes| bytes.checked_add(retained.0))
            .ok_or_else(|| prepared_capacity_failure(budget))?;
        let items = retained
            .1
            .checked_add(capacity)
            .ok_or_else(|| prepared_capacity_failure(budget))?;
        observe_prepared(budget, delta, bytes, items)?;
        records
            .try_reserve_exact(1)
            .map_err(|_| prepared_capacity_failure(budget))?;
    }
    let added_capacity = records
        .capacity()
        .checked_sub(old_capacity)
        .ok_or_else(|| prepared_capacity_failure(budget))?;
    let bytes = added_capacity
        .checked_mul(size_of::<T>())
        .and_then(|bytes| bytes.checked_add(retained.0))
        .ok_or_else(|| prepared_capacity_failure(budget))?;
    let items = retained
        .1
        .checked_add(added_capacity)
        .ok_or_else(|| prepared_capacity_failure(budget))?;
    observe_prepared(budget, delta, bytes, items)?;
    records.push(key);
    Ok(())
}
