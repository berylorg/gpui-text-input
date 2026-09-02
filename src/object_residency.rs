//! Fixed-capacity inline-object pages and pending-request projection.

use std::collections::VecDeque;

use crate::range_source::{
    ByteRange, InlineObjectFact, InlineObjectGap, ObjectContractError, ObjectCursor,
    ObjectDemandEnvelope, ObjectPage, ObjectPageEdgeFact, ObjectPageFailure, ObjectPageId,
    ObjectPurpose, ObjectRequest, ObjectRequestId, ObjectRequestKey, PresentationGeneration,
    RangeBinding, SourcePosition,
};
use crate::residency::ObjectAnchorProofs;

#[cfg(test)]
mod tests;
mod types;

pub use types::*;

/// Fixed-capacity projection over one exact object and presentation generation.
///
/// This owner stores only configured resident pages. It has no whole-source registry and never
/// scans text-page atoms to discover zero-width objects.
#[derive(Debug)]
pub struct ObjectResidency {
    binding: RangeBinding,
    presentation_generation: PresentationGeneration,
    limits: ObjectResidencyLimits,
    resident: VecDeque<ObjectPage>,
    pending: VecDeque<ObjectRequestKey>,
    cancelled: VecDeque<ObjectRequestKey>,
    highest_request: Option<ObjectRequestId>,
    resident_bytes: usize,
    resident_objects: usize,
    resident_presentation_bytes: usize,
    pending_bytes: usize,
    pending_objects: usize,
    #[cfg(test)]
    force_next_admission_limit: std::cell::Cell<bool>,
}

#[derive(Debug)]
pub(crate) struct PreparedObjectRebind {
    binding: RangeBinding,
    presentation_generation: PresentationGeneration,
    cancelled: Vec<ObjectRequestKey>,
}

impl PreparedObjectRebind {
    pub(crate) fn cancelled(&self) -> &[ObjectRequestKey] {
        &self.cancelled
    }

    pub(crate) fn retained_bytes(&self) -> usize {
        self.cancelled
            .capacity()
            .saturating_mul(std::mem::size_of::<ObjectRequestKey>())
    }

    pub(crate) fn retained_items(&self) -> usize {
        1usize.saturating_add(self.cancelled.capacity())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ResidentDisposition {
    Retain,
    Evict,
}

#[derive(Clone)]
pub(crate) struct ProjectedObjectPageIter<'a> {
    resident: std::collections::vec_deque::Iter<'a, ObjectPage>,
    disposition: std::slice::Iter<'a, ResidentDisposition>,
    inbound: Option<&'a ObjectPage>,
    remaining: usize,
}

impl<'a> Iterator for ProjectedObjectPageIter<'a> {
    type Item = &'a ObjectPage;

    fn next(&mut self) -> Option<Self::Item> {
        while let (Some(page), Some(disposition)) = (self.resident.next(), self.disposition.next())
        {
            if *disposition == ResidentDisposition::Retain {
                self.remaining -= 1;
                return Some(page);
            }
        }
        let page = self.inbound.take()?;
        self.remaining -= 1;
        Some(page)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining, Some(self.remaining))
    }
}

impl ExactSizeIterator for ProjectedObjectPageIter<'_> {}
impl std::iter::FusedIterator for ProjectedObjectPageIter<'_> {}

#[derive(Clone)]
pub(crate) struct TouchedObjectPageIter<'a> {
    resident: std::collections::vec_deque::Iter<'a, ObjectPage>,
    touched: Option<&'a ObjectPage>,
    touched_id: Option<ObjectPageId>,
    remaining: usize,
}

impl<'a> Iterator for TouchedObjectPageIter<'a> {
    type Item = &'a ObjectPage;

    fn next(&mut self) -> Option<Self::Item> {
        for page in self.resident.by_ref() {
            if Some(page.id()) != self.touched_id {
                self.remaining -= 1;
                return Some(page);
            }
        }
        let page = self.touched.take()?;
        self.remaining -= 1;
        Some(page)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining, Some(self.remaining))
    }
}

impl ExactSizeIterator for TouchedObjectPageIter<'_> {}
impl std::iter::FusedIterator for TouchedObjectPageIter<'_> {}

/// Fully validated, allocation-complete admission of one inline-object page.
#[derive(Debug)]
pub(crate) struct PreparedObjectPageAdmission {
    page: ObjectPage,
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

#[derive(Debug)]
pub(crate) struct PreparedObjectDemand {
    retired: Vec<ObjectRequestKey>,
    outcome: ObjectDemand,
}

impl PreparedObjectDemand {
    pub(crate) const fn outcome(&self) -> ObjectDemand {
        self.outcome
    }

    pub(crate) fn retained_bytes(&self) -> usize {
        self.retired
            .capacity()
            .saturating_mul(std::mem::size_of::<ObjectRequestKey>())
    }

    pub(crate) fn retained_items(&self) -> usize {
        1usize.saturating_add(self.retired.capacity())
    }
}

impl PreparedObjectPageAdmission {
    pub(crate) const fn page(&self) -> &ObjectPage {
        &self.page
    }

    pub(crate) const fn retained_bytes(&self) -> usize {
        self.retained_bytes
    }

    pub(crate) const fn retained_items(&self) -> usize {
        self.retained_items
    }

    pub(crate) fn into_page(self) -> ObjectPage {
        self.page
    }

    pub(crate) fn projected_resident_pages<'a>(
        &'a self,
        residency: &'a ObjectResidency,
    ) -> ProjectedObjectPageIter<'a> {
        ProjectedObjectPageIter {
            resident: residency.resident.iter(),
            disposition: self.disposition.iter(),
            inbound: Some(&self.page),
            remaining: self.projected_pages,
        }
    }
}

pub(crate) fn page_proves_gap(page: &ObjectPage, position: SourcePosition) -> bool {
    let demand = page.key().demand();
    let anchor = position.byte_offset;
    if !demand.contains_anchor(anchor) {
        return false;
    }
    let objects = page.objects();
    let cursor_matches = |cursor: ObjectCursor, neighbor: crate::InlineObjectNeighbor| {
        cursor.anchor() == anchor
            && cursor.id() == neighbor.id()
            && cursor.order() == neighbor.order()
    };
    let start = objects.partition_point(|object| object.anchor() < anchor);
    let end = objects.partition_point(|object| object.anchor() <= anchor);
    let anchored = &objects[start..end];
    let preceding_complete = start > 0
        || match page.preceding() {
            ObjectPageEdgeFact::EnvelopeBoundary => true,
            ObjectPageEdgeFact::Continues(cursor) => cursor.anchor() < anchor,
        };
    let following_complete = end < objects.len()
        || match page.following() {
            ObjectPageEdgeFact::EnvelopeBoundary => true,
            ObjectPageEdgeFact::Continues(cursor) => cursor.anchor() > anchor,
        };
    match position.gap {
        InlineObjectGap::NoObjects => {
            anchored.is_empty() && preceding_complete && following_complete
        }
        InlineObjectGap::Before(first) => {
            preceding_complete
                && anchored.first().is_some_and(|object| {
                    object.id() == first.id() && object.order() == first.order()
                })
        }
        InlineObjectGap::Between {
            preceding,
            following,
        } => {
            anchored.windows(2).any(|pair| {
                pair[0].id() == preceding.id()
                    && pair[0].order() == preceding.order()
                    && pair[1].id() == following.id()
                    && pair[1].order() == following.order()
            }) || (anchored.first().is_some_and(|object| {
                object.id() == following.id() && object.order() == following.order()
            }) && matches!(page.preceding(), ObjectPageEdgeFact::Continues(cursor) if cursor_matches(cursor, preceding)))
                || (anchored.last().is_some_and(|object| {
                    object.id() == preceding.id() && object.order() == preceding.order()
                }) && matches!(page.following(), ObjectPageEdgeFact::Continues(cursor) if cursor_matches(cursor, following)))
        }
        InlineObjectGap::After(last) => {
            following_complete
                && anchored.last().is_some_and(|object| {
                    object.id() == last.id() && object.order() == last.order()
                })
        }
    }
}

impl ObjectResidency {
    pub(crate) fn checked_initial_owner_storage_charge(
        limits: ObjectResidencyLimits,
    ) -> Option<crate::RangeSurfaceCharge> {
        Some(crate::RangeSurfaceCharge {
            bytes: std::mem::size_of::<Self>()
                .checked_add(
                    limits
                        .max_resident_pages()
                        .checked_mul(std::mem::size_of::<ObjectPage>())?,
                )?
                .checked_add(
                    limits
                        .max_pending_requests()
                        .checked_mul(std::mem::size_of::<ObjectRequestKey>())?,
                )?
                .checked_add(
                    limits
                        .max_pending_requests()
                        .checked_mul(std::mem::size_of::<ObjectRequestKey>())?,
                )?,
            items: 1usize
                .checked_add(limits.max_resident_pages())?
                .checked_add(limits.max_pending_requests())?
                .checked_add(limits.max_pending_requests())?,
        })
    }

    pub(crate) fn owner_storage_charge(&self) -> crate::RangeSurfaceCharge {
        crate::RangeSurfaceCharge {
            bytes: std::mem::size_of::<Self>()
                + (self.resident.capacity() - self.resident.len())
                    * std::mem::size_of::<ObjectPage>()
                + self.pending.capacity() * std::mem::size_of::<ObjectRequestKey>()
                + self.cancelled.capacity() * std::mem::size_of::<ObjectRequestKey>(),
            items: 1
                + (self.resident.capacity() - self.resident.len())
                + self.pending.capacity()
                + self.cancelled.capacity(),
        }
    }

    /// Creates an empty projection for one exact source and presentation generation.
    pub fn new(
        binding: RangeBinding,
        presentation_generation: PresentationGeneration,
        limits: ObjectResidencyLimits,
    ) -> Self {
        Self {
            binding,
            presentation_generation,
            limits,
            resident: VecDeque::with_capacity(limits.max_resident_pages()),
            pending: VecDeque::with_capacity(limits.max_pending_requests()),
            cancelled: VecDeque::with_capacity(limits.max_pending_requests()),
            highest_request: None,
            resident_bytes: 0,
            resident_objects: 0,
            resident_presentation_bytes: 0,
            pending_bytes: 0,
            pending_objects: 0,
            #[cfg(test)]
            force_next_admission_limit: std::cell::Cell::new(false),
        }
    }

    #[cfg(test)]
    pub(crate) fn force_next_admission_limit(&self) {
        self.force_next_admission_limit.set(true);
    }

    /// Returns the exact current text-source binding.
    pub const fn binding(&self) -> RangeBinding {
        self.binding
    }

    /// Returns the current immutable presentation generation.
    pub const fn presentation_generation(&self) -> PresentationGeneration {
        self.presentation_generation
    }

    /// Returns configured hard limits.
    pub const fn limits(&self) -> ObjectResidencyLimits {
        self.limits
    }

    /// Returns exact current retained and reserved counts.
    pub fn counts(&self) -> ObjectResidencyCounts {
        ObjectResidencyCounts {
            resident_pages: self.resident.len(),
            resident_objects: self.resident_objects,
            resident_bytes: self.resident_bytes,
            resident_presentation_bytes: self.resident_presentation_bytes,
            pending_requests: self.pending.len(),
            pending_objects: self.pending_objects,
            pending_bytes: self.pending_bytes,
        }
    }

    /// Returns resident bounded pages without constructing a combined object collection.
    pub fn resident_pages(&self) -> impl ExactSizeIterator<Item = &ObjectPage> {
        self.resident.iter()
    }

    pub(crate) fn resident_page_iter(&self) -> std::collections::vec_deque::Iter<'_, ObjectPage> {
        self.resident.iter()
    }

    /// Projects the exact resident order after an optional prepared MRU touch.
    pub(crate) fn resident_pages_after_touch(
        &self,
        touched: Option<ObjectPageId>,
    ) -> TouchedObjectPageIter<'_> {
        let touched_page = touched.map(|id| {
            self.peek_page_by_id(id)
                .expect("prepared resident object-page touch remains valid")
        });
        TouchedObjectPageIter {
            resident: self.resident.iter(),
            touched: touched_page,
            touched_id: touched,
            remaining: self.resident.len(),
        }
    }

    /// Returns one resident page by exact payload identity and marks it recent.
    pub fn page_by_id(&mut self, id: ObjectPageId) -> Option<&ObjectPage> {
        let index = self.resident.iter().position(|page| page.id() == id)?;
        let page = self.resident.remove(index).expect("resident index exists");
        self.resident.push_back(page);
        self.resident.back()
    }

    /// Borrows one exact resident object page without changing its recency.
    pub(crate) fn peek_page_by_id(&self, id: ObjectPageId) -> Option<&ObjectPage> {
        self.resident.iter().find(|page| page.id() == id)
    }

    /// Commits a previously prepared exact-page touch without allocation.
    pub(crate) fn commit_page_touch(&mut self, id: ObjectPageId) {
        let index = self
            .resident
            .iter()
            .position(|page| page.id() == id)
            .expect("prepared resident object-page touch remains valid");
        let page = self.resident.remove(index).expect("resident index exists");
        self.resident.push_back(page);
    }

    /// Looks up one exact object cursor within the bounded resident window.
    pub fn object_by_cursor(&self, cursor: crate::ObjectCursor) -> Option<&InlineObjectFact> {
        self.resident
            .iter()
            .flat_map(|page| page.objects())
            .find(|object| object.cursor() == cursor)
    }

    /// Proves one exact adjacent-object gap from the bounded admitted object projection.
    pub(crate) fn prove_position_gap(&self, position: SourcePosition) -> Option<ObjectPageId> {
        self.resident
            .iter()
            .find(|page| page_proves_gap(page, position))
            .map(ObjectPage::id)
    }

    /// Returns exact in-flight request keys.
    pub fn pending_requests(&self) -> impl ExactSizeIterator<Item = ObjectRequestKey> + '_ {
        self.pending.iter().copied()
    }

    /// Moves every resident page into a coherent publication candidate without cloning.
    pub fn take_resident_pages(&mut self) -> Vec<ObjectPage> {
        self.resident_bytes = 0;
        self.resident_objects = 0;
        self.resident_presentation_bytes = 0;
        self.resident.drain(..).collect()
    }

    pub(crate) fn take_resident_pages_into(
        &mut self,
        mut destination: Vec<ObjectPage>,
    ) -> Vec<ObjectPage> {
        debug_assert!(destination.capacity() >= self.resident.len());
        self.resident_bytes = 0;
        self.resident_objects = 0;
        self.resident_presentation_bytes = 0;
        while let Some(page) = self.resident.pop_front() {
            destination.push(page);
        }
        destination
    }

    /// Registers a typed demand, coalescing only an exact resident or pending envelope.
    pub fn demand(
        &mut self,
        id: ObjectRequestId,
        purpose: ObjectPurpose,
        demand: ObjectDemandEnvelope,
    ) -> Result<ObjectDemand, ObjectDemandError> {
        self.validate_demand_extent(demand)
            .map_err(ObjectDemandError::Malformed)?;
        let key = ObjectRequestKey::new(
            id,
            self.binding.binding(),
            self.binding.revision(),
            self.presentation_generation,
            purpose,
            demand,
        )
        .map_err(ObjectDemandError::Malformed)?;
        if let Some(page) = self
            .resident
            .iter()
            .find(|page| page.key().purpose() == purpose && page.key().demand() == demand)
        {
            return Ok(ObjectDemand::Resident(page.id()));
        }
        if self.highest_request.is_some_and(|highest| id <= highest) {
            return Err(ObjectDemandError::RequestIdInUse(id));
        }
        if let Some(existing) = self
            .pending
            .iter()
            .copied()
            .find(|pending| pending.purpose() == purpose && pending.demand() == demand)
        {
            return Ok(ObjectDemand::Coalesced(existing));
        }
        if self.pending.len() == self.limits.max_pending_requests() {
            return Err(ObjectDemandError::LimitExceeded(
                ObjectResidencyLimitKind::PendingRequests,
            ));
        }
        let pending_objects = self
            .pending_objects
            .checked_add(demand.max_objects())
            .filter(|count| *count <= self.limits.max_pending_objects())
            .ok_or(ObjectDemandError::LimitExceeded(
                ObjectResidencyLimitKind::PendingObjects,
            ))?;
        let pending_bytes = self
            .pending_bytes
            .checked_add(demand.max_retained_bytes())
            .filter(|bytes| *bytes <= self.limits.max_pending_bytes())
            .ok_or(ObjectDemandError::LimitExceeded(
                ObjectResidencyLimitKind::PendingBytes,
            ))?;
        self.pending.push_back(key);
        self.highest_request = Some(id);
        self.pending_objects = pending_objects;
        self.pending_bytes = pending_bytes;
        Ok(ObjectDemand::Requested(ObjectRequest::new(key)))
    }

    /// Prepares one demand against caller-projected resident pages and pending retirement.
    pub(crate) fn prepare_demand_after_retirement_from<'a>(
        &self,
        id: ObjectRequestId,
        purpose: ObjectPurpose,
        demand: ObjectDemandEnvelope,
        retired: &[ObjectRequestKey],
        residents: impl Iterator<Item = &'a ObjectPage>,
    ) -> Result<PreparedObjectDemand, ObjectDemandError> {
        self.validate_demand_extent(demand)
            .map_err(ObjectDemandError::Malformed)?;
        let key = ObjectRequestKey::new(
            id,
            self.binding.binding(),
            self.binding.revision(),
            self.presentation_generation,
            purpose,
            demand,
        )
        .map_err(ObjectDemandError::Malformed)?;
        let retired: Vec<_> = self
            .pending
            .iter()
            .copied()
            .filter(|pending| retired.contains(pending))
            .collect();
        if let Some(page) = residents
            .into_iter()
            .find(|page| page.key().purpose() == purpose && page.key().demand() == demand)
        {
            return Ok(PreparedObjectDemand {
                retired,
                outcome: ObjectDemand::Resident(page.id()),
            });
        }
        if self.highest_request.is_some_and(|highest| id <= highest) {
            return Err(ObjectDemandError::RequestIdInUse(id));
        }
        if let Some(existing) = self.pending.iter().copied().find(|pending| {
            !retired.contains(pending) && pending.purpose() == purpose && pending.demand() == demand
        }) {
            return Ok(PreparedObjectDemand {
                retired,
                outcome: ObjectDemand::Coalesced(existing),
            });
        }
        let retired_objects = retired.iter().fold(0usize, |objects, pending| {
            objects.saturating_add(pending.demand().max_objects())
        });
        let retired_bytes = retired.iter().fold(0usize, |bytes, pending| {
            bytes.saturating_add(pending.demand().max_retained_bytes())
        });
        if self.pending.len().saturating_sub(retired.len()) >= self.limits.max_pending_requests() {
            return Err(ObjectDemandError::LimitExceeded(
                ObjectResidencyLimitKind::PendingRequests,
            ));
        }
        self.pending_objects
            .checked_sub(retired_objects)
            .and_then(|objects| objects.checked_add(demand.max_objects()))
            .filter(|objects| *objects <= self.limits.max_pending_objects())
            .ok_or(ObjectDemandError::LimitExceeded(
                ObjectResidencyLimitKind::PendingObjects,
            ))?;
        self.pending_bytes
            .checked_sub(retired_bytes)
            .and_then(|bytes| bytes.checked_add(demand.max_retained_bytes()))
            .filter(|bytes| *bytes <= self.limits.max_pending_bytes())
            .ok_or(ObjectDemandError::LimitExceeded(
                ObjectResidencyLimitKind::PendingBytes,
            ))?;
        Ok(PreparedObjectDemand {
            retired,
            outcome: ObjectDemand::Requested(ObjectRequest::new(key)),
        })
    }

    /// Commits a prepared demand without validation, scanning, or allocation.
    pub(crate) fn commit_prepared_demand(
        &mut self,
        prepared: PreparedObjectDemand,
    ) -> ObjectDemand {
        for key in &prepared.retired {
            if let Some(index) = self.pending.iter().position(|pending| pending == key) {
                self.remove_pending(index);
                self.remember_cancelled(*key);
            }
        }
        if let ObjectDemand::Requested(request) = prepared.outcome {
            let key = request.key();
            self.pending.push_back(key);
            self.highest_request = Some(key.id());
            self.pending_objects = self
                .pending_objects
                .saturating_add(key.demand().max_objects());
            self.pending_bytes = self
                .pending_bytes
                .saturating_add(key.demand().max_retained_bytes());
        }
        prepared.outcome
    }

    /// Admits a page only for its exact pending key, current presentation generation, and exact
    /// text-owned UTF-8 scalar-boundary proofs.
    pub fn admit(
        &mut self,
        page: ObjectPage,
        anchor_proofs: ObjectAnchorProofs,
    ) -> Result<ObjectPageAdmission, ObjectPageAdmissionError> {
        let prepared = self.prepare_admit(page, anchor_proofs)?;
        Ok(self.commit_prepared_admit(prepared))
    }

    /// Prepares one object-page admission without mutating any owner state.
    pub(crate) fn prepare_admit(
        &self,
        page: ObjectPage,
        anchor_proofs: ObjectAnchorProofs,
    ) -> Result<PreparedObjectPageAdmission, ObjectPageAdmissionError> {
        let key = page.key();
        if !self.is_current(key) {
            return Err(ObjectPageAdmissionError::Stale(key));
        }
        #[cfg(test)]
        if self.force_next_admission_limit.replace(false) {
            return Err(ObjectPageAdmissionError::LimitExceeded(
                ObjectResidencyLimitKind::ResidentBytes,
            ));
        }
        let Some(pending_index) = self.pending.iter().position(|pending| *pending == key) else {
            return if self.cancelled.contains(&key) {
                Err(ObjectPageAdmissionError::Cancelled(key))
            } else {
                Err(ObjectPageAdmissionError::Unavailable(key))
            };
        };
        self.validate_page(&page, &anchor_proofs)
            .map_err(ObjectPageAdmissionError::Malformed)?;
        let charge = page.retained_charge();
        if charge.bytes() > self.limits.max_resident_bytes() {
            return Err(ObjectPageAdmissionError::LimitExceeded(
                ObjectResidencyLimitKind::ResidentBytes,
            ));
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
        let mut disposition = vec![ResidentDisposition::Retain; self.resident.len()];
        let mut surviving_pages = self.resident.len();
        let mut surviving_bytes = self.resident_bytes;
        let mut surviving_objects = self.resident_objects;
        let mut surviving_presentation_bytes = self.resident_presentation_bytes;
        if let Some(index) = reconciled_index {
            disposition[index] = ResidentDisposition::Evict;
            let existing = self.resident[index].retained_charge();
            surviving_pages =
                surviving_pages
                    .checked_sub(1)
                    .ok_or(ObjectPageAdmissionError::LimitExceeded(
                        ObjectResidencyLimitKind::ResidentPages,
                    ))?;
            surviving_bytes = surviving_bytes.checked_sub(existing.bytes()).ok_or(
                ObjectPageAdmissionError::LimitExceeded(ObjectResidencyLimitKind::ResidentBytes),
            )?;
            surviving_objects = surviving_objects.checked_sub(existing.objects()).ok_or(
                ObjectPageAdmissionError::LimitExceeded(ObjectResidencyLimitKind::ResidentObjects),
            )?;
            surviving_presentation_bytes = surviving_presentation_bytes
                .checked_sub(existing.presentation_bytes())
                .ok_or(ObjectPageAdmissionError::LimitExceeded(
                    ObjectResidencyLimitKind::ResidentPresentationBytes,
                ))?;
        }

        let mut evicted_pages: usize = 0;
        let mut evicted_objects: usize = 0;
        for (resident_index, existing) in self.resident.iter().enumerate() {
            if disposition[resident_index] == ResidentDisposition::Evict {
                continue;
            }
            let repeated_payload = existing
                .objects()
                .iter()
                .any(|left| page.objects().iter().any(|right| left.id() == right.id()));
            if repeated_payload {
                disposition[resident_index] = ResidentDisposition::Evict;
                let existing_charge = existing.retained_charge();
                surviving_pages = surviving_pages.checked_sub(1).ok_or(
                    ObjectPageAdmissionError::LimitExceeded(
                        ObjectResidencyLimitKind::ResidentPages,
                    ),
                )?;
                surviving_bytes = surviving_bytes.checked_sub(existing_charge.bytes()).ok_or(
                    ObjectPageAdmissionError::LimitExceeded(
                        ObjectResidencyLimitKind::ResidentBytes,
                    ),
                )?;
                surviving_objects = surviving_objects
                    .checked_sub(existing_charge.objects())
                    .ok_or(ObjectPageAdmissionError::LimitExceeded(
                        ObjectResidencyLimitKind::ResidentObjects,
                    ))?;
                surviving_presentation_bytes = surviving_presentation_bytes
                    .checked_sub(existing_charge.presentation_bytes())
                    .ok_or(ObjectPageAdmissionError::LimitExceeded(
                        ObjectResidencyLimitKind::ResidentPresentationBytes,
                    ))?;
                evicted_objects = evicted_objects
                    .checked_add(existing_charge.objects())
                    .ok_or(ObjectPageAdmissionError::LimitExceeded(
                        ObjectResidencyLimitKind::ResidentObjects,
                    ))?;
                evicted_pages =
                    evicted_pages
                        .checked_add(1)
                        .ok_or(ObjectPageAdmissionError::LimitExceeded(
                            ObjectResidencyLimitKind::ResidentPages,
                        ))?;
            }
        }

        let mut resident_index = 0;
        while surviving_pages >= self.limits.max_resident_pages()
            || surviving_objects
                .checked_add(charge.objects())
                .is_none_or(|objects| objects > self.limits.max_resident_objects())
            || surviving_bytes
                .checked_add(charge.bytes())
                .is_none_or(|bytes| bytes > self.limits.max_resident_bytes())
            || surviving_presentation_bytes
                .checked_add(charge.presentation_bytes())
                .is_none_or(|bytes| bytes > self.limits.max_resident_presentation_bytes())
        {
            while disposition[resident_index] == ResidentDisposition::Evict {
                resident_index += 1;
            }
            disposition[resident_index] = ResidentDisposition::Evict;
            let existing = self.resident[resident_index].retained_charge();
            surviving_pages =
                surviving_pages
                    .checked_sub(1)
                    .ok_or(ObjectPageAdmissionError::LimitExceeded(
                        ObjectResidencyLimitKind::ResidentPages,
                    ))?;
            surviving_bytes = surviving_bytes.checked_sub(existing.bytes()).ok_or(
                ObjectPageAdmissionError::LimitExceeded(ObjectResidencyLimitKind::ResidentBytes),
            )?;
            surviving_objects = surviving_objects.checked_sub(existing.objects()).ok_or(
                ObjectPageAdmissionError::LimitExceeded(ObjectResidencyLimitKind::ResidentObjects),
            )?;
            surviving_presentation_bytes = surviving_presentation_bytes
                .checked_sub(existing.presentation_bytes())
                .ok_or(ObjectPageAdmissionError::LimitExceeded(
                    ObjectResidencyLimitKind::ResidentPresentationBytes,
                ))?;
            evicted_objects = evicted_objects.checked_add(existing.objects()).ok_or(
                ObjectPageAdmissionError::LimitExceeded(ObjectResidencyLimitKind::ResidentObjects),
            )?;
            evicted_pages =
                evicted_pages
                    .checked_add(1)
                    .ok_or(ObjectPageAdmissionError::LimitExceeded(
                        ObjectResidencyLimitKind::ResidentPages,
                    ))?;
            resident_index += 1;
        }

        let resident_bytes = surviving_bytes.checked_add(charge.bytes()).ok_or(
            ObjectPageAdmissionError::LimitExceeded(ObjectResidencyLimitKind::ResidentBytes),
        )?;
        let resident_objects = surviving_objects.checked_add(charge.objects()).ok_or(
            ObjectPageAdmissionError::LimitExceeded(ObjectResidencyLimitKind::ResidentObjects),
        )?;
        let resident_presentation_bytes = surviving_presentation_bytes
            .checked_add(charge.presentation_bytes())
            .ok_or(ObjectPageAdmissionError::LimitExceeded(
                ObjectResidencyLimitKind::ResidentPresentationBytes,
            ))?;
        let _projected = self
            .resident
            .iter()
            .zip(disposition.iter())
            .filter(|(_, disposition)| **disposition == ResidentDisposition::Retain)
            .try_fold((0usize, 0usize), |(bytes, items), (resident, _)| {
                Some((
                    bytes.checked_add(resident.retained_charge().bytes())?,
                    items.checked_add(
                        resident
                            .retained_charge()
                            .allocated_items()
                            .checked_add(1)?,
                    )?,
                ))
            })
            .and_then(|(bytes, items)| {
                Some((
                    bytes.checked_add(charge.bytes())?,
                    items.checked_add(charge.allocated_items().checked_add(1)?)?,
                ))
            })
            .ok_or(ObjectPageAdmissionError::LimitExceeded(
                ObjectResidencyLimitKind::ResidentBytes,
            ))?;
        let projected_pages =
            surviving_pages
                .checked_add(1)
                .ok_or(ObjectPageAdmissionError::LimitExceeded(
                    ObjectResidencyLimitKind::ResidentPages,
                ))?;
        let destination = VecDeque::with_capacity(projected_pages);
        let retained_bytes = charge
            .bytes()
            .checked_add(
                disposition
                    .capacity()
                    .checked_mul(std::mem::size_of::<ResidentDisposition>())
                    .ok_or(ObjectPageAdmissionError::LimitExceeded(
                        ObjectResidencyLimitKind::ResidentBytes,
                    ))?,
            )
            .and_then(|bytes| {
                bytes.checked_add(
                    destination
                        .capacity()
                        .checked_mul(std::mem::size_of::<ObjectPage>())?,
                )
            })
            .ok_or(ObjectPageAdmissionError::LimitExceeded(
                ObjectResidencyLimitKind::ResidentBytes,
            ))?;
        let retained_items = charge
            .allocated_items()
            .checked_add(1)
            .and_then(|items| items.checked_add(disposition.capacity()))
            .and_then(|items| items.checked_add(destination.capacity()))
            .ok_or(ObjectPageAdmissionError::LimitExceeded(
                ObjectResidencyLimitKind::ResidentObjects,
            ))?;
        let page_id = page.id();
        let admission = if reconciled_index.is_some() {
            ObjectPageAdmission::Reconciled {
                page: page_id,
                evicted_pages,
                evicted_objects,
            }
        } else {
            ObjectPageAdmission::Admitted {
                page: page_id,
                evicted_pages,
                evicted_objects,
            }
        };
        Ok(PreparedObjectPageAdmission {
            page,
            pending_index,
            disposition,
            destination,
            admission,
            resident_bytes,
            resident_objects,
            resident_presentation_bytes,
            projected_pages,
            retained_bytes,
            retained_items,
        })
    }

    /// Commits a prepared object-page admission without allocation or revalidation.
    pub(crate) fn commit_prepared_admit(
        &mut self,
        prepared: PreparedObjectPageAdmission,
    ) -> ObjectPageAdmission {
        let PreparedObjectPageAdmission {
            page,
            pending_index,
            disposition,
            mut destination,
            admission,
            resident_bytes,
            resident_objects,
            resident_presentation_bytes,
            ..
        } = prepared;
        self.remove_pending(pending_index);
        for disposition in disposition {
            let resident = self
                .resident
                .pop_front()
                .expect("prepared disposition matches resident object-page count");
            if disposition == ResidentDisposition::Retain {
                destination.push_back(resident);
            }
        }
        destination.push_back(page);
        self.resident = destination;
        self.resident_bytes = resident_bytes;
        self.resident_objects = resident_objects;
        self.resident_presentation_bytes = resident_presentation_bytes;
        admission
    }

    /// Settles one exact request without a page and releases its reservations.
    pub fn settle(
        &mut self,
        key: ObjectRequestKey,
        failure: ObjectPageFailure,
    ) -> ObjectPageSettlement {
        if !self.is_current(key) {
            return ObjectPageSettlement::Stale;
        }
        let Some(index) = self.pending.iter().position(|pending| *pending == key) else {
            return if self.cancelled.contains(&key) {
                ObjectPageSettlement::AlreadyCancelled
            } else {
                ObjectPageSettlement::Unavailable
            };
        };
        self.remove_pending(index);
        if failure == ObjectPageFailure::Cancelled {
            self.remember_cancelled(key);
        }
        ObjectPageSettlement::Settled(failure)
    }

    /// Cancels one exact pending request and releases all of its reservations.
    pub fn cancel(&mut self, key: ObjectRequestKey) -> ObjectPageSettlement {
        self.settle(key, ObjectPageFailure::Cancelled)
    }

    /// Rebinds the source or presentation generation and releases all local capacity.
    ///
    /// Returned keys identify exact host requests that the caller must cancel.
    pub fn rebind(
        &mut self,
        binding: RangeBinding,
        presentation_generation: PresentationGeneration,
    ) -> Vec<ObjectRequestKey> {
        let request_generation_changed = binding.binding() != self.binding.binding()
            || binding.revision() != self.binding.revision()
            || presentation_generation != self.presentation_generation;
        let cancelled = self.pending.iter().copied().collect();
        self.binding = binding;
        self.presentation_generation = presentation_generation;
        self.clear_capacity();
        self.cancelled.clear();
        if request_generation_changed {
            self.highest_request = None;
        }
        cancelled
    }

    pub(crate) fn prepare_rebind(
        &self,
        binding: RangeBinding,
        presentation_generation: PresentationGeneration,
    ) -> PreparedObjectRebind {
        PreparedObjectRebind {
            binding,
            presentation_generation,
            cancelled: self.pending.iter().copied().collect(),
        }
    }

    pub(crate) fn commit_prepared_rebind(
        &mut self,
        prepared: PreparedObjectRebind,
    ) -> Vec<ObjectRequestKey> {
        let generation_changed = prepared.binding.binding() != self.binding.binding()
            || prepared.binding.revision() != self.binding.revision()
            || prepared.presentation_generation != self.presentation_generation;
        self.binding = prepared.binding;
        self.presentation_generation = prepared.presentation_generation;
        self.clear_capacity();
        self.cancelled.clear();
        if generation_changed {
            self.highest_request = None;
        }
        prepared.cancelled
    }

    /// Releases every resident page and pending request without installing another binding.
    pub fn dispose(&mut self) -> Vec<ObjectRequestKey> {
        let cancelled = self.pending.iter().copied().collect();
        self.resident = VecDeque::new();
        self.pending = VecDeque::new();
        self.cancelled = VecDeque::new();
        self.resident_bytes = 0;
        self.resident_objects = 0;
        self.resident_presentation_bytes = 0;
        self.pending_bytes = 0;
        self.pending_objects = 0;
        cancelled
    }

    /// Explicitly evicts one bounded resident page and releases exact retained capacity.
    pub fn evict(&mut self, page: ObjectPageId) -> bool {
        let Some(index) = self
            .resident
            .iter()
            .position(|resident| resident.id() == page)
        else {
            return false;
        };
        self.remove_resident(index);
        true
    }

    fn validate_demand_extent(
        &self,
        demand: ObjectDemandEnvelope,
    ) -> Result<(), ObjectContractError> {
        demand.validate_local()?;
        let extent = self.binding.extent();
        let valid = match demand {
            ObjectDemandEnvelope::Range { range, cursor, .. } => {
                extent.check_byte_range(range).is_ok()
                    && cursor.is_none_or(|cursor| range.contains_offset(cursor.anchor()))
            }
            ObjectDemandEnvelope::Anchor { anchor, cursor, .. } => {
                let point = ByteRange::new(anchor, anchor).expect("equal offsets are ordered");
                extent.check_byte_range(point).is_ok()
                    && cursor.is_none_or(|cursor| cursor.anchor() == anchor)
            }
        };
        if valid {
            Ok(())
        } else {
            Err(ObjectContractError::DemandOutsideExtent)
        }
    }

    fn validate_page(
        &self,
        page: &ObjectPage,
        anchor_proofs: &ObjectAnchorProofs,
    ) -> Result<(), ObjectContractError> {
        self.validate_demand_extent(page.key().demand())?;
        if anchor_proofs.range_binding() != self.binding
            || anchor_proofs.page() != page.id()
            || anchor_proofs.key() != page.key()
        {
            let anchor = page.objects().first().map_or_else(
                || match page.key().demand() {
                    ObjectDemandEnvelope::Range { range, .. } => range.start(),
                    ObjectDemandEnvelope::Anchor { anchor, .. } => anchor,
                },
                |object| object.anchor(),
            );
            return Err(ObjectContractError::ScalarBoundaryProofMismatch { anchor });
        }
        if let Some(resident) = self
            .resident
            .iter()
            .find(|resident| resident.id() == page.id())
        {
            if !page.reconciles_with(resident) {
                return Err(ObjectContractError::ConflictingPageIdentity { page: page.id() });
            }
        }
        let mut proofs = anchor_proofs.proofs().iter().copied();
        let mut previous_anchor = None;
        for object in page.objects() {
            let point = ByteRange::new(object.anchor(), object.anchor())
                .expect("equal offsets are ordered");
            if self.binding.extent().check_byte_range(point).is_err() {
                return Err(ObjectContractError::DemandOutsideExtent);
            }
            if previous_anchor != Some(object.anchor()) {
                let proof =
                    proofs
                        .next()
                        .ok_or(ObjectContractError::ScalarBoundaryProofMismatch {
                            anchor: object.anchor(),
                        })?;
                if proof.range_binding() != self.binding
                    || proof.binding() != page.key().binding()
                    || proof.revision() != page.key().revision()
                    || proof.offset() != object.anchor()
                {
                    return Err(ObjectContractError::ScalarBoundaryProofMismatch {
                        anchor: object.anchor(),
                    });
                }
                previous_anchor = Some(object.anchor());
            }
            for resident in self.resident.iter().flat_map(|page| page.objects()) {
                if object.id() == resident.id() && !object.reconciles_with(resident) {
                    return Err(ObjectContractError::ConflictingObjectIdentity {
                        object: object.id(),
                    });
                }
                if object.id() != resident.id()
                    && object.anchor() == resident.anchor()
                    && object.order() == resident.order()
                {
                    return Err(ObjectContractError::DuplicateObjectOrder {
                        anchor: object.anchor(),
                        order: object.order(),
                    });
                }
            }
        }
        if let Some(proof) = proofs.next() {
            return Err(ObjectContractError::ScalarBoundaryProofMismatch {
                anchor: proof.offset(),
            });
        }
        Ok(())
    }

    fn is_current(&self, key: ObjectRequestKey) -> bool {
        key.binding() == self.binding.binding()
            && key.revision() == self.binding.revision()
            && key.presentation_generation() == self.presentation_generation
    }

    fn remove_pending(&mut self, index: usize) {
        let key = self.pending.remove(index).expect("pending index exists");
        self.pending_objects -= key.demand().max_objects();
        self.pending_bytes -= key.demand().max_retained_bytes();
    }

    fn remove_resident(&mut self, index: usize) -> usize {
        let page = self.resident.remove(index).expect("resident index exists");
        let charge = page.retained_charge();
        self.resident_bytes -= charge.bytes();
        self.resident_objects -= charge.objects();
        self.resident_presentation_bytes -= charge.presentation_bytes();
        charge.objects()
    }

    fn remember_cancelled(&mut self, key: ObjectRequestKey) {
        if self.cancelled.len() == self.limits.max_pending_requests() {
            self.cancelled.pop_front();
        }
        self.cancelled.push_back(key);
    }

    fn clear_capacity(&mut self) {
        self.resident.clear();
        self.pending.clear();
        self.resident_bytes = 0;
        self.resident_objects = 0;
        self.resident_presentation_bytes = 0;
        self.pending_bytes = 0;
        self.pending_objects = 0;
    }
}
