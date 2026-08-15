//! App-neutral text-input primitives for GPUI applications.
//!
//! The crate separates the plain text editing model from any host
//! application meaning. Hosts decide what a field represents, how changes are
//! validated, and how accepted text is persisted or submitted.
//!
//! # Examples
//!
//! Single-line fields normalize inserted newlines into spaces:
//!
//! ```
//! use gpui_text_input::{TextInputOptions, TextInputState};
//!
//! let mut input = TextInputState::new("", TextInputOptions::single_line());
//! let change = input.paste("alpha\nbeta").expect("paste changes text");
//!
//! assert_eq!(input.text(), "alpha beta");
//! assert_eq!(change.replacement, "alpha beta");
//! ```
//!
//! Multiline fields preserve logical newlines and expose line movement:
//!
//! ```
//! use gpui_text_input::{TextInputOptions, TextInputState};
//!
//! let mut input = TextInputState::new("one\ntwo", TextInputOptions::multiline());
//! input.move_to_end();
//! input.insert_newline().expect("newline changes text");
//! input.paste("three").expect("paste changes text");
//! input.move_home();
//!
//! assert_eq!(input.text(), "one\ntwo\nthree");
//! assert_eq!(input.selection(), 8..8);
//! ```
//!
//! GPUI widgets are entity based. Install the app-neutral key bindings once,
//! then create a [`TextInput`] entity from a view:
//!
//! ```
//! use gpui::{App, Context};
//! use gpui_text_input::{
//!     TextInput, TextInputEnterKey, TextInputSingleLineVerticalKey, ensure_text_input_bindings,
//! };
//!
//! fn install_bindings(cx: &mut App) {
//!     ensure_text_input_bindings(cx);
//! }
//!
//! fn build_input(cx: &mut Context<TextInput>) -> TextInput {
//!     let mut input = TextInput::new("", "Value", cx);
//!     input.set_enter_key(TextInputEnterKey::Propagate);
//!     input.set_single_line_vertical_key(TextInputSingleLineVerticalKey::Propagate);
//!     input
//! }
//! ```
//!
//! Opaque atom ranges can be used by hosts that render domain content as text
//! markers while keeping the reusable editor unaware of the domain payload:
//!
//! ```
//! use gpui_text_input::{TextInputAtom, TextInputOptions, TextInputState};
//!
//! let mut input = TextInputState::new("See [A]", TextInputOptions::single_line());
//! input
//!     .set_atoms(vec![TextInputAtom::new("asset-a", 4..7, "[Attachment A]")])
//!     .expect("atom range should match display text");
//! input.select_all();
//!
//! let selection = input.selection_export().expect("selection should export");
//! assert_eq!(selection.display_text(), "See [A]");
//! assert_eq!(selection.copy_text(), "See [Attachment A]");
//! ```
//!
//! Retained-count diagnostics expose app-neutral lower-bound byte counts:
//!
//! ```
//! use gpui_text_input::{TextInputOptions, TextInputState};
//!
//! let input = TextInputState::new("draft", TextInputOptions::single_line());
//! let counts = input.retained_counts();
//!
//! assert_eq!(counts.current_text_bytes, "draft".len());
//! ```
//!
//! Range-backed hosts describe one exact revision and request only bounded
//! pages; the resident projection never owns the logical whole value:
//!
//! ```
//! use gpui_text_input::{
//!     BindingId, ByteOffset, LogicalExtent, PageDemand, PageDemandEnvelope, PageDirection,
//!     PagePurpose, PageRequestId, RangeBinding, RangeResidency, ResidencyLimits, SourceRevision,
//! };
//!
//! let binding = RangeBinding::new(
//!     BindingId::new(7),
//!     SourceRevision::new(3),
//!     LogicalExtent::new(1_000_000, 50_000),
//! );
//! let limits = ResidencyLimits::new(8, 64 * 1024, 4, 32 * 1024)
//!     .expect("finite nonzero limits");
//! let mut residency = RangeResidency::new(binding, limits);
//! let first_page = residency.demand(
//!     PageRequestId::new(1),
//!     PagePurpose::Viewport,
//!     PageDemandEnvelope::Adjacent {
//!         anchor: ByteOffset::new(0),
//!         direction: PageDirection::Forward,
//!         max_payload_bytes: 4096,
//!     },
//! ).expect("bounded demand");
//!
//! assert!(matches!(first_page, PageDemand::Requested(_)));
//! assert_eq!(residency.counts().resident_pages, 0);
//! ```
//!
//! Source-zero-width objects use a separate bounded source and residency domain. Their positions
//! name an exact adjacent-object gap and map directly to GPUI's canonical composite coordinates:
//!
//! ```
//! use gpui::{SharedString, StreamingLayoutPosition, px};
//! use gpui_text_input::{
//!     BindingId, ByteOffset, InlineObjectFact, InlineObjectGap,
//!     InlineObjectId, InlineObjectNeighbor, InlineObjectOrder, InlineObjectPresentation,
//!     LogicalExtent, ObjectDemand, ObjectDemandEnvelope, ObjectDirection, ObjectPage,
//!     ObjectPageEdgeFact, ObjectPageId, ObjectPurpose, ObjectRequestId, ObjectResidency,
//!     ObjectResidencyLimits, PresentationGeneration, RangeBinding, RangeResidency,
//!     ResidencyLimits, SourcePosition, SourceRevision,
//! };
//!
//! let binding = RangeBinding::new(
//!     BindingId::new(7),
//!     SourceRevision::new(3),
//!     LogicalExtent::new(1_000_000, 50_000),
//! );
//! let generation = PresentationGeneration::new(4);
//! let text_limits = ResidencyLimits::new(2, 64 * 1024, 2, 32 * 1024).expect("finite limits");
//! let text = RangeResidency::new(binding, text_limits);
//! let limits = ObjectResidencyLimits::new(4, 16, 64 * 1024, 16 * 1024, 2, 8, 32 * 1024)
//!     .expect("finite limits");
//! let mut objects = ObjectResidency::new(binding, generation, limits);
//! let demand = ObjectDemandEnvelope::anchor(
//!     ByteOffset::new(0), None, ObjectDirection::Forward, 4, 4096,
//! ).expect("bounded demand");
//! let ObjectDemand::Requested(request) = objects.demand(
//!     ObjectRequestId::new(1), ObjectPurpose::Viewport, demand,
//! ).expect("current demand") else { unreachable!() };
//!
//! let id = InlineObjectId::new(9);
//! let order = InlineObjectOrder::new(20);
//! let presentation = InlineObjectPresentation::new(
//!     5, SharedString::new_static("[object]"), px(64.), px(20.), px(15.), None,
//!     0, true,
//! ).expect("finite presentation");
//! let page = ObjectPage::new(
//!     ObjectPageId::new(1), request.key(),
//!     vec![InlineObjectFact::new(id, ByteOffset::new(0), order, "attachment", presentation)],
//!     ObjectPageEdgeFact::EnvelopeBoundary, ObjectPageEdgeFact::EnvelopeBoundary, true, None,
//! ).expect("coherent object page");
//! let proofs = text
//!     .prove_object_page_anchors(objects.binding(), &page)
//!     .expect("origin is a proven edge");
//! objects.admit(page, proofs).expect("admitted exact page");
//!
//! let before = SourcePosition::new(
//!     ByteOffset::new(0),
//!     InlineObjectGap::before(InlineObjectNeighbor::new(id, order)),
//! );
//! let gpui_position = StreamingLayoutPosition::from(before);
//! assert_eq!(gpui_position.byte_offset, 0);
//! assert_eq!(objects.counts().resident_objects, 1);
//! ```
//!
//! [`RangeTextInput`] mounts that contract as a GPUI entity. The widget emits
//! typed host work; the host fetches exact pages, stages mutations, performs
//! clipboard writes, and returns keyed terminal results without giving the
//! widget a whole-source string:
//!
//! ```no_run
//! use gpui_text_input::{RangeTextInput, RangeTextInputRequest};
//!
//! fn dispatch_one(input: &mut RangeTextInput) {
//!     let Some(request) = input.take_request() else {
//!         return;
//!     };
//!     match request {
//!         RangeTextInputRequest::Page(page) => {
//!             // Fetch only `page.key().demand()` from the exact named revision,
//!             // then return it through `RangeTextInput::deliver_page`.
//!             let _ = page;
//!         }
//!         RangeTextInputRequest::MutationPreflight(proposal) => {
//!             // Validate the exact proposal before accepting its bounded stream.
//!             let _ = proposal;
//!         }
//!         RangeTextInputRequest::HistoryIntent(intent) => {
//!             // Resolve the host-owned undo/redo intent to a `RangeHistoryPlan`, then
//!             // stream its exact fragments through `stage_history_fragment`.
//!             let _ = intent;
//!         }
//!         RangeTextInputRequest::ClipboardWrite(write) => {
//!             // Acknowledge the platform write with `settle_clipboard_write`.
//!             let _ = write;
//!         }
//!         _ => {}
//!     }
//! }
//! ```
//!
//! Atoms that cross page edges repeat only their stable facts and exact page
//! intersection, so adjacent bounded pages can reconcile them without a
//! whole-source atom registry:
//!
//! ```
//! use gpui_text_input::{AtomFact, AtomId, ByteRange};
//!
//! let whole = ByteRange::from_u64(2, 6).expect("checked range");
//! let left = AtomFact::new(
//!     AtomId::new(9),
//!     whole,
//!     ByteRange::from_u64(2, 4).expect("checked fragment"),
//!     "attachment",
//! );
//! let right = AtomFact::new(
//!     AtomId::new(9),
//!     whole,
//!     ByteRange::from_u64(4, 6).expect("checked fragment"),
//!     "attachment",
//! );
//!
//! assert!(left.reconciles_with(&right));
//! ```
//!
//! Range-backed edits stage one checked replacement and publish only the host's exact terminal
//! result. A composite position remains a host claim until mutation preflight reserves it against
//! normally admitted [`RangeResidency`] and [`ObjectResidency`] state. Committed successor
//! positions follow the same rule through [`RangeEditCoordinator::settle_committed`]; callers
//! cannot construct a [`MutationCommit`] or its proofs directly.
//!
//! Clipboard collection returns a value for a platform write boundary; a cut
//! deletion token is produced only after that write is acknowledged:
//!
//! ```
//! use gpui_text_input::{
//!     BindingId, ByteOffset, ClipboardId, ClipboardKind, ClipboardLimits, ClipboardProgress,
//!     ClipboardWriteOutcome, InlineObjectGap, LogicalExtent, RangeBinding,
//!     RangeClipboardCoordinator, SourcePosition, SourceRange, SourceRevision,
//! };
//!
//! let binding = RangeBinding::new(BindingId::new(2), SourceRevision::new(1), LogicalExtent::new(0, 0));
//! let mut clipboard = RangeClipboardCoordinator::new(binding, ClipboardLimits::new(64, 16)?);
//! let origin = SourcePosition::new(ByteOffset::new(0), InlineObjectGap::NoObjects);
//! let selection = SourceRange::new(origin, origin)?;
//! let progress = clipboard.begin(ClipboardId::new(3), ClipboardKind::Cut, selection)?;
//! let ClipboardProgress::Write(write) = progress else { unreachable!() };
//! assert_eq!(write.text(), "");
//! let deletion = clipboard.acknowledge_write(write.key(), ClipboardWriteOutcome::Written)?;
//! assert!(matches!(deletion, gpui_text_input::ClipboardCompletion::Delete(_)));
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! A restoration seed retains only exact fixed-size logical facts; importing it validates fresh
//! text and object pages before publishing a new surface:
//!
//! ```
//! use gpui::px;
//! use gpui_text_input::{
//!     BindingId, ByteOffset, InlineObjectGap, LogicalExtent, RangeBinding,
//!     RangeRestorationScrollAnchor, RangeRestorationSeed, RangeSourceSelection,
//!     SourcePosition, SourceRevision,
//! };
//! let binding = RangeBinding::new(
//!     BindingId::new(4), SourceRevision::new(8), LogicalExtent::new(12, 0),
//! );
//! let position = SourcePosition::new(ByteOffset::new(0), InlineObjectGap::NoObjects);
//! let seed = RangeRestorationSeed {
//!     binding,
//!     caret: position,
//!     selection: RangeSourceSelection::caret(position),
//!     scroll: RangeRestorationScrollAnchor { position, intra_anchor: px(0.) },
//!     history: None,
//! };
//! assert_eq!(seed.binding, binding);
//! ```
//!
//! Exact range-backed geometry owns canonical segmentation and consumes GPUI's window-affine
//! streaming layout boundary. Hosts can request pages, but cannot construct or ingest exact
//! checkpoints. Page admissions return typed progress plus every consumed-page or replaced-result
//! release; terminal failures likewise return their stage, release, and exact required byte and
//! semantic-item peak. The item peak consumes GPUI's returned fragment-graph and session charges
//! together with the crate-owned owner, input, job, page, cursor, checkpoint, and publication facts:
//!
//! ```no_run
//! use gpui::{SharedString, StreamingLayoutBinding, StreamingLayoutLimits,
//!     StreamingLayoutPosition, TextRun, black, font, px};
//! use gpui_text_input::{BindingId, ExactGeometryLimits, ExactGeometryOwner, GeometryJobId,
//!     LogicalExtent, RangeBinding, SourceRevision, StreamingGeometryStyle,
//!     StreamingOversizePresentation};
//! let source = RangeBinding::new(BindingId::new(4), SourceRevision::new(2), LogicalExtent::new(1024, 8));
//! let layout = StreamingLayoutBinding {
//!     input_id: 7, segment_policy_id: 11, start_position: StreamingLayoutPosition::at(0),
//!     wrap_width: px(640.), font_size: px(14.),
//!     line_height: px(20.), limits: StreamingLayoutLimits {
//!         segment_bytes: 4096, runs: 16, decorations: 16, glyphs: 8192,
//!         wraps: 1024, maps: 8193, fragments: 1, retained_items: 32 * 1024,
//!         retained_bytes: 512 * 1024,
//!     },
//! };
//! let run = TextRun { len: 0, font: font(".SystemUIFont"), color: black(),
//!     background_color: None, underline: None, strikethrough: None };
//! let oversize = StreamingOversizePresentation::new(
//!     SharedString::new_static(""), vec![], px(12.), px(20.), px(0.), None);
//! let mut geometry = ExactGeometryOwner::new(source, layout,
//!     StreamingGeometryStyle::new(run, oversize),
//!     ExactGeometryLimits::new(16 * 1024, 128, 2 * 1024 * 1024, 4096)?)?;
//! let start = geometry.start_index(GeometryJobId::new(1))?;
//! let _job = start.key();
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

//!
mod actions;
mod atom;
mod boundary;
mod change;
mod editing;
mod movement;
mod newline;
mod object_residency;
mod options;
mod range_clipboard;
mod range_edit;
mod range_geometry;
mod range_segmentation;
mod range_source;
mod range_widget;
mod residency;
mod state;
mod widget;

pub use actions::{
    Backspace, Copy, Cut, Delete, DeleteWordBackward, DeleteWordForward, Enter, InsertNewline,
    MoveDown, MoveEnd, MoveHome, MoveLeft, MoveRight, MoveToEnd, MoveToStart, MoveUp, MoveWordLeft,
    MoveWordRight, Paste, Redo, SelectAll, SelectDown, SelectEnd, SelectHome, SelectLeft,
    SelectRight, SelectToEnd, SelectToStart, SelectUp, SelectWordLeft, SelectWordRight,
    TEXT_INPUT_KEY_CONTEXT, Undo, ensure_text_input_bindings,
};
pub use atom::{
    TextInputAtom, TextInputAtomError, TextInputSelectionAtom, TextInputSelectionExport,
};
pub use change::TextInputChange;
pub use object_residency::{
    ObjectDemand, ObjectDemandError, ObjectPageAdmission, ObjectPageAdmissionError,
    ObjectPageSettlement, ObjectResidency, ObjectResidencyCounts, ObjectResidencyLimitError,
    ObjectResidencyLimitKind, ObjectResidencyLimits,
};
pub use options::{TextInputMode, TextInputOptions};
pub use range_clipboard::{
    ClipboardCancellation, ClipboardCompletion, ClipboardCounts, ClipboardError, ClipboardId,
    ClipboardKey, ClipboardKind, ClipboardLimits, ClipboardProgress, ClipboardState,
    ClipboardWriteOutcome, ClipboardWriteRequest, CutDeletion, RangeClipboardCoordinator,
};
pub use range_edit::{
    AtomChange, MutationCancellation, MutationCommit, MutationCounts, MutationDisposal,
    MutationError, MutationFragment, MutationFragmentPayload, MutationKey, MutationKind,
    MutationLimits, MutationOutcome, MutationPositions, MutationProposal, MutationSettlement,
    MutationState, ObjectChange, ObjectTarget, OperationId, RangeEditCoordinator, SuccessorObject,
};
pub use range_geometry::{
    BlockTarget, BlockTargetPublication, ExactGeometryAdmission, ExactGeometryAggregate,
    ExactGeometryCheckpoint, ExactGeometryCounts, ExactGeometryError, ExactGeometryFailure,
    ExactGeometryFailureStage, ExactGeometryIndex, ExactGeometryLimits, ExactGeometryOwner,
    ExactGeometryProgress, ExactGeometryRelease, ExactGeometryStart, GeometryJobId, GeometryJobKey,
    GeometryKey, GeometryQuality, LayoutEpoch, StreamingGeometryEstimate, StreamingGeometryStyle,
    StreamingOversizePresentation,
};
pub use range_segmentation::{
    AdjacentPageEdge, AdjacentPageRequest, ResolvedBoundary, SegmentationCancellation,
    SegmentationContinuation, SegmentationCounts, SegmentationDirection, SegmentationError,
    SegmentationKind, SegmentationLimits, SegmentationProgress, SegmentationResume,
};
pub use range_source::{
    AtomFact, AtomId, BindingId, ByteOffset, ByteRange, InlineObjectFact, InlineObjectGap,
    InlineObjectGapError, InlineObjectId, InlineObjectNeighbor, InlineObjectOrder,
    InlineObjectPresentation, LineOffset, LineRange, LogicalExtent, ObjectContractError,
    ObjectCursor, ObjectDemandEnvelope, ObjectDirection, ObjectPage, ObjectPageCharge,
    ObjectPageEdgeFact, ObjectPageFailure, ObjectPageId, ObjectPurpose, ObjectRequest,
    ObjectRequestId, ObjectRequestKey, PageDemandEnvelope, PageDirection, PageEdgeFact,
    PageFailure, PageId, PagePurpose, PageRequest, PageRequestId, PageRequestKey,
    PresentationGeneration, RangeBinding, RangeContractError, RangePage, RangePageCharge,
    SourcePosition, SourceRange, SourceRangeError, SourceRevision,
};
pub use range_widget::{
    CoherentRangeSurface, InlineObjectActivation, InlineObjectActivationKey,
    InlineObjectInputOrigin, InlineObjectRealizationLoss, InlineObjectRealizationLossReason,
    PlatformRangeResult, RangeHistoryFrontier, RangeHistoryIntent, RangeHistoryPlan,
    RangeRestorationScrollAnchor, RangeRestorationSeed, RangeSelection,
    RangeSourceSelection, RangeSurfaceCharge, RangeSurfaceHit, RangeTextInput,
    RangeTextInputConfig, RangeTextInputError, RangeTextInputEvent, RangeTextInputLimits,
    RangeTextInputRequest, RealizedInlineObjectAnchor, RealizedInlineObjectGeometry,
    RealizedInlineObjectPresentation, RealizedObjectGapGeometry,
};
pub use residency::{
    ObjectAnchorProofError, ObjectAnchorProofs, PageAdmission, PageAdmissionError, PageDemand,
    PageDemandError, PageSettlement, RangeResidency, ResidencyCounts, ResidencyLimitError,
    ResidencyLimitKind, ResidencyLimits, ScalarBoundaryProof, ScalarBoundaryProofError,
};
pub use state::{TextInputRetainedCounts, TextInputState};
pub use widget::{
    TextInput, TextInputAtomClipboardPolicy, TextInputCommand, TextInputEnterKey, TextInputEvent,
    TextInputGeometry, TextInputRichPastePolicy, TextInputScrollLimits, TextInputSelection,
    TextInputSingleLineVerticalKey, TextInputTheme, TextInputVerticalReveal,
    wrapped_visual_line_count_for_width,
};
