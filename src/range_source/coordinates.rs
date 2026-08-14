use super::RangeContractError;

macro_rules! opaque_id {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name(u64);

        impl $name {
            /// Wraps a host-assigned opaque value.
            pub const fn new(value: u64) -> Self {
                Self(value)
            }

            /// Returns the opaque value for host-side correlation.
            pub const fn get(self) -> u64 {
                self.0
            }
        }
    };
}

opaque_id!(BindingId, "Opaque identity of one host-owned text source.");
opaque_id!(
    SourceRevision,
    "Opaque revision of a host-owned text source."
);
opaque_id!(
    PageRequestId,
    "Unique identity of one bounded page request."
);
opaque_id!(PageId, "Stable identity of one returned page payload.");
opaque_id!(AtomId, "Opaque host-owned identity of an inline atom.");

/// An absolute byte offset in a logical UTF-8 source.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ByteOffset(u64);

impl ByteOffset {
    /// Creates an absolute byte offset.
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the absolute byte offset.
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// A checked half-open absolute byte range.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ByteRange {
    start: ByteOffset,
    end: ByteOffset,
}

impl ByteRange {
    /// Creates a range, rejecting an end before its start.
    pub fn new(start: ByteOffset, end: ByteOffset) -> Result<Self, RangeContractError> {
        if end < start {
            return Err(RangeContractError::ReversedByteRange { start, end });
        }
        Ok(Self { start, end })
    }

    /// Creates a range from raw absolute offsets.
    pub fn from_u64(start: u64, end: u64) -> Result<Self, RangeContractError> {
        Self::new(ByteOffset::new(start), ByteOffset::new(end))
    }

    /// Returns the inclusive start offset.
    pub const fn start(self) -> ByteOffset {
        self.start
    }

    /// Returns the exclusive end offset.
    pub const fn end(self) -> ByteOffset {
        self.end
    }

    /// Returns the range length in bytes.
    pub const fn len(self) -> u64 {
        self.end.0 - self.start.0
    }

    /// Reports whether the range is empty.
    pub const fn is_empty(self) -> bool {
        self.start.0 == self.end.0
    }

    /// Reports whether this range contains `offset`, including its terminal edge.
    pub const fn contains_offset(self, offset: ByteOffset) -> bool {
        self.start.0 <= offset.0 && offset.0 <= self.end.0
    }

    /// Reports whether this range completely contains `other`.
    pub const fn contains(self, other: Self) -> bool {
        self.start.0 <= other.start.0 && other.end.0 <= self.end.0
    }

    /// Reports whether two non-empty half-open ranges overlap.
    pub const fn overlaps(self, other: Self) -> bool {
        self.start.0 < other.end.0 && other.start.0 < self.end.0
    }

    /// Returns the non-empty intersection of two ranges.
    pub fn intersection(self, other: Self) -> Option<Self> {
        let start = self.start.max(other.start);
        let end = self.end.min(other.end);
        (start < end).then_some(Self { start, end })
    }
}

/// An absolute zero-based logical-line offset.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct LineOffset(u64);

impl LineOffset {
    /// Creates an absolute logical-line offset.
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the absolute logical-line offset.
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// A checked half-open logical-line range.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct LineRange {
    start: LineOffset,
    end: LineOffset,
}

impl LineRange {
    /// Creates a logical-line range, rejecting reversed endpoints.
    pub fn new(start: LineOffset, end: LineOffset) -> Result<Self, RangeContractError> {
        if end < start {
            return Err(RangeContractError::ReversedLineRange { start, end });
        }
        Ok(Self { start, end })
    }

    /// Returns the inclusive start line.
    pub const fn start(self) -> LineOffset {
        self.start
    }

    /// Returns the exclusive end line.
    pub const fn end(self) -> LineOffset {
        self.end
    }

    /// Returns the number of logical lines.
    pub const fn len(self) -> u64 {
        self.end.0 - self.start.0
    }

    /// Reports whether the range is empty.
    pub const fn is_empty(self) -> bool {
        self.start.0 == self.end.0
    }
}

/// Checked byte and logical-line extents of one exact source revision.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct LogicalExtent {
    byte_len: u64,
    line_count: u64,
}

impl LogicalExtent {
    /// Creates an extent. Zero bytes and zero logical lines are valid.
    pub const fn new(byte_len: u64, line_count: u64) -> Self {
        Self {
            byte_len,
            line_count,
        }
    }

    /// Returns total logical UTF-8 bytes.
    pub const fn byte_len(self) -> u64 {
        self.byte_len
    }

    /// Returns total logical lines.
    pub const fn line_count(self) -> u64 {
        self.line_count
    }

    /// Checks a byte range against this extent.
    pub fn check_byte_range(self, range: ByteRange) -> Result<(), RangeContractError> {
        if range.end().get() > self.byte_len {
            return Err(RangeContractError::ByteRangeOutsideExtent {
                range,
                byte_len: self.byte_len,
            });
        }
        Ok(())
    }

    /// Checks a logical-line range against this extent.
    pub fn check_line_range(self, range: LineRange) -> Result<(), RangeContractError> {
        if range.end().get() > self.line_count {
            return Err(RangeContractError::LineRangeOutsideExtent {
                range,
                line_count: self.line_count,
            });
        }
        Ok(())
    }
}

/// Exact identity and extent of a current host-owned source revision.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct RangeBinding {
    binding: BindingId,
    revision: SourceRevision,
    extent: LogicalExtent,
}

impl RangeBinding {
    /// Creates an exact source binding.
    pub const fn new(binding: BindingId, revision: SourceRevision, extent: LogicalExtent) -> Self {
        Self {
            binding,
            revision,
            extent,
        }
    }

    /// Returns the host binding identity.
    pub const fn binding(self) -> BindingId {
        self.binding
    }

    /// Returns the exact source revision.
    pub const fn revision(self) -> SourceRevision {
        self.revision
    }

    /// Returns the exact logical extent.
    pub const fn extent(self) -> LogicalExtent {
        self.extent
    }
}
