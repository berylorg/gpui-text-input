use super::*;

pub(crate) struct PreparedRangePageStorage {
    pending_index: usize,
    disposition: Vec<ResidentDisposition>,
    destination: VecDeque<RangePage>,
    admission: PageAdmission,
    resident_bytes: usize,
    projected_pages: usize,
    retained_bytes: usize,
    retained_items: usize,
}

impl PreparedRangePageStorage {
    pub(crate) fn with_page(self, page: RangePage) -> PreparedRangePageAdmission {
        PreparedRangePageAdmission {
            page,
            pending_index: self.pending_index,
            disposition: self.disposition,
            destination: self.destination,
            admission: self.admission,
            resident_bytes: self.resident_bytes,
            projected_pages: self.projected_pages,
            retained_bytes: self.retained_bytes,
            retained_items: self.retained_items,
        }
    }
}

fn storage_charge(dispositions: usize, pages: usize) -> Option<(usize, usize)> {
    Some((
        dispositions
            .checked_mul(std::mem::size_of::<ResidentDisposition>())?
            .checked_add(pages.checked_mul(std::mem::size_of::<RangePage>())?)?,
        dispositions.checked_add(pages)?,
    ))
}

impl RangeResidency {
    pub(crate) fn prepare_admit_storage(
        &self,
        page: &RangePage,
        mut admit: impl FnMut(usize, usize) -> bool,
    ) -> Result<Option<PreparedRangePageStorage>, PageAdmissionError> {
        let overflow = || PageAdmissionError::LimitExceeded(ResidencyLimitKind::ResidentBytes);
        let key = page.key();
        self.check_current(key)?;
        #[cfg(test)]
        if self.force_next_admission_limit.replace(false) {
            return Err(overflow());
        }
        let Some(pending_index) = self.pending.iter().position(|pending| *pending == key) else {
            return Err(if self.cancelled.contains(&key) {
                PageAdmissionError::Cancelled(key)
            } else {
                PageAdmissionError::Unavailable(key)
            });
        };
        self.validate_page(page)
            .map_err(PageAdmissionError::Malformed)?;
        if page.retained_bytes() > self.limits.max_resident_bytes() {
            return Err(overflow());
        }

        let overlaps = |resident: &RangePage| {
            resident.range().overlaps(page.range()) || resident.id() == page.id()
        };
        let (mut surviving_pages, mut surviving_bytes) = self
            .resident
            .iter()
            .filter(|resident| !overlaps(resident))
            .try_fold((0usize, 0usize), |(pages, bytes), resident| {
                Some((
                    pages.checked_add(1)?,
                    bytes.checked_add(resident.retained_bytes())?,
                ))
            })
            .ok_or_else(overflow)?;
        let mut evict_before = 0;
        while surviving_pages >= self.limits.max_resident_pages()
            || surviving_bytes
                .checked_add(page.retained_bytes())
                .is_none_or(|bytes| bytes > self.limits.max_resident_bytes())
        {
            let resident = self.resident.get(evict_before).ok_or_else(overflow)?;
            evict_before += 1;
            if !overlaps(resident) {
                surviving_pages = surviving_pages.checked_sub(1).ok_or_else(overflow)?;
                surviving_bytes = surviving_bytes
                    .checked_sub(resident.retained_bytes())
                    .ok_or_else(overflow)?;
            }
        }
        let retained =
            |index: usize, resident: &RangePage| index >= evict_before && !overlaps(resident);
        self.resident
            .iter()
            .enumerate()
            .filter(|(index, resident)| retained(*index, resident))
            .try_fold(
                (
                    page.retained_charge().bytes(),
                    page.retained_charge().items(),
                ),
                |(bytes, items), (_, resident)| {
                    Some((
                        bytes.checked_add(resident.retained_charge().bytes())?,
                        items.checked_add(resident.retained_charge().items())?,
                    ))
                },
            )
            .ok_or_else(overflow)?;
        let projected_pages = surviving_pages.checked_add(1).ok_or_else(overflow)?;
        let resident_bytes = surviving_bytes
            .checked_add(page.retained_bytes())
            .ok_or_else(overflow)?;
        let (bytes, items) =
            storage_charge(self.resident.len(), projected_pages).ok_or_else(overflow)?;
        if !admit(bytes, items) {
            return Ok(None);
        }
        let disposition: Vec<_> = self
            .resident
            .iter()
            .enumerate()
            .map(|(index, resident)| {
                if retained(index, resident) {
                    ResidentDisposition::Retain
                } else {
                    ResidentDisposition::Evict
                }
            })
            .collect();
        let (bytes, items) =
            storage_charge(disposition.capacity(), projected_pages).ok_or_else(overflow)?;
        if !admit(bytes, items) {
            return Ok(None);
        }
        let destination = VecDeque::with_capacity(projected_pages);
        let (bytes, items) =
            storage_charge(disposition.capacity(), destination.capacity()).ok_or_else(overflow)?;
        if !admit(bytes, items) {
            return Ok(None);
        }
        Ok(Some(PreparedRangePageStorage {
            pending_index,
            disposition,
            destination,
            admission: PageAdmission::Admitted {
                page: page.id(),
                evicted_pages: self.resident.len() - surviving_pages,
            },
            resident_bytes,
            projected_pages,
            retained_bytes: page
                .retained_charge()
                .bytes()
                .checked_add(bytes)
                .ok_or_else(overflow)?,
            retained_items: page
                .retained_charge()
                .items()
                .checked_add(items)
                .ok_or_else(overflow)?,
        }))
    }
}
