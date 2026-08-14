use super::SegmentationContinuation;
use crate::range_source::{BindingId, ByteOffset, PageRequestKey, SourceRevision};

/// Finite per-page and per-resume limits for one segmentation continuation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SegmentationLimits {
    max_page_bytes: u64,
    max_grapheme_steps_per_resume: usize,
}

impl SegmentationLimits {
    /// Creates nonzero hard limits for supplied page bytes and grapheme-cursor calls.
    pub fn new(
        max_page_bytes: u64,
        max_grapheme_steps_per_resume: usize,
    ) -> Result<Self, SegmentationError> {
        if max_page_bytes < 4 || max_grapheme_steps_per_resume == 0 {
            return Err(SegmentationError::InvalidLimits);
        }
        Ok(Self {
            max_page_bytes,
            max_grapheme_steps_per_resume,
        })
    }

    /// Maximum UTF-8 bytes in any requested page.
    pub const fn max_page_bytes(self) -> u64 {
        self.max_page_bytes
    }

    /// Maximum dependency cursor calls made by one grapheme resume.
    pub const fn max_grapheme_steps_per_resume(self) -> usize {
        self.max_grapheme_steps_per_resume
    }
}

/// A boundary policy supported by the range-backed text model.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SegmentationKind {
    /// Unicode extended grapheme-cluster boundaries.
    Grapheme,
    /// Unicode word boundaries from the crate's pinned segmentation fork.
    Word,
    /// Logical-line boundaries at document edges and immediately after `\n`.
    LogicalLine,
}

/// The direction in which the next strict boundary is sought.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SegmentationDirection {
    /// Seek a boundary whose offset is greater than the origin.
    Forward,
    /// Seek a boundary whose offset is less than the origin.
    Reverse,
}

/// The exact edge at which another bounded page must be supplied.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AdjacentPageEdge {
    /// The dependency requested the adjacent chunk beginning at this offset.
    NextChunk(ByteOffset),
    /// The dependency requested the adjacent chunk ending at this offset.
    PrevChunk(ByteOffset),
    /// The dependency requested pre-context ending at this offset.
    PreContext(ByteOffset),
    /// The dependency cursor advanced or accepted context and must revisit from this proven edge.
    Replay(ByteOffset),
}

/// A typed request presented when a continuation needs another exact page.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AdjacentPageRequest {
    pub(super) binding: BindingId,
    pub(super) revision: SourceRevision,
    pub(super) kind: SegmentationKind,
    pub(super) edge: AdjacentPageEdge,
}

/// Fixed segmentation residency visible to capacity tests and owners.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SegmentationCounts {
    /// One live typed continuation.
    pub continuations: usize,
    /// One exact request awaits a page.
    pub pending_pages: usize,
    /// Segmentation continuations never retain an admitted page.
    pub resident_pages: usize,
    /// Segmentation continuations never retain page payload bytes.
    pub resident_page_bytes: usize,
}

/// Exact pending request released when traversal is cancelled.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SegmentationCancellation {
    pub(super) request: PageRequestKey,
}

impl SegmentationCancellation {
    /// Returns the request identity that the host may cancel and release.
    pub const fn pending_request(self) -> PageRequestKey {
        self.request
    }
}

impl AdjacentPageRequest {
    /// Returns the source binding required by this continuation.
    pub fn binding(self) -> BindingId {
        self.binding
    }

    /// Returns the exact source revision required by this continuation.
    pub fn revision(self) -> SourceRevision {
        self.revision
    }

    /// Returns the segmentation policy being continued.
    pub fn kind(self) -> SegmentationKind {
        self.kind
    }

    /// Returns the exact adjacency constraint for the next bounded range.
    pub fn edge(self) -> AdjacentPageEdge {
        self.edge
    }
}

/// A resolved strict boundary for one exact binding and revision.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResolvedBoundary {
    pub(super) binding: BindingId,
    pub(super) revision: SourceRevision,
    pub(super) kind: SegmentationKind,
    pub(super) direction: SegmentationDirection,
    pub(super) origin: ByteOffset,
    pub(super) offset: ByteOffset,
    pub(super) document_edge: bool,
}

impl ResolvedBoundary {
    /// Returns the source binding for which this boundary was proven.
    pub fn binding(self) -> BindingId {
        self.binding
    }

    /// Returns the exact source revision for which this boundary was proven.
    pub fn revision(self) -> SourceRevision {
        self.revision
    }

    /// Returns the boundary policy used.
    pub fn kind(self) -> SegmentationKind {
        self.kind
    }

    /// Returns the traversal direction.
    pub fn direction(self) -> SegmentationDirection {
        self.direction
    }

    /// Returns the original traversal offset.
    pub fn origin(self) -> ByteOffset {
        self.origin
    }

    /// Returns the resolved global byte offset.
    pub fn offset(self) -> ByteOffset {
        self.offset
    }

    /// Reports whether traversal stopped at the start or end of the document.
    pub fn is_document_edge(self) -> bool {
        self.document_edge
    }
}

/// The result of starting or resuming bounded segmentation.
#[derive(Debug)]
pub enum SegmentationProgress {
    /// The strict boundary (or terminal document edge) has been proven.
    Complete(ResolvedBoundary),
    /// The contained continuation names the exact next bounded page request.
    NeedPage(SegmentationContinuation),
}

/// Result of one bounded resume on an existing continuation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SegmentationResume {
    /// The strict boundary (or terminal document edge) has been proven.
    Complete(ResolvedBoundary),
    /// The continuation was rebound to the exact request returned by `pending_request`.
    NeedPage,
}

/// A typed segmentation or page-admission failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SegmentationError {
    /// A required page-byte or work limit was zero.
    InvalidLimits,
    /// The origin or document extent cannot be represented by the cursor dependency.
    OffsetOutOfRange,
    /// The origin lies beyond the logical byte extent or inside a UTF-8 scalar.
    InvalidOrigin,
    /// A page request is not for segmentation or does not match this continuation.
    InvalidRequest,
    /// A returned page does not exactly match the pending request.
    ObsoletePage,
    /// A requested page exceeds the configured byte cap.
    PageRangeLimitExceeded,
    /// A page's document-edge facts contradict its checked logical range.
    MalformedPage,
    /// The requested range is not non-empty and exactly adjacent at the required edge.
    NonAdjacentRequest,
    /// The pinned Unicode cursor rejected otherwise admitted input.
    CursorContract,
}
