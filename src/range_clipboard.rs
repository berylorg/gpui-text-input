use crate::{
    AtomId, BindingId, ByteOffset, ByteRange, InlineObjectFact, MutationKey, MutationKind,
    MutationProposal, ObjectCursor, ObjectDemandEnvelope, ObjectDirection, ObjectPage,
    ObjectPageFailure, ObjectPurpose, ObjectRequest, ObjectRequestId, ObjectRequestKey,
    OperationId, PageDirection, PageEdgeFact, PageFailure, PagePurpose, PageRequest, PageRequestId,
    PageRequestKey, PresentationGeneration, RangeBinding, RangePage, SourcePosition, SourceRange,
    SourceRevision,
};

mod collection;
mod lifecycle;

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
}

impl ClipboardKey {
    pub const fn new(
        id: ClipboardId,
        binding: BindingId,
        revision: SourceRevision,
        selection: SourceRange,
    ) -> Self {
        Self {
            id,
            binding,
            revision,
            selection,
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
    max_object_page_objects: usize,
    max_object_page_retained_bytes: usize,
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
        {
            return Err(ClipboardError::InvalidLimits);
        }
        Ok(Self {
            max_bytes,
            max_text_page_bytes,
            max_object_page_objects,
            max_object_page_retained_bytes,
        })
    }
    pub const fn max_bytes(self) -> usize {
        self.max_bytes
    }

    pub const fn max_text_page_bytes(self) -> u64 {
        self.max_text_page_bytes
    }

    pub const fn max_object_page_objects(self) -> usize {
        self.max_object_page_objects
    }

    pub const fn max_object_page_retained_bytes(self) -> usize {
        self.max_object_page_retained_bytes
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClipboardState {
    Idle,
    CollectingObjects,
    ObjectPagePending,
    CollectingText,
    TextPagePending,
    AwaitingWrite,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ClipboardCounts {
    pub staged_bytes: usize,
    pub pending_text_pages: usize,
    pub pending_object_pages: usize,
    pub retained_object_facts: usize,
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
    Write(ClipboardWriteRequest),
    Terminal(ClipboardCompletion),
}

#[derive(Debug, Eq, PartialEq)]
pub struct ClipboardWriteRequest {
    key: ClipboardKey,
    text: String,
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
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClipboardWriteOutcome {
    Written,
    Failed,
    Cancelled,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClipboardCompletion {
    Copied,
    Delete(CutDeletion),
    WriteFailed,
    Cancelled,
    TextPageFailed(PageFailure),
    ObjectPageFailed(ObjectPageFailure),
    TextPageTooLarge,
    TooLarge,
    Malformed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CutDeletion {
    binding: RangeBinding,
    selection: SourceRange,
    selection_line_breaks: u64,
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
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClipboardCancellation {
    key: ClipboardKey,
    pending_text_page: Option<PageRequestKey>,
    pending_object_page: Option<ObjectRequestKey>,
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
struct ActiveClipboard {
    key: ClipboardKey,
    kind: ClipboardKind,
    state: ClipboardState,
    text_cursor: ByteOffset,
    text_target: Option<ByteOffset>,
    pending_text: Option<PageRequestKey>,
    pending_object: Option<ObjectRequestKey>,
    object_cursor: Option<ObjectCursor>,
    object_page_complete: bool,
    prior_object: Option<ObjectCursor>,
    current_object: Option<InlineObjectFact>,
    queued_objects: std::collections::VecDeque<InlineObjectFact>,
    start_gap_proven: bool,
    end_gap_proven: bool,
    start_anchor_had_object: bool,
    end_anchor_had_object: bool,
    output: String,
    open_atom: Option<OpenAtom>,
    source_line_breaks: u64,
}

#[derive(Debug)]
pub struct RangeClipboardCoordinator {
    binding: RangeBinding,
    presentation_generation: PresentationGeneration,
    limits: ClipboardLimits,
    active: Option<ActiveClipboard>,
    last_terminal: Option<ClipboardKey>,
    highest_request: Option<PageRequestId>,
    highest_object_request: Option<ObjectRequestId>,
}

impl RangeClipboardCoordinator {
    pub const fn new(binding: RangeBinding, limits: ClipboardLimits) -> Self {
        Self::new_composite(binding, PresentationGeneration::new(0), limits)
    }

    pub const fn new_composite(
        binding: RangeBinding,
        presentation_generation: PresentationGeneration,
        limits: ClipboardLimits,
    ) -> Self {
        Self {
            binding,
            presentation_generation,
            limits,
            active: None,
            last_terminal: None,
            highest_request: None,
            highest_object_request: None,
        }
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
            .map_or(ClipboardCounts::default(), |active| ClipboardCounts {
                staged_bytes: active.output.len(),
                pending_text_pages: usize::from(active.pending_text.is_some()),
                pending_object_pages: usize::from(active.pending_object.is_some()),
                retained_object_facts: active.queued_objects.len()
                    + usize::from(active.current_object.is_some()),
            })
    }

    pub fn begin(
        &mut self,
        id: ClipboardId,
        kind: ClipboardKind,
        selection: SourceRange,
    ) -> Result<ClipboardProgress, ClipboardError> {
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
        let key = ClipboardKey::new(
            id,
            self.binding.binding(),
            self.binding.revision(),
            selection,
        );
        if self.last_terminal == Some(key) {
            return Err(ClipboardError::Obsolete(key));
        }
        self.active = Some(ActiveClipboard {
            key,
            kind,
            state: ClipboardState::CollectingObjects,
            text_cursor: selection.start().byte_offset,
            text_target: None,
            pending_text: None,
            pending_object: None,
            object_cursor: None,
            object_page_complete: false,
            prior_object: None,
            current_object: None,
            queued_objects: std::collections::VecDeque::new(),
            start_gap_proven: false,
            end_gap_proven: false,
            start_anchor_had_object: false,
            end_anchor_had_object: false,
            output: String::new(),
            open_atom: None,
            source_line_breaks: 0,
        });
        if selection.is_empty() {
            Ok(self.complete_collection())
        } else {
            Ok(ClipboardProgress::NeedObjectPage { key, cursor: None })
        }
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
        self.begin(id, kind, selection)
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
            limits.max_object_page_objects,
            limits.max_object_page_retained_bytes,
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
            }),
        })
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
