//! Capped clipboard collection over exact range-source pages.

use crate::{
    AtomId, BindingId, ByteOffset, ByteRange, MutationKey, MutationKind, MutationProposal,
    OperationId, PageDirection, PageEdgeFact, PageFailure, PagePurpose, PageRequest, PageRequestId,
    PageRequestKey, RangeBinding, RangePage, SourceRevision,
};

mod collection;
mod lifecycle;

/// Unique identity of one copy or cut collection.
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

/// Exact immutable identity of one captured selection.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ClipboardKey {
    id: ClipboardId,
    binding: BindingId,
    revision: SourceRevision,
    selection: ByteRange,
}

impl ClipboardKey {
    pub const fn new(
        id: ClipboardId,
        binding: BindingId,
        revision: SourceRevision,
        selection: ByteRange,
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
    pub const fn selection(self) -> ByteRange {
        self.selection
    }
}

/// Whether a completed selection is copied or copied before deletion.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClipboardKind {
    Copy,
    Cut,
}

/// Hard bounds for the contiguous result and each requested/retained source page.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClipboardLimits {
    max_bytes: usize,
    max_page_bytes: u64,
}

impl ClipboardLimits {
    pub fn new(max_bytes: usize, max_page_bytes: u64) -> Result<Self, ClipboardError> {
        if max_page_bytes < 4 {
            return Err(ClipboardError::InvalidLimits);
        }
        Ok(Self {
            max_bytes,
            max_page_bytes,
        })
    }
    pub const fn max_bytes(self) -> usize {
        self.max_bytes
    }
    pub const fn max_page_bytes(self) -> u64 {
        self.max_page_bytes
    }
}

/// Observable state of the single clipboard collection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClipboardState {
    Idle,
    Collecting,
    PagePending,
    AwaitingWrite,
}

/// Exact locally retained clipboard counts.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ClipboardCounts {
    pub staged_bytes: usize,
    pub pending_pages: usize,
}

/// The next action after starting or admitting a clipboard page.
#[derive(Debug, Eq, PartialEq)]
pub enum ClipboardProgress {
    NeedPage {
        key: ClipboardKey,
        next_offset: ByteOffset,
    },
    Write(ClipboardWriteRequest),
    Terminal(ClipboardCompletion),
}

/// Complete contiguous value to pass to the platform clipboard boundary.
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

/// Platform clipboard acknowledgement; no platform API is called by this crate.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClipboardWriteOutcome {
    Written,
    Failed,
    Cancelled,
}

/// Terminal clipboard result or the deletion authorized by a successful cut write.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClipboardCompletion {
    Copied,
    Delete(CutDeletion),
    WriteFailed,
    Cancelled,
    PageFailed(PageFailure),
    PageTooLarge,
    TooLarge,
    Malformed,
}

/// A cut deletion token that can exist only after a successful clipboard write.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CutDeletion {
    binding: RangeBinding,
    selection: ByteRange,
    selection_line_breaks: u64,
}

impl CutDeletion {
    pub const fn binding(self) -> RangeBinding {
        self.binding
    }
    pub const fn selection(self) -> ByteRange {
        self.selection
    }
    pub const fn selection_line_breaks(self) -> u64 {
        self.selection_line_breaks
    }
    pub const fn proposal(self, operation: OperationId) -> MutationProposal {
        MutationProposal::new(
            MutationKey::new(self.binding.binding(), self.binding.revision(), operation),
            MutationKind::Edit,
            self.selection,
            self.selection_line_breaks,
        )
    }
}

/// Rejected clipboard transition or response.
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
    WrongPageKey {
        expected: PageRequestKey,
        actual: PageRequestKey,
    },
    ObsoletePage(PageRequestKey),
}

/// Lifecycle cancellation details, including an exact pending host request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClipboardCancellation {
    key: ClipboardKey,
    pending_page: Option<PageRequestKey>,
}

impl ClipboardCancellation {
    pub const fn key(self) -> ClipboardKey {
        self.key
    }
    pub const fn pending_page(self) -> Option<PageRequestKey> {
        self.pending_page
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
    next_offset: ByteOffset,
    pending: Option<PageRequestKey>,
    output: String,
    open_atom: Option<OpenAtom>,
    source_line_breaks: u64,
}

/// Collects one complete selection in logical order under a hard byte cap.
#[derive(Debug)]
pub struct RangeClipboardCoordinator {
    binding: RangeBinding,
    limits: ClipboardLimits,
    active: Option<ActiveClipboard>,
    last_terminal: Option<ClipboardKey>,
    highest_request: Option<PageRequestId>,
}

impl RangeClipboardCoordinator {
    pub const fn new(binding: RangeBinding, limits: ClipboardLimits) -> Self {
        Self {
            binding,
            limits,
            active: None,
            last_terminal: None,
            highest_request: None,
        }
    }
    pub const fn binding(&self) -> RangeBinding {
        self.binding
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
                pending_pages: usize::from(active.pending.is_some()),
            })
    }

    pub fn begin(
        &mut self,
        id: ClipboardId,
        kind: ClipboardKind,
        selection: ByteRange,
    ) -> Result<ClipboardProgress, ClipboardError> {
        if let Some(active) = &self.active {
            return Err(ClipboardError::Busy(active.key));
        }
        if self.binding.extent().check_byte_range(selection).is_err() {
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
            state: ClipboardState::Collecting,
            next_offset: selection.start(),
            pending: None,
            output: String::new(),
            open_atom: None,
            source_line_breaks: 0,
        });
        if selection.is_empty() {
            Ok(self.complete_collection())
        } else {
            Ok(ClipboardProgress::NeedPage {
                key,
                next_offset: selection.start(),
            })
        }
    }

    /// Creates the next bounded clipboard page demand from the proven collection edge.
    pub fn request_page(
        &mut self,
        key: ClipboardKey,
        id: PageRequestId,
    ) -> Result<PageRequest, ClipboardError> {
        let limits = self.limits;
        if self.highest_request.is_some_and(|highest| id <= highest) {
            return Err(ClipboardError::RequestIdInUse(id));
        }
        let active = self.active_mut(key, ClipboardState::Collecting)?;
        let page_key = PageRequestKey::adjacent(
            id,
            key.binding(),
            key.revision(),
            PagePurpose::Clipboard,
            active.next_offset,
            PageDirection::Forward,
            limits.max_page_bytes,
        )
        .map_err(|_| ClipboardError::InvalidLimits)?;
        active.pending = Some(page_key);
        active.state = ClipboardState::PagePending;
        self.highest_request = Some(id);
        Ok(PageRequest::new(page_key))
    }

    pub fn settle_page(
        &mut self,
        key: PageRequestKey,
        failure: PageFailure,
    ) -> Result<ClipboardProgress, ClipboardError> {
        let Some(active) = &self.active else {
            return Err(ClipboardError::ObsoletePage(key));
        };
        let Some(expected) = active.pending else {
            return Err(ClipboardError::WrongState {
                expected: ClipboardState::PagePending,
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
            ClipboardCompletion::PageFailed(failure)
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
