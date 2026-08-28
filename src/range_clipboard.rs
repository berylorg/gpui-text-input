use crate::{
    AtomId, BindingId, ByteOffset, ByteRange, InlineObjectFact, MutationKey, MutationKind,
    MutationPositions, MutationProposal, ObjectCursor, ObjectDemandEnvelope, ObjectDirection,
    ObjectPage, ObjectPageCharge, ObjectPageFailure, ObjectPurpose, ObjectRequest, ObjectRequestId,
    ObjectRequestKey, OperationId, PageDirection, PageEdgeFact, PageFailure, PagePurpose,
    PageRequest, PageRequestId, PageRequestKey, PresentationGeneration, RangeBinding, RangePage,
    RangePageCharge, SourcePosition, SourceRange, SourceRevision, TextInputAtomClipboardPolicy,
};

mod collection;
mod lifecycle;
mod provenance;
mod storage;

use provenance::ProvenanceCollection;
pub use provenance::{
    ClipboardProvenanceClosure, ClipboardProvenanceCursor, ClipboardProvenanceIdentity,
    ClipboardProvenanceItem, ClipboardProvenanceLimits, ClipboardProvenancePage,
    ClipboardProvenancePageKey, ClipboardProvenancePolicy,
};
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use storage::ExactOutput;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ClipboardId(u64);

impl ClipboardId {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ClipboardKey {
    id: ClipboardId,
    binding: BindingId,
    revision: SourceRevision,
    selection: SourceRange,
    predecessor: MutationPositions,
}

impl ClipboardKey {
    pub const fn new(
        id: ClipboardId,
        binding: BindingId,
        revision: SourceRevision,
        selection: SourceRange,
        predecessor: MutationPositions,
    ) -> Self {
        Self {
            id,
            binding,
            revision,
            selection,
            predecessor,
        }
    }

    pub const fn id(self) -> ClipboardId {
        self.id
    }

    pub const fn binding(self) -> BindingId {
        self.binding
    }

    pub const fn revision(self) -> SourceRevision {
        self.revision
    }

    pub const fn selection(self) -> SourceRange {
        self.selection
    }

    pub const fn predecessor(self) -> MutationPositions {
        self.predecessor
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClipboardKind {
    Copy,
    Cut,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClipboardLimits {
    max_bytes: usize,
    max_text_page_bytes: u64,
    object_and_provenance_page_items: u64,
    object_and_provenance_page_bytes: u64,
}

impl ClipboardLimits {
    pub fn new(max_bytes: usize, max_text_page_bytes: u64) -> Result<Self, ClipboardError> {
        Self::new_composite(max_bytes, max_text_page_bytes, 32, 64 * 1024)
    }

    pub fn new_composite(
        max_bytes: usize,
        max_text_page_bytes: u64,
        max_object_page_objects: usize,
        max_object_page_retained_bytes: usize,
    ) -> Result<Self, ClipboardError> {
        if max_text_page_bytes < 4
            || max_object_page_objects == 0
            || max_object_page_retained_bytes == 0
            || u32::try_from(max_object_page_objects).is_err()
            || u32::try_from(max_object_page_retained_bytes).is_err()
        {
            return Err(ClipboardError::InvalidLimits);
        }
        Ok(Self {
            max_bytes,
            max_text_page_bytes,
            object_and_provenance_page_items: max_object_page_objects as u64,
            object_and_provenance_page_bytes: max_object_page_retained_bytes as u64,
        })
    }

    pub const fn with_provenance(mut self, provenance: ClipboardProvenancePolicy) -> Self {
        let (items, bytes) = match provenance {
            ClipboardProvenancePolicy::Omit => (0, 0),
            ClipboardProvenancePolicy::Stream(limits) => (
                limits.max_page_items() as u64,
                limits.max_page_retained_bytes() as u64,
            ),
        };
        self.object_and_provenance_page_items =
            (self.object_and_provenance_page_items & u32::MAX as u64) | (items << 32);
        self.object_and_provenance_page_bytes =
            (self.object_and_provenance_page_bytes & u32::MAX as u64) | (bytes << 32);
        self
    }
    pub const fn max_bytes(self) -> usize {
        self.max_bytes
    }

    pub const fn max_text_page_bytes(self) -> u64 {
        self.max_text_page_bytes
    }

    pub const fn max_object_page_objects(self) -> usize {
        (self.object_and_provenance_page_items & u32::MAX as u64) as usize
    }

    pub const fn max_object_page_retained_bytes(self) -> usize {
        (self.object_and_provenance_page_bytes & u32::MAX as u64) as usize
    }

    pub const fn provenance(self) -> ClipboardProvenancePolicy {
        let items = (self.object_and_provenance_page_items >> 32) as usize;
        let bytes = (self.object_and_provenance_page_bytes >> 32) as usize;
        if items == 0 {
            ClipboardProvenancePolicy::Omit
        } else {
            ClipboardProvenancePolicy::Stream(ClipboardProvenanceLimits::from_valid(items, bytes))
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClipboardState {
    Idle,
    CollectingObjects,
    ObjectPagePending,
    CollectingText,
    TextPagePending,
    AwaitingProvenancePage,
    AwaitingWrite,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ClipboardCounts {
    pub owned_bytes: usize,
    pub owned_items: usize,
    pub staged_bytes: usize,
    pub pending_text_pages: usize,
    pub pending_object_pages: usize,
    pub retained_object_facts: usize,
    pub retained_provenance_items: usize,
    pub retained_provenance_bytes: usize,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ClipboardOwnershipCharge {
    bytes: usize,
    items: usize,
}

#[derive(Debug, Eq, PartialEq)]
pub struct ClipboardPreparedBegin {
    coordinator_instance: u64,
    generation: u64,
    operation_identity: u64,
    presentation_generation: PresentationGeneration,
    key: ClipboardKey,
    kind: ClipboardKind,
    peak: ClipboardOwnershipCharge,
    successor: ClipboardOwnershipCharge,
}

impl ClipboardPreparedBegin {
    pub const fn peak_ownership(&self) -> ClipboardOwnershipCharge {
        self.peak
    }

    pub const fn successor_ownership(&self) -> ClipboardOwnershipCharge {
        self.successor
    }

    pub const fn key(&self) -> ClipboardKey {
        self.key
    }
}

#[derive(Debug, Eq, PartialEq)]
pub struct ClipboardPreparedStep {
    coordinator_instance: u64,
    operation_key: ClipboardKey,
    operation_identity: u64,
    generation: u64,
    peak: ClipboardOwnershipCharge,
    successor: ClipboardOwnershipCharge,
    kind: PreparedClipboardStepKind,
}

impl ClipboardPreparedStep {
    pub const fn peak_ownership(&self) -> ClipboardOwnershipCharge {
        self.peak
    }

    pub const fn successor_ownership(&self) -> ClipboardOwnershipCharge {
        self.successor
    }

    pub const fn transfers_response(&self) -> bool {
        matches!(
            self.kind,
            PreparedClipboardStepKind::RetainTextResponse { .. }
                | PreparedClipboardStepKind::RetainObjectResponse { .. }
                | PreparedClipboardStepKind::TerminalTextResponse { .. }
                | PreparedClipboardStepKind::TerminalObjectResponse { .. }
        )
    }
}

#[derive(Debug, Eq, PartialEq)]
pub struct ClipboardPreparedCommit {
    progress: Option<ClipboardProgress>,
    released_text_page: Option<PageRequestKey>,
    released_object_page: Option<ObjectRequestKey>,
}

impl ClipboardPreparedCommit {
    pub fn into_progress(self) -> Option<ClipboardProgress> {
        self.progress
    }

    pub const fn released_text_page(&self) -> Option<PageRequestKey> {
        self.released_text_page
    }

    pub const fn released_object_page(&self) -> Option<ObjectRequestKey> {
        self.released_object_page
    }
}

impl ClipboardOwnershipCharge {
    pub const fn bytes(self) -> usize {
        self.bytes
    }

    pub const fn items(self) -> usize {
        self.items
    }
}

#[derive(Debug, Eq, PartialEq)]
pub enum ClipboardProgress {
    NeedTextPage {
        key: ClipboardKey,
        next_offset: ByteOffset,
        target: ByteOffset,
    },
    NeedObjectPage {
        key: ClipboardKey,
        cursor: Option<ObjectCursor>,
    },
    ProvenancePage(ClipboardProvenancePage),
    Write(ClipboardWriteRequest),
    Terminal(ClipboardCompletion),
}

#[derive(Debug, Eq, PartialEq)]
pub struct ClipboardWriteRequest {
    key: ClipboardKey,
    text: String,
    provenance: Option<ClipboardProvenanceClosure>,
}

impl ClipboardWriteRequest {
    pub const fn key(&self) -> ClipboardKey {
        self.key
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn into_text(self) -> String {
        self.text
    }

    pub const fn provenance(&self) -> Option<ClipboardProvenanceClosure> {
        self.provenance
    }

    pub(crate) fn payload_allocation_charge(&self) -> (usize, usize) {
        (self.text.capacity(), self.text.capacity())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClipboardWriteOutcome {
    Written,
    Failed,
    Cancelled,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClipboardCompletion {
    Propagate(ClipboardKind),
    Copied,
    Delete(CutDeletion),
    WriteFailed,
    Cancelled,
    TextPageFailed(PageFailure),
    ObjectPageFailed(ObjectPageFailure),
    TextPageTooLarge,
    TooLarge,
    Malformed,
    AllocationFailed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CutDeletion {
    binding: RangeBinding,
    selection: SourceRange,
    selection_line_breaks: u64,
    predecessor: MutationPositions,
}

impl CutDeletion {
    pub const fn binding(self) -> RangeBinding {
        self.binding
    }

    pub const fn selection(self) -> SourceRange {
        self.selection
    }

    pub const fn selection_line_breaks(self) -> u64 {
        self.selection_line_breaks
    }

    pub const fn predecessor(self) -> MutationPositions {
        self.predecessor
    }

    pub fn proposal(
        self,
        operation: OperationId,
        replacement: SourceRange,
    ) -> Result<MutationProposal, crate::MutationError> {
        if replacement != self.selection {
            return Err(crate::MutationError::IncompatibleReplacementPositions);
        }
        Ok(MutationProposal::new(
            MutationKey::new(self.binding.binding(), self.binding.revision(), operation),
            MutationKind::Edit,
            self.predecessor,
            replacement,
            self.selection_line_breaks,
        ))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ClipboardError {
    InvalidLimits,
    RequestIdInUse(PageRequestId),
    Busy(ClipboardKey),
    NoActive,
    WrongState {
        expected: ClipboardState,
        actual: ClipboardState,
    },
    WrongKey {
        expected: ClipboardKey,
        actual: ClipboardKey,
    },
    Obsolete(ClipboardKey),
    SelectionOutsideExtent,
    IncompatibleSelection,
    WrongPageKey {
        expected: PageRequestKey,
        actual: PageRequestKey,
    },
    ObsoletePage(PageRequestKey),
    ObjectRequestIdInUse(ObjectRequestId),
    WrongObjectPageKey {
        expected: ObjectRequestKey,
        actual: ObjectRequestKey,
    },
    ObsoleteObjectPage(ObjectRequestKey),
    WrongProvenancePage {
        expected: ClipboardProvenancePageKey,
        actual: ClipboardProvenancePageKey,
    },
    ProvenancePageCollision(ClipboardProvenancePageKey),
    ObsoleteProvenancePage(ClipboardProvenancePageKey),
    PreparationInUse,
    PreparationOverflow,
    StalePreparation,
    WrongPreparation,
    AllocationFailed,
    AllocationExceededPreparation,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClipboardCancellation {
    key: ClipboardKey,
    pending_text_page: Option<PageRequestKey>,
    pending_object_page: Option<ObjectRequestKey>,
    pending_provenance_page: Option<ClipboardProvenancePageKey>,
    awaiting_write: bool,
}

impl ClipboardCancellation {
    pub const fn key(self) -> ClipboardKey {
        self.key
    }

    pub const fn pending_text_page(self) -> Option<PageRequestKey> {
        self.pending_text_page
    }

    pub const fn pending_object_page(self) -> Option<ObjectRequestKey> {
        self.pending_object_page
    }

    pub const fn pending_provenance_page(self) -> Option<ClipboardProvenancePageKey> {
        self.pending_provenance_page
    }

    pub const fn awaiting_write(self) -> bool {
        self.awaiting_write
    }
}

impl std::fmt::Display for ClipboardError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "range clipboard contract rejected: {self:?}")
    }
}

impl std::error::Error for ClipboardError {}

#[derive(Debug)]
struct OpenAtom {
    id: AtomId,
    global_range: ByteRange,
    fallback_output: std::ops::Range<usize>,
}

#[derive(Debug)]
struct RetainedTextResponse {
    page: RangePage,
    consumed_end: ByteOffset,
    consumed_len: usize,
    cursor: usize,
    atom_index: usize,
}

#[derive(Debug)]
struct RetainedObjectResponse {
    key: ObjectRequestKey,
    objects: std::collections::VecDeque<InlineObjectFact>,
    complete: bool,
    continuation: Option<ObjectCursor>,
}

#[derive(Debug, Eq, PartialEq)]
enum PreparedClipboardStepKind {
    RetainTextResponse {
        key: PageRequestKey,
        response_identity: u64,
        response_charge: RangePageCharge,
        consumed_end: ByteOffset,
        consumed_len: usize,
        line_breaks: u64,
    },
    RetainObjectResponse {
        key: ObjectRequestKey,
        response_identity: u64,
        response_charge: ObjectPageCharge,
    },
    TerminalTextResponse {
        key: PageRequestKey,
        response_identity: u64,
        response_charge: RangePageCharge,
        completion: ClipboardCompletion,
    },
    TerminalObjectResponse {
        key: ObjectRequestKey,
        response_identity: u64,
        response_charge: ObjectPageCharge,
        completion: ClipboardCompletion,
    },
    AllocateOutput,
    AllocateProvenanceBuilder,
    AppendText {
        start: usize,
        end: usize,
    },
    AppendAtom {
        atom_index: usize,
        fragment_end: usize,
        opens: bool,
    },
    AdvanceAtom {
        atom_index: usize,
        fragment_end: usize,
        closes: bool,
    },
    FinishTextResponse,
    TakeObject,
    ProcessObject {
        selected: bool,
        leading: SourcePosition,
        trailing: SourcePosition,
    },
    FinishObjectResponse,
    EmitProvenance,
    CompleteCollection,
    NeedTextPage {
        target: ByteOffset,
    },
    NeedObjectPage,
    Terminal(ClipboardCompletion),
}

#[derive(Debug)]
struct ActiveClipboard {
    key: ClipboardKey,
    operation_identity: u64,
    kind: ClipboardKind,
    phase: ClipboardCollectionPhase,
    state: ClipboardState,
    text_cursor: ByteOffset,
    text_target: Option<ByteOffset>,
    pending_text: Option<PageRequestKey>,
    pending_object: Option<ObjectRequestKey>,
    object_cursor: Option<ObjectCursor>,
    object_page_complete: bool,
    prior_object: Option<ObjectCursor>,
    current_object: Option<InlineObjectFact>,
    retained_text_response: Option<RetainedTextResponse>,
    retained_object_response: Option<RetainedObjectResponse>,
    start_gap_proven: bool,
    end_gap_proven: bool,
    start_anchor_had_object: bool,
    end_anchor_had_object: bool,
    output: ExactOutput,
    open_atom: Option<OpenAtom>,
    source_line_breaks: u64,
    provenance: Option<Box<ProvenanceCollection>>,
}

impl ActiveClipboard {
    fn ownership_charge(&self) -> Option<ClipboardOwnershipCharge> {
        let text_response =
            self.retained_text_response
                .as_ref()
                .map_or(Some((0usize, 0usize)), |response| {
                    let charge = response.page.retained_charge();
                    Some((
                        charge
                            .bytes()
                            .checked_sub(std::mem::size_of::<RangePage>())?,
                        charge.items().checked_sub(1).unwrap_or(0),
                    ))
                })?;
        let object_records = self
            .retained_object_response
            .as_ref()
            .map_or(0, |response| response.objects.capacity())
            .checked_mul(std::mem::size_of::<InlineObjectFact>())?;
        let object_payload_bytes = self
            .retained_object_response
            .iter()
            .flat_map(|response| response.objects.iter())
            .chain(self.current_object.iter())
            .try_fold(0usize, |total, object| {
                total.checked_add(object.owned_payload_allocation_bytes()?)
            })?;
        let object_items = self
            .retained_object_response
            .as_ref()
            .map_or(0, |response| response.objects.capacity());
        let provenance = self
            .provenance
            .as_ref()
            .map_or(Some((0usize, 0usize)), |provenance| {
                provenance.ownership_charge()
            })?;
        Some(ClipboardOwnershipCharge {
            bytes: std::mem::size_of::<Self>()
                .checked_add(self.output.capacity())?
                .checked_add(text_response.0)?
                .checked_add(object_records)?
                .checked_add(object_payload_bytes)?
                .checked_add(provenance.0)?,
            items: 1usize
                .checked_add(self.output.capacity())?
                .checked_add(text_response.1)?
                .checked_add(object_items)?
                .checked_add(provenance.1)?,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ClipboardCollectionPhase {
    Classifying,
    Collecting,
}

#[derive(Debug)]
pub struct RangeClipboardCoordinator {
    binding: RangeBinding,
    presentation_generation: PresentationGeneration,
    atom_policy: TextInputAtomClipboardPolicy,
    limits: ClipboardLimits,
    active: Option<Box<ActiveClipboard>>,
    last_terminal: Option<ClipboardKey>,
    highest_request: Option<PageRequestId>,
    highest_object_request: Option<ObjectRequestId>,
    preparation_generation: u64,
    coordinator_instance: u64,
    next_operation_identity: u64,
}

static NEXT_CLIPBOARD_COORDINATOR_INSTANCE: AtomicU64 = AtomicU64::new(1);

fn next_clipboard_coordinator_instance() -> Result<u64, ClipboardError> {
    NEXT_CLIPBOARD_COORDINATOR_INSTANCE
        .fetch_update(
            AtomicOrdering::Relaxed,
            AtomicOrdering::Relaxed,
            |current| current.checked_add(1),
        )
        .map_err(|_| ClipboardError::PreparationOverflow)
}

impl RangeClipboardCoordinator {
    pub fn new(
        binding: RangeBinding,
        atom_policy: TextInputAtomClipboardPolicy,
        limits: ClipboardLimits,
    ) -> Result<Self, ClipboardError> {
        Self::new_composite(binding, PresentationGeneration::new(0), atom_policy, limits)
    }

    pub fn new_composite(
        binding: RangeBinding,
        presentation_generation: PresentationGeneration,
        atom_policy: TextInputAtomClipboardPolicy,
        limits: ClipboardLimits,
    ) -> Result<Self, ClipboardError> {
        Ok(Self {
            binding,
            presentation_generation,
            atom_policy,
            limits,
            active: None,
            last_terminal: None,
            highest_request: None,
            highest_object_request: None,
            preparation_generation: 0,
            coordinator_instance: next_clipboard_coordinator_instance()?,
            next_operation_identity: 0,
        })
    }

    pub const fn binding(&self) -> RangeBinding {
        self.binding
    }

    pub const fn presentation_generation(&self) -> PresentationGeneration {
        self.presentation_generation
    }

    pub fn state(&self) -> ClipboardState {
        self.active
            .as_ref()
            .map_or(ClipboardState::Idle, |active| active.state)
    }

    pub fn counts(&self) -> ClipboardCounts {
        self.active
            .as_ref()
            .map_or(ClipboardCounts::default(), |active| {
                let ownership = active
                    .ownership_charge()
                    .expect("admitted clipboard ownership fits usize");
                ClipboardCounts {
                    owned_bytes: ownership.bytes(),
                    owned_items: ownership.items(),
                    staged_bytes: active.output.capacity(),
                    pending_text_pages: usize::from(active.pending_text.is_some()),
                    pending_object_pages: usize::from(active.pending_object.is_some()),
                    retained_object_facts: active
                        .retained_object_response
                        .as_ref()
                        .map_or(0, |response| response.objects.len())
                        + usize::from(active.current_object.is_some()),
                    retained_provenance_items: active
                        .provenance
                        .as_ref()
                        .and_then(|provenance| provenance.current_page.as_ref())
                        .map_or(0, |page| page.items().len()),
                    retained_provenance_bytes: active
                        .provenance
                        .as_ref()
                        .map_or(0, |provenance| provenance.retained_bytes()),
                }
            })
    }

    pub fn ownership_charge(&self) -> ClipboardOwnershipCharge {
        self.active
            .as_ref()
            .map_or(ClipboardOwnershipCharge::default(), |active| {
                active
                    .ownership_charge()
                    .expect("admitted clipboard ownership fits usize")
            })
    }

    pub(crate) fn current_provenance_page(&self) -> Option<&ClipboardProvenancePage> {
        self.active
            .as_ref()?
            .provenance
            .as_ref()?
            .current_page
            .as_ref()
    }

    pub(crate) fn pending_text_page(&self) -> Option<PageRequestKey> {
        self.active.as_ref().and_then(|active| active.pending_text)
    }

    pub(crate) fn pending_object_page(&self) -> Option<ObjectRequestKey> {
        self.active
            .as_ref()
            .and_then(|active| active.pending_object)
    }

    pub(crate) fn has_prepared_work(&self) -> bool {
        self.active
            .as_ref()
            .is_some_and(|active| match active.state {
                ClipboardState::CollectingObjects | ClipboardState::CollectingText => true,
                ClipboardState::TextPagePending => active.retained_text_response.is_some(),
                ClipboardState::ObjectPagePending => active.retained_object_response.is_some(),
                ClipboardState::Idle
                | ClipboardState::AwaitingProvenancePage
                | ClipboardState::AwaitingWrite => false,
            })
    }

    pub fn prepare_begin(
        &self,
        id: ClipboardId,
        kind: ClipboardKind,
        selection: SourceRange,
        predecessor: MutationPositions,
    ) -> Result<ClipboardPreparedBegin, ClipboardError> {
        if let Some(active) = &self.active {
            return Err(ClipboardError::Busy(active.key));
        }
        let byte_selection =
            ByteRange::new(selection.start().byte_offset, selection.end().byte_offset)
                .expect("source range has ordered byte offsets");
        if self
            .binding
            .extent()
            .check_byte_range(byte_selection)
            .is_err()
        {
            return Err(ClipboardError::SelectionOutsideExtent);
        }
        if predecessor.caret() != predecessor.selection_head()
            || SourceRange::new(
                if matches!(
                    predecessor
                        .selection_anchor()
                        .compare_in_revision(predecessor.selection_head()),
                    Some(std::cmp::Ordering::Greater)
                ) {
                    predecessor.selection_head()
                } else {
                    predecessor.selection_anchor()
                },
                if matches!(
                    predecessor
                        .selection_anchor()
                        .compare_in_revision(predecessor.selection_head()),
                    Some(std::cmp::Ordering::Greater)
                ) {
                    predecessor.selection_anchor()
                } else {
                    predecessor.selection_head()
                },
            )
            .map_err(|_| ClipboardError::IncompatibleSelection)?
                != selection
        {
            return Err(ClipboardError::IncompatibleSelection);
        }
        let key = ClipboardKey::new(
            id,
            self.binding.binding(),
            self.binding.revision(),
            selection,
            predecessor,
        );
        if self.last_terminal == Some(key) {
            return Err(ClipboardError::Obsolete(key));
        }
        let operation_identity = self
            .next_operation_identity
            .checked_add(1)
            .ok_or(ClipboardError::PreparationOverflow)?;
        let provenance = match self.limits.provenance() {
            ClipboardProvenancePolicy::Omit => ClipboardOwnershipCharge::default(),
            ClipboardProvenancePolicy::Stream(_) => ClipboardOwnershipCharge {
                bytes: std::mem::size_of::<ProvenanceCollection>(),
                items: 1,
            },
        };
        let successor = ClipboardOwnershipCharge {
            bytes: std::mem::size_of::<ActiveClipboard>()
                .checked_add(provenance.bytes)
                .ok_or(ClipboardError::PreparationOverflow)?,
            items: 1usize
                .checked_add(provenance.items)
                .ok_or(ClipboardError::PreparationOverflow)?,
        };
        Ok(ClipboardPreparedBegin {
            coordinator_instance: self.coordinator_instance,
            generation: self.preparation_generation,
            operation_identity,
            presentation_generation: self.presentation_generation,
            key,
            kind,
            peak: successor,
            successor,
        })
    }

    pub fn commit_begin(
        &mut self,
        prepared: ClipboardPreparedBegin,
    ) -> Result<ClipboardProgress, ClipboardError> {
        if let Some(active) = &self.active {
            return Err(ClipboardError::Busy(active.key));
        }
        if prepared.coordinator_instance != self.coordinator_instance
            || prepared.generation != self.preparation_generation
            || prepared.operation_identity
                != self
                    .next_operation_identity
                    .checked_add(1)
                    .ok_or(ClipboardError::PreparationOverflow)?
            || prepared.key.binding() != self.binding.binding()
            || prepared.key.revision() != self.binding.revision()
            || prepared.presentation_generation != self.presentation_generation
            || self.last_terminal == Some(prepared.key)
        {
            return Err(ClipboardError::StalePreparation);
        }
        let next_generation = self
            .preparation_generation
            .checked_add(1)
            .ok_or(ClipboardError::PreparationOverflow)?;
        let ClipboardPreparedBegin {
            operation_identity,
            key,
            kind,
            ..
        } = prepared;
        let selection = key.selection();
        self.active = Some(Box::new(ActiveClipboard {
            key,
            operation_identity,
            kind,
            phase: if self.atom_policy == TextInputAtomClipboardPolicy::Propagate {
                ClipboardCollectionPhase::Classifying
            } else {
                ClipboardCollectionPhase::Collecting
            },
            state: ClipboardState::CollectingObjects,
            text_cursor: selection.start().byte_offset,
            text_target: None,
            pending_text: None,
            pending_object: None,
            object_cursor: None,
            object_page_complete: false,
            prior_object: None,
            current_object: None,
            retained_text_response: None,
            retained_object_response: None,
            start_gap_proven: false,
            end_gap_proven: false,
            start_anchor_had_object: false,
            end_anchor_had_object: false,
            output: ExactOutput::default(),
            open_atom: None,
            source_line_breaks: 0,
            provenance: match self.limits.provenance() {
                ClipboardProvenancePolicy::Omit => None,
                ClipboardProvenancePolicy::Stream(limits) => {
                    Some(Box::new(ProvenanceCollection::new(limits)))
                }
            },
        }));
        self.next_operation_identity = operation_identity;
        self.preparation_generation = next_generation;
        if selection.is_empty() {
            Ok(self.complete_collection())
        } else {
            Ok(ClipboardProgress::NeedObjectPage { key, cursor: None })
        }
    }

    pub fn begin(
        &mut self,
        id: ClipboardId,
        kind: ClipboardKind,
        selection: SourceRange,
        predecessor: MutationPositions,
    ) -> Result<ClipboardProgress, ClipboardError> {
        let prepared = self.prepare_begin(id, kind, selection, predecessor)?;
        self.commit_begin(prepared)
    }

    pub fn begin_selection(
        &mut self,
        id: ClipboardId,
        kind: ClipboardKind,
        anchor: SourcePosition,
        head: SourcePosition,
    ) -> Result<ClipboardProgress, ClipboardError> {
        let selection = match anchor.compare_in_revision(head) {
            Some(std::cmp::Ordering::Greater) => SourceRange::new(head, anchor),
            Some(_) => SourceRange::new(anchor, head),
            None => return Err(ClipboardError::IncompatibleSelection),
        }
        .map_err(|_| ClipboardError::IncompatibleSelection)?;
        self.begin(
            id,
            kind,
            selection,
            MutationPositions::new(head, anchor, head),
        )
    }

    pub fn request_text_page(
        &mut self,
        key: ClipboardKey,
        id: PageRequestId,
    ) -> Result<PageRequest, ClipboardError> {
        let limits = self.limits;
        if self.highest_request.is_some_and(|highest| id <= highest) {
            return Err(ClipboardError::RequestIdInUse(id));
        }
        let active = self.active_mut(key, ClipboardState::CollectingText)?;
        let page_key = PageRequestKey::adjacent(
            id,
            key.binding(),
            key.revision(),
            PagePurpose::Clipboard,
            active.text_cursor,
            PageDirection::Forward,
            limits.max_text_page_bytes,
        )
        .map_err(|_| ClipboardError::InvalidLimits)?;
        active.pending_text = Some(page_key);
        active.state = ClipboardState::TextPagePending;
        self.highest_request = Some(id);
        Ok(PageRequest::new(page_key))
    }

    pub fn request_object_page(
        &mut self,
        key: ClipboardKey,
        id: ObjectRequestId,
    ) -> Result<ObjectRequest, ClipboardError> {
        if self
            .highest_object_request
            .is_some_and(|highest| id <= highest)
        {
            return Err(ClipboardError::ObjectRequestIdInUse(id));
        }
        let binding = self.binding;
        let generation = self.presentation_generation;
        let limits = self.limits;
        let active = self.active_mut(key, ClipboardState::CollectingObjects)?;
        let selection = key.selection();
        let range = ByteRange::new(selection.start().byte_offset, selection.end().byte_offset)
            .expect("source range has ordered bytes");
        let demand = ObjectDemandEnvelope::range(
            range,
            active.object_cursor,
            ObjectDirection::Forward,
            limits.max_object_page_objects(),
            limits.max_object_page_retained_bytes(),
        )
        .map_err(|_| ClipboardError::InvalidLimits)?;
        let object_key = ObjectRequestKey::new(
            id,
            binding.binding(),
            binding.revision(),
            generation,
            ObjectPurpose::Clipboard,
            demand,
        )
        .map_err(|_| ClipboardError::InvalidLimits)?;
        active.pending_object = Some(object_key);
        active.state = ClipboardState::ObjectPagePending;
        self.highest_object_request = Some(id);
        Ok(ObjectRequest::new(object_key))
    }

    pub fn settle_text_page(
        &mut self,
        key: PageRequestKey,
        failure: PageFailure,
    ) -> Result<ClipboardProgress, ClipboardError> {
        let Some(active) = &self.active else {
            return Err(ClipboardError::ObsoletePage(key));
        };
        let Some(expected) = active.pending_text else {
            return Err(ClipboardError::WrongState {
                expected: ClipboardState::TextPagePending,
                actual: active.state,
            });
        };
        if key != expected {
            return Err(ClipboardError::WrongPageKey {
                expected,
                actual: key,
            });
        }
        let clipboard_key = active.key;
        self.finish(clipboard_key);
        let completion = if failure == PageFailure::Cancelled {
            ClipboardCompletion::Cancelled
        } else {
            ClipboardCompletion::TextPageFailed(failure)
        };
        Ok(ClipboardProgress::Terminal(completion))
    }

    pub fn settle_object_page(
        &mut self,
        key: ObjectRequestKey,
        failure: ObjectPageFailure,
    ) -> Result<ClipboardProgress, ClipboardError> {
        let Some(active) = &self.active else {
            return Err(ClipboardError::ObsoleteObjectPage(key));
        };
        let Some(expected) = active.pending_object else {
            return Err(ClipboardError::WrongState {
                expected: ClipboardState::ObjectPagePending,
                actual: active.state,
            });
        };
        if key != expected {
            return Err(ClipboardError::WrongObjectPageKey {
                expected,
                actual: key,
            });
        }
        let clipboard_key = active.key;
        self.finish(clipboard_key);
        let completion = if failure == ObjectPageFailure::Cancelled {
            ClipboardCompletion::Cancelled
        } else {
            ClipboardCompletion::ObjectPageFailed(failure)
        };
        Ok(ClipboardProgress::Terminal(completion))
    }

    pub fn acknowledge_write(
        &mut self,
        key: ClipboardKey,
        outcome: ClipboardWriteOutcome,
    ) -> Result<ClipboardCompletion, ClipboardError> {
        let active = self.active_mut(key, ClipboardState::AwaitingWrite)?;
        let kind = active.kind;
        let selection_line_breaks = active.source_line_breaks;
        let binding = RangeBinding::new(key.binding(), key.revision(), self.binding.extent());
        self.finish(key);
        Ok(match outcome {
            ClipboardWriteOutcome::Failed => ClipboardCompletion::WriteFailed,
            ClipboardWriteOutcome::Cancelled => ClipboardCompletion::Cancelled,
            ClipboardWriteOutcome::Written if kind == ClipboardKind::Copy => {
                ClipboardCompletion::Copied
            }
            ClipboardWriteOutcome::Written => ClipboardCompletion::Delete(CutDeletion {
                binding,
                selection: key.selection(),
                selection_line_breaks,
                predecessor: key.predecessor(),
            }),
        })
    }

    pub fn acknowledge_provenance_page(
        &mut self,
        page: ClipboardProvenancePage,
    ) -> Result<ClipboardPreparedStep, ClipboardError> {
        self.ensure_preparation_generation_available()?;
        let actual = page.key();
        let Some(active) = self.active.as_mut() else {
            return Err(ClipboardError::ObsoleteProvenancePage(actual));
        };
        if active.state != ClipboardState::AwaitingProvenancePage {
            return Err(ClipboardError::WrongState {
                expected: ClipboardState::AwaitingProvenancePage,
                actual: active.state,
            });
        }
        let provenance = active
            .provenance
            .as_mut()
            .ok_or(ClipboardError::WrongState {
                expected: ClipboardState::AwaitingProvenancePage,
                actual: active.state,
            })?;
        let expected = provenance
            .current_page
            .as_ref()
            .map(ClipboardProvenancePage::key)
            .ok_or(ClipboardError::ObsoleteProvenancePage(actual))?;
        match provenance.acknowledge(page) {
            Ok(()) => {
                active.state = ClipboardState::CollectingObjects;
                self.prepare_next()
            }
            Err(true) => {
                let key = active.key;
                self.finish(key);
                Err(ClipboardError::ProvenancePageCollision(actual))
            }
            Err(false) => Err(ClipboardError::WrongProvenancePage { expected, actual }),
        }
    }

    fn active_for_key(&self, key: ClipboardKey) -> Result<&ActiveClipboard, ClipboardError> {
        let Some(active) = &self.active else {
            return if self.last_terminal == Some(key) {
                Err(ClipboardError::Obsolete(key))
            } else {
                Err(ClipboardError::NoActive)
            };
        };
        if active.key != key {
            return Err(ClipboardError::WrongKey {
                expected: active.key,
                actual: key,
            });
        }
        Ok(active)
    }

    fn active_mut(
        &mut self,
        key: ClipboardKey,
        expected: ClipboardState,
    ) -> Result<&mut ActiveClipboard, ClipboardError> {
        let active = self.active_for_key(key)?;
        if active.state != expected {
            return Err(ClipboardError::WrongState {
                expected,
                actual: active.state,
            });
        }
        Ok(self.active.as_mut().expect("active checked"))
    }

    fn finish(&mut self, key: ClipboardKey) {
        self.active = None;
        self.last_terminal = Some(key);
    }
}
