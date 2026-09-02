//! Fixed-capacity resident-page and pending-request projection.

use std::collections::VecDeque;

use crate::range_source::{
    ByteOffset, ByteRange, ObjectPage, PageDemandEnvelope, PageFailure, PageId, PagePurpose,
    PageRequest, PageRequestId, PageRequestKey, RangeBinding, RangeContractError, RangePage,
};

mod types;

pub use types::*;

/// Fixed-capacity projection over one exact host-owned source revision.
///
/// This type stores independent bounded pages. It provides no operation that
/// concatenates them or materializes the logical whole source.
#[derive(Debug)]
pub struct RangeResidency {
    binding: RangeBinding,
    limits: ResidencyLimits,
    resident: VecDeque<RangePage>,
    pending: VecDeque<PageRequestKey>,
    cancelled: VecDeque<PageRequestKey>,
    highest_request: Option<PageRequestId>,
    resident_bytes: usize,
    pending_bytes: u64,
    #[cfg(test)]
    force_next_admission_limit: std::cell::Cell<bool>,
}

#[derive(Debug)]
pub(crate) struct PreparedResidencyRebind {
    binding: RangeBinding,
    cancelled: Vec<PageRequestKey>,
    successor: Option<PageRequest>,
}

impl PreparedResidencyRebind {
    pub(crate) fn cancelled(&self) -> &[PageRequestKey] {
        &self.cancelled
    }

    pub(crate) fn retained_bytes(&self) -> usize {
        self.cancelled
            .capacity()
            .saturating_mul(std::mem::size_of::<PageRequestKey>())
    }

    pub(crate) fn retained_items(&self) -> usize {
        1usize
            .saturating_add(self.cancelled.capacity())
            .saturating_add(usize::from(self.successor.is_some()))
    }

    pub(crate) const fn successor(&self) -> Option<PageRequest> {
        self.successor
    }
}

/// Bounded post-retirement page-demand delta for one widget transition.
#[derive(Debug)]
pub(crate) struct PreparedPageDemand {
    retired: Vec<PageRequestKey>,
    outcome: PageDemand,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ResidentDisposition {
    Retain,
    Evict,
}

#[derive(Clone)]
pub(crate) struct ProjectedRangePageIter<'a> {
    resident: std::collections::vec_deque::Iter<'a, RangePage>,
    disposition: std::slice::Iter<'a, ResidentDisposition>,
    inbound: Option<&'a RangePage>,
    remaining: usize,
}

impl<'a> Iterator for ProjectedRangePageIter<'a> {
    type Item = &'a RangePage;

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

impl ExactSizeIterator for ProjectedRangePageIter<'_> {}
impl std::iter::FusedIterator for ProjectedRangePageIter<'_> {}

#[derive(Clone)]
pub(crate) struct TouchedRangePageIter<'a> {
    resident: std::collections::vec_deque::Iter<'a, RangePage>,
    touched: Option<&'a RangePage>,
    touched_id: Option<PageId>,
    remaining: usize,
}

impl<'a> Iterator for TouchedRangePageIter<'a> {
    type Item = &'a RangePage;

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

impl ExactSizeIterator for TouchedRangePageIter<'_> {}
impl std::iter::FusedIterator for TouchedRangePageIter<'_> {}

/// Fully validated, allocation-complete admission of one text page.
///
/// The disposition is parallel to the current resident order. The empty destination owns enough
/// storage for every retained page plus the inbound page, so commit only moves values.
#[derive(Debug)]
pub(crate) struct PreparedRangePageAdmission {
    page: RangePage,
    pending_index: usize,
    disposition: Vec<ResidentDisposition>,
    destination: VecDeque<RangePage>,
    admission: PageAdmission,
    resident_bytes: usize,
    projected_pages: usize,
    retained_bytes: usize,
    retained_items: usize,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct PreparedRangePageSettlement {
    key: PageRequestKey,
    pending_index: usize,
    failure: PageFailure,
}

impl PreparedRangePageAdmission {
    pub(crate) const fn page(&self) -> &RangePage {
        &self.page
    }

    pub(crate) const fn retained_bytes(&self) -> usize {
        self.retained_bytes
    }

    pub(crate) const fn retained_items(&self) -> usize {
        self.retained_items
    }

    pub(crate) fn into_page(self) -> RangePage {
        self.page
    }

    pub(crate) fn projected_resident_pages<'a>(
        &'a self,
        residency: &'a RangeResidency,
    ) -> ProjectedRangePageIter<'a> {
        ProjectedRangePageIter {
            resident: residency.resident.iter(),
            disposition: self.disposition.iter(),
            inbound: Some(&self.page),
            remaining: self.projected_pages,
        }
    }
}

impl PreparedPageDemand {
    pub(crate) const fn outcome(&self) -> PageDemand {
        self.outcome
    }

    pub(crate) fn retained_bytes(&self) -> usize {
        self.retired
            .capacity()
            .saturating_mul(std::mem::size_of::<PageRequestKey>())
            .saturating_add(match self.outcome {
                PageDemand::Requested(request) => {
                    usize::try_from(request.key().demand().max_payload_bytes())
                        .expect("validated page demand fits usize")
                }
                _ => 0,
            })
    }

    pub(crate) fn retained_items(&self) -> usize {
        1usize
            .saturating_add(self.retired.capacity())
            .saturating_add(usize::from(matches!(
                self.outcome,
                PageDemand::Requested(_)
            )))
    }
}

impl RangeResidency {
    pub(crate) fn checked_initial_owner_storage_charge(
        limits: ResidencyLimits,
    ) -> Option<crate::RangeSurfaceCharge> {
        Some(crate::RangeSurfaceCharge {
            bytes: std::mem::size_of::<Self>()
                .checked_add(
                    limits
                        .max_resident_pages()
                        .checked_mul(std::mem::size_of::<RangePage>())?,
                )?
                .checked_add(
                    limits
                        .max_pending_requests()
                        .checked_mul(std::mem::size_of::<PageRequestKey>())?,
                )?
                .checked_add(
                    limits
                        .max_pending_requests()
                        .checked_mul(std::mem::size_of::<PageRequestKey>())?,
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
                    * std::mem::size_of::<RangePage>()
                + self.pending.capacity() * std::mem::size_of::<PageRequestKey>()
                + self.cancelled.capacity() * std::mem::size_of::<PageRequestKey>(),
            items: 1
                + (self.resident.capacity() - self.resident.len())
                + self.pending.capacity()
                + self.cancelled.capacity(),
        }
    }

    /// Creates an empty projection bound to one exact source revision.
    pub fn new(binding: RangeBinding, limits: ResidencyLimits) -> Self {
        Self {
            binding,
            limits,
            resident: VecDeque::with_capacity(limits.max_resident_pages()),
            pending: VecDeque::with_capacity(limits.max_pending_requests()),
            cancelled: VecDeque::with_capacity(limits.max_pending_requests()),
            highest_request: None,
            resident_bytes: 0,
            pending_bytes: 0,
            #[cfg(test)]
            force_next_admission_limit: std::cell::Cell::new(false),
        }
    }

    #[cfg(test)]
    pub(crate) fn force_next_admission_limit(&self) {
        self.force_next_admission_limit.set(true);
    }

    /// Returns the exact current binding, revision, and logical extent.
    pub const fn binding(&self) -> RangeBinding {
        self.binding
    }

    /// Returns the configured hard limits.
    pub const fn limits(&self) -> ResidencyLimits {
        self.limits
    }

    /// Returns exact current capacity counts.
    pub fn counts(&self) -> ResidencyCounts {
        ResidencyCounts {
            resident_pages: self.resident.len(),
            resident_bytes: self.resident_bytes,
            pending_requests: self.pending.len(),
            pending_bytes: self.pending_bytes,
        }
    }

    /// Returns a resident page covering the complete range and marks it recent.
    pub fn page_covering(&mut self, range: ByteRange) -> Option<&RangePage> {
        let index = self
            .resident
            .iter()
            .position(|page| page.range().contains(range))?;
        let page = self.resident.remove(index).expect("resident index exists");
        self.resident.push_back(page);
        self.resident.back()
    }

    /// Returns one exact resident page by payload identity and marks it recent.
    pub(crate) fn page_by_id(&mut self, id: PageId) -> Option<&RangePage> {
        let index = self.resident.iter().position(|page| page.id() == id)?;
        let page = self.resident.remove(index).expect("resident index exists");
        self.resident.push_back(page);
        self.resident.back()
    }

    /// Borrows one exact resident page without changing its recency.
    pub(crate) fn peek_page_by_id(&self, id: PageId) -> Option<&RangePage> {
        self.resident.iter().find(|page| page.id() == id)
    }

    /// Commits a previously prepared exact-page touch without allocation.
    pub(crate) fn commit_page_touch(&mut self, id: PageId) {
        let index = self
            .resident
            .iter()
            .position(|page| page.id() == id)
            .expect("prepared resident text-page touch remains valid");
        let page = self.resident.remove(index).expect("resident index exists");
        self.resident.push_back(page);
    }

    /// Returns resident pages without constructing a combined source value.
    pub fn resident_pages(&self) -> impl ExactSizeIterator<Item = &RangePage> {
        self.resident.iter()
    }

    pub(crate) fn resident_page_iter(&self) -> std::collections::vec_deque::Iter<'_, RangePage> {
        self.resident.iter()
    }

    /// Projects the exact resident order after an optional prepared MRU touch.
    pub(crate) fn resident_pages_after_touch(
        &self,
        touched: Option<PageId>,
    ) -> TouchedRangePageIter<'_> {
        let touched_page = touched.map(|id| {
            self.peek_page_by_id(id)
                .expect("prepared resident text-page touch remains valid")
        });
        TouchedRangePageIter {
            resident: self.resident.iter(),
            touched: touched_page,
            touched_id: touched,
            remaining: self.resident.len(),
        }
    }

    /// Proves one exact UTF-8 scalar boundary from the bounded current text projection.
    ///
    /// Source origin and end are proven by the exact logical extent. Every interior proof comes
    /// from one currently resident, constructor-validated UTF-8 page; missing text is never
    /// guessed, rounded, or delegated to the object source.
    pub fn prove_scalar_boundary(
        &self,
        offset: ByteOffset,
    ) -> Result<ScalarBoundaryProof, ScalarBoundaryProofError> {
        let extent = self.binding.extent().byte_len();
        if offset.get() > extent {
            return Err(ScalarBoundaryProofError::OutsideExtent(offset));
        }
        if offset.get() == 0 || offset.get() == extent {
            return Ok(ScalarBoundaryProof::new(self.binding, offset, None));
        }

        let mut covered = false;
        for page in &self.resident {
            if !page.range().contains_offset(offset) {
                continue;
            }
            covered = true;
            let local = usize::try_from(offset.get() - page.range().start().get())
                .map_err(|_| ScalarBoundaryProofError::Unavailable(offset))?;
            if page.text().is_char_boundary(local) {
                return Ok(ScalarBoundaryProof::new(
                    self.binding,
                    offset,
                    Some(page.id()),
                ));
            }
        }

        Err(if covered {
            ScalarBoundaryProofError::NotScalarBoundary(offset)
        } else {
            ScalarBoundaryProofError::Unavailable(offset)
        })
    }

    /// Issues exact text-owned scalar-boundary proofs for one bounded object page.
    ///
    /// `expected_binding` is the admitting object projection's complete binding, revision, and
    /// logical extent. The returned opaque batch is tied to that binding, the page identity, and
    /// request key and contains only the page's strictly deduplicated anchors. No proof registry
    /// or whole-source value is retained.
    pub fn prove_object_page_anchors(
        &self,
        expected_binding: RangeBinding,
        page: &ObjectPage,
    ) -> Result<ObjectAnchorProofs, ObjectAnchorProofError> {
        if expected_binding != self.binding
            || page.key().binding() != expected_binding.binding()
            || page.key().revision() != expected_binding.revision()
        {
            return Err(ObjectAnchorProofError::Stale(page.key()));
        }

        let mut proofs = Vec::new();
        let mut previous = None;
        for object in page.objects() {
            if previous == Some(object.anchor()) {
                continue;
            }
            proofs.push(
                self.prove_scalar_boundary(object.anchor())
                    .map_err(ObjectAnchorProofError::Scalar)?,
            );
            previous = Some(object.anchor());
        }
        if proofs
            .iter()
            .any(|proof| proof.range_binding() != expected_binding)
        {
            return Err(ObjectAnchorProofError::Stale(page.key()));
        }
        Ok(ObjectAnchorProofs::new(
            expected_binding,
            page.id(),
            page.key(),
            proofs,
        ))
    }

    /// Moves every resident page into one publication candidate.
    ///
    /// Pending requests remain owned by this projection. The transfer is clone-free so a caller
    /// can account the exact page graph once while atomically publishing a coherent surface.
    pub fn take_resident_pages(&mut self) -> Vec<RangePage> {
        self.resident_bytes = 0;
        self.resident.drain(..).collect()
    }

    pub(crate) fn take_resident_pages_into(
        &mut self,
        mut destination: Vec<RangePage>,
    ) -> Vec<RangePage> {
        debug_assert!(destination.capacity() >= self.resident.len());
        self.resident_bytes = 0;
        while let Some(page) = self.resident.pop_front() {
            destination.push(page);
        }
        destination
    }

    /// Returns exact in-flight request keys.
    pub fn pending_requests(&self) -> impl ExactSizeIterator<Item = PageRequestKey> + '_ {
        self.pending.iter().copied()
    }

    /// Registers a typed demand, coalescing resident proof or identical pending work.
    pub fn demand(
        &mut self,
        id: PageRequestId,
        purpose: PagePurpose,
        demand: PageDemandEnvelope,
    ) -> Result<PageDemand, PageDemandError> {
        let point = match demand {
            PageDemandEnvelope::Adjacent { anchor, .. } => anchor,
            PageDemandEnvelope::Validation { candidate, .. } => candidate,
        };
        self.binding
            .extent()
            .check_byte_range(ByteRange::new(point, point).expect("equal offsets are ordered"))
            .map_err(PageDemandError::Malformed)?;

        let key = match demand {
            PageDemandEnvelope::Adjacent {
                anchor,
                direction,
                max_payload_bytes,
            } => PageRequestKey::adjacent(
                id,
                self.binding.binding(),
                self.binding.revision(),
                purpose,
                anchor,
                direction,
                max_payload_bytes,
            ),
            PageDemandEnvelope::Validation {
                candidate,
                max_payload_bytes,
            } => PageRequestKey::validation(
                id,
                self.binding.binding(),
                self.binding.revision(),
                purpose,
                candidate,
                max_payload_bytes,
            ),
        }
        .map_err(PageDemandError::Malformed)?;

        if let Some(resident) = self
            .resident
            .iter()
            .find_map(|page| resident_satisfaction(page, demand))
        {
            return Ok(resident);
        }
        if self.highest_request.is_some_and(|highest| id <= highest) {
            return Err(PageDemandError::RequestIdInUse(id));
        }
        if let Some(existing) = self
            .pending
            .iter()
            .copied()
            .find(|key| key.purpose() == purpose && key.demand() == demand)
        {
            return Ok(PageDemand::Coalesced(existing));
        }
        if self.pending.len() == self.limits.max_pending_requests() {
            return Err(PageDemandError::LimitExceeded(
                ResidencyLimitKind::PendingRequests,
            ));
        }
        let new_pending_bytes = self
            .pending_bytes
            .checked_add(demand.max_payload_bytes())
            .ok_or(PageDemandError::LimitExceeded(
                ResidencyLimitKind::PendingBytes,
            ))?;
        if new_pending_bytes > self.limits.max_pending_bytes() {
            return Err(PageDemandError::LimitExceeded(
                ResidencyLimitKind::PendingBytes,
            ));
        }
        self.pending.push_back(key);
        self.highest_request = Some(id);
        self.pending_bytes = new_pending_bytes;
        Ok(PageDemand::Requested(PageRequest::new(key)))
    }

    /// Prepares one demand against a read-only projection in which named requests are retired.
    pub(crate) fn prepare_demand_after_retirement(
        &self,
        id: PageRequestId,
        purpose: PagePurpose,
        demand: PageDemandEnvelope,
        retired: &[PageRequestKey],
    ) -> Result<PreparedPageDemand, PageDemandError> {
        self.prepare_demand_after_retirement_from(
            id,
            purpose,
            demand,
            retired,
            self.resident.iter(),
        )
    }

    /// Prepares one demand against caller-projected resident pages and pending retirement.
    pub(crate) fn prepare_demand_after_retirement_from<'a>(
        &self,
        id: PageRequestId,
        purpose: PagePurpose,
        demand: PageDemandEnvelope,
        retired: &[PageRequestKey],
        residents: impl Iterator<Item = &'a RangePage>,
    ) -> Result<PreparedPageDemand, PageDemandError> {
        let point = match demand {
            PageDemandEnvelope::Adjacent { anchor, .. } => anchor,
            PageDemandEnvelope::Validation { candidate, .. } => candidate,
        };
        self.binding
            .extent()
            .check_byte_range(ByteRange::new(point, point).expect("equal offsets are ordered"))
            .map_err(PageDemandError::Malformed)?;
        let key = match demand {
            PageDemandEnvelope::Adjacent {
                anchor,
                direction,
                max_payload_bytes,
            } => PageRequestKey::adjacent(
                id,
                self.binding.binding(),
                self.binding.revision(),
                purpose,
                anchor,
                direction,
                max_payload_bytes,
            ),
            PageDemandEnvelope::Validation {
                candidate,
                max_payload_bytes,
            } => PageRequestKey::validation(
                id,
                self.binding.binding(),
                self.binding.revision(),
                purpose,
                candidate,
                max_payload_bytes,
            ),
        }
        .map_err(PageDemandError::Malformed)?;
        let retired: Vec<_> = self
            .pending
            .iter()
            .copied()
            .filter(|pending| retired.contains(pending))
            .collect();
        if let Some(resident) = residents
            .into_iter()
            .find_map(|page| resident_satisfaction(page, demand))
        {
            return Ok(PreparedPageDemand {
                retired,
                outcome: resident,
            });
        }
        if self.highest_request.is_some_and(|highest| id <= highest) {
            return Err(PageDemandError::RequestIdInUse(id));
        }
        if let Some(existing) = self.pending.iter().copied().find(|pending| {
            !retired.contains(pending) && pending.purpose() == purpose && pending.demand() == demand
        }) {
            return Ok(PreparedPageDemand {
                retired,
                outcome: PageDemand::Coalesced(existing),
            });
        }
        let retired_bytes = retired.iter().fold(0u64, |bytes, pending| {
            bytes.saturating_add(pending.demand().max_payload_bytes())
        });
        let pending_len = self.pending.len().saturating_sub(retired.len());
        if pending_len >= self.limits.max_pending_requests() {
            return Err(PageDemandError::LimitExceeded(
                ResidencyLimitKind::PendingRequests,
            ));
        }
        let pending_bytes = self
            .pending_bytes
            .checked_sub(retired_bytes)
            .and_then(|bytes| bytes.checked_add(demand.max_payload_bytes()))
            .filter(|bytes| *bytes <= self.limits.max_pending_bytes())
            .ok_or(PageDemandError::LimitExceeded(
                ResidencyLimitKind::PendingBytes,
            ))?;
        let _ = pending_bytes;
        Ok(PreparedPageDemand {
            retired,
            outcome: PageDemand::Requested(PageRequest::new(key)),
        })
    }

    /// Commits a previously prepared demand without validation, scanning, or allocation.
    pub(crate) fn commit_prepared_demand(&mut self, prepared: PreparedPageDemand) -> PageDemand {
        for key in &prepared.retired {
            if let Some(index) = self.pending.iter().position(|pending| pending == key) {
                self.remove_pending(index);
                self.remember_cancelled(*key);
            }
        }
        if let PageDemand::Requested(request) = prepared.outcome {
            let key = request.key();
            self.pending.push_back(key);
            self.highest_request = Some(key.id());
            self.pending_bytes = self
                .pending_bytes
                .saturating_add(key.demand().max_payload_bytes());
        }
        prepared.outcome
    }

    /// Admits a page only for its exact pending request key.
    pub fn admit(&mut self, page: RangePage) -> Result<PageAdmission, PageAdmissionError> {
        let prepared = self.prepare_admit(page)?;
        Ok(self.commit_prepared_admit(prepared))
    }

    /// Prepares one page admission without mutating residency, request, or cancellation state.
    pub(crate) fn prepare_admit(
        &self,
        page: RangePage,
    ) -> Result<PreparedRangePageAdmission, PageAdmissionError> {
        let key = page.key();
        self.check_current(key)?;
        #[cfg(test)]
        if self.force_next_admission_limit.replace(false) {
            return Err(PageAdmissionError::LimitExceeded(
                ResidencyLimitKind::ResidentBytes,
            ));
        }
        let Some(pending_index) = self.pending.iter().position(|pending| *pending == key) else {
            if self.cancelled.contains(&key) {
                return Err(PageAdmissionError::Cancelled(key));
            }
            return Err(PageAdmissionError::Unavailable(key));
        };

        if let Err(error) = self.validate_page(&page) {
            return Err(PageAdmissionError::Malformed(error));
        }
        if page.retained_bytes() > self.limits.max_resident_bytes() {
            return Err(PageAdmissionError::LimitExceeded(
                ResidencyLimitKind::ResidentBytes,
            ));
        }

        let mut disposition = vec![ResidentDisposition::Retain; self.resident.len()];
        let mut evicted_pages: usize = 0;
        let mut surviving_pages = self.resident.len();
        let mut surviving_payload_bytes = self.resident_bytes;
        for (index, resident) in self.resident.iter().enumerate() {
            let overlaps = resident.range().overlaps(page.range()) || resident.id() == page.id();
            if overlaps {
                disposition[index] = ResidentDisposition::Evict;
                surviving_pages =
                    surviving_pages
                        .checked_sub(1)
                        .ok_or(PageAdmissionError::LimitExceeded(
                            ResidencyLimitKind::ResidentPages,
                        ))?;
                surviving_payload_bytes = surviving_payload_bytes
                    .checked_sub(resident.retained_bytes())
                    .ok_or(PageAdmissionError::LimitExceeded(
                        ResidencyLimitKind::ResidentBytes,
                    ))?;
                evicted_pages =
                    evicted_pages
                        .checked_add(1)
                        .ok_or(PageAdmissionError::LimitExceeded(
                            ResidencyLimitKind::ResidentPages,
                        ))?;
            }
        }

        let mut index = 0;
        while surviving_pages >= self.limits.max_resident_pages()
            || surviving_payload_bytes
                .checked_add(page.retained_bytes())
                .is_none_or(|bytes| bytes > self.limits.max_resident_bytes())
        {
            while disposition[index] == ResidentDisposition::Evict {
                index += 1;
            }
            disposition[index] = ResidentDisposition::Evict;
            surviving_pages =
                surviving_pages
                    .checked_sub(1)
                    .ok_or(PageAdmissionError::LimitExceeded(
                        ResidencyLimitKind::ResidentPages,
                    ))?;
            surviving_payload_bytes = surviving_payload_bytes
                .checked_sub(self.resident[index].retained_bytes())
                .ok_or(PageAdmissionError::LimitExceeded(
                    ResidencyLimitKind::ResidentBytes,
                ))?;
            index += 1;
            evicted_pages =
                evicted_pages
                    .checked_add(1)
                    .ok_or(PageAdmissionError::LimitExceeded(
                        ResidencyLimitKind::ResidentPages,
                    ))?;
        }

        let page_id = page.id();
        let resident_bytes = surviving_payload_bytes
            .checked_add(page.retained_bytes())
            .ok_or(PageAdmissionError::LimitExceeded(
                ResidencyLimitKind::ResidentBytes,
            ))?;
        let _projected = self
            .resident
            .iter()
            .zip(disposition.iter())
            .filter(|(_, disposition)| **disposition == ResidentDisposition::Retain)
            .try_fold((0usize, 0usize), |(bytes, items), (resident, _)| {
                Some((
                    bytes.checked_add(resident.retained_charge().bytes())?,
                    items.checked_add(resident.retained_charge().items())?,
                ))
            })
            .and_then(|(bytes, items)| {
                Some((
                    bytes.checked_add(page.retained_charge().bytes())?,
                    items.checked_add(page.retained_charge().items())?,
                ))
            })
            .ok_or(PageAdmissionError::LimitExceeded(
                ResidencyLimitKind::ResidentBytes,
            ))?;
        let projected_pages =
            surviving_pages
                .checked_add(1)
                .ok_or(PageAdmissionError::LimitExceeded(
                    ResidencyLimitKind::ResidentPages,
                ))?;
        let destination = VecDeque::with_capacity(projected_pages);
        let retained_bytes = page
            .retained_charge()
            .bytes()
            .checked_add(
                disposition
                    .capacity()
                    .checked_mul(std::mem::size_of::<ResidentDisposition>())
                    .ok_or(PageAdmissionError::LimitExceeded(
                        ResidencyLimitKind::ResidentBytes,
                    ))?,
            )
            .and_then(|bytes| {
                bytes.checked_add(
                    destination
                        .capacity()
                        .checked_mul(std::mem::size_of::<RangePage>())?,
                )
            })
            .ok_or(PageAdmissionError::LimitExceeded(
                ResidencyLimitKind::ResidentBytes,
            ))?;
        let retained_items = page
            .retained_charge()
            .items()
            .checked_add(disposition.capacity())
            .and_then(|items| items.checked_add(destination.capacity()))
            .ok_or(PageAdmissionError::LimitExceeded(
                ResidencyLimitKind::ResidentPages,
            ))?;
        Ok(PreparedRangePageAdmission {
            page,
            pending_index,
            disposition,
            destination,
            admission: PageAdmission::Admitted {
                page: page_id,
                evicted_pages,
            },
            resident_bytes,
            projected_pages,
            retained_bytes,
            retained_items,
        })
    }

    /// Commits a prepared page admission by moving into already allocated destination storage.
    pub(crate) fn commit_prepared_admit(
        &mut self,
        prepared: PreparedRangePageAdmission,
    ) -> PageAdmission {
        let PreparedRangePageAdmission {
            page,
            pending_index,
            disposition,
            mut destination,
            admission,
            resident_bytes,
            ..
        } = prepared;
        self.remove_pending(pending_index);
        for disposition in disposition {
            let resident = self
                .resident
                .pop_front()
                .expect("prepared disposition matches resident page count");
            if disposition == ResidentDisposition::Retain {
                destination.push_back(resident);
            }
        }
        destination.push_back(page);
        self.resident = destination;
        self.resident_bytes = resident_bytes;
        admission
    }

    /// Settles an exact request as cancelled or unavailable and releases it.
    pub fn settle(&mut self, key: PageRequestKey, failure: PageFailure) -> PageSettlement {
        let prepared = match self.prepare_settle(key, failure) {
            Ok(prepared) => prepared,
            Err(settlement) => return settlement,
        };
        self.commit_prepared_settle(prepared)
    }

    pub(crate) fn prepare_settle(
        &self,
        key: PageRequestKey,
        failure: PageFailure,
    ) -> Result<PreparedRangePageSettlement, PageSettlement> {
        if !self.is_current(key) {
            return Err(PageSettlement::Stale);
        }
        let Some(pending_index) = self.pending.iter().position(|pending| *pending == key) else {
            return Err(if self.cancelled.contains(&key) {
                PageSettlement::AlreadyCancelled
            } else {
                PageSettlement::Unavailable
            });
        };
        Ok(PreparedRangePageSettlement {
            key,
            pending_index,
            failure,
        })
    }

    pub(crate) fn commit_prepared_settle(
        &mut self,
        prepared: PreparedRangePageSettlement,
    ) -> PageSettlement {
        debug_assert_eq!(
            self.pending.get(prepared.pending_index),
            Some(&prepared.key)
        );
        self.remove_pending(prepared.pending_index);
        if prepared.failure == PageFailure::Cancelled {
            self.remember_cancelled(prepared.key);
        }
        PageSettlement::Settled(prepared.failure)
    }

    /// Cancels one exact pending request and releases its capacity.
    pub fn cancel(&mut self, key: PageRequestKey) -> PageSettlement {
        self.settle(key, PageFailure::Cancelled)
    }

    /// Replaces the binding or revision and releases all local page capacity.
    ///
    /// Returned keys identify the host requests the caller must cancel.
    pub fn rebind(&mut self, binding: RangeBinding) -> Vec<PageRequestKey> {
        let request_generation_changed = binding.binding() != self.binding.binding()
            || binding.revision() != self.binding.revision();
        let cancelled = self.pending.iter().copied().collect();
        self.binding = binding;
        self.resident.clear();
        self.pending.clear();
        self.cancelled.clear();
        if request_generation_changed {
            self.highest_request = None;
        }
        self.resident_bytes = 0;
        self.pending_bytes = 0;
        cancelled
    }

    pub(crate) fn prepare_rebind(&self, binding: RangeBinding) -> PreparedResidencyRebind {
        PreparedResidencyRebind {
            binding,
            cancelled: self.pending.iter().copied().collect(),
            successor: None,
        }
    }

    pub(crate) fn prepare_rebind_with_demand(
        &self,
        binding: RangeBinding,
        request: PageRequest,
    ) -> Result<PreparedResidencyRebind, PageDemandError> {
        if request.key().binding() != binding.binding()
            || request.key().revision() != binding.revision()
        {
            let point = match request.key().demand() {
                PageDemandEnvelope::Adjacent { anchor, .. } => anchor,
                PageDemandEnvelope::Validation { candidate, .. } => candidate,
            };
            return Err(PageDemandError::Malformed(
                RangeContractError::ByteRangeOutsideExtent {
                    range: ByteRange::new(point, point).expect("equal offsets are ordered"),
                    byte_len: binding.extent().byte_len(),
                },
            ));
        }
        if self.limits.max_pending_requests() < 1 {
            return Err(PageDemandError::LimitExceeded(
                ResidencyLimitKind::PendingRequests,
            ));
        }
        if request.key().demand().max_payload_bytes() > self.limits.max_pending_bytes() {
            return Err(PageDemandError::LimitExceeded(
                ResidencyLimitKind::PendingBytes,
            ));
        }
        Ok(PreparedResidencyRebind {
            binding,
            cancelled: self.pending.iter().copied().collect(),
            successor: Some(request),
        })
    }

    pub(crate) fn commit_prepared_rebind(
        &mut self,
        prepared: PreparedResidencyRebind,
    ) -> Vec<PageRequestKey> {
        let generation_changed = prepared.binding.binding() != self.binding.binding()
            || prepared.binding.revision() != self.binding.revision();
        self.binding = prepared.binding;
        self.resident.clear();
        self.pending.clear();
        self.cancelled.clear();
        if generation_changed {
            self.highest_request = None;
        }
        self.resident_bytes = 0;
        self.pending_bytes = 0;
        if let Some(request) = prepared.successor {
            let key = request.key();
            self.pending.push_back(key);
            self.highest_request = Some(key.id());
            self.pending_bytes = key.demand().max_payload_bytes();
        }
        prepared.cancelled
    }

    /// Releases every resident page and pending request without installing another binding.
    pub fn dispose(&mut self) -> Vec<PageRequestKey> {
        let cancelled = self.pending.iter().copied().collect();
        self.resident = VecDeque::new();
        self.pending = VecDeque::new();
        self.cancelled = VecDeque::new();
        self.resident_bytes = 0;
        self.pending_bytes = 0;
        cancelled
    }

    /// Explicitly evicts one resident page and releases its retained bytes.
    pub fn evict(&mut self, page_id: PageId) -> bool {
        let Some(index) = self.resident.iter().position(|page| page.id() == page_id) else {
            return false;
        };
        self.remove_resident(index);
        true
    }

    fn validate_page(&self, page: &RangePage) -> Result<(), RangeContractError> {
        self.binding.extent().check_byte_range(page.range())?;
        for atom in page.atoms() {
            if atom.global_range().is_empty()
                || self
                    .binding
                    .extent()
                    .check_byte_range(atom.global_range())
                    .is_err()
            {
                return Err(RangeContractError::MalformedAtomRange {
                    atom: atom.id(),
                    global_range: atom.global_range(),
                    fragment_range: atom.fragment_range(),
                });
            }
            for resident_atom in self.resident.iter().flat_map(|page| page.atoms()) {
                if atom.id() == resident_atom.id() {
                    if !atom.reconciles_with(resident_atom) {
                        return Err(RangeContractError::ConflictingAtomFacts { atom: atom.id() });
                    }
                } else if atom.global_range().overlaps(resident_atom.global_range()) {
                    return Err(RangeContractError::OverlappingAtomFacts {
                        first: resident_atom.id(),
                        second: atom.id(),
                    });
                }
            }
        }
        let preceding_is_boundary =
            page.preceding() == crate::range_source::PageEdgeFact::DocumentBoundary;
        let following_is_boundary =
            page.following() == crate::range_source::PageEdgeFact::DocumentBoundary;
        if preceding_is_boundary != (page.range().start().get() == 0)
            || following_is_boundary
                != (page.range().end().get() == self.binding.extent().byte_len())
            || page.end_of_source() != following_is_boundary
        {
            return Err(RangeContractError::MalformedEdgeFacts);
        }
        Ok(())
    }

    fn check_current(&self, key: PageRequestKey) -> Result<(), PageAdmissionError> {
        if self.is_current(key) {
            Ok(())
        } else {
            Err(PageAdmissionError::Stale(key))
        }
    }

    fn is_current(&self, key: PageRequestKey) -> bool {
        key.binding() == self.binding.binding() && key.revision() == self.binding.revision()
    }

    fn remove_pending(&mut self, index: usize) {
        let key = self.pending.remove(index).expect("pending index exists");
        self.pending_bytes -= key.max_payload_bytes();
    }

    fn remove_resident(&mut self, index: usize) {
        let page = self.resident.remove(index).expect("resident index exists");
        self.resident_bytes -= page.retained_bytes();
    }

    fn remember_cancelled(&mut self, key: PageRequestKey) {
        if self.cancelled.len() == self.limits.max_pending_requests() {
            self.cancelled.pop_front();
        }
        self.cancelled.push_back(key);
    }
}

fn resident_satisfaction(page: &RangePage, demand: PageDemandEnvelope) -> Option<PageDemand> {
    if page.range().len() > demand.max_payload_bytes() {
        return None;
    }
    match demand {
        PageDemandEnvelope::Adjacent {
            anchor, direction, ..
        } => {
            let anchored = match direction {
                crate::range_source::PageDirection::Forward => page.range().start() == anchor,
                crate::range_source::PageDirection::Backward => page.range().end() == anchor,
            };
            let progresses_or_matches_edge = !page.range().is_empty()
                || match direction {
                    crate::range_source::PageDirection::Forward => {
                        page.following() == crate::range_source::PageEdgeFact::DocumentBoundary
                    }
                    crate::range_source::PageDirection::Backward => {
                        page.preceding() == crate::range_source::PageEdgeFact::DocumentBoundary
                    }
                };
            (anchored && progresses_or_matches_edge)
                .then_some(PageDemand::ResidentAdjacent(page.id()))
        }
        PageDemandEnvelope::Validation { candidate, .. } => {
            if !page.range().contains_offset(candidate) {
                return None;
            }
            let local = usize::try_from(candidate.get() - page.range().start().get()).ok()?;
            Some(PageDemand::ResidentValidation {
                page: page.id(),
                candidate_is_boundary: page.text().is_char_boundary(local),
            })
        }
    }
}

#[cfg(test)]
mod transition_tests {
    use super::*;
    use crate::{BindingId, LogicalExtent, PageDirection, SourceRevision};

    fn binding() -> RangeBinding {
        RangeBinding::new(
            BindingId::new(91),
            SourceRevision::new(1),
            LogicalExtent::new(16, 1),
        )
    }

    fn limits(pending_bytes: u64) -> ResidencyLimits {
        ResidencyLimits::new(2, 32, 2, pending_bytes).unwrap()
    }

    fn demand(anchor: u64, bytes: u64) -> PageDemandEnvelope {
        PageDemandEnvelope::Adjacent {
            anchor: ByteOffset::new(anchor),
            direction: PageDirection::Forward,
            max_payload_bytes: bytes,
        }
    }

    fn requested(outcome: PageDemand) -> PageRequestKey {
        match outcome {
            PageDemand::Requested(request) => request.key(),
            other => panic!("expected exact request, got {other:?}"),
        }
    }

    #[test]
    fn prepared_demand_never_coalesces_a_retired_request_and_commits_without_rollback() {
        let envelope = demand(0, 8);
        let mut residency = RangeResidency::new(binding(), limits(8));
        let retired = requested(
            residency
                .demand(PageRequestId::new(1), PagePurpose::GeometryTarget, envelope)
                .unwrap(),
        );
        let before = residency.counts();
        let prepared = residency
            .prepare_demand_after_retirement(
                PageRequestId::new(2),
                PagePurpose::GeometryTarget,
                envelope,
                &[retired],
            )
            .unwrap();
        let successor = requested(prepared.outcome());
        assert_eq!(successor.id(), PageRequestId::new(2));
        assert_eq!(residency.counts(), before);

        assert_eq!(
            residency.commit_prepared_demand(prepared),
            PageDemand::Requested(PageRequest::new(successor))
        );
        assert_eq!(
            residency.pending_requests().collect::<Vec<_>>(),
            vec![successor]
        );
    }

    #[test]
    fn prepared_demand_reuses_surviving_pending_work_but_excludes_named_retirement() {
        let mut residency = RangeResidency::new(binding(), limits(16));
        let survivor_envelope = demand(0, 8);
        let survivor = requested(
            residency
                .demand(
                    PageRequestId::new(1),
                    PagePurpose::GeometryTarget,
                    survivor_envelope,
                )
                .unwrap(),
        );
        let retired = requested(
            residency
                .demand(
                    PageRequestId::new(2),
                    PagePurpose::GeometryTarget,
                    demand(8, 8),
                )
                .unwrap(),
        );
        let prepared = residency
            .prepare_demand_after_retirement(
                PageRequestId::new(3),
                PagePurpose::GeometryTarget,
                survivor_envelope,
                &[retired],
            )
            .unwrap();
        assert_eq!(prepared.outcome(), PageDemand::Coalesced(survivor));
        assert_eq!(
            residency.commit_prepared_demand(prepared),
            PageDemand::Coalesced(survivor)
        );
        assert_eq!(
            residency.pending_requests().collect::<Vec<_>>(),
            vec![survivor]
        );
    }

    #[test]
    fn post_retirement_pending_charge_accepts_exact_fit_and_rejects_one_under() {
        let mut exact = RangeResidency::new(binding(), limits(8));
        let retired = requested(
            exact
                .demand(
                    PageRequestId::new(1),
                    PagePurpose::GeometryTarget,
                    demand(0, 7),
                )
                .unwrap(),
        );
        assert!(
            exact
                .prepare_demand_after_retirement(
                    PageRequestId::new(2),
                    PagePurpose::GeometryTarget,
                    demand(0, 8),
                    &[retired],
                )
                .is_ok()
        );

        let mut under = RangeResidency::new(binding(), limits(7));
        let retired = requested(
            under
                .demand(
                    PageRequestId::new(1),
                    PagePurpose::GeometryTarget,
                    demand(0, 7),
                )
                .unwrap(),
        );
        assert!(matches!(
            under.prepare_demand_after_retirement(
                PageRequestId::new(2),
                PagePurpose::GeometryTarget,
                demand(0, 8),
                &[retired],
            ),
            Err(PageDemandError::LimitExceeded(
                ResidencyLimitKind::PendingBytes
            ))
        ));
    }
}
