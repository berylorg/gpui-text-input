use gpui::{Pixels, SharedString, StreamingLayoutBinding, px};
use gpui_scrollbar::ScrollbarStyle;
use std::sync::{Arc, Mutex, MutexGuard};

use crate::{
    BlockTarget, ByteOffset, ByteRange, ClipboardKey, ClipboardLimits, ClipboardWriteRequest,
    ExactGeometryError, ExactGeometryLimits, MutationBeginRequest, MutationCancelRequest,
    MutationCommitRequest, MutationError, MutationFinishInput, MutationKey, MutationLimits,
    MutationOutcome, MutationPageRequest, ObjectRequest, ObjectRequestKey, ObjectResidencyLimits,
    PageRequest, PageRequestKey, PresentationGeneration, RangeBinding, ResidencyLimits,
    SegmentationLimits, StreamingGeometryStyle, TextInputAtomClipboardPolicy, TextInputCommand,
    TextInputEnterKey, TextInputRichPastePolicy,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RangeTextInputLimits {
    pub max_surface_bytes: usize,
    pub max_surface_items: usize,
    pub max_realization_work_per_frame: usize,
    pub max_realized_block_extent: Pixels,
    pub page_bytes: u64,
    pub platform_bytes: u64,
    pub max_intra_anchor: Pixels,
}

impl RangeTextInputLimits {
    pub fn new(
        max_surface_bytes: usize,
        max_surface_items: usize,
        max_realization_work_per_frame: usize,
        max_realized_block_extent: Pixels,
        page_bytes: u64,
        platform_bytes: u64,
        max_intra_anchor: Pixels,
    ) -> Result<Self, RangeTextInputError> {
        if max_surface_bytes == 0
            || max_surface_items == 0
            || max_realization_work_per_frame == 0
            || max_realized_block_extent <= Pixels::ZERO
            || !f32::from(max_realized_block_extent).is_finite()
            || page_bytes == 0
            || platform_bytes == 0
            || max_intra_anchor < Pixels::ZERO
            || !f32::from(max_intra_anchor).is_finite()
        {
            return Err(RangeTextInputError::InvalidLimits);
        }
        Ok(Self {
            max_surface_bytes,
            max_surface_items,
            max_realization_work_per_frame,
            max_realized_block_extent,
            page_bytes,
            platform_bytes,
            max_intra_anchor,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RangeRealizationCapacityState {
    Normal,
    CapacitySaturated,
    ViewportExceedsRenderingCapacity,
    CapacitySaturatedViewportExceedsRenderingCapacity,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum RangeRealizationPriority {
    Caret,
    Ime,
    DirectedSelection,
    ActiveInteraction,
    ScrollAnchor,
    NearbyContent,
}

impl RangeRealizationPriority {
    pub const ORDERED: [Self; 6] = [
        Self::Caret,
        Self::Ime,
        Self::DirectedSelection,
        Self::ActiveInteraction,
        Self::ScrollAnchor,
        Self::NearbyContent,
    ];
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RangeRealizationStep {
    pub spent: usize,
    pub remaining: usize,
    pub progressed: bool,
    pub reached_external_boundary: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RangeRealizationOwnership {
    pub owned_bytes: usize,
    pub owned_items: usize,
    pub resident_page_bytes: usize,
    pub resident_object_bytes: usize,
    pub pending_page_bytes: usize,
    pub pending_object_bytes: usize,
    pub geometry_bytes: usize,
    pub geometry_items: usize,
    pub request_storage_bytes: usize,
    pub request_storage_items: usize,
    pub request_payload_bytes: usize,
    pub request_payload_items: usize,
    pub deferred_response_bytes: usize,
    pub deferred_response_items: usize,
    pub response_custody_bytes: usize,
    pub response_custody_items: usize,
    pub response_custody_count: usize,
    pub response_processing_bytes: usize,
    pub response_processing_items: usize,
    pub page_alias_storage_bytes: usize,
    pub page_alias_storage_items: usize,
    pub page_alias_waits: usize,
    pub pending_configuration_bytes: usize,
    pub pending_configuration_items: usize,
    pub candidate_bytes: usize,
    pub candidate_items: usize,
    pub pending_geometry_record_bytes: usize,
    pub pending_geometry_record_items: usize,
    pub dispatched_record_bytes: usize,
    pub dispatched_record_items: usize,
    pub resident_pages: usize,
    pub resident_objects: usize,
    pub pending_page_requests: usize,
    pub pending_object_requests: usize,
    pub dispatched_page_requests: usize,
    pub dispatched_object_requests: usize,
    pub active_geometry_jobs: usize,
    pub pending_geometry_pages: usize,
    pub pending_geometry_objects: usize,
    pub resident_geometry_page_waits: usize,
    pub coalesced_geometry_page_waits: usize,
    pub index_geometry_page_waits: usize,
    pub target_geometry_page_waits: usize,
    pub deferred_geometry_responses: usize,
    pub pending_target_intents: usize,
    pub pending_layout_intents: usize,
    pub pending_presentation_intents: usize,
    pub pending_rebind_intents: usize,
    pub scheduled_continuations: usize,
    pub queued_requests: usize,
    pub candidates: usize,
    pub checkpoints: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RangeRealizationDiagnostics {
    pub max_surface_bytes: usize,
    pub max_surface_items: usize,
    pub max_realization_work_per_frame: usize,
    pub max_realized_block_extent: Pixels,
    pub max_resident_pages: usize,
    pub max_resident_page_bytes: usize,
    pub max_owned_pages: usize,
    pub max_pending_page_requests: usize,
    pub max_pending_page_bytes: u64,
    pub max_resident_object_pages: usize,
    pub max_resident_objects: usize,
    pub max_resident_object_bytes: usize,
    pub max_owned_objects: usize,
    pub max_pending_object_requests: usize,
    pub max_pending_object_bytes: usize,
    pub max_queued_requests: usize,
    pub max_geometry_bytes: usize,
    pub max_geometry_items: usize,
    pub max_checkpoints: usize,
    pub frame_generation: u64,
    pub continuation_scheduled: bool,
    pub frame: RangeRealizationStep,
    pub capacity: RangeRealizationCapacityState,
    pub filler_count: usize,
    pub current: RangeRealizationOwnership,
    pub high_water: RangeRealizationOwnership,
    pub surface_charge: crate::RangeSurfaceCharge,
    pub surface_high_water: crate::RangeSurfaceCharge,
    pub geometry_high_water_bytes: usize,
    pub geometry_high_water_items: usize,
}

#[derive(Clone)]
pub struct RangeTextInputConfig {
    pub binding: RangeBinding,
    pub presentation_generation: PresentationGeneration,
    pub enter_key: TextInputEnterKey,
    pub atom_clipboard_policy: TextInputAtomClipboardPolicy,
    pub rich_paste_policy: TextInputRichPastePolicy,
    pub layout: StreamingLayoutBinding,
    pub style: StreamingGeometryStyle,
    pub geometry_limits: ExactGeometryLimits,
    pub residency_limits: ResidencyLimits,
    pub object_residency_limits: ObjectResidencyLimits,
    pub mutation_limits: MutationLimits,
    pub clipboard_limits: ClipboardLimits,
    pub segmentation_limits: SegmentationLimits,
    pub limits: RangeTextInputLimits,
    pub settlement_coordinator: RangeSettlementCoordinator,
    pub viewport_extent: Pixels,
    pub overscan: Pixels,
    pub placeholder: SharedString,
    pub theme: crate::TextInputTheme,
    pub scrollbar_style: ScrollbarStyle,
}

#[derive(Clone, Debug)]
pub struct RangeSettlementCoordinator {
    state: Arc<Mutex<RangeSettlementState>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RangeSettlementSlot {
    Mutation(MutationKey),
    History(RangeHistoryIntent),
}

impl RangeSettlementSlot {
    const fn key(self) -> MutationKey {
        match self {
            Self::Mutation(key) => key,
            Self::History(intent) => intent.key(),
        }
    }
}

#[derive(Debug)]
struct RangeSettlementState {
    capacity: usize,
    next_operation: u64,
    slots: Vec<RangeSettlementSlot>,
}

impl RangeSettlementCoordinator {
    pub fn new(capacity: usize) -> Result<Self, RangeTextInputError> {
        if capacity == 0 {
            return Err(RangeTextInputError::InvalidLimits);
        }
        let mut slots = Vec::new();
        slots
            .try_reserve_exact(capacity)
            .map_err(|_| RangeTextInputError::DetachedCapacity)?;
        Ok(Self {
            state: Arc::new(Mutex::new(RangeSettlementState {
                capacity,
                next_operation: 1,
                slots,
            })),
        })
    }

    #[cfg(test)]
    pub(crate) fn new_with_next_operation(
        capacity: usize,
        next_operation: u64,
    ) -> Result<Self, RangeTextInputError> {
        let coordinator = Self::new(capacity)?;
        coordinator.lock().next_operation = next_operation;
        Ok(coordinator)
    }

    pub fn capacity(&self) -> usize {
        self.lock().capacity
    }

    pub fn retained_count(&self) -> usize {
        self.lock().slots.len()
    }

    pub fn settle_mutation(&self, key: MutationKey) -> bool {
        self.release(RangeSettlementSlot::Mutation(key))
    }

    pub fn settle_history(&self, intent: RangeHistoryIntent) -> bool {
        self.release(RangeSettlementSlot::History(intent))
    }

    pub(crate) fn allocate_operation(&self) -> Result<crate::OperationId, RangeTextInputError> {
        let mut state = self.lock();
        let operation = state.next_operation;
        state.next_operation = operation.checked_add(1).ok_or(RangeTextInputError::Stale)?;
        Ok(crate::OperationId::new(operation))
    }

    pub(crate) fn claim_host_operation(
        &self,
        operation: crate::OperationId,
    ) -> Result<(), RangeTextInputError> {
        let mut state = self.lock();
        if operation.get() != state.next_operation {
            return Err(RangeTextInputError::Stale);
        }
        let next_operation = operation
            .get()
            .checked_add(1)
            .ok_or(RangeTextInputError::Stale)?;
        state.next_operation = next_operation;
        Ok(())
    }

    pub(crate) fn reserve_mutation(&self, key: MutationKey) -> Result<(), RangeTextInputError> {
        self.reserve(RangeSettlementSlot::Mutation(key))
    }

    pub(crate) fn reserve_history(
        &self,
        intent: RangeHistoryIntent,
    ) -> Result<(), RangeTextInputError> {
        self.reserve(RangeSettlementSlot::History(intent))
    }

    pub(crate) fn contains_history(&self, intent: RangeHistoryIntent) -> bool {
        self.lock()
            .slots
            .contains(&RangeSettlementSlot::History(intent))
    }

    fn reserve(&self, slot: RangeSettlementSlot) -> Result<(), RangeTextInputError> {
        let mut state = self.lock();
        if state
            .slots
            .iter()
            .any(|retained| retained.key().operation() == slot.key().operation())
        {
            return Err(RangeTextInputError::Stale);
        }
        if state.slots.len() == state.capacity {
            return Err(RangeTextInputError::DetachedCapacity);
        }
        state.next_operation = state
            .next_operation
            .max(slot.key().operation().get().saturating_add(1));
        state.slots.push(slot);
        Ok(())
    }

    fn release(&self, slot: RangeSettlementSlot) -> bool {
        let mut state = self.lock();
        let Some(index) = state.slots.iter().position(|retained| *retained == slot) else {
            return false;
        };
        state.slots.swap_remove(index);
        true
    }

    fn lock(&self) -> MutexGuard<'_, RangeSettlementState> {
        self.state.lock().unwrap_or_else(|error| error.into_inner())
    }
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
    pub binding: RangeBinding,
    pub id: u64,
    pub undo_available: bool,
    pub redo_available: bool,
}

impl RangeHistoryFrontier {
    pub const fn unavailable(binding: RangeBinding) -> Self {
        Self {
            binding,
            id: 0,
            undo_available: false,
            redo_available: false,
        }
    }

    pub const fn binding(self) -> RangeBinding {
        self.binding
    }

    pub const fn allows(self, kind: crate::MutationKind) -> bool {
        match kind {
            crate::MutationKind::Undo => self.undo_available,
            crate::MutationKind::Redo => self.redo_available,
            crate::MutationKind::Edit => false,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RangeHistoryIntent {
    key: MutationKey,
    binding: RangeBinding,
    kind: crate::MutationKind,
    frontier: RangeHistoryFrontier,
    caret: crate::SourcePosition,
    selection: RangeSourceSelection,
}

impl RangeHistoryIntent {
    pub const fn new(
        key: MutationKey,
        binding: RangeBinding,
        kind: crate::MutationKind,
        frontier: RangeHistoryFrontier,
        caret: crate::SourcePosition,
        selection: RangeSourceSelection,
    ) -> Self {
        Self {
            key,
            binding,
            kind,
            frontier,
            caret,
            selection,
        }
    }
    pub const fn key(self) -> MutationKey {
        self.key
    }
    pub const fn binding(self) -> RangeBinding {
        self.binding
    }
    pub const fn kind(self) -> crate::MutationKind {
        self.kind
    }
    pub const fn frontier(self) -> RangeHistoryFrontier {
        self.frontier
    }
    pub const fn caret(self) -> crate::SourcePosition {
        self.caret
    }
    pub const fn selection(self) -> RangeSourceSelection {
        self.selection
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RangeHistorySession {
    intent: RangeHistoryIntent,
}

impl RangeHistorySession {
    pub const fn new(intent: RangeHistoryIntent) -> Self {
        Self { intent }
    }
    pub const fn intent(self) -> RangeHistoryIntent {
        self.intent
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RangeHistoryCommit {
    binding: RangeBinding,
    caret: crate::SourcePosition,
    selection: RangeSourceSelection,
    frontier: RangeHistoryFrontier,
}

impl RangeHistoryCommit {
    pub const fn new(
        binding: RangeBinding,
        caret: crate::SourcePosition,
        selection: RangeSourceSelection,
        frontier: RangeHistoryFrontier,
    ) -> Self {
        Self {
            binding,
            caret,
            selection,
            frontier,
        }
    }
    pub const fn binding(self) -> RangeBinding {
        self.binding
    }
    pub const fn caret(self) -> crate::SourcePosition {
        self.caret
    }
    pub const fn selection(self) -> RangeSourceSelection {
        self.selection
    }
    pub const fn frontier(self) -> RangeHistoryFrontier {
        self.frontier
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RangeHistoryOutcome {
    Committed(RangeHistoryCommit),
    Rejected,
    Conflict,
    Cancelled,
    Error,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RangeHistorySettlement {
    Current(RangeHistoryOutcome),
    Obsolete(RangeHistoryOutcome),
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

#[derive(Debug, PartialEq)]
pub struct InlineObjectSurfaceAttachment {
    pub(super) id: u64,
    pub(super) anchor: RealizedInlineObjectAnchor,
}

impl InlineObjectSurfaceAttachment {
    pub const fn anchor(&self) -> RealizedInlineObjectAnchor {
        self.anchor
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InlineObjectSurfaceDismissal {
    RefocusObject,
    ClearObject,
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
    MutationBegin(MutationBeginRequest),
    MutationSourcePage(MutationPageRequest),
    MutationProposalPage(MutationPageRequest),
    MutationFinishInput(MutationFinishInput),
    MutationCommit(MutationCommitRequest),
    CancelMutation(MutationCancelRequest),
    DetachedMutation(MutationKey),
    HistoryIntent(RangeHistoryIntent),
    CancelHistoryIntent(RangeHistoryIntent),
    ClipboardWrite(ClipboardWriteRequest),
    CancelClipboardWrite(ClipboardKey),
}

#[derive(Clone, Debug, PartialEq)]
pub enum RangeTextInputEvent {
    CommandPropagated(TextInputCommand),
    InlineAtomClicked(crate::AtomId),
    FocusLost,
    MutationSettled {
        key: MutationKey,
        outcome: MutationOutcome,
    },
    HistorySettled {
        intent: RangeHistoryIntent,
        outcome: RangeHistoryOutcome,
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
    UnsupportedMutationKind,
    Stale,
    MalformedSeed,
    NotQuiescent,
    SurfaceCapacity,
    PageResponseCapacity(crate::RangePage),
    ObjectResponseCapacity(crate::ObjectPage),
    PageResponseRejected(crate::RangePage),
    ObjectResponseRejected(crate::ObjectPage),
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

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct DesiredSurface {
    pub source_selection: Option<RangeSourceSelection>,
    pub composition: Option<ByteRange>,
    pub scroll: RangeScrollAnchor,
    pub target_block: Pixels,
    pub realization_anchor_block: Pixels,
    pub viewport_extent: Pixels,
    pub realization_extent: Pixels,
    pub overscan: Pixels,
    pub preserve_scroll_anchor: bool,
    pub reveal_caret: bool,
    pub inline_object_interaction: Option<DesiredInlineObjectInteraction>,
    pub capacity_saturated: bool,
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
    pub fn origin(viewport_extent: Pixels, realization_extent: Pixels, overscan: Pixels) -> Self {
        Self {
            source_selection: None,
            composition: None,
            scroll: RangeScrollAnchor {
                source: ByteOffset::new(0),
                intra_anchor: Pixels::ZERO,
            },
            target_block: Pixels::ZERO,
            realization_anchor_block: Pixels::ZERO,
            viewport_extent,
            realization_extent,
            overscan,
            preserve_scroll_anchor: false,
            reveal_caret: true,
            inline_object_interaction: None,
            capacity_saturated: false,
        }
    }

    pub fn target(self) -> BlockTarget {
        BlockTarget::new(
            self.realization_anchor_block,
            self.realization_extent.min(self.viewport_extent),
            self.overscan,
        )
    }

    pub fn priority(self) -> RangeRealizationPriority {
        RangeRealizationPriority::ORDERED
            .into_iter()
            .find(|priority| match priority {
                RangeRealizationPriority::Caret => {
                    self.reveal_caret
                        && self.source_selection.is_some_and(|selection| {
                            selection.range().is_ok_and(|range| range.is_empty())
                        })
                }
                RangeRealizationPriority::Ime => self.composition.is_some(),
                RangeRealizationPriority::DirectedSelection => {
                    self.reveal_caret && self.source_selection.is_some()
                }
                RangeRealizationPriority::ActiveInteraction => matches!(
                    self.inline_object_interaction,
                    Some(DesiredInlineObjectInteraction::Set { .. })
                ),
                RangeRealizationPriority::ScrollAnchor => {
                    !self.reveal_caret || self.preserve_scroll_anchor
                }
                RangeRealizationPriority::NearbyContent => true,
            })
            .expect("nearby content is the terminal realization priority")
    }

    pub fn next_capacity_fallback(self, line_height: Pixels) -> Option<Self> {
        let minimum = self.realization_extent.min(line_height).max(px(1.));
        let mut next = self;
        if next.overscan > Pixels::ZERO {
            next.overscan = Pixels::ZERO;
        } else if next.realization_extent > minimum {
            next.realization_extent =
                px((f32::from(next.realization_extent) * 0.5).max(f32::from(minimum)));
        } else {
            return None;
        }
        next.capacity_saturated = true;
        Some(next)
    }
}
