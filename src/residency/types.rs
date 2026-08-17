use crate::range_source::{
    BindingId, ByteOffset, ObjectPageId, ObjectRequestKey, PageFailure, PageId, PageRequest,
    PageRequestId, PageRequestKey, RangeBinding, RangeContractError, SourceRevision,
};

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

    pub const fn range_binding(self) -> RangeBinding {
        self.range_binding
    }

    pub const fn binding(self) -> BindingId {
        self.range_binding.binding()
    }

    pub const fn revision(self) -> SourceRevision {
        self.range_binding.revision()
    }

    pub const fn offset(self) -> ByteOffset {
        self.offset
    }

    pub const fn source_page(self) -> Option<PageId> {
        self.source_page
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScalarBoundaryProofError {
    OutsideExtent(ByteOffset),
    NotScalarBoundary(ByteOffset),
    Unavailable(ByteOffset),
}

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

    pub const fn range_binding(&self) -> RangeBinding {
        self.range_binding
    }

    pub const fn page(&self) -> ObjectPageId {
        self.page
    }

    pub const fn key(&self) -> ObjectRequestKey {
        self.key
    }

    pub fn len(&self) -> usize {
        self.proofs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.proofs.is_empty()
    }

    pub(crate) fn proofs(&self) -> &[ScalarBoundaryProof] {
        &self.proofs
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ObjectAnchorProofError {
    Stale(ObjectRequestKey),
    Scalar(ScalarBoundaryProofError),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResidencyLimits {
    max_resident_pages: usize,
    max_resident_bytes: usize,
    max_pending_requests: usize,
    max_pending_bytes: u64,
}

impl ResidencyLimits {
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

    pub const fn max_resident_pages(self) -> usize {
        self.max_resident_pages
    }

    pub const fn max_resident_bytes(self) -> usize {
        self.max_resident_bytes
    }

    pub const fn max_pending_requests(self) -> usize {
        self.max_pending_requests
    }

    pub const fn max_pending_bytes(self) -> u64 {
        self.max_pending_bytes
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResidencyLimitKind {
    ResidentPages,
    ResidentBytes,
    PendingRequests,
    PendingBytes,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResidencyLimitError {
    ZeroLimit(ResidencyLimitKind),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PageDemand {
    ResidentAdjacent(PageId),
    ResidentValidation {
        page: PageId,
        candidate_is_boundary: bool,
    },
    Coalesced(PageRequestKey),
    Requested(PageRequest),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PageDemandError {
    Malformed(RangeContractError),
    RequestIdInUse(PageRequestId),
    LimitExceeded(ResidencyLimitKind),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PageAdmission {
    Admitted { page: PageId, evicted_pages: usize },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PageAdmissionError {
    Stale(PageRequestKey),
    Cancelled(PageRequestKey),
    Unavailable(PageRequestKey),
    Malformed(RangeContractError),
    LimitExceeded(ResidencyLimitKind),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PageSettlement {
    Settled(PageFailure),
    Stale,
    AlreadyCancelled,
    Unavailable,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ResidencyCounts {
    pub resident_pages: usize,
    pub resident_bytes: usize,
    pub pending_requests: usize,
    pub pending_bytes: u64,
}
