use super::*;

pub(crate) struct PreparedObjectPageStorage {
    pending_index: usize,
    disposition: Vec<ResidentDisposition>,
    destination: VecDeque<ObjectPage>,
    admission: ObjectPageAdmission,
    resident_bytes: usize,
    resident_objects: usize,
    resident_presentation_bytes: usize,
    projected_pages: usize,
    retained_bytes: usize,
    retained_items: usize,
}

impl PreparedObjectPageStorage {
    pub(crate) fn with_page(self, page: ObjectPage) -> PreparedObjectPageAdmission {
        PreparedObjectPageAdmission {
            page,
            pending_index: self.pending_index,
            disposition: self.disposition,
            destination: self.destination,
            admission: self.admission,
            resident_bytes: self.resident_bytes,
            resident_objects: self.resident_objects,
            resident_presentation_bytes: self.resident_presentation_bytes,
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
            .checked_add(pages.checked_mul(std::mem::size_of::<ObjectPage>())?)?,
        dispositions.checked_add(pages)?,
    ))
}

impl ObjectResidency {
    pub(crate) fn prepare_delivered_storage(
        &self,
        page: &ObjectPage,
        text: &crate::RangeResidency,
        admit: impl FnMut(usize, usize) -> bool,
    ) -> Result<Option<PreparedObjectPageStorage>, ObjectPageAdmissionError> {
        self.prepare_storage(
            page,
            || {
                let mismatch = |_| ObjectContractError::ScalarBoundaryProofMismatch {
                    anchor: page
                        .objects()
                        .first()
                        .map_or(crate::ByteOffset::new(0), |object| object.anchor()),
                };
                let proofs = text
                    .object_page_anchor_proofs(self.binding, page)
                    .map_err(mismatch)?;
                self.validate_page_proofs(
                    page,
                    self.binding,
                    page.id(),
                    page.key(),
                    proofs.map(|proof| proof.map_err(mismatch)),
                )
            },
            admit,
        )
    }

    pub(super) fn prepare_storage(
        &self,
        page: &ObjectPage,
        validate: impl FnOnce() -> Result<(), ObjectContractError>,
        mut admit: impl FnMut(usize, usize) -> bool,
    ) -> Result<Option<PreparedObjectPageStorage>, ObjectPageAdmissionError> {
        let overflow =
            || ObjectPageAdmissionError::LimitExceeded(ObjectResidencyLimitKind::ResidentBytes);
        let key = page.key();
        if !self.is_current(key) {
            return Err(ObjectPageAdmissionError::Stale(key));
        }
        #[cfg(test)]
        if self.force_next_admission_limit.replace(false) {
            return Err(overflow());
        }
        let Some(pending_index) = self.pending.iter().position(|pending| *pending == key) else {
            return Err(if self.cancelled.contains(&key) {
                ObjectPageAdmissionError::Cancelled(key)
            } else {
                ObjectPageAdmissionError::Unavailable(key)
            });
        };
        validate().map_err(ObjectPageAdmissionError::Malformed)?;
        let charge = page.retained_charge();
        if charge.bytes() > self.limits.max_resident_bytes() {
            return Err(overflow());
        }
        if charge.objects() > self.limits.max_resident_objects() {
            return Err(ObjectPageAdmissionError::LimitExceeded(
                ObjectResidencyLimitKind::ResidentObjects,
            ));
        }
        if charge.presentation_bytes() > self.limits.max_resident_presentation_bytes() {
            return Err(ObjectPageAdmissionError::LimitExceeded(
                ObjectResidencyLimitKind::ResidentPresentationBytes,
            ));
        }
        let reconciled_index = self
            .resident
            .iter()
            .position(|resident| resident.id() == page.id());
        let replaced = |resident: &ObjectPage| {
            resident.id() == page.id()
                || resident
                    .objects()
                    .iter()
                    .any(|left| page.objects().iter().any(|right| left.id() == right.id()))
        };
        let (
            mut surviving_pages,
            mut surviving_bytes,
            mut surviving_objects,
            mut surviving_presentation_bytes,
        ) = self
            .resident
            .iter()
            .filter(|resident| !replaced(resident))
            .try_fold(
                (0usize, 0usize, 0usize, 0usize),
                |(pages, bytes, objects, presentation), resident| {
                    let charge = resident.retained_charge();
                    Some((
                        pages.checked_add(1)?,
                        bytes.checked_add(charge.bytes())?,
                        objects.checked_add(charge.objects())?,
                        presentation.checked_add(charge.presentation_bytes())?,
                    ))
                },
            )
            .ok_or_else(overflow)?;
        let mut evict_before = 0;
        while surviving_pages >= self.limits.max_resident_pages()
            || surviving_bytes
                .checked_add(charge.bytes())
                .is_none_or(|n| n > self.limits.max_resident_bytes())
            || surviving_objects
                .checked_add(charge.objects())
                .is_none_or(|n| n > self.limits.max_resident_objects())
            || surviving_presentation_bytes
                .checked_add(charge.presentation_bytes())
                .is_none_or(|n| n > self.limits.max_resident_presentation_bytes())
        {
            let resident = self.resident.get(evict_before).ok_or_else(overflow)?;
            evict_before += 1;
            if !replaced(resident) {
                let charge = resident.retained_charge();
                surviving_pages = surviving_pages.checked_sub(1).ok_or_else(overflow)?;
                surviving_bytes = surviving_bytes
                    .checked_sub(charge.bytes())
                    .ok_or_else(overflow)?;
                surviving_objects = surviving_objects
                    .checked_sub(charge.objects())
                    .ok_or_else(overflow)?;
                surviving_presentation_bytes = surviving_presentation_bytes
                    .checked_sub(charge.presentation_bytes())
                    .ok_or_else(overflow)?;
            }
        }
        let retained =
            |index: usize, resident: &ObjectPage| index >= evict_before && !replaced(resident);
        let mut evicted_pages = 0usize;
        let mut evicted_objects = 0usize;
        let mut projected = (
            charge.bytes(),
            charge
                .allocated_items()
                .checked_add(1)
                .ok_or_else(overflow)?,
        );
        for (index, resident) in self.resident.iter().enumerate() {
            if retained(index, resident) {
                projected.0 = projected
                    .0
                    .checked_add(resident.retained_charge().bytes())
                    .ok_or_else(overflow)?;
                projected.1 = projected
                    .1
                    .checked_add(resident.retained_charge().allocated_items())
                    .and_then(|items| items.checked_add(1))
                    .ok_or_else(overflow)?;
            } else if Some(index) != reconciled_index {
                evicted_pages = evicted_pages.checked_add(1).ok_or_else(overflow)?;
                evicted_objects = evicted_objects
                    .checked_add(resident.retained_charge().objects())
                    .ok_or_else(overflow)?;
            }
        }
        let projected_pages = surviving_pages.checked_add(1).ok_or_else(overflow)?;
        let resident_bytes = surviving_bytes
            .checked_add(charge.bytes())
            .ok_or_else(overflow)?;
        let resident_objects = surviving_objects
            .checked_add(charge.objects())
            .ok_or_else(overflow)?;
        let resident_presentation_bytes = surviving_presentation_bytes
            .checked_add(charge.presentation_bytes())
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
        let admission = if reconciled_index.is_some() {
            ObjectPageAdmission::Reconciled {
                page: page.id(),
                evicted_pages,
                evicted_objects,
            }
        } else {
            ObjectPageAdmission::Admitted {
                page: page.id(),
                evicted_pages,
                evicted_objects,
            }
        };
        Ok(Some(PreparedObjectPageStorage {
            pending_index,
            disposition,
            destination,
            admission,
            resident_bytes,
            resident_objects,
            resident_presentation_bytes,
            projected_pages,
            retained_bytes: charge.bytes().checked_add(bytes).ok_or_else(overflow)?,
            retained_items: charge
                .allocated_items()
                .checked_add(1)
                .and_then(|n| n.checked_add(items))
                .ok_or_else(overflow)?,
        }))
    }
}
