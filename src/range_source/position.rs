use std::cmp::Ordering;

use gpui::{
    StreamingLayoutPosition, StreamingObjectEdge, StreamingObjectGap, StreamingObjectId,
    StreamingObjectOrder,
};

use super::ByteOffset;

/// Stable opaque host-owned identity of one source-zero-width inline object.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct InlineObjectId(u128);

impl InlineObjectId {
    /// Wraps a host-assigned opaque identity without interpreting it.
    pub const fn new(value: u128) -> Self {
        Self(value)
    }

    /// Returns the exact opaque value for host and GPUI correlation.
    pub const fn get(self) -> u128 {
        self.0
    }
}

/// Opaque host-owned total-order key for objects sharing one byte anchor.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct InlineObjectOrder(u128);

impl InlineObjectOrder {
    /// Wraps one host-assigned same-anchor order key.
    pub const fn new(value: u128) -> Self {
        Self(value)
    }

    /// Returns the exact opaque ordering value.
    pub const fn get(self) -> u128 {
        self.0
    }
}

/// One named object edge in an adjacent-object gap witness.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct InlineObjectNeighbor {
    id: InlineObjectId,
    order: InlineObjectOrder,
}

impl InlineObjectNeighbor {
    /// Creates one object edge from its stable identity and same-anchor order key.
    pub const fn new(id: InlineObjectId, order: InlineObjectOrder) -> Self {
        Self { id, order }
    }

    /// Returns the stable object identity.
    pub const fn id(self) -> InlineObjectId {
        self.id
    }

    /// Returns the object's same-anchor order key.
    pub const fn order(self) -> InlineObjectOrder {
        self.order
    }
}

/// Constant-size exact witness for one source position at an inline-object anchor.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum InlineObjectGap {
    /// No source-zero-width objects exist at the byte anchor.
    NoObjects,
    /// The position precedes the first named object.
    Before(InlineObjectNeighbor),
    /// The position lies between the two named immediately adjacent objects.
    Between {
        /// Immediately preceding object.
        preceding: InlineObjectNeighbor,
        /// Immediately following object.
        following: InlineObjectNeighbor,
    },
    /// The position follows the last named object.
    After(InlineObjectNeighbor),
}

impl InlineObjectGap {
    /// Creates the sole position at an anchor proven to have no inline objects.
    pub const fn no_objects() -> Self {
        Self::NoObjects
    }

    /// Creates the gap before a proven first object.
    pub const fn before(first: InlineObjectNeighbor) -> Self {
        Self::Before(first)
    }

    /// Creates the gap between two proven adjacent objects.
    pub fn between(
        preceding: InlineObjectNeighbor,
        following: InlineObjectNeighbor,
    ) -> Result<Self, InlineObjectGapError> {
        if preceding.id == following.id {
            return Err(InlineObjectGapError::DuplicateIdentity(preceding.id));
        }
        if preceding.order >= following.order {
            return Err(InlineObjectGapError::NonIncreasingOrder {
                preceding: preceding.order,
                following: following.order,
            });
        }
        Ok(Self::Between {
            preceding,
            following,
        })
    }

    /// Creates the gap after a proven last object.
    pub const fn after(last: InlineObjectNeighbor) -> Self {
        Self::After(last)
    }

    /// Checks the relative order of two gaps at the same proven byte anchor.
    ///
    /// The caller remains responsible for proving both witnesses against the same binding and
    /// revision. `None` is returned when a no-object witness is compared with an object witness.
    pub fn compare_at_same_anchor(self, other: Self) -> Option<Ordering> {
        use InlineObjectGap::{After, Before, Between, NoObjects};
        match (self, other) {
            (NoObjects, NoObjects) => Some(Ordering::Equal),
            (NoObjects, _) | (_, NoObjects) => None,
            (Before(left), Before(right)) => (left == right).then_some(Ordering::Equal),
            (After(left), After(right)) => (left == right).then_some(Ordering::Equal),
            (Before(_), _) | (_, After(_)) => Some(Ordering::Less),
            (After(_), _) | (_, Before(_)) => Some(Ordering::Greater),
            (
                Between {
                    preceding: left_preceding,
                    following: left_following,
                },
                Between {
                    preceding: right_preceding,
                    following: right_following,
                },
            ) => {
                if left_preceding == right_preceding && left_following == right_following {
                    Some(Ordering::Equal)
                } else {
                    Some(
                        (left_preceding.order, left_preceding.id)
                            .cmp(&(right_preceding.order, right_preceding.id)),
                    )
                }
            }
        }
    }
}

/// Rejection of a locally malformed adjacent-object witness.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InlineObjectGapError {
    /// Both edges named the same object identity.
    DuplicateIdentity(InlineObjectId),
    /// A between-gap did not name strictly increasing order keys.
    NonIncreasingOrder {
        /// Preceding order key.
        preceding: InlineObjectOrder,
        /// Following order key.
        following: InlineObjectOrder,
    },
    /// A GPUI gap used an unsupported or malformed edge combination.
    InvalidGpuiGap,
}

/// Fixed-size canonical position in one revision's ordered text-and-object stream.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct SourcePosition {
    /// Proven UTF-8 byte boundary in the authoritative source.
    pub byte_offset: ByteOffset,
    /// Proven adjacent-object gap at that byte boundary.
    pub gap: InlineObjectGap,
}

impl SourcePosition {
    /// Creates a claimed composite source position.
    ///
    /// Mutation entry points validate the claim against admitted text and object residency before
    /// retaining it as proof-backed state.
    pub const fn new(byte_offset: ByteOffset, gap: InlineObjectGap) -> Self {
        Self { byte_offset, gap }
    }

    /// Checks the composite ordering of two positions from the same exact revision.
    pub fn compare_in_revision(self, other: Self) -> Option<Ordering> {
        match self.byte_offset.cmp(&other.byte_offset) {
            Ordering::Equal => self.gap.compare_at_same_anchor(other.gap),
            ordering => Some(ordering),
        }
    }
}

/// Checked half-open range in the composite source stream.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct SourceRange {
    start: SourcePosition,
    end: SourcePosition,
}

impl SourceRange {
    /// Creates a claimed composite range, rejecting reversed or incompatible same-anchor gaps.
    ///
    /// Mutation entry points separately require admitted residency proof for both endpoints.
    pub fn new(start: SourcePosition, end: SourcePosition) -> Result<Self, SourceRangeError> {
        match start.compare_in_revision(end) {
            Some(Ordering::Greater) => Err(SourceRangeError::Reversed { start, end }),
            None => Err(SourceRangeError::IncompatibleGapWitnesses { start, end }),
            Some(Ordering::Equal | Ordering::Less) => Ok(Self { start, end }),
        }
    }

    /// Returns the inclusive composite start.
    pub const fn start(self) -> SourcePosition {
        self.start
    }

    /// Returns the exclusive composite end.
    pub const fn end(self) -> SourcePosition {
        self.end
    }

    /// Reports whether the range is empty.
    pub fn is_empty(self) -> bool {
        self.start.byte_offset.get() == self.end.byte_offset.get() && self.start.gap == self.end.gap
    }
}

/// Rejection of an invalid composite source range.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceRangeError {
    /// The end precedes the start.
    Reversed {
        /// Proposed start.
        start: SourcePosition,
        /// Proposed end.
        end: SourcePosition,
    },
    /// Same-anchor witnesses cannot both describe one coherent object collection.
    IncompatibleGapWitnesses {
        /// Proposed start.
        start: SourcePosition,
        /// Proposed end.
        end: SourcePosition,
    },
}

impl From<InlineObjectId> for StreamingObjectId {
    fn from(value: InlineObjectId) -> Self {
        Self(value.get())
    }
}

impl From<StreamingObjectId> for InlineObjectId {
    fn from(value: StreamingObjectId) -> Self {
        Self::new(value.0)
    }
}

impl From<InlineObjectOrder> for StreamingObjectOrder {
    fn from(value: InlineObjectOrder) -> Self {
        Self(value.get())
    }
}

impl From<StreamingObjectOrder> for InlineObjectOrder {
    fn from(value: StreamingObjectOrder) -> Self {
        Self::new(value.0)
    }
}

impl From<InlineObjectNeighbor> for (StreamingObjectId, StreamingObjectOrder) {
    fn from(value: InlineObjectNeighbor) -> Self {
        (value.id.into(), value.order.into())
    }
}

impl From<InlineObjectGap> for StreamingObjectGap {
    fn from(value: InlineObjectGap) -> Self {
        match value {
            InlineObjectGap::NoObjects => Self::no_objects(),
            InlineObjectGap::Before(first) => Self::before(first.id.into(), first.order.into()),
            InlineObjectGap::Between {
                preceding,
                following,
            } => Self::between(
                preceding.id.into(),
                preceding.order.into(),
                following.id.into(),
                following.order.into(),
            ),
            InlineObjectGap::After(last) => Self::after(last.id.into(), last.order.into()),
        }
    }
}

impl TryFrom<StreamingObjectGap> for InlineObjectGap {
    type Error = InlineObjectGapError;

    fn try_from(value: StreamingObjectGap) -> Result<Self, Self::Error> {
        let neighbor = |edge| match edge {
            StreamingObjectEdge::Object { id, order } => {
                Some(InlineObjectNeighbor::new(id.into(), order.into()))
            }
            _ => None,
        };
        match (value.preceding, value.following) {
            (StreamingObjectEdge::NoObject, StreamingObjectEdge::NoObject) => Ok(Self::NoObjects),
            (StreamingObjectEdge::BeforeAll, following) => neighbor(following)
                .map(Self::Before)
                .ok_or(InlineObjectGapError::InvalidGpuiGap),
            (preceding, StreamingObjectEdge::AfterAll) => neighbor(preceding)
                .map(Self::After)
                .ok_or(InlineObjectGapError::InvalidGpuiGap),
            (preceding, following) => Self::between(
                neighbor(preceding).ok_or(InlineObjectGapError::InvalidGpuiGap)?,
                neighbor(following).ok_or(InlineObjectGapError::InvalidGpuiGap)?,
            ),
        }
    }
}

impl From<SourcePosition> for StreamingLayoutPosition {
    fn from(value: SourcePosition) -> Self {
        Self::with_gap(value.byte_offset.get(), value.gap.into())
    }
}

impl TryFrom<StreamingLayoutPosition> for SourcePosition {
    type Error = InlineObjectGapError;

    fn try_from(value: StreamingLayoutPosition) -> Result<Self, Self::Error> {
        Ok(Self::new(
            ByteOffset::new(value.byte_offset),
            value.gap.try_into()?,
        ))
    }
}
