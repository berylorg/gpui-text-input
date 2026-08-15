use crate::range_source::{
    BindingId, ByteOffset, ByteRange, InlineObjectId, InlineObjectNeighbor, InlineObjectOrder,
    SourceRevision,
};

use super::{ObjectContractError, ObjectRequestId, PresentationGeneration};

/// App-neutral reason a bounded inline-object page is required.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum ObjectPurpose {
    /// Visible viewport and overscan realization.
    Viewport,
    /// Caret movement or adjacent-object validation.
    Caret,
    /// Selection movement or adjacent-object validation.
    Selection,
    /// Bounded clipboard representation construction.
    Clipboard,
    /// Background visual-line or geometry indexing.
    GeometryIndex,
    /// Exact block-target geometry resolution.
    GeometryTarget,
    /// Validation and realization of a compact restoration seed.
    Restoration,
    /// Validation of one committed mutation's successor positions.
    MutationSuccessor,
    /// Platform text-range or replacement coordination.
    PlatformRange,
}

/// Direction of one bounded object demand.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ObjectDirection {
    /// Objects follow the optional cursor in source order.
    Forward,
    /// Objects precede the optional cursor, while each page remains source ordered.
    Backward,
}

/// Exact cursor of one object in the ordered source stream.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ObjectCursor {
    anchor: ByteOffset,
    order: InlineObjectOrder,
    id: InlineObjectId,
}

impl ObjectCursor {
    /// Creates one exact same-revision object cursor.
    pub const fn new(anchor: ByteOffset, order: InlineObjectOrder, id: InlineObjectId) -> Self {
        Self { anchor, order, id }
    }

    /// Returns the object's UTF-8 anchor.
    pub const fn anchor(self) -> ByteOffset {
        self.anchor
    }

    /// Returns its same-anchor order key.
    pub const fn order(self) -> InlineObjectOrder {
        self.order
    }

    /// Returns its stable identity.
    pub const fn id(self) -> InlineObjectId {
        self.id
    }

    /// Returns the named edge used in a source-position witness.
    pub const fn neighbor(self) -> InlineObjectNeighbor {
        InlineObjectNeighbor::new(self.id, self.order)
    }
}

/// Source-selection envelope frozen by one inline-object request.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ObjectDemandEnvelope {
    /// Requests objects anchored anywhere in one bounded byte interval.
    Range {
        /// Inclusive anchor interval. Objects consume no bytes, so both edges are eligible.
        range: ByteRange,
        /// Optional exclusive object cursor for continuation.
        cursor: Option<ObjectCursor>,
        /// Direction of progress from the cursor or interval edge.
        direction: ObjectDirection,
        /// Maximum objects in the response.
        max_objects: usize,
        /// Maximum exact retained response bytes.
        max_retained_bytes: usize,
    },
    /// Requests objects at one exact proven UTF-8 anchor.
    Anchor {
        /// Exact eligible anchor.
        anchor: ByteOffset,
        /// Optional exclusive same-anchor cursor for continuation.
        cursor: Option<ObjectCursor>,
        /// Direction of progress.
        direction: ObjectDirection,
        /// Maximum objects in the response.
        max_objects: usize,
        /// Maximum exact retained response bytes.
        max_retained_bytes: usize,
    },
}

impl ObjectDemandEnvelope {
    /// Creates a checked bounded interval demand.
    pub fn range(
        range: ByteRange,
        cursor: Option<ObjectCursor>,
        direction: ObjectDirection,
        max_objects: usize,
        max_retained_bytes: usize,
    ) -> Result<Self, ObjectContractError> {
        let demand = Self::Range {
            range,
            cursor,
            direction,
            max_objects,
            max_retained_bytes,
        };
        demand.validate_local()?;
        Ok(demand)
    }

    /// Creates a checked exact-anchor demand.
    pub fn anchor(
        anchor: ByteOffset,
        cursor: Option<ObjectCursor>,
        direction: ObjectDirection,
        max_objects: usize,
        max_retained_bytes: usize,
    ) -> Result<Self, ObjectContractError> {
        let demand = Self::Anchor {
            anchor,
            cursor,
            direction,
            max_objects,
            max_retained_bytes,
        };
        demand.validate_local()?;
        Ok(demand)
    }

    /// Returns the optional exclusive continuation cursor.
    pub const fn cursor(self) -> Option<ObjectCursor> {
        match self {
            Self::Range { cursor, .. } | Self::Anchor { cursor, .. } => cursor,
        }
    }

    /// Returns the direction of object progress.
    pub const fn direction(self) -> ObjectDirection {
        match self {
            Self::Range { direction, .. } | Self::Anchor { direction, .. } => direction,
        }
    }

    /// Returns the response object-count ceiling.
    pub const fn max_objects(self) -> usize {
        match self {
            Self::Range { max_objects, .. } | Self::Anchor { max_objects, .. } => max_objects,
        }
    }

    /// Returns the exact retained-byte ceiling.
    pub const fn max_retained_bytes(self) -> usize {
        match self {
            Self::Range {
                max_retained_bytes, ..
            }
            | Self::Anchor {
                max_retained_bytes, ..
            } => max_retained_bytes,
        }
    }

    /// Reports whether an anchor is eligible, including both interval edges.
    pub const fn contains_anchor(self, anchor: ByteOffset) -> bool {
        match self {
            Self::Range { range, .. } => range.contains_offset(anchor),
            Self::Anchor {
                anchor: expected, ..
            } => anchor.get() == expected.get(),
        }
    }

    pub(crate) fn validate_local(self) -> Result<(), ObjectContractError> {
        if self.max_objects() == 0 {
            return Err(ObjectContractError::ZeroObjectLimit);
        }
        if self.max_retained_bytes() == 0 {
            return Err(ObjectContractError::ZeroRetainedByteLimit);
        }
        if self
            .cursor()
            .is_some_and(|cursor| !self.contains_anchor(cursor.anchor()))
        {
            return Err(ObjectContractError::CursorOutsideEnvelope);
        }
        Ok(())
    }
}

/// Exact immutable key of an object demand and its response.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ObjectRequestKey {
    id: ObjectRequestId,
    binding: BindingId,
    revision: SourceRevision,
    presentation_generation: PresentationGeneration,
    purpose: ObjectPurpose,
    demand: ObjectDemandEnvelope,
}

impl ObjectRequestKey {
    /// Creates a request key from one already checked bounded demand.
    pub fn new(
        id: ObjectRequestId,
        binding: BindingId,
        revision: SourceRevision,
        presentation_generation: PresentationGeneration,
        purpose: ObjectPurpose,
        demand: ObjectDemandEnvelope,
    ) -> Result<Self, ObjectContractError> {
        demand.validate_local()?;
        Ok(Self {
            id,
            binding,
            revision,
            presentation_generation,
            purpose,
            demand,
        })
    }

    /// Returns the unique request identity.
    pub const fn id(self) -> ObjectRequestId {
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

    /// Returns the immutable presentation generation.
    pub const fn presentation_generation(self) -> PresentationGeneration {
        self.presentation_generation
    }

    /// Returns the request purpose.
    pub const fn purpose(self) -> ObjectPurpose {
        self.purpose
    }

    /// Returns the exact frozen demand envelope.
    pub const fn demand(self) -> ObjectDemandEnvelope {
        self.demand
    }
}

/// One bounded inline-object request for dispatch to the host.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ObjectRequest {
    key: ObjectRequestKey,
}

impl ObjectRequest {
    /// Creates a request from its exact immutable key.
    pub const fn new(key: ObjectRequestKey) -> Self {
        Self { key }
    }

    /// Returns the exact request/response key.
    pub const fn key(self) -> ObjectRequestKey {
        self.key
    }
}
