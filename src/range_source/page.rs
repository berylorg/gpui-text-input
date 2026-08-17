use std::fmt;

use super::{
    AtomId, ByteOffset, ByteRange, LineOffset, LineRange, PageDemandEnvelope, PageDirection,
    PageId, PageRequestKey,
};

#[path = "page_impl.rs"]
mod page_impl;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum PageEdgeFact {
    DocumentBoundary,
    Continues,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AtomFact {
    id: AtomId,
    global_range: ByteRange,
    fragment_range: ByteRange,
    fallback_copy: String,
}

impl AtomFact {
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

    pub const fn id(&self) -> AtomId {
        self.id
    }

    pub const fn global_range(&self) -> ByteRange {
        self.global_range
    }

    pub const fn fragment_range(&self) -> ByteRange {
        self.fragment_range
    }

    pub fn fallback_copy(&self) -> &str {
        &self.fallback_copy
    }

    pub fn reconciles_with(&self, other: &Self) -> bool {
        self.id == other.id
            && self.global_range == other.global_range
            && self.fallback_copy == other.fallback_copy
    }

    fn retained_bytes(&self) -> usize {
        self.fallback_copy.len()
    }
}

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

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RangePageCharge {
    bytes: usize,
    items: usize,
}

impl RangePageCharge {
    pub const fn bytes(self) -> usize {
        self.bytes
    }

    pub const fn items(self) -> usize {
        self.items
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PageFailure {
    Cancelled,
    Unavailable,
    Malformed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum RangeContractError {
    ReversedByteRange {
        start: ByteOffset,
        end: ByteOffset,
    },
    ReversedLineRange {
        start: LineOffset,
        end: LineOffset,
    },
    ByteRangeOutsideExtent {
        range: ByteRange,
        byte_len: u64,
    },
    LineRangeOutsideExtent {
        range: LineRange,
        line_count: u64,
    },
    PagePayloadLimitTooSmall {
        max_payload_bytes: u64,
    },
    ReturnedRangeOutsideEnvelope {
        demand: PageDemandEnvelope,
        returned: ByteRange,
    },
    NonProgressingPage {
        anchor: ByteOffset,
        direction: PageDirection,
    },
    PayloadLengthMismatch {
        range: ByteRange,
        actual_bytes: usize,
    },
    MalformedEdgeFacts,
    MalformedAtomRange {
        atom: AtomId,
        global_range: ByteRange,
        fragment_range: ByteRange,
    },
    DuplicateAtomFact {
        atom: AtomId,
    },
    ConflictingAtomFacts {
        atom: AtomId,
    },
    OverlappingAtomFacts {
        first: AtomId,
        second: AtomId,
    },
    PayloadByteCountOverflow,
}

impl fmt::Display for RangeContractError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "malformed range-source contract: {self:?}")
    }
}

impl std::error::Error for RangeContractError {}
