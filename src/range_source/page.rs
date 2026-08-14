use std::fmt;

use super::{
    AtomId, ByteOffset, ByteRange, LineOffset, LineRange, PageDemandEnvelope, PageDirection,
    PageId, PageRequestKey,
};

#[path = "page_impl.rs"]
mod page_impl;

/// Authoritative fact about one edge of a returned page.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum PageEdgeFact {
    /// The page edge is also the logical document edge.
    DocumentBoundary,
    /// Source text exists beyond this page edge.
    Continues,
}

/// An authoritative opaque atom occurrence within one page.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AtomFact {
    id: AtomId,
    global_range: ByteRange,
    fragment_range: ByteRange,
    fallback_copy: String,
}

impl AtomFact {
    /// Creates one authoritative page fragment of an atom.
    ///
    /// [`RangePage::new`] verifies that `fragment_range` is the exact intersection of
    /// `global_range` with that page and accepts at most one fact per `id` in a page. Adjacent
    /// pages reconcile fragments by `id`, global range, and fallback text without retaining a
    /// whole-source atom registry.
    pub fn new(
        id: AtomId,
        global_range: ByteRange,
        fragment_range: ByteRange,
        fallback_copy: impl Into<String>,
    ) -> Self {
        Self {
            id,
            global_range,
            fragment_range,
            fallback_copy: fallback_copy.into(),
        }
    }

    /// Returns the opaque host-owned atom identity.
    pub const fn id(&self) -> AtomId {
        self.id
    }

    /// Returns the atom's authoritative global visible byte range.
    pub const fn global_range(&self) -> ByteRange {
        self.global_range
    }

    /// Returns this page's exact nonempty intersection with the global atom range.
    pub const fn fragment_range(&self) -> ByteRange {
        self.fragment_range
    }

    /// Returns the authoritative plain-text clipboard fallback.
    pub fn fallback_copy(&self) -> &str {
        &self.fallback_copy
    }

    /// Reports whether two page fragments carry the same stable atom facts.
    pub fn reconciles_with(&self, other: &Self) -> bool {
        self.id == other.id
            && self.global_range == other.global_range
            && self.fallback_copy == other.fallback_copy
    }

    fn retained_bytes(&self) -> usize {
        self.fallback_copy.len()
    }
}

/// One exact, bounded UTF-8 page returned by a host.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RangePage {
    id: PageId,
    key: PageRequestKey,
    range: ByteRange,
    text: String,
    atoms: Vec<AtomFact>,
    preceding: PageEdgeFact,
    following: PageEdgeFact,
    end_of_source: bool,
    retained_bytes: usize,
    retained_charge: RangePageCharge,
}

/// Exact borrowed-record charge for one validated range page.
///
/// Bytes follow the crate's semantic-record model: the initialized [`RangePage`] record, every
/// initialized [`AtomFact`] record, and their retained UTF-8 payloads are counted exactly once.
/// Container headers are already part of their enclosing records, while allocator spare capacity
/// is excluded consistently with GPUI streaming-layout charges.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RangePageCharge {
    bytes: usize,
    items: usize,
}

impl RangePageCharge {
    /// Complete borrowed record and payload bytes.
    pub const fn bytes(self) -> usize {
        self.bytes
    }

    /// Page and atom semantic records.
    pub const fn items(self) -> usize {
        self.items
    }
}

/// Terminal host-side failure for an exact page request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PageFailure {
    /// The exact request was cancelled before success.
    Cancelled,
    /// The exact requested source range is currently unavailable.
    Unavailable,
    /// The host response could not satisfy the validated page contract.
    Malformed,
}

/// Malformed range-source input rejected at the public boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum RangeContractError {
    /// A byte range ended before it started.
    ReversedByteRange { start: ByteOffset, end: ByteOffset },
    /// A logical-line range ended before it started.
    ReversedLineRange { start: LineOffset, end: LineOffset },
    /// A byte range exceeded the bound revision's extent.
    ByteRangeOutsideExtent { range: ByteRange, byte_len: u64 },
    /// A logical-line range exceeded the bound revision's extent.
    LineRangeOutsideExtent { range: LineRange, line_count: u64 },
    /// A page demand cannot guarantee UTF-8 scalar progress below four payload bytes.
    PagePayloadLimitTooSmall { max_payload_bytes: u64 },
    /// A source-selected range did not satisfy its frozen demand envelope.
    ReturnedRangeOutsideEnvelope {
        demand: PageDemandEnvelope,
        returned: ByteRange,
    },
    /// An adjacent response made no progress away from a non-document-edge anchor.
    NonProgressingPage {
        anchor: ByteOffset,
        direction: PageDirection,
    },
    /// Page text length did not equal the returned range length.
    PayloadLengthMismatch {
        range: ByteRange,
        actual_bytes: usize,
    },
    /// Page edge facts contradicted one another.
    MalformedEdgeFacts,
    /// An atom's global range or exact page fragment was malformed.
    MalformedAtomRange {
        atom: AtomId,
        global_range: ByteRange,
        fragment_range: ByteRange,
    },
    /// One exact page repeated an atom identity instead of supplying one intersection fact.
    DuplicateAtomFact { atom: AtomId },
    /// Fragments using one stable atom identity disagreed on authoritative facts.
    ConflictingAtomFacts { atom: AtomId },
    /// Different atom identities claimed overlapping authoritative global ranges.
    OverlappingAtomFacts { first: AtomId, second: AtomId },
    /// Retained payload byte accounting overflowed.
    PayloadByteCountOverflow,
}

impl fmt::Display for RangeContractError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "malformed range-source contract: {self:?}")
    }
}

impl std::error::Error for RangeContractError {}
