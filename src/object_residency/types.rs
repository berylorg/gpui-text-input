use crate::{
    ObjectContractError, ObjectPageFailure, ObjectPageId, ObjectRequest, ObjectRequestId,
    ObjectRequestKey,
};

/// Finite hard limits for one bounded inline-object projection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ObjectResidencyLimits {
    max_resident_pages: usize,
    max_resident_objects: usize,
    max_resident_bytes: usize,
    max_resident_presentation_bytes: usize,
    max_pending_requests: usize,
    max_pending_objects: usize,
    max_pending_bytes: usize,
}

impl ObjectResidencyLimits {
    /// Creates finite nonzero resident and pending limits.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        max_resident_pages: usize,
        max_resident_objects: usize,
        max_resident_bytes: usize,
        max_resident_presentation_bytes: usize,
        max_pending_requests: usize,
        max_pending_objects: usize,
        max_pending_bytes: usize,
    ) -> Result<Self, ObjectResidencyLimitError> {
        let values = [
            (max_resident_pages, ObjectResidencyLimitKind::ResidentPages),
            (
                max_resident_objects,
                ObjectResidencyLimitKind::ResidentObjects,
            ),
            (max_resident_bytes, ObjectResidencyLimitKind::ResidentBytes),
            (
                max_resident_presentation_bytes,
                ObjectResidencyLimitKind::ResidentPresentationBytes,
            ),
            (
                max_pending_requests,
                ObjectResidencyLimitKind::PendingRequests,
            ),
            (
                max_pending_objects,
                ObjectResidencyLimitKind::PendingObjects,
            ),
            (max_pending_bytes, ObjectResidencyLimitKind::PendingBytes),
        ];
        if let Some((_, kind)) = values.into_iter().find(|(value, _)| *value == 0) {
            return Err(ObjectResidencyLimitError::ZeroLimit(kind));
        }
        Ok(Self {
            max_resident_pages,
            max_resident_objects,
            max_resident_bytes,
            max_resident_presentation_bytes,
            max_pending_requests,
            max_pending_objects,
            max_pending_bytes,
        })
    }

    /// Returns the maximum resident object-page count.
    pub const fn max_resident_pages(self) -> usize {
        self.max_resident_pages
    }
    /// Returns the maximum resident object-fact count.
    pub const fn max_resident_objects(self) -> usize {
        self.max_resident_objects
    }
    /// Returns the maximum complete resident record and payload bytes.
    pub const fn max_resident_bytes(self) -> usize {
        self.max_resident_bytes
    }
    /// Returns the separate resident display and fallback byte cap.
    pub const fn max_resident_presentation_bytes(self) -> usize {
        self.max_resident_presentation_bytes
    }
    /// Returns the maximum in-flight object-request count.
    pub const fn max_pending_requests(self) -> usize {
        self.max_pending_requests
    }
    /// Returns the sum cap for in-flight requested object counts.
    pub const fn max_pending_objects(self) -> usize {
        self.max_pending_objects
    }
    /// Returns the sum cap for in-flight retained-byte ceilings.
    pub const fn max_pending_bytes(self) -> usize {
        self.max_pending_bytes
    }
}

/// Named capacity governed by [`ObjectResidencyLimits`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ObjectResidencyLimitKind {
    /// Resident object-page slots.
    ResidentPages,
    /// Resident object-fact records.
    ResidentObjects,
    /// Complete retained object-page bytes.
    ResidentBytes,
    /// Retained display and fallback bytes.
    ResidentPresentationBytes,
    /// In-flight request slots.
    PendingRequests,
    /// Sum of in-flight requested object ceilings.
    PendingObjects,
    /// Sum of in-flight requested retained-byte ceilings.
    PendingBytes,
}

/// Invalid finite object-residency configuration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ObjectResidencyLimitError {
    /// A required hard limit was zero.
    ZeroLimit(ObjectResidencyLimitKind),
}

/// Result of registering one bounded object demand.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ObjectDemand {
    /// A resident page already exactly satisfies the demand.
    Resident(ObjectPageId),
    /// An exact equivalent request is already in flight.
    Coalesced(ObjectRequestKey),
    /// The caller must dispatch this newly admitted request.
    Requested(ObjectRequest),
}

/// Typed rejection of new object demand.
#[derive(Clone, Debug, PartialEq)]
pub enum ObjectDemandError {
    /// The demand or bound extent was malformed.
    Malformed(ObjectContractError),
    /// This request identity is not strictly newer in the current generation.
    RequestIdInUse(ObjectRequestId),
    /// One pending capacity would be exceeded.
    LimitExceeded(ObjectResidencyLimitKind),
}

/// Result of admitting one exact successful object response.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ObjectPageAdmission {
    /// The response entered the resident projection after named LRU evictions.
    Admitted {
        /// Admitted page identity.
        page: ObjectPageId,
        /// Number of bounded pages released before admission.
        evicted_pages: usize,
        /// Number of object facts released with those pages.
        evicted_objects: usize,
    },
    /// The exact payload for an already-resident stable page identity was reconciled under the
    /// response's current request key without double-retaining it.
    Reconciled {
        /// Reconciled page identity.
        page: ObjectPageId,
        /// Number of other bounded pages released because they repeated reconciled object facts.
        evicted_pages: usize,
        /// Number of object facts released with those other pages.
        evicted_objects: usize,
    },
}

/// Typed rejection of an object response.
#[derive(Clone, Debug, PartialEq)]
pub enum ObjectPageAdmissionError {
    /// The response belongs to a noncurrent source or presentation generation.
    Stale(ObjectRequestKey),
    /// Its exact request was previously cancelled.
    Cancelled(ObjectRequestKey),
    /// No in-flight request accepts this exact key.
    Unavailable(ObjectRequestKey),
    /// The response conflicts with the bounded current projection.
    Malformed(ObjectContractError),
    /// One resident capacity would be exceeded by this page alone.
    LimitExceeded(ObjectResidencyLimitKind),
}

/// Result of terminally settling one in-flight object request without a page.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ObjectPageSettlement {
    /// The exact request settled and released its pending reservations.
    Settled(ObjectPageFailure),
    /// The settlement belongs to a noncurrent generation.
    Stale,
    /// The exact request was already cancelled.
    AlreadyCancelled,
    /// No in-flight request has this exact key.
    Unavailable,
}

/// Exact current resource counts for one object projection.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ObjectResidencyCounts {
    /// Number of resident object pages.
    pub resident_pages: usize,
    /// Number of resident object facts.
    pub resident_objects: usize,
    /// Complete resident record and payload bytes.
    pub resident_bytes: usize,
    /// Resident display and fallback payload bytes.
    pub resident_presentation_bytes: usize,
    /// Number of in-flight object requests.
    pub pending_requests: usize,
    /// Sum of in-flight requested object ceilings.
    pub pending_objects: usize,
    /// Sum of in-flight requested retained-byte ceilings.
    pub pending_bytes: usize,
}
