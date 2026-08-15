//! Exact, bounded coordination for one host-owned range mutation.

use crate::{
    AtomId, BindingId, ByteRange, InlineObjectGap, InlineObjectId, InlineObjectOrder,
    LogicalExtent, ObjectResidency, RangeBinding, RangeResidency, SourcePosition, SourceRange,
    SourceRevision,
};

mod coordinator;
mod lifecycle;
mod preflight;
mod proof;
mod settlement;
mod staging;

pub(crate) use coordinator::required_base_positions;
pub use proof::*;

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

opaque_id!(OperationId, "Unique identity of one range-backed mutation.");

/// The host-owned operation requested through the shared mutation boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MutationKind {
    Edit,
    Undo,
    Redo,
}

/// Exact immutable identity of one mutation transaction.
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

/// A checked proposal for replacing one exact range at one base revision.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MutationProposal {
    key: MutationKey,
    kind: MutationKind,
    replacement: SourceRange,
    replacement_line_breaks: u64,
}

impl MutationProposal {
    /// Creates a proposal with the exact normalized `\n` count removed by the replacement.
    pub const fn new(
        key: MutationKey,
        kind: MutationKind,
        replacement: SourceRange,
        replacement_line_breaks: u64,
    ) -> Self {
        Self {
            key,
            kind,
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

/// Hard retained-capacity limits for one staged mutation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MutationLimits {
    max_fragments: usize,
    max_staged_bytes: usize,
    max_objects: usize,
    max_object_bytes: usize,
    max_presentation_bytes: usize,
}

impl MutationLimits {
    pub fn new(max_fragments: usize, max_staged_bytes: usize) -> Result<Self, MutationError> {
        if max_fragments == 0 {
            return Err(MutationError::InvalidLimits);
        }
        Ok(Self {
            max_fragments,
            max_staged_bytes,
            max_objects: max_fragments,
            max_object_bytes: max_staged_bytes,
            max_presentation_bytes: max_staged_bytes,
        })
    }
    pub const fn max_fragments(self) -> usize {
        self.max_fragments
    }
    pub const fn max_staged_bytes(self) -> usize {
        self.max_staged_bytes
    }

    /// Refines the independent object-count and retained-byte envelopes.
    pub fn with_object_limits(
        mut self,
        max_objects: usize,
        max_object_bytes: usize,
        max_presentation_bytes: usize,
    ) -> Result<Self, MutationError> {
        if max_objects == 0 {
            return Err(MutationError::InvalidLimits);
        }
        self.max_objects = max_objects;
        self.max_object_bytes = max_object_bytes;
        self.max_presentation_bytes = max_presentation_bytes;
        Ok(self)
    }

    pub const fn max_objects(self) -> usize {
        self.max_objects
    }

    pub const fn max_object_bytes(self) -> usize {
        self.max_object_bytes
    }

    pub const fn max_presentation_bytes(self) -> usize {
        self.max_presentation_bytes
    }
}

/// One host-significant atom change carried alongside inserted UTF-8.
///
/// Insert and remove sets are independently unique and ordered. Exactly one remove and one insert
/// may share an [`AtomId`], representing a stable-identity move within the atomic replacement.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AtomChange {
    Insert {
        id: AtomId,
        inserted_range: ByteRange,
        fallback_copy: String,
    },
    Remove {
        id: AtomId,
        source_range: ByteRange,
    },
}

/// One exact source-zero-width object isolated by adjacent composite gaps.
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

/// Authoritative successor occurrence of one source-zero-width object.
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

/// One source-zero-width object mutation carried by the shared staged transaction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ObjectChange {
    Insert {
        at: SourcePosition,
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
        to: SourcePosition,
        object: SuccessorObject,
    },
}

/// The payload of one ordered staging fragment.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MutationFragmentPayload {
    Utf8 { inserted_offset: u64, text: String },
    Atom(AtomChange),
    Object(ObjectChange),
    Terminal { intended: MutationPositions },
}

/// One exactly keyed, ordered mutation fragment.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MutationFragment {
    key: MutationKey,
    ordinal: usize,
    payload: std::sync::Arc<MutationFragmentPayload>,
}

impl MutationFragment {
    pub fn new(key: MutationKey, ordinal: usize, payload: MutationFragmentPayload) -> Self {
        Self {
            key,
            ordinal,
            payload: std::sync::Arc::new(payload),
        }
    }
    pub const fn key(&self) -> MutationKey {
        self.key
    }
    pub const fn ordinal(&self) -> usize {
        self.ordinal
    }
    pub fn payload(&self) -> &MutationFragmentPayload {
        self.payload.as_ref()
    }
}

/// Observable phase of the coordinator's single active transaction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MutationState {
    Idle,
    Preflight,
    Staging,
    CommitPending,
    DetachedCommit,
}

/// Exact retained staging counts.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MutationCounts {
    pub fragments: usize,
    pub staged_bytes: usize,
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
            fragments: self.fragments.checked_add(other.fragments)?,
            staged_bytes: self.staged_bytes.checked_add(other.staged_bytes)?,
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

/// The host's one exact terminal result.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MutationOutcome {
    Committed(MutationCommit),
    Rejected,
    Conflict,
    Cancelled,
    Error,
}

/// Whether a valid terminal result still belongs to the current binding.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MutationSettlement {
    Current(MutationOutcome),
    Obsolete(MutationOutcome),
}

/// Result of requesting cancellation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MutationCancellation {
    Cancelled,
    AwaitingHostSettlement,
}

/// Lifecycle disposition for the active transaction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MutationDisposal {
    /// Pre-admission work was cancelled without mutation.
    Cancelled(MutationKey),
    /// An admitted commit was detached and must still settle at the host.
    Detached(MutationKey),
}

/// A rejected state transition or malformed fragment/result.
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
    FragmentOutOfOrder {
        expected: usize,
        actual: usize,
    },
    FragmentLimitExceeded,
    StagedByteLimitExceeded,
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
    MissingTerminalFragment,
    PostTerminalFragment,
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
    next_ordinal: usize,
    inserted_bytes: u64,
    inserted_line_breaks: u64,
    fragment_count: usize,
    staged_bytes: usize,
    object_count: usize,
    object_bytes: usize,
    presentation_bytes: usize,
    proof_count: usize,
    source_page_count: usize,
    terminal_seen: bool,
    intended: Option<MutationPositions>,
    detached: bool,
    fragments: Vec<MutationFragment>,
    source_proofs: Vec<SourcePositionProof>,
}

/// Coordinates exactly one edit, undo, or redo transaction at a time.
#[derive(Debug)]
pub struct RangeEditCoordinator {
    binding: RangeBinding,
    limits: MutationLimits,
    active: Option<ActiveMutation>,
    last_terminal: Option<MutationKey>,
    released: MutationCounts,
}
