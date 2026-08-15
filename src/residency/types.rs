use crate::range_source::{
    BindingId, ByteOffset, ObjectPageId, ObjectRequestKey, PageFailure, PageId, PageRequest,
    PageRequestId, PageRequestKey, RangeBinding, RangeContractError, SourceRevision,
};

/// Opaque proof that one byte offset is a UTF-8 scalar boundary in one exact source binding.
///
/// Proofs are minted only by [`crate::RangeResidency`] from a bounded admitted text page or an
/// exact document-edge fact. Callers cannot construct or alter them.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScalarBoundaryProof {
    range_binding: RangeBinding,
    offset: ByteOffset,
    source_page: Option<PageId>,
}

impl ScalarBoundaryProof {
    pub(crate) const fn new(
        range_binding: RangeBinding,
        offset: ByteOffset,
        source_page: Option<PageId>,
    ) -> Self {
        Self {
            range_binding,
            offset,
            source_page,
        }
    }

    /// Returns the complete exact source binding, revision, and logical extent of this proof.
    pub const fn range_binding(self) -> RangeBinding {
        self.range_binding
    }

    /// Returns the exact source binding in which the boundary was proven.
    pub const fn binding(self) -> BindingId {
        self.range_binding.binding()
    }

    /// Returns the exact source revision in which the boundary was proven.
    pub const fn revision(self) -> SourceRevision {
        self.range_binding.revision()
    }

    /// Returns the exact proven UTF-8 byte boundary.
    pub const fn offset(self) -> ByteOffset {
        self.offset
    }

    /// Returns the bounded resident text page that supplied the proof.
    ///
    /// `None` identifies the exact source origin or end, which the current logical extent proves
    /// without retaining a payload page.
    pub const fn source_page(self) -> Option<PageId> {
        self.source_page
    }
}

/// Typed failure to prove an exact UTF-8 scalar boundary from bounded text residency.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScalarBoundaryProofError {
    /// The requested offset exceeds the current logical byte extent.
    OutsideExtent(ByteOffset),
    /// A resident text page covers the offset and proves that it is inside a UTF-8 scalar.
    NotScalarBoundary(ByteOffset),
    /// No bounded resident text page currently covers the non-edge offset.
    Unavailable(ByteOffset),
}

/// Opaque, page-bounded set of text-owned scalar-boundary proofs for one object response.
///
/// The batch contains exactly one proof for each distinct anchor in the object page and is minted
/// only by [`crate::RangeResidency::prove_object_page_anchors`]. Its size is therefore bounded by
/// that page's object-count ceiling.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObjectAnchorProofs {
    range_binding: RangeBinding,
    page: ObjectPageId,
    key: ObjectRequestKey,
    proofs: Vec<ScalarBoundaryProof>,
}

impl ObjectAnchorProofs {
    pub(crate) fn new(
        range_binding: RangeBinding,
        page: ObjectPageId,
        key: ObjectRequestKey,
        proofs: Vec<ScalarBoundaryProof>,
    ) -> Self {
        Self {
            range_binding,
            page,
            key,
            proofs,
        }
    }

    /// Returns the complete exact source binding covered by this proof batch.
    pub const fn range_binding(&self) -> RangeBinding {
        self.range_binding
    }

    /// Returns the exact object-page identity covered by this proof batch.
    pub const fn page(&self) -> ObjectPageId {
        self.page
    }

    /// Returns the exact object request/response key covered by this proof batch.
    pub const fn key(&self) -> ObjectRequestKey {
        self.key
    }

    /// Returns the number of distinct proven object anchors.
    pub fn len(&self) -> usize {
        self.proofs.len()
    }

    /// Reports whether the object page has no anchors to prove.
    pub fn is_empty(&self) -> bool {
        self.proofs.is_empty()
    }

    pub(crate) fn proofs(&self) -> &[ScalarBoundaryProof] {
        &self.proofs
    }
}

/// Typed failure to issue a page-bound object-anchor proof batch.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ObjectAnchorProofError {
    /// The object page or admitting projection has another binding, revision, or logical extent.
    Stale(ObjectRequestKey),
    /// One requested object anchor could not be proven exactly.
    Scalar(ScalarBoundaryProofError),
}

/// Finite hard limits for one range-backed resident projection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResidencyLimits {
    max_resident_pages: usize,
    max_resident_bytes: usize,
    max_pending_requests: usize,
    max_pending_bytes: u64,
}

impl ResidencyLimits {
    /// Creates finite, nonzero resident and pending limits.
    pub fn new(
        max_resident_pages: usize,
        max_resident_bytes: usize,
        max_pending_requests: usize,
        max_pending_bytes: u64,
    ) -> Result<Self, ResidencyLimitError> {
        if max_resident_pages == 0 {
            return Err(ResidencyLimitError::ZeroLimit(
                ResidencyLimitKind::ResidentPages,
            ));
        }
        if max_resident_bytes == 0 {
            return Err(ResidencyLimitError::ZeroLimit(
                ResidencyLimitKind::ResidentBytes,
            ));
        }
        if max_pending_requests == 0 {
            return Err(ResidencyLimitError::ZeroLimit(
                ResidencyLimitKind::PendingRequests,
            ));
        }
        if max_pending_bytes == 0 {
            return Err(ResidencyLimitError::ZeroLimit(
                ResidencyLimitKind::PendingBytes,
            ));
        }
        Ok(Self {
            max_resident_pages,
            max_resident_bytes,
            max_pending_requests,
            max_pending_bytes,
        })
    }

    /// Returns the maximum resident page count.
    pub const fn max_resident_pages(self) -> usize {
        self.max_resident_pages
    }

    /// Returns the maximum retained resident payload bytes.
    pub const fn max_resident_bytes(self) -> usize {
        self.max_resident_bytes
    }

    /// Returns the maximum in-flight request count.
    pub const fn max_pending_requests(self) -> usize {
        self.max_pending_requests
    }

    /// Returns the maximum sum of in-flight requested byte lengths.
    pub const fn max_pending_bytes(self) -> u64 {
        self.max_pending_bytes
    }
}

/// Named capacity governed by [`ResidencyLimits`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResidencyLimitKind {
    /// Resident page slots.
    ResidentPages,
    /// Retained resident payload bytes.
    ResidentBytes,
    /// In-flight request slots.
    PendingRequests,
    /// Sum of in-flight requested byte lengths.
    PendingBytes,
}

/// Invalid finite-limit configuration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResidencyLimitError {
    /// A required hard limit was zero.
    ZeroLimit(ResidencyLimitKind),
}

/// Result of registering bounded page demand.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PageDemand {
    /// A resident page already satisfies an adjacent demand.
    ResidentAdjacent(PageId),
    /// A resident page explicitly proves the candidate's exact UTF-8 boundary status.
    ResidentValidation {
        page: PageId,
        candidate_is_boundary: bool,
    },
    /// An overlapping in-flight request serializes this demand.
    ///
    /// The caller re-demands any uncovered suffix or prefix after that exact request settles; no
    /// hidden queue or second reservation is retained for the coalesced demand.
    Coalesced(PageRequestKey),
    /// The caller must dispatch this newly admitted request.
    Requested(PageRequest),
}

/// Typed rejection of new page demand.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PageDemandError {
    /// The range is malformed for the current source extent.
    Malformed(RangeContractError),
    /// The request identity is already in flight under another exact key.
    RequestIdInUse(PageRequestId),
    /// The applicable pending capacity is exhausted.
    LimitExceeded(ResidencyLimitKind),
}

/// Result of admitting an exact successful page response.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PageAdmission {
    /// The page entered the resident projection.
    Admitted { page: PageId, evicted_pages: usize },
}

/// Typed rejection of a page response.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PageAdmissionError {
    /// The response belongs to a non-current binding or revision.
    Stale(PageRequestKey),
    /// The exact request was previously cancelled.
    Cancelled(PageRequestKey),
    /// No pending request accepts this exact response key.
    Unavailable(PageRequestKey),
    /// The response contradicts the current exact source contract.
    Malformed(RangeContractError),
    /// The page cannot fit the configured resident byte limit.
    LimitExceeded(ResidencyLimitKind),
}

/// Result of terminally settling an in-flight request without a page.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PageSettlement {
    /// The exact request settled and released pending capacity.
    Settled(PageFailure),
    /// The settlement belongs to a non-current binding or revision.
    Stale,
    /// The exact request was already cancelled.
    AlreadyCancelled,
    /// No pending request has this exact key.
    Unavailable,
}

/// Exact current resource counts for one projection.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ResidencyCounts {
    /// Number of resident pages.
    pub resident_pages: usize,
    /// Retained resident text and atom fallback bytes.
    pub resident_bytes: usize,
    /// Number of in-flight page requests.
    pub pending_requests: usize,
    /// Sum of in-flight requested byte lengths.
    pub pending_bytes: u64,
}
