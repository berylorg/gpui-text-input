//! Exact, bounded coordination for one host-owned range mutation.

use crate::{AtomId, BindingId, ByteRange, LogicalExtent, RangeBinding, SourceRevision};

mod lifecycle;
mod preflight;
mod settlement;
mod staging;

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
    replacement: ByteRange,
    replacement_line_breaks: u64,
}

impl MutationProposal {
    /// Creates a proposal with the exact normalized `\n` count removed by the replacement.
    pub const fn new(
        key: MutationKey,
        kind: MutationKind,
        replacement: ByteRange,
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
    pub const fn replacement(self) -> ByteRange {
        self.replacement
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
}

impl MutationLimits {
    pub fn new(max_fragments: usize, max_staged_bytes: usize) -> Result<Self, MutationError> {
        if max_fragments == 0 {
            return Err(MutationError::InvalidLimits);
        }
        Ok(Self {
            max_fragments,
            max_staged_bytes,
        })
    }
    pub const fn max_fragments(self) -> usize {
        self.max_fragments
    }
    pub const fn max_staged_bytes(self) -> usize {
        self.max_staged_bytes
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

/// The payload of one ordered staging fragment.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MutationFragmentPayload {
    Utf8 { inserted_offset: u64, text: String },
    Atom(AtomChange),
    Terminal,
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
}

/// The host's one exact terminal result.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MutationOutcome {
    Committed(RangeBinding),
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
    MalformedBaseExtent,
    MalformedReplacementLineBreaks,
    FragmentOutOfOrder {
        expected: usize,
        actual: usize,
    },
    FragmentLimitExceeded,
    StagedByteLimitExceeded,
    InsertOffsetMismatch {
        expected: u64,
        actual: u64,
    },
    MalformedAtomChange,
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
    IncoherentSuccessor,
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
    terminal_seen: bool,
    detached: bool,
    fragments: Vec<MutationFragment>,
}

/// Coordinates exactly one edit, undo, or redo transaction at a time.
#[derive(Debug)]
pub struct RangeEditCoordinator {
    binding: RangeBinding,
    limits: MutationLimits,
    active: Option<ActiveMutation>,
    last_terminal: Option<MutationKey>,
}

impl RangeEditCoordinator {
    pub const fn new(binding: RangeBinding, limits: MutationLimits) -> Self {
        Self {
            binding,
            limits,
            active: None,
            last_terminal: None,
        }
    }
    pub const fn binding(&self) -> RangeBinding {
        self.binding
    }
    pub fn state(&self) -> MutationState {
        self.active
            .as_ref()
            .map_or(MutationState::Idle, |active| active.state)
    }
    pub fn active_key(&self) -> Option<MutationKey> {
        self.active.as_ref().map(|a| a.proposal.key())
    }
    pub fn counts(&self) -> MutationCounts {
        self.active
            .as_ref()
            .map_or(MutationCounts::default(), |a| MutationCounts {
                fragments: a.fragment_count,
                staged_bytes: a.staged_bytes,
            })
    }
    /// Returns staged fragments in their exact admitted order without joining their UTF-8.
    pub fn staged_fragments(&self) -> &[MutationFragment] {
        self.active
            .as_ref()
            .map_or(&[], |active| active.fragments.as_slice())
    }

    pub fn admit_commit(&mut self, key: MutationKey) -> Result<(), MutationError> {
        let active = self.active_mut(key, MutationState::Staging)?;
        if !active.terminal_seen {
            return Err(MutationError::MissingTerminalFragment);
        }
        active.state = MutationState::CommitPending;
        Ok(())
    }

    pub fn cancel(&mut self, key: MutationKey) -> Result<MutationCancellation, MutationError> {
        let state = self.active_for_key(key)?.state;
        match state {
            MutationState::Preflight | MutationState::Staging => {
                self.finish(key, MutationOutcome::Cancelled, false);
                Ok(MutationCancellation::Cancelled)
            }
            MutationState::CommitPending | MutationState::DetachedCommit => {
                Ok(MutationCancellation::AwaitingHostSettlement)
            }
            MutationState::Idle => Err(MutationError::NoActive),
        }
    }

    fn check_key(&self, key: MutationKey) -> Result<(), MutationError> {
        let expected = MutationKey::new(
            self.binding.binding(),
            self.binding.revision(),
            key.operation(),
        );
        if key.binding() != expected.binding() || key.base_revision() != expected.base_revision() {
            return Err(MutationError::WrongKey {
                expected,
                actual: key,
            });
        }
        Ok(())
    }
    fn active_for_key(&self, key: MutationKey) -> Result<&ActiveMutation, MutationError> {
        let Some(active) = &self.active else {
            return if self.last_terminal == Some(key) {
                Err(MutationError::ObsoleteOperation(key))
            } else {
                Err(MutationError::NoActive)
            };
        };
        if active.proposal.key() != key {
            return Err(MutationError::WrongKey {
                expected: active.proposal.key(),
                actual: key,
            });
        }
        Ok(active)
    }
    fn active_mut(
        &mut self,
        key: MutationKey,
        expected: MutationState,
    ) -> Result<&mut ActiveMutation, MutationError> {
        let active = self.active_for_key(key)?;
        if active.state != expected {
            return Err(MutationError::WrongState {
                expected,
                actual: active.state,
            });
        }
        Ok(self.active.as_mut().expect("active checked"))
    }
    fn finish(
        &mut self,
        key: MutationKey,
        outcome: MutationOutcome,
        obsolete: bool,
    ) -> MutationSettlement {
        self.active = None;
        self.last_terminal = Some(key);
        if !obsolete {
            if let MutationOutcome::Committed(successor) = outcome {
                self.binding = successor;
            }
            MutationSettlement::Current(outcome)
        } else {
            MutationSettlement::Obsolete(outcome)
        }
    }
}
