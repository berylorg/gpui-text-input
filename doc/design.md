# Goals

Provide reusable GPUI text-input primitives for applications that already build their UI on `gpui`.

The crate exists to supply app-neutral text editing behavior and presentation for single-line and multiline text fields. Host applications remain responsible for their own settings schemas, command semantics, rich draft content, persistence, and backend integration.

## Non-goals

This crate does not define application-specific settings schemas, validation rules, persistence paths, apply or cancel policy, or preference semantics.

This crate does not depend on Beryl, `gpui-settings-window`, Myrrh, Codex, or any other host application crate.

This crate does not own Beryl composer concepts, including image atoms, image marker rendering or context menus, submit-to-Codex behavior, transcript quote insertion, backend input serialization, image-label allocation, workspace image assets, or Beryl-private clipboard metadata.

This crate does not support non-GPUI UI frameworks.

This crate does not retain accessibility-specific inline-object labels or descriptions, construct
an operating-system accessibility tree, publish platform semantic nodes, or receive assistive-
technology actions. Inline-object semantic state and activation eligibility are bounded app-neutral
presentation and interaction facts only.

# Decisions

## Standalone Crate

The crate is a standalone Cargo package named `gpui-text-input`.

It depends directly on `gpui` and exposes an app-neutral public boundary. Consumers that need a forked GPUI package can align or patch `gpui` from their own workspace.

## Ownership Boundary

The ordinary owned-value single-line and multiline variants own their authoritative in-memory UTF-8
value, cursor and selection state, IME composition, generic editing and clipboard mechanics, bounded
undo and redo history, focus integration, layout, scrolling, and app-neutral events.

The range-backed multiline variant does not own authoritative text or storage. Its host owns the
authoritative revisioned UTF-8 and inline-object sources, opaque binding identity, logical extent,
bounded page sources, cursor-based staged ordinary-edit sink, and any supported historical-root
selection authority with exact undo and redo availability. The widget owns only its fixed resident
text-page and object-page windows, revision-bound viewport and range requests, compact cursor and
selection source positions, composition state, scroll state, bounded pending ordinary-edit
coordination, and one fixed-size pending historical-selection intent. One logical ordinary mutation
may consume any representable number of bounded source and fragment pages over time; the widget
never accumulates the complete operation, concatenates pages into an authoritative whole value, or
reconstructs host undo history. Undo and redo carry no source or proposal pages through the
ordinary-edit sink: they select an existing immutable host root through fixed-size intent and
settlement state.

Every range-backed request names the exact binding identity and source revision. A response applies
only to that binding and revision; rebinding never lets an old response become current by offset
coincidence. The public boundary gives page requests, staged mutations, clipboard reads, and layout
jobs distinct request or job identities, and gives historical-root selections distinct operation
identities, so callers can cancel, reject, reconcile, and release them exactly.
Partial application, result-free adoption, and cleanup that mutates authoritative text are invalid
host behavior.

Multiline text-input widgets use app-neutral `gpui-scrollbar` primitives for scrollbar chrome,
managed visibility and fade behavior, and pointer direct manipulation when measured content
overflows vertically. Text-input state owns editing-interaction coordination, wheel scrolling,
keyboard-driven reveal behavior, vertical scroll offset, and scroll-limit clamping in both ownership
variants; `gpui-scrollbar` receives callback-backed geometry and reports page or drag requests
without owning text-input policy or host text authority.

Scrollbar geometry supplied by multiline text-input widgets is derived from the current authoritative text-input scroll offset and the latest measured scroll limits. Painted geometry may provide bounds and limits, but stale painted offsets must not drive scrollbar thumb position after wheel scrolling or keyboard reveal changes.

The crate supports app-neutral source-covering atoms in owned-value and range-backed variants. A
source-covering atom owns one nonempty UTF-8 source range. The range-backed variant additionally
supports source-zero-width inline objects anchored at proven UTF-8 scalar boundaries; they consume
no source bytes. Both forms have stable host-owned identities and fallback copy text. Each zero-
width object additionally has one bounded host-owned order key that totally orders objects sharing
an anchor, plus bounded presentation facts. The crate does not interpret
what an object means or serialize any host domain payload.

Owned-value variants keep their source-covering atom ranges valid across generic edits, selection,
navigation, deletion, undo, redo, and plain clipboard export. Range-backed hosts return
source-covering atoms with revisioned text pages and source-zero-width objects through a distinct
revision-bound bounded object-page source. The widget merge-joins only the facts needed by its
coherent resident projection. It never injects fallback or display text into authoritative UTF-8,
constructs a synthetic source range for a zero-width object, or retains a whole-source object
registry.

Host applications own domain meaning for the text, field validation, settings apply behavior,
command submission, transcript quoting, non-text attachments, backend input serialization,
persistence, and any application-specific clipboard metadata. The range-backed host additionally
owns the authoritative source and mutation boundary above.

## Range-Backed Source Boundary

A range-backed binding is the pair of one opaque host binding identity and one exact source
revision. Its source describes the checked logical UTF-8 extent and serves only bounded absolute
ranges. Each page carries the binding and revision, its source-selected exact logical range, valid
UTF-8 bytes, authoritative source-covering atom facts for that range, and checked preceding,
following, and end-of-source facts. A page edge is a proven UTF-8 scalar boundary but is not
implicitly a grapheme, word, atom, or logical-line boundary.

Page demands additionally carry a unique request identity and purpose, such as viewport, caret,
selection, segmentation, clipboard, platform replay, restoration, or geometry indexing. An
adjacent demand names one already proven UTF-8 boundary, a forward or backward direction, and a
positive maximum payload-byte count. The authoritative source selects the other UTF-8-safe edge;
the returned range begins at a forward anchor or ends at a backward anchor, makes positive progress
unless that anchor is the matching document edge, and never exceeds the demand's byte ceiling. The
ceiling is at least four bytes so any UTF-8 scalar can make progress. The returned edge becomes a
proven anchor for a successor demand without exposing or retaining the rest of the source.

A bounded validation demand checks an offset that is not already proven, including every distinct
restoration-seed offset. The source returns a capped UTF-8-safe window containing that offset, or
the exact document-edge fact, so the widget can prove whether the offset is a scalar boundary. A
non-boundary offset is rejected; it is never rounded, clamped, or translated. Empty-source and both
document-edge cases remain representable without manufacturing payload bytes.

The request key freezes the demand envelope rather than a caller-guessed returned range. A response
applies only when its request identity, binding, revision, purpose, direction or validation kind,
anchor or candidate, returned range, edge facts, and byte ceiling satisfy that exact envelope.
Mismatched, duplicate, stale, non-progressing, over-cap, or invalid-UTF-8 responses cannot enter the
resident projection and release their payload normally. The host and widget keep page payload,
resident-page, and in-flight-request counts and bytes within configured finite limits.

A source-zero-width object demand is separate from a UTF-8 page demand and carries the same exact
binding and revision discipline plus the current host-owned presentation generation. It names one
bounded byte interval or one exact byte anchor, an optional same-anchor order cursor, a direction,
a unique request identity, and positive retained-byte and object-count ceilings. The response
contains only objects inside that envelope, preserves strict `(anchor, order-key, object-id)` order,
and treats both interval byte edges as eligible zero-width anchors, including exact source end. The
host must not translate that interval through a terminal-exclusive source without preserving the
terminal-anchor facts. Each response proves its preceding, following, and completion facts and
supplies a continuation cursor when more objects remain. A response may advance only its object
cursor while consuming no UTF-8 bytes, so an
arbitrarily large set at one anchor remains pageable. Duplicate identities, duplicate same-anchor
order keys, inconsistent continuation facts, an object anchored at a non-scalar boundary, or a
presentation-generation mismatch is malformed or obsolete as appropriate.

One source position is a proven UTF-8 byte offset plus a constant-size inline gap witness. The gap
witness names the adjacent source-zero-width objects at that anchor, or the before-all or after-all
edge, and proves that those identities and order keys are adjacent in the exact source revision.
It therefore represents positions before, between, and after any number of objects at one anchor
without assigning source bytes to them. Caret, selection, composition, replacement, clipboard, and
restoration endpoints use source positions. A byte offset alone is accepted only when no inline-gap
choice is ambiguous at that revision.

The crate owns authoritative grapheme, word, and logical-line segmentation across page boundaries.
For each unresolved leading or trailing page edge it retains a typed continuation naming the
binding, revision, segmentation kind, exact edge offset, and next bounded adjacent range. A
grapheme continuation preserves the exact resolved dependency cursor and its fixed category,
counter, and boundary state across successive adjacent pages; it consumes and releases each page
without retaining or requesting a cumulative containing range. Word and logical-line continuations
likewise retain only their bounded streaming state.

Each resume consumes a configured finite byte and work quantum independently of total segment
length. An exact grapheme may span arbitrarily many pages, but the continuation retains only fixed
state plus exact start and current offsets until a boundary or document edge is proven. Total work
may grow with segment length while retained bytes, page records, and continuation records remain
fixed. Neither the host nor widget may impose a semantic segment-length cap, treat an arbitrary page
edge as a boundary, substitute a different word policy, accumulate the segment bytes, or assemble
the whole document to resolve one continuation.

## Range-Backed Ordinary Edit Transactions

An ordinary edit uses one cursor-paged app-neutral host-owned transaction session keyed by binding
identity, base revision, and unique operation identity. Begin captures the exact predecessor caret
and directed selection, including which endpoint is active, and names the exact composite
replacement range whose endpoints are predecessor source positions. The widget then exchanges
bounded source pages and canonical proposal fragments through explicit cursors. The session keeps
only fixed control state, cumulative checked counts and lengths, the next cursor, and a canonical
cumulative identity; it never retains a whole-operation fragment collection or requires the host
to return one.

Every source or proposal page has a positive item and retained-byte ceiling, an exact cursor and
ordinal, a canonical page identity, and the prior cumulative identity. Exact replay requires all
canonical bytes and the cumulative chain to agree. An occupied cursor or operation identity with
different canonical bytes is a collision, not continuation. `finish-input` is an explicit
authenticated message that fixes the final cumulative identity and declared totals; absence of
`finish-input` is never end-of-input, and no arbitrary cumulative fragment-count ceiling such as
256 or 257 may stand in for it. Per-command, resident-page, queue, and per-frame limits remain
fixed while total operation work and durable progress may grow.

The protocol carries inserted UTF-8, source-covering atom changes, and source-zero-width object
insertions, removals, replacements, or moves as bounded ordered fragments. Removal coordinates are
exact predecessor positions. Every inserted or moved zero-width object carries its authoritative
successor-relative anchor and same-anchor order key, so an object may be placed in newly inserted
text without pretending that the successor coordinate existed in the predecessor. Unchanged
objects retain their relative order unless the transaction explicitly moves them. Staging does
not change authoritative text or objects, and the host may reject the envelope or a page before
commit without exposing a partial replacement.

Typing, paste, deletion, cut, and a host-initiated inline-object command all enter this same session
protocol. Small keystrokes use a low-latency single-page fast path through the same begin,
cumulative identity, `finish-input`, commit, and settlement rules; it is not a second edit
authority. A host-initiated command supplies its exact base binding and revision, operation
identity, predecessor caret and directed selection, composite range, bounded page source, and
intended successor positions through the widget boundary; it cannot mutate widget projection
state directly. The widget rejects an external proposal when the binding is stale, the editor is
not mutable, another mutation owns the single transaction slot, or its bounded envelope is invalid.
The shared settlement coordinator is the sole allocator and admission validator for host-visible
operation identities across live and detached widget generations. It atomically validates each
allocated operation against its host-dispatch frontier, while its finite slots separately retain
admitted settlement custody. The host adds no second last-seen sequence beside it.

Within one transaction, unchanged zero-width objects before the replacement keep their relative
place before inserted content and unchanged objects after it keep their relative place after that
content. The host assigns authoritative successor anchors and same-anchor order keys, preserving
the relative order of unchanged objects unless the transaction explicitly moves them. This rule
allows text insertion before, between, or after same-anchor objects without manufacturing bytes for
the objects themselves.

Cancellation observed before commit admission terminates the transaction as `Cancelled` and leaves
the base revision unchanged. Once commit is admitted, later cancellation cannot retract it or
replace its exact result. The host settles every transaction exactly once as `Committed` with the
new revision and coherent extent, `Rejected`, `Conflict`, `Cancelled`, or `Error`. `Rejected`,
`Cancelled`, and `Error` prove that this transaction made no change from its base; `Conflict` proves
that the base is no longer current and that this transaction applied none of its replacement. A host
with an internally uncertain commit must reconcile it before returning one of these terminal
outcomes; the widget never retries or infers the outcome.

`Committed` atomically publishes the complete replacement and no other outcome publishes any of
it. The committed result supplies the exact successor binding and logical UTF-8 extent plus exact
successor caret and selection source positions, or an exact host-produced position mapping for the
captured endpoints. The widget validates those positions against the successor text and object
sources before adoption. The widget releases each consumed source or proposal payload immediately
after its cumulative identity and bounded effects are durably or synchronously accepted; replay
retains identities and control state, not payload bytes. Terminal settlement releases all
remaining session pages, cursors, reservations, and queued work as one cleanup operation.
Cancellation, failure, late response, collision, or cleanup cannot delete a prefix, retain a
suffix as visible content, publish only object changes, or partially apply compensating work. One
logical transaction has exactly one terminal settlement even when it requires many bounded
commands, and the widget adopts the successor revision only with a coherent source projection for
that exact committed result.

## Range-Backed Historical Root Selection

When undo or redo is supported, the host publishes one opaque history-authority identity and exact
undo and redo availability bound to the live binding identity, source revision, and logical extent.
Availability from any other binding, revision, extent, or authority identity is stale and cannot
enable a command. The widget does not infer availability from resident pages, roots, prior commands,
or locally retained edit records.

Undo and redo share the range-backed widget's single live operation slot with ordinary edits, but
they do not use the ordinary cursor-paged mutation proposal protocol. One request contains only its
direction, unique operation identity, exact base binding, revision and logical extent, opaque
history-authority identity, and captured caret and directed selection. It names no replacement
range, inverse text, proposal source, marker registry, root graph, or historical content. The host
authenticates that fixed-size intent against its current immutable history authority and selects an
existing immutable historical root.

Cancellation observed before selection-command admission settles the request as `Cancelled` and
leaves the base binding, revision, history authority, caret, and directed selection unchanged. Once
admitted, cancellation cannot retract or replace the result. The host reconciles any indeterminate
command custody and settles the request exactly once as `Committed`, `Rejected`, `Conflict`,
`Cancelled`, or `Error`; indeterminate custody is not a sixth terminal outcome. `Rejected`,
`Cancelled`, and `Error` prove that the selection changed no live root or history frontier, while
`Conflict` proves that the named base or history authority was no longer current and that this
request selected nothing. Replay accepts only the exact operation and terminal closure and never
reissues the logical undo or redo from guessed state.

Every terminal settlement is fixed-size control state: it contains the exact operation and base
identity, its outcome, and only the compact successor facts below when committed. It cannot embed
historical content, inverse or replacement payload, page data, object collections, or a root graph.

`Committed` returns only the successor binding identity, source revision and logical extent, the
exact restored caret and directed selection, and the successor opaque history-authority identity
with exact undo and redo availability. It returns no inverse, replacement, text page, object page,
root graph, or document-sized value. The widget adopts that result through one atomic live rebind:
the old binding's requests, resident text and object pages, interaction state, and availability
become retired together, and the successor identity, extent, positions, and availability become
current together. The successor content is then realized exclusively through the ordinary bounded
text-page and object-page sources under the successor binding and revision. Until the required new
pages validate the returned positions and form a coherent surface, any retained prior pixels are
inert pending presentation rather than editable or hit-testable successor content. A position or
gap mismatch leaves the committed successor binding fail-closed and never revives the old root or
reclassifies the host's terminal settlement.

The host provides a configured finite settlement-custody capacity shared across live and detached
widget generations. Every ordinary commit or historical-root selection reserves one compact slot
before admission, so rebind or unmount can transfer its fixed operation identity, exact base key,
and terminal reconciliation state into detached custody without retaining proposal, inverse, page,
marker, root-graph, or whole-value payload. A later generation may operate while earlier admitted
operations settle only when another slot is available; exhaustion rejects new admission without
mutating the current generation or falsifying semantic undo/redo availability. A late result
matches and releases only its exact reserved slot, cannot change a replacement binding or resurrect
a retired generation, and remains under bounded host reconciliation until one terminal outcome
releases it. Repeated rebinds therefore retain at most the configured number of fixed-size
settlement records, independent of operation size, history depth, draft size, or generation count.

## Range-Backed Geometry And Clipboard

Range-backed multiline layout maintains a bounded background visual-line and geometry index. Every
index job and result is keyed by binding identity, source revision, layout epoch, and unique job
identity. The layout epoch changes whenever wrapping width, font or shaping inputs, line metrics,
atom geometry, or another geometry-affecting input changes. Jobs page through source ranges without
retaining shaped output for every visited line.

The geometry owner admits byte and semantic-item residency independently under a configured
retained-memory budget and a configured per-frame work budget. Neither budget is derived from raw
viewport dimensions. Admission consumes GPUI's exact returned fragment-graph item charge and
session continuation charge, then combines them with exact crate-owned owner, immutable input,
desired and active job, borrowed page, cursor and atom, segment, checkpoint, candidate, fragment,
and publication records. Admission includes their peak coexistence with the prior coherent
publication; the exact cap is accepted and one under is rejected atomically without replacing
that publication.

Realization is credit- and capacity-gated. It prioritizes caret, IME, and directed-selection
geometry first, the active interaction or scroll anchor second, and nearby content last. Nominally
visible regions that cannot be admitted are coalesced into bounded filler coverage rather than one
demand or placeholder per line, object, or pixel interval. The widget exposes a typed
capacity-saturated state, including when the nominal viewport exceeds current rendering capacity,
and retains no unbounded realization-demand queue. Logical scroll extent continues to come from
the paged sparse index; interaction with filler re-anchors the target and issues only the next
bounded fetch and realization demand.

Layout uses GPUI's canonical bounded streaming text-layout boundary. The crate derives shaping
segments from its exact grapheme, logical-line, and opaque-atom cursors independently of page edges
and request timing. An ordinary segment ends at the last complete grapheme or atom boundary within
the configured segment-byte cap. GPUI shapes it as an independent shaping context and carries only
bounded visual-line placement state into the next segment. A single indivisible grapheme or atom
that exceeds the cap is scanned to its exact logical end without retaining its complete bytes, then
represented by one compact oversize-layout atom. Its source range remains authoritative for
selection, editing, copy, and replacement.

Source-zero-width objects enter that same inline layout stream at their exact anchors and in their
authoritative same-anchor order. They contribute visual geometry but never advance the UTF-8 source
cursor. A bounded immutable presentation fact supplies the realized object's display content,
layout metrics or presentation key, semantic state, and activation eligibility without exposing
host-domain payload. Presentation facts have separate
configured count and retained-byte caps. A geometry-affecting presentation change starts a new
layout epoch; a paint-only change still publishes under an exact presentation generation.

The layout cursor and sparse checkpoints retain a compact object continuation in addition to the
UTF-8 offset. They may stream arbitrarily many same-anchor objects through bounded pages and work
quanta without retaining the complete set. The coherent viewport records exact bounds and hit
regions only for realized objects in the viewport and bounded overscan. It does not keep an object
realized merely to preserve an offscreen activation anchor.

Each index job begins from one crate-produced compact layout checkpoint and advances through exact
source pages and canonical shaping segments. A checkpoint contains no source text or shaped glyph
payload; it carries the exact source offset and object cursor, block offset, visual-line count,
logical-line and segment position, inline placement continuation, and shaping-input identity needed
to resume.
Checkpoints are produced only by the crate's streaming layout path and cannot be asserted by the
host.

The complete index retains a fixed-capacity monotonic sparse set of those checkpoints, including
the source origin and terminal aggregate. Deterministic compaction may increase the distance between
retained checkpoints but never changes their exact facts. Completing a full bounded-memory scan
proves exact total visual-line count and content height. It does not require retaining one entry or
one shaped result per logical or visual line.

An exact block-target job selects the greatest retained checkpoint at or before the requested block
offset, restores its compact continuation, and streams forward until it resolves the target and the
required viewport plus overscan. Its worst-case work may grow with source length, but its resident
pages, segment bytes, continuation, unpublished fragments, and output window remain within fixed
configured limits. Local wheel, keyboard-reveal, and pointer work may similarly advance from the
current coherent checkpoint without waiting for a complete global index.

Absolute scrollbar drag or page targeting may record a desired block offset while aggregates are
estimated, but it does not infer or mutate a logical source anchor until a complete exact index can
start the block-target job. The old coherent surface remains current while indexing or target
resolution is pending. Estimated byte ratios, page-fragment shaping, caller-supplied checkpoint
facts, and whole-line assembly never authorize source positioning.

The painted viewport, pointer hit testing, caret geometry, selection geometry, inline-object
geometry, and reveal target are exact for the current binding, revision, presentation generation,
and layout epoch. Hit testing distinguishes every same-anchor object and every adjacent inline gap.
An ordinary inline-object activation reports the exact binding, source revision, object identity,
order key, presentation generation, layout epoch, realized bounds, and input origin. Keyboard
activation reports the same realized anchor geometry. Until the background index is complete, only
the total visual-line count, total content height, and scrollbar extent may be explicitly estimated
and refined. Estimated totals never authorize caret placement, selection geometry, object
activation, or source positioning and become exact only when the complete index for that key is
proven. Results from an older binding, revision, presentation generation, epoch, or job identity
are obsolete and discarded.

Viewport publication freezes its composite selection, caret, composition, scroll anchor, exact
checkpoint, resident text and object pages, shaped fragments, source-covering atoms, source-zero-
width objects, caret and selection geometry, hit regions, and reveal facts under one key and one
actual-value capacity admission. Desired interaction state is not current presentation state.
Pending, failed, cancelled, stale, or over-cap replacement work leaves the complete prior coherent
publication unchanged.

Every retained target inline-object fragment owns one matching compact authenticated presentation
record through exact-geometry preparation, accounting, and publication. This target-owned record
authorizes realized presentation and activation even after the supplying source page leaves generic
object residency, and its count is bounded by retained target fragments rather than the total object
run. Resident object pages remain an independent bounded proof/cache owner for composite gaps,
editing, clipboard, and continuation; target presentation records cannot substitute for those
proofs.

An active pointer or keyboard interaction retains its exact composite source gap as successor
target authority, including the same-anchor order witness, even when the prior coherent surface can
map that gap. Retarget preparation and index-response continuation prefer this interaction anchor
over a byte-only fallback. The ordinary bounded exact scanner resolves it for the successor surface;
the widget does not pin its former object page, retain the same-anchor run, or manufacture a direct
lookup side channel.

The widget owns only its retained projection. The containing shell and renderer own rejection or
clamping when an OS drawable surface, framebuffer, or renderer allocation is itself
unrepresentable; the widget does not reinterpret that external surface limit as a document, edit,
selection, or undo limit.

Restoration validates the seed's binding, revision, extent, bounded intra-anchor offset, and every
distinct caret, selection, scroll, viewport, and overscan range through bounded source and layout
continuations. Off-viewport endpoints retain exact logical facts and an exact resolved block
relationship; an absent rectangle is valid only when that relationship proves the endpoint is
outside the published viewport. No restored surface publishes until these facts and the visible
window are coherent.

Range-backed copy and cut bind to the exact current binding, revision, and selected composite
range. They merge UTF-8 and inline-object pages in source order, emitting each selected object's
fallback text at its exact source position and same-anchor order. One host-configured hard
clipboard-byte cap applies to the contiguous result; the widget does not start a clipboard write
unless it has the complete exact representation within that cap. Text-page or object-page failure,
revision conflict, cancellation, rebind, unmount, or a representation exceeding the cap makes no
document mutation.

An already published coherent surface remains the sole clipboard command surface while a
same-binding, same-revision nonterminal geometry target is pending. Copy and cut remain available
when that published surface and its current composite selection otherwise admit the command. Begin
captures only that published binding, revision, and selection; it does not read, adopt, or publish
the target candidate's desired selection, geometry, residency, or interaction state. The pending
target continues independently and may later publish or fail without requalifying the clipboard
operation already bound to the prior surface. This availability does not make the target candidate
interactive for caret placement, hit testing, object activation, or another geometry-dependent
command. An absent coherent surface, a different binding or revision, or an unpublished selection
remains ineligible.

The range-backed clipboard boundary has one explicit provenance policy. `Omit` produces the
ordinary capped plain-text write without provenance. `Stream` emits bounded provenance pages for
selected source-zero-width objects while constructing that same one contiguous result; it does not
collect a whole provenance vector, register objects, merge the selection a second time, or build a
second text value. Source-covering atom fallbacks are not provenance-page items: one such fallback
may cross several text pages and has no object-item continuation in the source protocol. Its plain
fallback remains part of the exact contiguous result under either policy.

Each streamed provenance item contains the stable object identity, exact source anchor and
same-anchor order, and the checked half-open output-byte range occupied by that object's fallback.
The enclosing clipboard key qualifies every page to the exact binding, revision, directed
predecessor positions, and composite selection, including endpoint gap witnesses. Empty fallbacks
produce empty output ranges but still advance the page item ordinal and object cursor, so any
number of same-anchor objects remains pageable independently of output bytes.

Every provenance page has a positive item ceiling and positive retained-byte ceiling, an exact
selection-qualified start cursor, page ordinal, next cursor, canonical page identity, prior
cumulative identity, and resulting cumulative identity. Canonical identities include every
semantic field and the prior chain. The coordinator retains the current page through one shared
bounded allocation until acknowledgement and otherwise retains only fixed cumulative cursor,
count, byte, ordinal, and identity state. The host must return that exact current page for
acknowledgement. An exact current full page key plus structural equality advances the stream. The
same current full key with differing structure, including a malformed current-key page, is a
collision that terminates and releases the stream. A different full key--whether from a foreign
operation, previous, skipped or reordered ordinal, or stale selection--is not an acknowledgement of
current custody: it is rejected as stale or wrong without mutating or clearing the current page, so
the correct current page remains acknowledgeable or cancellable.

Page accounting includes the exact fixed item-storage extent, its shared allocation header and handles, and
peak coexistence with the in-progress contiguous output, current source page, queued object facts,
and final write ownership. A configured page envelope that cannot hold one fixed-size item is
invalid. Item counts, ordinals, output offsets, fallback-byte totals, page counts, and cumulative
identities advance with checked arithmetic. Exceeding either page limit or any checked total
terminates without a write.

The clipboard coordinator reports one exact dynamic ownership charge to the widget surface budget.
It includes the active owner allocation, the contiguous-output backing extent, queued-object storage
capacity, current and queued object payload allocation capacities, and either the provenance builder allocation
or the current shared page allocation. The widget combines that charge with current source-response
custody and queued-request ownership for every admission and high-water observation. When the
coordinator and queued or dispatched request hold the same provenance page, its shared allocation
is charged exactly once while both fixed handle records remain covered by their respective owner
storage. Exact acknowledgement captures the chained fields, releases both current-page handles,
and leaves only fixed cumulative state; a next builder is allocated lazily only when another
selected object arrives, so the acknowledged page never coexists with replacement builder
capacity. Final-write transfer releases object/provenance allocations before the single output
allocation moves to request custody; cancellation and settlement return the coordinator charge to
zero.

Starting an operation uses the same coordinator-owned preparation boundary. An inactive
coordinator prepares an opaque begin token without allocation or mutation; the token binds the
non-reused coordinator instance, inactive preparation generation, intended clipboard key, kind,
selection, and predecessor, and reports the exact active-owner and optional provenance-owner
successor charge. The widget admits its current surface ownership plus that charge before commit.
Byte- or item-one-under rejection leaves the coordinator inactive with zero clipboard ownership;
exact-fit commit revalidates the token and only then allocates and publishes the active operation.
This rule applies equally to empty and nonempty selections and to omitted and streamed provenance.

Text-page and object-page response charges include their text, atom, and object vector capacities,
every fallback string capacity, and every presentation allocation extent rather than only initialized
lengths. These exact charges apply both to initial widget custody and coordinator transfer, including
tiny logical pages whose supplied owners have large spare capacity.
Every widget owner of an object page likewise charges the page header plus every allocated object-
vector slot, including spare slots. Geometry preparation, deferred response custody, residency,
surface publication, diagnostics, and their coexistence peaks use that allocation-slot count plus
one page-owner item; semantic object-count limits alone continue to use initialized object length.
Inline-object display presentation uses one immutable shared backing from the admitted object fact
through GPUI fragment construction and target publication. Cloning source response custody creates
a distinct backing, while geometry fragment and target handles alias the admitted source backing
without copying it. The combined widget charge identifies that overlap and charges the backing once
across response, residency, geometry staging, publication, and release; fixed handles remain covered
by their containing records. Admission observes the exact successor before publishing any alias.

Text-page and object-page responses enter the clipboard merge only through the coordinator-owned
prepare/commit lifecycle. Preparation borrows the exact retained response, validates its current
key and continuation, and runs the single authoritative merge engine only to its next fixed-size
semantic step. It returns an opaque response-qualified step plus the exact coordinator peak and
successor ownership charges. Preparation neither allocates, mutates coordinator state, consumes the
response, nor clears dispatch custody. It may therefore be repeated byte-for-byte after a rejected
admission without replay ambiguity.

The widget admits each prepared step against the exact current surface ownership with the prior
coordinator charge replaced by the prepared peak. A rejected, stale, overflowing, or one-under
admission leaves both response and dispatch custody unchanged. Accepted commit consumes that opaque
step only when its non-reused coordinator instance, active clipboard key, monotonic operation
instance, preparation generation, and exact immutable response-instance identity still match. A
byte-equal clone of the same immutable response retains that identity; an independently constructed
response, including an empty or equal-charge response, receives another checked non-reused identity.
Commit also requires the same exact retained charge and prepared structure, so a clone whose deep
allocation capacities differ does not satisfy a token prepared from its lineage peer.
It applies the already selected
transition without another merge or projection. A terminal response preparation commits through the
same response-specific entry point: it consumes that exact response, releases its dispatch, and
returns terminal progress atomically. Multi-step response processing retains the one bounded source
response until its final prepared step commits; source release and dispatch removal occur together
only then. Cancellation, rebind, unmount, collision, finish, or coordinator reuse invalidates every
outstanding prepared step. Repeated preparation is harmless; after one token commits, every
duplicate is stale even when its charge and response body match.

The first prepared transition that needs nonempty output charges and fallibly establishes one exact
fixed backing allocation for the configured complete-output byte ceiling. Every later append writes
into that same allocation without growth or accumulated-payload copying, and final write transfer
reuses it allocation-free. An operation that produces no bytes never allocates it. Provenance builders
likewise use fallibly allocated exact fixed item storage; page emission moves that storage into one
fixed shared page owner without a second item allocation. No allocator-reported excess capacity or
post-allocation observation participates in admission.
Failure to establish an already admitted exact output or provenance allocation is a terminal local
resource failure, not stale input and not retryable capacity. The coordinator closes the active
operation atomically, releases any retained response and dispatch custody, discards prepared local
work, and returns a typed terminal completion; the widget publishes no clipboard write or cut
deletion and schedules no retry from that failed operation.

Prepared steps cover the single output-backing allocation, source-covering atom fragments
that continue across text pages, current and next zero-width object facts with deep fallback and
presentation payloads, lazy provenance-builder allocation, builder-to-current-page transfer, and
final output transfer. Their response identity, preparation generation, source cursor, output
offset, object cursor, provenance cursor, and ownership arithmetic are checked. A step prepared from
one state or response cannot commit against another, and overflow fails before allocation or
mutation. The successor generation is reserved before any allocation or mutation; exhaustion leaves
the response, coordinator, and dispatch unchanged. When admission rejects coordinator-derived work
after a response was already transferred or released, the unchanged generation remains runnable and
the widget schedules that exact work without redispatching a response. No widget-side merge forecast,
whole-page conservative reservation, cloned retry payload, output growth copy, or post-mutation
capacity check is part of this boundary.

After the final page is acknowledged, the single cap-bounded contiguous write request closes the
stream. Its compact closure binds the exact page and item totals, fallback-byte total, output-byte
total, prior cumulative identity, and a canonical final identity that also covers the complete
plain-text bytes. `Omit` carries an explicit absent closure rather than simulating an empty stream.
The final write never owns provenance items and no provenance allocation is live when it is
dispatched.

Malformed source pages, provenance limit exhaustion, current-key collision, text or object failure,
cancellation, rebind, unmount, coordinator disposal or reuse, clipboard write success, and
clipboard write failure release current page and final-write custody exactly. A stale or wrong-key
page is rejected without releasing current custody. No terminal path authorizes a partial
provenance stream, a clipboard write after failed closure, or cut deletion before successful final
write acknowledgement.

Cut writes the complete clipboard representation first. Only a successful clipboard write may
start the staged deletion transaction against the same binding, base revision, and selected range.
If that later deletion conflicts, is rejected, is cancelled, or fails, the copied clipboard value
may remain but that deletion transaction applies no change. The crate provides no cut path that
deletes before or independently of successful copy.

## Range-Backed Prepublication Realization

The public range-backed API provides one non-mounted prepublication realization boundary. It
accepts an exact compact restoration seed and prepares a coherent range-backed publication
candidate without constructing, mounting, rendering, or hiding a `text-input` element.
The session is app-neutral and has no slot, view, window contribution, focus handle, tab stop, key
context, platform-input handler, pointer or wheel hitbox, scrollbar interaction route, event
subscription, or widget notification path. It may use explicitly supplied GPUI text and layout
resources, but it never enters element layout, prepaint, paint, focus, or event dispatch.

Session creation freezes the seed's binding identity, source revision and logical extent, caret and
directed selection source positions, logical vertical-scroll anchor and bounded object
continuation, and optional opaque history-authority identity with exact undo and redo availability.
The owner supplies the currently authoritative binding descriptor and the same bounded text-page,
object-page, and presentation sources used by an ordinary range-backed widget. Creation rejects an
unequal binding, revision, or extent before admitting work. Before readiness, the session obtains
one fixed-size exact-current owner validation result that repeats the binding, revision, extent,
history-authority identity, and availability. Mismatch, rejection, or obsolescence prevents
candidate publication. An absent history authority is likewise an exact validated state and is
never inferred from prior widget state. The session accepts no mutation, clipboard, IME,
historical-root selection, selection-change, scroll-command, or activation input; all seed facts
remain immutable while it realizes them.

The owner also supplies one immutable realization environment. It contains the available inline
and block allocation, viewport and bounded overscan target, scale factor, text style and shaping
inputs, line metrics, presentation generation and inline-object metric inputs, every other layout-
epoch input, retained-byte and semantic-item capacities, request-window capacities, and per-step
work quantum. The session derives no geometry from a mounted element or hidden window. Changing any
geometry-affecting environment fact makes the session and its result stale; the owner starts a new
session rather than translating a candidate between environments.

The environment is bound to the exact window-affine GPUI text system used for shaping and retains
one owner-supplied bounded cleanup ledger. A session may advance only through that text system, and
its candidate may be adopted only by an ordinary widget in the same window-affine environment.
Environment identifiers alone do not substitute for this identity check.

Progress is explicitly host-driven and asynchronous. One service step consumes at most the
configured work quantum and returns a bounded status plus only the next admitted effects. Effects
use the ordinary exact request keys and finite windows for text pages, object pages, segmentation
continuations, geometry indexing, and block-target realization. The owner dispatches those effects,
delivers each result through the session's exact key, and schedules another service step; the
session does not spawn work, poll itself, depend on render callbacks, or busy-wait. Total service
steps and total source bytes visited may grow with document or segment length, while each step,
request, response, continuation, resident owner, and unpublished geometry owner remains within the
configured finite capacities.

Prepublication realization reuses the ordinary range-backed source validation, segmentation,
streaming layout, geometry indexing, retained-memory accounting, and staged-publication
preparation. It does not implement a second layout engine, renderer, surface format, or position
policy. The session never concatenates the source, object collection, selected range, logical line,
history, or requested viewport into a whole-value buffer and never asks the owner for a whole
value. Source-zero-width objects retain their exact anchors, order cursors, gap witnesses,
presentation generation, and bounded paging discipline.

A ready result is one one-shot coherent publication candidate with its exact source key,
presentation generation, layout epoch, realization environment identity, restoration facts,
bounded resident inputs, prepared geometry, capacity state, and complete retained-capacity charge.
Readiness applies the same coherence rules as ordinary mounted publication: caret, directed
selection, scroll continuation, required viewport facts, inline-object gaps, and every exact
geometry fact used by the candidate agree under one key. Bounded filler and a declared capacity-
saturated state may cover lower-priority nominal viewport regions under the ordinary policy, but
failure to realize the minimum exact caret, selection, and active scroll-anchor facts is a capacity
outcome rather than a partial ready result.

The candidate has no pixels and cannot paint, receive input, emit widget events, or become visible
by itself. A fresh ordinary range-backed widget may consume it only when its source binding,
revision, extent, history facts, presentation generation, layout-epoch inputs, realization
environment, and configured capacities still match exactly. Consumption moves the candidate's
bounded owners into the widget's ordinary staged-publication boundary without cloning or
re-requesting them; that boundary performs the final atomic admission before the widget enables
focus or input participation. A mismatch rejects and releases the candidate. After adoption,
ordinary render, prepaint, paint, hit testing, scrolling, focus, event routing, and later
realization are unchanged and read the same coherent surface used by every other range-backed
publication.

Every session and request carries a non-reused session generation in addition to its ordinary
binding, revision, presentation, epoch, request, and job keys. Cancellation or disposal cancels
all cancellable external effects, releases resident text and object pages, segmentation and
geometry continuations, prepared fragments, request slots, and staged-publication capacity, and
produces no candidate. A late result for a cancelled, replaced, completed, or stale session is
obsolete and releases its payload without changing another session or mounted widget. Dropping or
rejecting an unconsumed candidate releases all of its transferred-ready capacity.

Before any request effect becomes observable, the session admits one exact cleanup record into the
environment's configured finite ledger and charges that record and its reserved slot. Delivery,
consumption, transfer, cancellation, and release update the same record; they never create an
untracked interval. Destruction of a session or candidate performs only a synchronous, non-
allocating, non-callback, non-panicking mark of its already registered records as cleanup-ready.
It invokes no host code and cannot reenter the owner. The host drains those exact cancellation and
release effects through the ordinary explicit service path with bounded work, and acknowledges each
record before its slot can be reused. Ledger exhaustion is an ordinary capacity outcome before a
request is exposed. Forgetting a Rust value does not authorize further progress or slot reuse; the
owner must retain and drain the ledger for the lifetime of its environment.

Malformed responses, exact-key collisions, source or history mismatch, arithmetic failure,
deterministic geometry failure, and unsupported environment input terminate the session with a
typed content-free failure and release its complete custody. Initial capacity denial leaves no
session or allocation. A retryable exact-response admission denial preserves only that response's
already bounded custody under the ordinary staged-publication rules and reports capacity-blocked
without advancing; subsequent cancellation, terminal capacity failure, or disposal releases it.
No failure, cancellation, capacity outcome, or stale completion publishes a partial candidate,
retains unbounded work, revives detached widget state, or changes authoritative text or history.

## Range-Backed Atomic Interaction Publication

The range-backed widget owns one bounded staged-publication boundary for layout, presentation,
true-rebind, selection, reveal, and inline-object activation transitions that cross geometry, text
or object residency, queued host requests, coherent-surface candidates, or interaction events. A
transition prepares one compact candidate against the exact current widget generation. It does not
mutate current owners, desired state, active-object state, request queues, counters, scrollbar
ownership, or externally visible events while admission remains fallible.

The candidate contains only the proposed operation and bounded deltas: exact geometry input and
job replacement, the retirement set for superseded jobs and requests, post-retirement text and
object residency dispositions, queued and dispatched request effects, proposed desired selection
and surface candidate, an already prepared terminal coherent surface when the target is complete,
active-object result, compact lifecycle changes, and deferred events. It contains no coherent-
surface copy, resident-page graph copy, whole source, object registry, or second mounted owner
graph.

Host page delivery enters this boundary before consuming its dispatched key or changing text,
object, or geometry residency. Text and object owners prepare an inbound-page disposition and a
read-only projected resident iterator. Nonterminal geometry preparation owns only the new scanner
delta and preallocated destination storage. Terminal preparation constructs the final compact
target, including fragment-matched inline-object presentation records, from the admitted records
plus that delta and charges its coexistence with the unchanged
active job and all current resident pages. Commit then moves the prepared admissions and clears the
dispatched key without rollback or allocation.

A malformed text-page, object-page, or residency-backed geometry delivery terminally settles the
named request or job step and releases both the delivered payload and its reservation. It is not
retryable under the same key. A well-formed exact-key delivery rejected only because explicit
terminal surface-publication capacity is unavailable consumes neither the dispatched key nor its
reservation, changes no resident owner, and remains retryable with that exact key. Every other
deterministic preparation or candidate-capacity failure atomically closes the exact response,
dispatch, and geometry job while preserving the prior coherent publication and unrelated residency.

Public selection, restoration, scroll, layout, presentation, and index transitions align or
replace in-flight target geometry. A completed response that no longer aligns with its terminal
surface candidate therefore violates that invariant rather than naming a retarget transition.
Terminal preparation classifies such deterministic invariant or surface failures before custody
handling, closes the named response and geometry work with a content-free failure, and creates no
capacity fallback or pending successor intent. The prior coherent publication and unrelated
residency remain unchanged.

Exact-geometry scan component capacity exceeded by an immutable response under unchanged configured
layout limits is a deterministic preparation failure, not retryable terminal-publication capacity.
It uses the same atomic response-and-job closure. Content-free diagnostics retain only the last
response-rejection class, rejection count, and exact-geometry failure stage; they expose no request
key, source offset, payload, or content. A monotonic content-free counter separately records
successful superseded-job object-response settlement. Only explicit surface-publication capacity
preserves exact response custody for retry, and every retained retry schedules realization
liveness.

Pending Select All is part of terminal preparation rather than a successor transition. The exact
completed index retains constant-size first and last object cursors, from which the candidate derives
document endpoints before coherent-surface admission. Rejection therefore preserves the prior
selection and request state; success publishes the terminal surface with the exact full selection.

Preparation computes residency against a read-only post-retirement projection. Entries named by
the retirement set are treated as absent and their exact charges are subtracted before the new
demand is classified. Coalescing is valid only onto a pending request that survives the same
candidate; otherwise the demand uses a surviving resident page or reserves one new exact request.
When a geometry object demand coalesces onto a surviving external request, or a superseded geometry
job's exact response arrives while a newer job is active, that response first settles its own exact
dispatch into object residency and releases it once. The current logical geometry job then consumes
the admitted page through the existing resident-object path when its demand matches, without
cloning or reinterpreting the response key. A superseded response never fails the current job; the
combined candidate retains the external response and dispatch for exact retry only when it is
rejected solely for explicit terminal surface-publication capacity. Residency-admission or any
other candidate-capacity failure terminally settles the old exact response, dispatch, and any
remaining old-job custody without residency admission or retry and without failing the newer job.
When an index completes while its prepared target transition remains nonterminal, presentation
overlap is derived from that transition's active scanner. Absence of a terminal target publication
is not a capacity failure and does not retain the completed response for retry.
Rapid target replacement prepares retirement and successor admission together, so it neither
cancels the current target before successor admission nor rejects the successor merely because the
superseded target still owns the single active slot.

Candidate admission uses delta-only recursive charges and includes every retained byte and semantic
item owned by the candidate itself: each box, each vector's actual allocated capacity, the proposed
geometry job and terminal surface, request and event records, and their peak coexistence with the
current owners and prior coherent publication. Preparation allocates one empty bounded destination
request queue with capacity for surviving current requests and prepared effects. Commit moves
surviving requests into that storage without cloning their payloads or allocating. Exact fit is
accepted and one under is rejected. Preparation may inspect only configured bounded owner
collections and compact checkpoints; it performs no logical-source scan and allocates no whole-
state snapshot.

After every capacity, identity, arithmetic, owner-key, residency, queue, geometry, surface, and
lifecycle check succeeds, commit runs synchronously without yielding or invoking a host callback.
The scrollbar's read-only exact-current-owner check is the final fallible gate for true rebind. The
widget first moves the prepared deltas into geometry, residency, lifecycle, the preallocated request
queue, desired, coherent-surface or surface-candidate, active-object, and identity-counter state.
It then performs the now-infallible exact scrollbar-owner replacement before any request, event,
drag-cancellation, or notification effect becomes observable. Only after both owners are coherent
may it expose cancellations and releases, new page or object requests, realization-loss or
activation events, restoration outcomes, drag cancellation, and notification. A reentrant response
or host action therefore observes only the fully committed generation.

Any rejected or discarded candidate releases only its own exactly accounted temporary capacity.
The prior geometry key and configuration, coherent surface, current and desired selection, active
object, resident and pending pages, request queues, counters, scrollbar owner, and event stream
remain unchanged. Rejection uses no compensating rollback and cannot leave a later publication path
for the rejected desired state.

Candidate preparation and commit occur only on explicit interaction or lifecycle transitions.
Ordinary render, paint, caret lookup, hit testing, presentation-metadata reads, and stable-frame work read
the immutable coherent surface directly and do not inspect a candidate. This boundary adds no lock,
registry, routine scan, or per-lookup allocation to those paths and preserves GPUI's existing text-
map asymptotics and constant-time realized-object lookup after fragment identification.

Prepaint may spend one configured bounded work quantum advancing already admitted geometry whose
required text, object, and continuation inputs are resident. This resident-only advancement stops
before dispatching a request, invoking a host callback, or performing any other external work. If
it reaches terminal object work, it leaves terminal object publication and every externally
observable effect to the widget's ordinary service path through this staged-publication boundary.

Wheel, scrollbar, reveal, and other rapid-retarget entry points derive proposed desired state in a
local value and pass it directly into candidate preparation. They never write widget desired state
or notify merely because preparation was attempted. A terminal target is not a special fallible
tail: its coherent surface, restoration validation, replacement peak, active-object loss, and
request-queue transfer are prepared and charged inside the same candidate before mutation.

## Text Model

The owned-value text model stores plain UTF-8 text and exposes caret, selection, marked-text, and
edit operations in terms of valid text boundaries and inline gaps. The range-backed model exposes
compact source positions against one exact host revision while retaining only bounded text and
object pages; nonresident byte-boundary or inline-gap decisions request the required revision-bound
facts instead of guessing.

Left and right movement traverse the composite source order. Each source-zero-width object is one
indivisible step even when several share one byte anchor. The caret may stop before the first,
between adjacent objects, and after the last. Selection endpoints preserve those exact gaps, so a
selection may include one same-anchor object without including its neighbors or any source byte.
Backspace, Delete, replacement, and cut target the adjacent or selected object through the ordinary
staged mutation boundary; they never emulate object removal by deleting display or fallback text.

Character-wise movement and deletion operate on Unicode grapheme boundaries. Word-wise movement,
selection, deletion, and double-click selection use the crate-owned Unicode word segments exposed by
the exact resolved `unicode-segmentation` dependency. The policy is Windows-first and deterministic:
previous-word skips immediately preceding whitespace and lands at the start of the preceding
non-whitespace segment; next-word crosses the current segment and following whitespace and lands at
the next segment start; deletion consumes the identical ranges; and a point selection selects the
containing segment. Empty text and document edges clamp to the nearest valid UTF-8 boundary. Hosts
must not substitute an OS-control-specific or application-specific word algorithm.

Single-line fields normalize inserted newline characters into non-line-breaking spacing. Multiline fields normalize line endings to `\n` and preserve newline insertion.

Read-only mode preserves focus, caret movement, selection, copy, and text-range queries while rejecting destructive edits, cut, paste, undo, redo, and IME replacements that would mutate text.

## Range-Backed Live Appearance

The mounted range-backed widget accepts its complete `TextInputTheme` and `ScrollbarStyle`
together through one synchronous appearance update. Subsequent paint uses that current theme for
ordinary text, placeholder, selection, caret, marked underline, source-covering atoms, oversize
atoms, and source-zero-width objects, and that current style for scrollbar chrome. An absent
ordinary-text theme color resolves from the current GPUI text style at paint time.

This appearance update preserves the widget entity and focus handle, binding and revision,
authoritative content, caret and directed selection, composition, history availability, logical
scroll state, interaction state, resident pages, coherent geometry, pending requests and jobs, and
ordinary-edit or historical-selection settlement custody. It neither emits an editor event nor
dispatches, cancels, or releases host work. It schedules repaint without rebuilding the editor or
replacing its scrollbar interaction owner.

Theme colors use the owned GPUI immutable streaming-fragment paint overrides. They do not reshape
fragments, start a layout transition, change retained geometry or its accounting, or advance the
layout epoch or host-owned object presentation generation. Current colors also apply to fragments
that complete after the update. Scrollbar style changes affect chrome without changing the text
viewport or logical scroll position. Fonts, wrapping, line metrics, and revisioned host object
presentation remain inputs to their existing geometry or presentation update boundaries; live
appearance does not replace those boundaries or coordinate application-wide theme publication.

Focused verification must inspect actual updated scene paint and preservation of retained editor
state, including pending work, using the ordinary mounted render path. Theme updates require no
additional resident source, shaped fragment, or work queue capacity.

## Widget Layer

The GPUI widget layer owns focus handling, platform text-input integration, keyboard action routing
for baseline text editing, pointer hit testing, selection painting, caret painting, realized inline-
object presentation, placeholder rendering, and visible-range
behavior. In the range-backed variant these mechanics operate only on the current coherent resident
projection and the bounded coordination state defined above. The widget has no accessibility-
specific payload, OS accessibility tree, or assistive-technology action route.

A disabled widget retains its ordinary sizing, clipping, painting, and bounded range-realization
layout and prepaint, but the widget layer installs no focus, tab, key-context, action, pointer,
wheel, platform-input, or scrollbar route and therefore creates no input hitbox. This lets a host
advance a disabled noninteractive realization surface through ordinary GPUI lifecycle work without
creating a hidden input or event surface.

The widget exposes app-neutral callbacks, events, and key-propagation policies for text-input activity. Those hooks report or delegate baseline text-input activity; they do not encode host commands such as settings apply, conversation submission, color-picker opening, numeric stepping, or backend steering.

Rebinding or unmounting cancels every cancellable page, segmentation, clipboard, and geometry job
owned by that widget instance and releases its resident pages, staged clipboard bytes, job slots,
and other local capacity. A pre-admission ordinary edit or historical-root selection receives
cancellation. An already admitted operation transfers only its pre-reserved compact settlement
custody to the host and still reaches its exact terminal there, but its result is obsolete for the
detached widget and cannot mutate a later binding. Late page and job responses are rejected by their
request or job key; late operation results are consumed only by their exact detached-custody key.

An opt-in `test-support` feature may expose a checked one-way qualification operation that lowers
only the configured range-backed surface-item limit on an already initialized widget. It rejects
widening and performs no allocation, notification, publication, or event work. The seam exists only
so crate-root integration tests can measure one successful transition and exercise exact-fit and
one-under admission on the same live subject; it is absent from default builds and is not a runtime
configuration contract for applications.

At a quiescent range-backed cut with no active composition, pre-admission ordinary edit or
historical-root selection, or admitted operation, the host may synchronously request one compact
restoration seed before rebind or unmount. The seed contains only the exact binding identity,
source revision and logical extent, caret and selection
source positions with their constant-size inline-gap witnesses, a logical vertical-scroll anchor
with its bounded intra-anchor object continuation, and optional opaque host-supplied history-
authority identity with exact undo and redo availability bound to that same source key. The widget
does not inspect that authority. The seed contains no text, object presentation, resident page,
shaped layout, composition, undo
payload, clipboard payload, pending request, job, or staged mutation. A host that requires exact
restoration must first fence new edits and settle or explicitly cancel every nonquiescent state
through its ordinary exact boundary; an already admitted commit cannot be covered by a seed captured
at its prior revision.

Quiescent additionally means that the widget owns no active or unpublished viewport, index,
block-target, page, segmentation, platform-range, clipboard, undo, or redo operation. Cancellable
work must settle or cancel and release normally. An admitted ordinary edit or historical-root
selection remains nonquiescent until its exact host settlement arrives; rebind or unmount may
detach it, but seed export cannot cancel it or describe its base revision as current.

A new range-backed widget may accept that seed only for the identical binding, revision, and logical
extent. It validates the compact positions, UTF-8 boundaries, adjacent-object gap witnesses, and
scroll continuation through bounded text and object pages, re-requests the ranges needed for caret,
selection, viewport, and overscan, and publishes no restored surface until those facts are
coherent. A mismatched, malformed, boundary-invalid, or nonadjacent seed is rejected rather than
clamped, translated to another revision, or used to retain old widget state. Restoration transfers
no resident or staged capacity, in-flight operation, or detached-settlement custody from the
unmounted instance.

Owned-value undo and redo histories are retained editor mechanics, not host application state. Each
stack is bounded by both snapshot count and retained UTF-8 byte budget, and hosts may clear that
history explicitly without changing the current buffer. Range-backed undo and redo, when supported,
are host-owned historical-root selections using the fixed-size intent, exact five-outcome
settlement, and atomic live-rebind boundary above; the widget retains no authoritative undo snapshot
stack or inverse content for that variant.

## Application Neutrality

The public API uses generic text-input names and value types. It must not expose host application nouns, Codex protocol types, Beryl workspace types, settings-window row types, image-label concepts, or persistence concepts.

Inline non-text hooks must be modeled as opaque editor primitives with host-owned semantics. Stable
object identities, order keys, bounded presentation facts, source positions, and
activation geometry are generic widget data; they carry no application meaning. The crate must not
special-case Beryl images, settings fields, backend payloads, or any other consumer domain.

# Engineering Rigor

Profile: `production-application/v1`

Modifiers:

- `shared-resource-protection/v1`
