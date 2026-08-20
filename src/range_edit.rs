use crate::{
    AtomId, BindingId, ByteRange, InlineObjectGap, InlineObjectId, InlineObjectOrder,
    LogicalExtent, ObjectResidency, RangeBinding, RangeResidency, SourcePosition, SourceRange,
    SourceRevision,
};

mod coordinator;
mod lifecycle;
mod preflight;
mod proof;
mod protocol;
mod settlement;
mod staging;

pub use proof::*;
pub use protocol::*;

macro_rules! opaque_id {
    ($name:ident, $doc:literal) => {
        #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name(u64);

        impl $name {
            pub const fn new(value: u64) -> Self {
                Self(value)
            }

            pub const fn get(self) -> u64 {
                self.0
            }
        }
    };
}

opaque_id!(OperationId, "Unique identity of one range-backed mutation.");

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MutationKind {
    Edit,
    Undo,
    Redo,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct MutationKey {
    binding: BindingId,
    base_revision: SourceRevision,
    operation: OperationId,
}

impl MutationKey {
    pub const fn new(
        binding: BindingId,
        base_revision: SourceRevision,
        operation: OperationId,
    ) -> Self {
        Self {
            binding,
            base_revision,
            operation,
        }
    }

    pub const fn binding(self) -> BindingId {
        self.binding
    }

    pub const fn base_revision(self) -> SourceRevision {
        self.base_revision
    }

    pub const fn operation(self) -> OperationId {
        self.operation
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MutationProposal {
    key: MutationKey,
    kind: MutationKind,
    predecessor: MutationPositions,
    replacement: SourceRange,
    replacement_line_breaks: u64,
}

impl MutationProposal {
    pub const fn new(
        key: MutationKey,
        kind: MutationKind,
        predecessor: MutationPositions,
        replacement: SourceRange,
        replacement_line_breaks: u64,
    ) -> Self {
        Self {
            key,
            kind,
            predecessor,
            replacement,
            replacement_line_breaks,
        }
    }

    pub const fn key(self) -> MutationKey {
        self.key
    }

    pub const fn kind(self) -> MutationKind {
        self.kind
    }

    pub const fn predecessor(self) -> MutationPositions {
        self.predecessor
    }

    pub const fn replacement(self) -> SourceRange {
        self.replacement
    }

    pub fn replacement_bytes(self) -> ByteRange {
        ByteRange::new(
            self.replacement.start().byte_offset,
            self.replacement.end().byte_offset,
        )
        .expect("source range byte offsets are ordered")
    }

    pub const fn replacement_line_breaks(self) -> u64 {
        self.replacement_line_breaks
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MutationLimits {
    max_page_items: usize,
    max_page_bytes: usize,
    max_page_objects: usize,
    max_page_object_bytes: usize,
    max_page_presentation_bytes: usize,
}

impl MutationLimits {
    pub fn new(max_page_items: usize, max_page_bytes: usize) -> Result<Self, MutationError> {
        if max_page_items == 0 || max_page_bytes == 0 {
            return Err(MutationError::InvalidLimits);
        }
        Ok(Self {
            max_page_items,
            max_page_bytes,
            max_page_objects: max_page_items,
            max_page_object_bytes: max_page_bytes,
            max_page_presentation_bytes: max_page_bytes,
        })
    }

    pub const fn max_page_items(self) -> usize {
        self.max_page_items
    }

    pub const fn max_page_bytes(self) -> usize {
        self.max_page_bytes
    }

    pub fn with_object_limits(
        mut self,
        max_page_objects: usize,
        max_page_object_bytes: usize,
        max_page_presentation_bytes: usize,
    ) -> Result<Self, MutationError> {
        if max_page_objects == 0 {
            return Err(MutationError::InvalidLimits);
        }
        self.max_page_objects = max_page_objects;
        self.max_page_object_bytes = max_page_object_bytes;
        self.max_page_presentation_bytes = max_page_presentation_bytes;
        Ok(self)
    }

    pub const fn max_page_objects(self) -> usize {
        self.max_page_objects
    }

    pub const fn max_page_object_bytes(self) -> usize {
        self.max_page_object_bytes
    }

    pub const fn max_page_presentation_bytes(self) -> usize {
        self.max_page_presentation_bytes
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AtomChange {
    Insert {
        id: AtomId,
        inserted_range: ByteRange,
        fallback_copy: Box<str>,
    },
    Remove {
        id: AtomId,
        source_range: ByteRange,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ObjectTarget {
    range: SourceRange,
    id: InlineObjectId,
    order: InlineObjectOrder,
}

impl ObjectTarget {
    pub fn new(
        range: SourceRange,
        id: InlineObjectId,
        order: InlineObjectOrder,
    ) -> Result<Self, MutationError> {
        if range.start().byte_offset != range.end().byte_offset || range.is_empty() {
            return Err(MutationError::MalformedObjectChange);
        }
        let follows_start = match range.start().gap {
            InlineObjectGap::Before(next) => next.id() == id && next.order() == order,
            InlineObjectGap::Between { following, .. } => {
                following.id() == id && following.order() == order
            }
            InlineObjectGap::NoObjects | InlineObjectGap::After(_) => false,
        };
        let precedes_end = match range.end().gap {
            InlineObjectGap::After(previous) => previous.id() == id && previous.order() == order,
            InlineObjectGap::Between { preceding, .. } => {
                preceding.id() == id && preceding.order() == order
            }
            InlineObjectGap::NoObjects | InlineObjectGap::Before(_) => false,
        };
        if !follows_start || !precedes_end {
            return Err(MutationError::MalformedObjectChange);
        }
        Ok(Self { range, id, order })
    }

    pub const fn range(self) -> SourceRange {
        self.range
    }

    pub const fn id(self) -> InlineObjectId {
        self.id
    }

    pub const fn order(self) -> InlineObjectOrder {
        self.order
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SuccessorObject {
    id: InlineObjectId,
    anchor: crate::ByteOffset,
    order: InlineObjectOrder,
    retained_bytes: usize,
    presentation_bytes: usize,
}

impl SuccessorObject {
    pub const fn new(
        id: InlineObjectId,
        anchor: crate::ByteOffset,
        order: InlineObjectOrder,
        retained_bytes: usize,
        presentation_bytes: usize,
    ) -> Self {
        Self {
            id,
            anchor,
            order,
            retained_bytes,
            presentation_bytes,
        }
    }

    pub const fn id(self) -> InlineObjectId {
        self.id
    }

    pub const fn anchor(self) -> crate::ByteOffset {
        self.anchor
    }

    pub const fn order(self) -> InlineObjectOrder {
        self.order
    }

    pub const fn retained_bytes(self) -> usize {
        self.retained_bytes
    }

    pub const fn presentation_bytes(self) -> usize {
        self.presentation_bytes
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ObjectChange {
    Insert {
        object: SuccessorObject,
    },
    Remove {
        target: ObjectTarget,
    },
    Replace {
        target: ObjectTarget,
        object: SuccessorObject,
    },
    Move {
        target: ObjectTarget,
        object: SuccessorObject,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MutationState {
    Idle,
    PreflightPending,
    InputStreaming,
    FinishPending,
    CommitPending,
    Settled,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MutationCounts {
    pub current_pages: usize,
    pub retained_bytes: usize,
    pub objects: usize,
    pub object_bytes: usize,
    pub presentation_bytes: usize,
    pub proofs: usize,
    pub source_pages: usize,
    pub transactions: usize,
}

impl MutationCounts {
    fn checked_add(self, other: Self) -> Option<Self> {
        Some(Self {
            current_pages: self.current_pages.checked_add(other.current_pages)?,
            retained_bytes: self.retained_bytes.checked_add(other.retained_bytes)?,
            objects: self.objects.checked_add(other.objects)?,
            object_bytes: self.object_bytes.checked_add(other.object_bytes)?,
            presentation_bytes: self
                .presentation_bytes
                .checked_add(other.presentation_bytes)?,
            proofs: self.proofs.checked_add(other.proofs)?,
            source_pages: self.source_pages.checked_add(other.source_pages)?,
            transactions: self.transactions.checked_add(other.transactions)?,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MutationOutcome {
    Committed(MutationCommit),
    Rejected,
    Conflict,
    Cancelled,
    Error,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MutationSettlement {
    Current(MutationOutcome),
    Obsolete(MutationOutcome),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MutationCancellation {
    Cancelled,
    AwaitingHostSettlement,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MutationDisposal {
    Cancelled(MutationKey),
    Detached(MutationKey),
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum MutationError {
    InvalidLimits,
    Busy(MutationKey),
    NoActive,
    WrongState {
        expected: MutationState,
        actual: MutationState,
    },
    WrongKey {
        expected: MutationKey,
        actual: MutationKey,
    },
    ObsoleteOperation(MutationKey),
    ReplacementOutsideExtent,
    PositionOutsideExtent,
    IncompatibleReplacementPositions,
    MalformedBaseExtent,
    MalformedReplacementLineBreaks,
    WrongLane,
    CursorMismatch,
    OrdinalMismatch {
        expected: u64,
        actual: u64,
    },
    PriorIdentityMismatch,
    PageReplay,
    PageCollision,
    OperationCollision,
    MalformedPage,
    PageItemLimitExceeded,
    PageByteLimitExceeded,
    CumulativeOverflow,
    ObjectLimitExceeded,
    ObjectByteLimitExceeded,
    PresentationByteLimitExceeded,
    InsertOffsetMismatch {
        expected: u64,
        actual: u64,
    },
    MalformedAtomChange,
    MalformedObjectChange,
    ObjectChangeOutsideReplacement,
    DuplicateObjectChange(InlineObjectId),
    DuplicateSuccessorObjectOrder {
        anchor: crate::ByteOffset,
        order: InlineObjectOrder,
    },
    SuccessorObjectsOutOfOrder,
    SuccessorObjectOutsideExtent,
    DuplicateAtomInsert(AtomId),
    DuplicateAtomRemove(AtomId),
    DuplicateAtomRemoveRange(ByteRange),
    InsertedAtomRangeOutOfOrder {
        previous: ByteRange,
        actual: ByteRange,
    },
    RemovedAtomRangeOutOfOrder {
        previous: ByteRange,
        actual: ByteRange,
    },
    MissingFinishInput,
    PostFinishInput,
    FinishMismatch,
    MissingTextBoundaryProof,
    InvalidTextBoundaryProof,
    InvalidObjectGapProof,
    StalePositionProof,
    WrongSuccessorPositionProof,
    WrongSuccessorPositions,
    MissingPositionProof(SourcePosition),
    UnexpectedPositionProof(SourcePosition),
    DuplicatePositionProof(SourcePosition),
    PositionProofLimitExceeded,
    IncoherentSuccessor,
    ReleaseCountOverflow,
}

impl std::fmt::Display for MutationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "range mutation contract rejected: {self:?}")
    }
}

impl std::error::Error for MutationError {}

#[derive(Debug)]
struct ActiveMutation {
    proposal: MutationProposal,
    base_extent: LogicalExtent,
    state: MutationState,
    source: LaneState,
    proposal_lane: LaneState,
    intended: Option<MutationPositions>,
    intended_extent: Option<LogicalExtent>,
    initial_source_cursor: MutationCursor,
    initial_proposal_cursor: MutationCursor,
    detached: bool,
    sequence: MutationSequenceState,
    tracked_active_object: Option<(InlineObjectId, InlineObjectOrder)>,
    active_object_effect: Option<ActiveObjectEffect>,
}

#[derive(Clone, Copy, Debug)]
struct LaneState {
    next_cursor: MutationCursor,
    next_ordinal: u64,
    cumulative_identity: MutationIdentity,
    totals: MutationTotals,
    last_page: Option<PageReceipt>,
}

#[derive(Clone, Copy, Debug)]
struct PageReceipt {
    key: MutationPageKey,
    page_identity: MutationIdentity,
    cumulative_identity: MutationIdentity,
}

#[derive(Clone, Copy, Debug, Default)]
struct MutationSequenceState {
    inserted_bytes: u64,
    inserted_line_breaks: u64,
    last_inserted_atom: Option<(AtomId, ByteRange)>,
    last_removed_atom: Option<(AtomId, ByteRange)>,
    last_object_target: Option<ObjectTarget>,
    last_successor_object: Option<SuccessorObject>,
}

#[derive(Clone, Copy, Debug)]
struct ProposalPageCandidate {
    sequence: MutationSequenceState,
    active_object_effect: Option<ActiveObjectEffect>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ActiveObjectEffect {
    Removed {
        id: InlineObjectId,
        order: InlineObjectOrder,
    },
    Replaced {
        id: InlineObjectId,
        order: InlineObjectOrder,
    },
}

#[derive(Debug)]
pub struct RangeEditCoordinator {
    binding: RangeBinding,
    limits: MutationLimits,
    active: Option<ActiveMutation>,
    last_terminal: Option<MutationKey>,
    operation_high_water: Option<OperationId>,
    high_water_begin_identity: Option<MutationIdentity>,
    ever_started: bool,
    released: MutationCounts,
}
