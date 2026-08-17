use gpui::{Pixels, SharedString, StreamingLayoutBinding};
use gpui_scrollbar::ScrollbarStyle;

use crate::{
    BlockTarget, ByteOffset, ByteRange, ClipboardKey, ClipboardLimits, ClipboardWriteRequest,
    ExactGeometryError, ExactGeometryLimits, MutationError, MutationFragment, MutationKey,
    MutationLimits, MutationOutcome, MutationProposal, ObjectRequest, ObjectRequestKey,
    ObjectResidencyLimits, PageRequest, PageRequestKey, PresentationGeneration, RangeBinding,
    ResidencyLimits, SegmentationLimits, StreamingGeometryStyle,
};

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

#[derive(Clone)]
pub struct RangeTextInputConfig {
    pub binding: RangeBinding,
    pub presentation_generation: PresentationGeneration,
    pub layout: StreamingLayoutBinding,
    pub style: StreamingGeometryStyle,
    pub geometry_limits: ExactGeometryLimits,
    pub residency_limits: ResidencyLimits,
    pub object_residency_limits: ObjectResidencyLimits,
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

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct RangeScrollAnchor {
    pub source: ByteOffset,
    pub intra_anchor: Pixels,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RangeSourceSelection {
    pub anchor: crate::SourcePosition,
    pub head: crate::SourcePosition,
}

impl RangeSourceSelection {
    pub const fn caret(position: crate::SourcePosition) -> Self {
        Self {
            anchor: position,
            head: position,
        }
    }

    pub fn range(self) -> Result<crate::SourceRange, crate::SourceRangeError> {
        match self.anchor.compare_in_revision(self.head) {
            Some(std::cmp::Ordering::Greater) => crate::SourceRange::new(self.head, self.anchor),
            _ => crate::SourceRange::new(self.anchor, self.head),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RangeRestorationScrollAnchor {
    pub position: crate::SourcePosition,
    pub intra_anchor: Pixels,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RangeHistoryFrontier {
    pub id: u64,
    pub undo_available: bool,
    pub redo_available: bool,
}

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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RangeHistoryPlan {
    intent: RangeHistoryIntent,
    proposal: MutationProposal,
    positions: crate::MutationPositions,
}

impl RangeHistoryPlan {
    pub const fn new(
        intent: RangeHistoryIntent,
        proposal: MutationProposal,
        positions: crate::MutationPositions,
    ) -> Self {
        Self {
            intent,
            proposal,
            positions,
        }
    }
    pub const fn intent(self) -> RangeHistoryIntent {
        self.intent
    }
    pub const fn proposal(self) -> MutationProposal {
        self.proposal
    }
    pub const fn positions(self) -> crate::MutationPositions {
        self.positions
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RangeRestorationSeed {
    pub binding: RangeBinding,
    pub caret: crate::SourcePosition,
    pub selection: RangeSourceSelection,
    pub scroll: RangeRestorationScrollAnchor,
    pub history: Option<RangeHistoryFrontier>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RealizedInlineObjectAnchor {
    pub binding: RangeBinding,
    pub object_id: crate::InlineObjectId,
    pub order: crate::InlineObjectOrder,
    pub presentation_generation: crate::PresentationGeneration,
    pub layout_epoch: crate::LayoutEpoch,
    pub bounds: gpui::Bounds<Pixels>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InlineObjectActivationKey {
    Enter,
    Space,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum InlineObjectInputOrigin {
    Pointer { point: gpui::Point<Pixels> },
    Keyboard { key: InlineObjectActivationKey },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InlineObjectActivation {
    pub anchor: RealizedInlineObjectAnchor,
    pub origin: InlineObjectInputOrigin,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InlineObjectRealizationLossReason {
    SelectionChanged,
    FocusLost,
    Disabled,
    Removed,
    Replaced,
    Superseded,
    Unrealized,
    Disposed,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InlineObjectRealizationLoss {
    pub anchor: RealizedInlineObjectAnchor,
    pub reason: InlineObjectRealizationLossReason,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct ActiveInlineObject {
    pub anchor: RealizedInlineObjectAnchor,
    pub leading: crate::SourcePosition,
    pub trailing: crate::SourcePosition,
    pub activation_eligible: bool,
}

#[derive(Debug)]
pub enum RangeTextInputRequest {
    Page(PageRequest),
    CancelPage(PageRequestKey),
    ReleasePage(PageRequestKey),
    ObjectPage(ObjectRequest),
    CancelObjectPage(ObjectRequestKey),
    ReleaseObjectPage(ObjectRequestKey),
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
    CancelClipboardWrite(ClipboardKey),
}

#[derive(Clone, Debug, PartialEq)]
pub enum RangeTextInputEvent {
    InlineAtomClicked(crate::AtomId),
    FocusLost,
    MutationSettled {
        key: MutationKey,
        outcome: MutationOutcome,
    },
    RestorationRejected,
    InlineObjectActivated(InlineObjectActivation),
    InlineObjectRealizationLost(InlineObjectRealizationLoss),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PlatformRangeResult {
    Pending(PageRequestKey),
    Ready(String),
}

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
    pub source_selection: Option<RangeSourceSelection>,
    pub composition: Option<ByteRange>,
    pub scroll: RangeScrollAnchor,
    pub target_block: Pixels,
    pub viewport_extent: Pixels,
    pub overscan: Pixels,
    pub preserve_scroll_anchor: bool,
    pub reveal_caret: bool,
    pub inline_object_interaction: Option<DesiredInlineObjectInteraction>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum DesiredInlineObjectInteraction {
    Set {
        object_id: crate::InlineObjectId,
        order: crate::InlineObjectOrder,
        activation_eligible: bool,
        origin: Option<InlineObjectInputOrigin>,
    },
    Clear(InlineObjectRealizationLossReason),
}

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
            source_selection: None,
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
            inline_object_interaction: None,
        }
    }

    pub fn target(self) -> BlockTarget {
        BlockTarget::new(self.target_block, self.viewport_extent, self.overscan)
    }
}
