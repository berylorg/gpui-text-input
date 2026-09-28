use std::mem::size_of;

use crate::{GeometryJobKey, ObjectRequestKey, PageRequestKey};

use super::super::{ExactGeometryCounts, ExactGeometryError, ExactGeometryRelease};

#[derive(Default)]
pub(super) struct PreparedRelease {
    pub(super) jobs: [Option<GeometryJobKey>; 4],
    pub(super) page: Option<PageRequestKey>,
    pub(super) object_page: Option<ObjectRequestKey>,
    pub(super) counts: ExactGeometryCounts,
}

impl PreparedRelease {
    pub(super) fn prepare(
        self,
        bytes: usize,
        items: usize,
        max_bytes: usize,
        max_items: usize,
    ) -> Result<(ExactGeometryRelease, usize, usize), ExactGeometryError> {
        let job_count = self.jobs.iter().flatten().count();
        let page_count = usize::from(self.page.is_some());
        let object_count = usize::from(self.object_page.is_some());
        let storage = job_count * size_of::<GeometryJobKey>()
            + page_count * size_of::<PageRequestKey>()
            + object_count * size_of::<ObjectRequestKey>();
        let mut required_bytes = bytes
            .checked_add(storage)
            .ok_or(ExactGeometryError::CapacityExceeded)?;
        let mut required_items = items
            .checked_add(job_count + page_count + object_count)
            .ok_or(ExactGeometryError::CapacityExceeded)?;
        let mut reserve = |count: usize, record_size: usize, actual_capacity: Option<usize>| {
            if let Some(capacity) = actual_capacity {
                let extra = capacity
                    .checked_sub(count)
                    .ok_or(ExactGeometryError::CapacityExceeded)?;
                required_bytes = extra
                    .checked_mul(record_size)
                    .and_then(|extra| required_bytes.checked_add(extra))
                    .ok_or(ExactGeometryError::CapacityExceeded)?;
                required_items = required_items
                    .checked_add(extra)
                    .ok_or(ExactGeometryError::CapacityExceeded)?;
            }
            if required_bytes > max_bytes || required_items > max_items {
                return Err(ExactGeometryError::CapacityExceeded);
            }
            Ok(())
        };
        reserve(0, 0, None)?;
        let mut jobs = Vec::new();
        jobs.try_reserve_exact(job_count)
            .map_err(|_| ExactGeometryError::CapacityExceeded)?;
        reserve(job_count, size_of::<GeometryJobKey>(), Some(jobs.capacity()))?;
        let mut pages = Vec::new();
        pages.try_reserve_exact(page_count)
            .map_err(|_| ExactGeometryError::CapacityExceeded)?;
        reserve(page_count, size_of::<PageRequestKey>(), Some(pages.capacity()))?;
        let mut object_pages = Vec::new();
        object_pages.try_reserve_exact(object_count)
            .map_err(|_| ExactGeometryError::CapacityExceeded)?;
        reserve(
            object_count,
            size_of::<ObjectRequestKey>(),
            Some(object_pages.capacity()),
        )?;
        jobs.extend(self.jobs.into_iter().flatten());
        jobs.sort();
        jobs.dedup();
        pages.extend(self.page);
        object_pages.extend(self.object_page);
        Ok((
            ExactGeometryRelease {
                jobs,
                pages,
                object_pages,
                counts: self.counts,
            },
            required_bytes,
            required_items,
        ))
    }
}
