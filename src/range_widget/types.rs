use gpui::{Pixels, SharedString, StreamingLayoutBinding};
use gpui_scrollbar::ScrollbarStyle;

use crate::{
    BlockTarget, ByteOffset, ByteRange, ClipboardKey, ClipboardLimits, ClipboardWriteRequest,
    ExactGeometryError, ExactGeometryLimits, MutationError, MutationFragment, MutationKey,
    MutationLimits, MutationOutcome, MutationProposal, PageFailure, PageRequest, PageRequestKey,
    RangeBinding, ResidencyLimits, SegmentationLimits, StreamingGeometryStyle,
};

/// Exact hard limits owned by one mounted range-backed widget.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RangeTextInputLimits {
    pub max_surface_bytes: usize,
    pub max_surface_items: usize,
    pub page_bytes: u64,
    pub platform_bytes: u64,
    pub max_intra_anchor: Pixels,
    pub max_detached_edits: usize,
}

impl RangeTextInputLimits {
    pub fn new(
        max_surface_bytes: usize,
        max_surface_items: usize,
        page_bytes: u64,
        platform_bytes: u64,
        max_intra_anchor: Pixels,
        max_detached_edits: usize,
    ) -> Result<Self, RangeTextInputError> {
        if max_surface_bytes == 0
            || max_surface_items == 0
            || page_bytes == 0
            || platform_bytes == 0
            || max_intra_anchor < Pixels::ZERO
            || !f32::from(max_intra_anchor).is_finite()
            || max_detached_edits == 0
        {
            return Err(RangeTextInputError::InvalidLimits);
        }
        Ok(Self {
            max_surface_bytes,
            max_surface_items,
            page_bytes,
            platform_bytes,
            max_intra_anchor,
            max_detached_edits,
        })
    }
}

/// Complete construction inputs for one exact range-backed widget.
#[derive(Clone)]
pub struct RangeTextInputConfig {
    pub binding: RangeBinding,
    pub layout: StreamingLayoutBinding,
    pub style: StreamingGeometryStyle,
    pub geometry_limits: ExactGeometryLimits,
    pub residency_limits: ResidencyLimits,
    pub mutation_limits: MutationLimits,
    pub clipboard_limits: ClipboardLimits,
    pub segmentation_limits: SegmentationLimits,
    pub limits: RangeTextInputLimits,
    pub viewport_extent: Pixels,
    pub overscan: Pixels,
    pub placeholder: SharedString,
    pub theme: crate::TextInputTheme,
    pub scrollbar_style: ScrollbarStyle,
}

/// Selection direction and endpoints frozen in a coherent publication.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RangeSelection {
    pub anchor: ByteOffset,
    pub head: ByteOffset,
}

impl RangeSelection {
    pub const fn caret(offset: ByteOffset) -> Self {
        Self {
            anchor: offset,
            head: offset,
        }
    }

    pub fn range(self) -> ByteRange {
        ByteRange::new(self.anchor.min(self.head), self.anchor.max(self.head))
            .expect("ordered selection")
    }
}

/// Compact logical vertical position.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RangeScrollAnchor {
    pub source: ByteOffset,
    pub intra_anchor: Pixels,
}

/// Optional opaque host-owned undo/redo frontier carried by restoration only.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RangeHistoryFrontier {
    pub id: u64,
    pub undo_available: bool,
    pub redo_available: bool,
}

/// Exact host-owned undo or redo intent emitted before any mutation proposal exists.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RangeHistoryIntent {
    key: MutationKey,
    kind: crate::MutationKind,
}

impl RangeHistoryIntent {
    pub const fn new(key: MutationKey, kind: crate::MutationKind) -> Self {
        Self { key, kind }
    }
    pub const fn key(self) -> MutationKey {
        self.key
    }
    pub const fn kind(self) -> crate::MutationKind {
        self.kind
    }
}

/// Exact logical result plan returned by the host-owned history authority.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RangeHistoryPlan {
    intent: RangeHistoryIntent,
    proposal: MutationProposal,
    selection: RangeSelection,
}

impl RangeHistoryPlan {
    pub const fn new(
        intent: RangeHistoryIntent,
        proposal: MutationProposal,
        selection: RangeSelection,
    ) -> Self {
        Self {
            intent,
            proposal,
            selection,
        }
    }
    pub const fn intent(self) -> RangeHistoryIntent {
        self.intent
    }
    pub const fn proposal(self) -> MutationProposal {
        self.proposal
    }
    pub const fn selection(self) -> RangeSelection {
        self.selection
    }
}

/// Compact state exported only at a fully quiescent cut.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RangeRestorationSeed {
    pub binding: RangeBinding,
    pub caret: ByteOffset,
    pub selection: RangeSelection,
    pub scroll: RangeScrollAnchor,
    pub viewport: ByteRange,
    pub overscan: ByteRange,
    pub history: Option<RangeHistoryFrontier>,
}

/// App-neutral typed work emitted to the host.
#[derive(Debug)]
pub enum RangeTextInputRequest {
    Page(PageRequest),
    CancelPage(PageRequestKey),
    ReleasePage(PageRequestKey),
    MutationPreflight(MutationProposal),
    MutationFragment {
        key: MutationKey,
        fragment: MutationFragment,
    },
    MutationCommit(MutationKey),
    CancelMutation(MutationKey),
    DetachedMutation(MutationKey),
    HistoryIntent(RangeHistoryIntent),
    CancelHistoryIntent(RangeHistoryIntent),
    ClipboardWrite(ClipboardWriteRequest),
}

/// App-neutral mounted interaction outcomes that do not require host work.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RangeTextInputEvent {
    InlineAtomClicked(crate::AtomId),
    FocusLost,
    MutationSettled {
        key: MutationKey,
        outcome: MutationOutcome,
    },
    RestorationRejected,
}

/// Result of a platform query that may require bounded replay first.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PlatformRangeResult {
    Pending(PageRequestKey),
    Ready(String),
}

/// Terminal or contract failure at the widget boundary.
#[derive(Debug)]
#[non_exhaustive]
pub enum RangeTextInputError {
    InvalidLimits,
    NotMounted,
    Busy,
    Pending,
    ReadOnly,
    Stale,
    MalformedSeed,
    NotQuiescent,
    SurfaceCapacity,
    DetachedCapacity,
    IncompleteSurface,
    Geometry(ExactGeometryError),
    Mutation(MutationError),
    Contract(crate::RangeContractError),
}

impl std::fmt::Display for RangeTextInputError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "range text input rejected operation: {self:?}")
    }
}

impl std::error::Error for RangeTextInputError {}

impl From<ExactGeometryError> for RangeTextInputError {
    fn from(value: ExactGeometryError) -> Self {
        Self::Geometry(value)
    }
}

impl From<MutationError> for RangeTextInputError {
    fn from(value: MutationError) -> Self {
        Self::Mutation(value)
    }
}

impl From<crate::RangeContractError> for RangeTextInputError {
    fn from(value: crate::RangeContractError) -> Self {
        Self::Contract(value)
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct DesiredSurface {
    pub selection: RangeSelection,
    pub composition: Option<ByteRange>,
    pub scroll: RangeScrollAnchor,
    pub target_block: Pixels,
    pub viewport_extent: Pixels,
    pub overscan: Pixels,
    pub preserve_scroll_anchor: bool,
    pub reveal_caret: bool,
}

/// Frozen logical and visual facts owned by one exact realization job.
#[derive(Clone, Copy, Debug)]
pub(super) struct SurfaceCandidate {
    pub job: crate::GeometryJobKey,
    pub binding: RangeBinding,
    pub desired: DesiredSurface,
    pub restoration: Option<RangeRestorationSeed>,
}

impl DesiredSurface {
    pub fn origin(viewport_extent: Pixels, overscan: Pixels) -> Self {
        Self {
            selection: RangeSelection::caret(ByteOffset::new(0)),
            composition: None,
            scroll: RangeScrollAnchor {
                source: ByteOffset::new(0),
                intra_anchor: Pixels::ZERO,
            },
            target_block: Pixels::ZERO,
            viewport_extent,
            overscan,
            preserve_scroll_anchor: false,
            reveal_caret: true,
        }
    }

    pub fn target(self) -> BlockTarget {
        BlockTarget::new(self.target_block, self.viewport_extent, self.overscan)
    }
}

/// Settlement input for an admitted or detached exact host mutation.
pub type RangeMutationResult = MutationOutcome;

/// Terminal page input for a keyed host failure.
pub type RangePageFailure = PageFailure;

/// Key of a pending clipboard operation.
pub type RangeClipboardKey = ClipboardKey;
