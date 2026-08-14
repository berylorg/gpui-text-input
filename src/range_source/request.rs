use super::{BindingId, ByteOffset, PageRequestId, RangeContractError, SourceRevision};

/// App-neutral reason a page is required.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum PagePurpose {
    /// Visible viewport and overscan realization.
    Viewport,
    /// Caret movement or validation.
    Caret,
    /// Selection movement or validation.
    Selection,
    /// Grapheme, word, or logical-line segmentation.
    Segmentation,
    /// Bounded clipboard representation construction.
    Clipboard,
    /// Background visual-line or geometry indexing.
    GeometryIndex,
    /// Exact block-position target resolution after geometry indexing.
    GeometryTarget,
    /// Exact UTF-16 replay for platform text-input replacement or query.
    PlatformRange,
    /// Validation and realization of a compact restoration seed.
    Restoration,
}

/// Direction of one bounded page demand from a proven UTF-8 anchor.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum PageDirection {
    /// The returned page starts at the anchor and advances toward the document end.
    Forward,
    /// The returned page ends at the anchor and advances toward the document start.
    Backward,
}

/// Source-selection envelope frozen by one page request.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum PageDemandEnvelope {
    /// Requests positive adjacent progress from an already proven UTF-8 boundary.
    Adjacent {
        /// Proven UTF-8 boundary from which the source must advance.
        anchor: ByteOffset,
        /// Direction in which the source selects the other page edge.
        direction: PageDirection,
        /// Inclusive ceiling for the returned UTF-8 payload length.
        max_payload_bytes: u64,
    },
    /// Requests a bounded source window proving whether an untrusted offset is a boundary.
    Validation {
        /// Untrusted absolute byte offset that the returned window must cover.
        candidate: ByteOffset,
        /// Inclusive ceiling for the returned UTF-8 payload length.
        max_payload_bytes: u64,
    },
}

impl PageDemandEnvelope {
    /// Returns the payload-byte ceiling frozen by this demand.
    pub const fn max_payload_bytes(self) -> u64 {
        match self {
            Self::Adjacent {
                max_payload_bytes, ..
            }
            | Self::Validation {
                max_payload_bytes, ..
            } => max_payload_bytes,
        }
    }
}

/// Exact immutable key of a page demand and its response.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct PageRequestKey {
    id: PageRequestId,
    binding: BindingId,
    revision: SourceRevision,
    purpose: PagePurpose,
    demand: PageDemandEnvelope,
}

impl PageRequestKey {
    /// Creates a bounded adjacent demand from a proven UTF-8 anchor.
    pub fn adjacent(
        id: PageRequestId,
        binding: BindingId,
        revision: SourceRevision,
        purpose: PagePurpose,
        anchor: ByteOffset,
        direction: PageDirection,
        max_payload_bytes: u64,
    ) -> Result<Self, RangeContractError> {
        Self::with_demand(
            id,
            binding,
            revision,
            purpose,
            PageDemandEnvelope::Adjacent {
                anchor,
                direction,
                max_payload_bytes,
            },
        )
    }

    /// Creates a bounded demand that validates one untrusted byte offset.
    pub fn validation(
        id: PageRequestId,
        binding: BindingId,
        revision: SourceRevision,
        purpose: PagePurpose,
        candidate: ByteOffset,
        max_payload_bytes: u64,
    ) -> Result<Self, RangeContractError> {
        Self::with_demand(
            id,
            binding,
            revision,
            purpose,
            PageDemandEnvelope::Validation {
                candidate,
                max_payload_bytes,
            },
        )
    }

    fn with_demand(
        id: PageRequestId,
        binding: BindingId,
        revision: SourceRevision,
        purpose: PagePurpose,
        demand: PageDemandEnvelope,
    ) -> Result<Self, RangeContractError> {
        if demand.max_payload_bytes() < 4 {
            return Err(RangeContractError::PagePayloadLimitTooSmall {
                max_payload_bytes: demand.max_payload_bytes(),
            });
        }
        Ok(Self {
            id,
            binding,
            revision,
            purpose,
            demand,
        })
    }

    /// Returns the unique request identity.
    pub const fn id(self) -> PageRequestId {
        self.id
    }

    /// Returns the host binding identity.
    pub const fn binding(self) -> BindingId {
        self.binding
    }

    /// Returns the exact source revision.
    pub const fn revision(self) -> SourceRevision {
        self.revision
    }

    /// Returns the request purpose.
    pub const fn purpose(self) -> PagePurpose {
        self.purpose
    }

    /// Returns the frozen source-selection envelope.
    pub const fn demand(self) -> PageDemandEnvelope {
        self.demand
    }

    /// Returns the maximum returned UTF-8 payload length.
    pub const fn max_payload_bytes(self) -> u64 {
        self.demand.max_payload_bytes()
    }
}

/// One bounded range request for dispatch to the host.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct PageRequest {
    key: PageRequestKey,
}

impl PageRequest {
    /// Creates a request from its exact key.
    pub const fn new(key: PageRequestKey) -> Self {
        Self { key }
    }

    /// Returns the exact request/response key.
    pub const fn key(self) -> PageRequestKey {
        self.key
    }
}
