//! Fixed-capacity resident-page and pending-request projection.

use std::collections::VecDeque;

use crate::range_source::{
    ByteRange, PageDemandEnvelope, PageFailure, PageId, PagePurpose, PageRequest, PageRequestId,
    PageRequestKey, RangeBinding, RangeContractError, RangePage,
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
}

impl RangeResidency {
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
        }
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

    /// Returns resident pages without constructing a combined source value.
    pub fn resident_pages(&self) -> impl ExactSizeIterator<Item = &RangePage> {
        self.resident.iter()
    }

    /// Moves every resident page into one publication candidate.
    ///
    /// Pending requests remain owned by this projection. The transfer is clone-free so a caller
    /// can account the exact page graph once while atomically publishing a coherent surface.
    pub fn take_resident_pages(&mut self) -> Vec<RangePage> {
        self.resident_bytes = 0;
        self.resident.drain(..).collect()
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

    /// Admits a page only for its exact pending request key.
    pub fn admit(&mut self, page: RangePage) -> Result<PageAdmission, PageAdmissionError> {
        let key = page.key();
        self.check_current(key)?;
        let Some(index) = self.pending.iter().position(|pending| *pending == key) else {
            if self.cancelled.contains(&key) {
                return Err(PageAdmissionError::Cancelled(key));
            }
            return Err(PageAdmissionError::Unavailable(key));
        };
        self.remove_pending(index);

        if let Err(error) = self.validate_page(&page) {
            return Err(PageAdmissionError::Malformed(error));
        }
        if page.retained_bytes() > self.limits.max_resident_bytes() {
            return Err(PageAdmissionError::LimitExceeded(
                ResidencyLimitKind::ResidentBytes,
            ));
        }

        let mut evicted_pages = 0;
        let mut index = 0;
        while index < self.resident.len() {
            let overlaps = self.resident[index].range().overlaps(page.range())
                || self.resident[index].id() == page.id();
            if overlaps {
                self.remove_resident(index);
                evicted_pages += 1;
            } else {
                index += 1;
            }
        }
        while self.resident.len() == self.limits.max_resident_pages()
            || self.resident_bytes.saturating_add(page.retained_bytes())
                > self.limits.max_resident_bytes()
        {
            self.remove_resident(0);
            evicted_pages += 1;
        }

        self.resident_bytes += page.retained_bytes();
        let page_id = page.id();
        self.resident.push_back(page);
        Ok(PageAdmission::Admitted {
            page: page_id,
            evicted_pages,
        })
    }

    /// Settles an exact request as cancelled or unavailable and releases it.
    pub fn settle(&mut self, key: PageRequestKey, failure: PageFailure) -> PageSettlement {
        if !self.is_current(key) {
            return PageSettlement::Stale;
        }
        let Some(index) = self.pending.iter().position(|pending| *pending == key) else {
            return if self.cancelled.contains(&key) {
                PageSettlement::AlreadyCancelled
            } else {
                PageSettlement::Unavailable
            };
        };
        self.remove_pending(index);
        if failure == PageFailure::Cancelled {
            self.remember_cancelled(key);
        }
        PageSettlement::Settled(failure)
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

    /// Releases every resident page and pending request without installing another binding.
    pub fn dispose(&mut self) -> Vec<PageRequestKey> {
        let cancelled = self.pending.iter().copied().collect();
        self.resident.clear();
        self.pending.clear();
        self.cancelled.clear();
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
