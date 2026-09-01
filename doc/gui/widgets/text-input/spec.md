# Name

Canonical name: text-input

Sometimes known as: text field, editor field

# Purpose

`text-input` is an app-neutral GPUI text editor widget for single-line and multiline input. Its
owned-value variants own the authoritative in-memory value, editing, selection, clipboard commands,
IME composition, bounded undo and redo, placeholder rendering, opaque atom ranges, geometry, and
multiline scrolling.

For the range-backed multiline variant, the host owns authoritative revisioned text and storage,
the opaque binding identity, bounded text and inline-object page sources, staged ordinary-edit sink,
and historical-root selection authority with exact undo and redo availability when supported. The
widget owns bounded viewport, segmentation, clipboard, and geometry requests, resident-page and
inline-object realization, compact selection and caret source positions, composition, scrolling,
pending ordinary-edit coordination, one fixed-size pending historical-selection intent, and atomic
adoption of exact host results.

The range-backed variant also exposes the package design's non-mounted prepublication realization
boundary. That boundary prepares the same bounded coherent publication candidate used by an
ordinary widget without creating a visible or hidden widget, focus target, input route, event
source, or second rendering path.

Hosts own validation, submission, persistence, labels, surrounding field chrome, and domain meaning of the text or atoms.

# References

Contracts: N/A

Widgets:

- scrollbar

# Anatomy

The widget contains a root GPUI element, focus handle, placeholder, shaped resident logical lines,
selection quads, caret quad, optional IME marked-text underline, optional opaque atom text and
background runs, optional multiline vertical scrollbar, geometry cache, scroll offsets, and visible
byte range. Owned-value variants additionally contain the authoritative text state and bounded undo
and redo stacks.

The range-backed multiline variant replaces whole-text and undo-stack state with a host-owned exact
binding identity, source revision and logical extent, bounded text and source-zero-width object
page sources, compact caret and selection source positions, fixed resident text-page and object-page
windows, fixed-capacity pending-request windows, authoritative grapheme, word, logical-line, and
same-anchor object continuations, a bounded background visual-line index, a bounded streaming-layout
continuation, an optional exact block-target job, one cursor-based staged ordinary host-mutation
session, and one fixed-size historical-root selection slot. The ordinary session retains fixed
control state, cumulative identity, and bounded source and proposal pages rather than a whole-
operation fragment collection. The historical slot retains only its direction, operation identity,
exact base key, opaque history-authority identity, and captured positions. Request, job, and
operation records carry exact keys; host results either carry the exact committed successor or
prove a non-mutating terminal outcome.

A prepublication realization session is a detached bounded work owner rather than widget anatomy.
It holds one exact restoration seed, immutable owner-supplied viewport and layout environment,
finite request and continuation windows, bounded resident inputs, one ordinary geometry/index
pipeline, and at most one coherent publication candidate. It contains no root element, focus
handle, scrollbar child, hitbox, event handler, painted surface, or authoritative source value.
Its environment also owns a finite cleanup ledger bound to the exact window-affine text system.
Every externally visible request has one pre-admitted exact ledger record before dispatch.

A source position contains one proven UTF-8 byte offset and one constant-size inline-gap witness.
The witness proves the adjacent object identities and order keys at that anchor, including the
before-all and after-all edges. It can name every caret or selection position before, between, and
after same-anchor objects without assigning source bytes to those objects.

The root element owns the text-input key context, GPUI text input handler, pointer handlers, wheel handler, and optional scrollbar child.

# Look

The root fills available inline size, can shrink in constrained inline space, clips overflow, and uses an I-beam cursor when enabled. Single-line mode uses the current GPUI line height. Multiline mode fills available block size.

Text color may be supplied by `TextInputTheme`; otherwise text inherits the current GPUI text style. Placeholder, selection, caret, marked underline, atom text, and atom background are supplied by `TextInputTheme`. The caret is painted as a narrow quad.

An indivisible grapheme or opaque atom larger than the configured shaping-segment cap is shown as
one compact oversize layout atom using the ordinary atom presentation. It exposes no elided source
content and preserves the exact logical range for selection, copy, replacement, and deletion.

In range-backed multiline mode, a source-zero-width inline object uses its bounded host-supplied
presentation. Its display content, semantic state, metrics or presentation key, and activation
eligibility are revisioned widget inputs rather than authoritative source text. The default
presentation uses the atom roles below; a host-supplied presentation remains constrained to the
object's exact measured inline bounds. The widget retains no accessibility-specific label or
description, constructs no OS accessibility tree, and receives no assistive-technology actions.

The widget does not draw its own border, background, label, validation message, or padding outside the text content area.

# States

Supported states are enabled, disabled, editable, read-only, focused, unfocused, empty placeholder,
plain text, active selection, caret insertion, IME marked text, source-covering atom,
source-zero-width inline object, inline-object active, inline-object activation, single-line,
multiline, range-backed multiline, edit preflight accepted, edit preflight rejected, edit staging,
edit commit pending, edit committed, edit rejected, edit conflict, edit cancelled, edit failed, text
source-page pending, edit-fragment-page pending, edit-input finished, historical-root selection
pending, historical-root rebind pending, object page pending, platform-range pending, coherent resident range, page failed, page
cancelled, obsolete response, segmentation pending, clipboard pending, clipboard failed, clipboard
cancelled, geometry-
index pending, geometry-index refining, geometry-index complete, geometry-index failed, geometry-
index cancelled, block-target pending, block-target failed, block-target cancelled, oversize layout
atom, overflowing, non-overflowing, scrollbar estimated, scrollbar exact, scrollbar visible,
scrollbar animating, capacity-saturated, viewport-exceeds-rendering-capacity, bounded filler, and
pointer selecting. The detached prepublication boundary separately reports initializing,
validating, waiting-for-response, advancing, capacity-blocked, ready, cancelled, stale, and failed
lifecycle outcomes; these are API outcomes and never visible widget states by themselves.

Pending work keeps the last coherent surface. Failure and cancellation settle only the named
operation and expose an app-neutral outcome to the host. Obsolete responses are silently excluded
from current widget state after their capacity is released. The widget does not define host
validation, required-field, persistence-error, or feature-feedback presentation.

Range-backed layout, presentation, true-rebind, selection, reveal, and inline-object activation
replacements use one bounded staged publication. Until the complete cross-owner replacement is
admitted, the current and desired selection, active object, coherent surface, resident and pending
pages, queued requests, scrollbar owner, and event stream remain unchanged. Successful admission
commits internal state before cancellation, request, activation, realization-loss, restoration, or
notification effects become observable. Rejected rapid retargeting retains the prior target and
cannot publish the rejected selection later.

Wheel and scrollbar movement keep their proposed desired scroll state local until this admission.
When target geometry is already complete, coherent-surface construction and replacement capacity
are part of the same candidate rather than a later publication attempt. True rebind validates the
current scrollbar owner without mutation, commits widget state, performs the exact scrollbar owner
replacement synchronously, and only then exposes drag cancellation or other effects.

Host text-page and object-page delivery also prepares residency, geometry deltas, dispatched-key
removal, terminal target construction, and coherent-surface replacement before changing an owner.
Malformed text-page, object-page, or residency-backed geometry delivery terminally settles the
named request or job step and releases its payload and reservation; that key cannot be retried.
A well-formed exact-key delivery rejected only by candidate or terminal-publication capacity leaves
the complete pending and resident fingerprint unchanged and may be retried with that exact key.
Public desired-state transitions align or replace in-flight target geometry. A terminal response
that does not align with its surface candidate is therefore a deterministic invariant failure: it
closes the named response and geometry work with a content-free error while preserving the prior
coherent publication, and it creates neither a capacity fallback nor a pending successor intent.
Exact-geometry scan component capacity exceeded by an immutable response under unchanged configured
layout limits is likewise deterministic, not terminal-publication capacity: it closes that exact
response and job through the same atomic failure boundary. Content-free diagnostics expose the last
response-rejection class, rejection count, and exact-geometry failure stage without keys, offsets,
payloads, or source content. A monotonic content-free count also reports successful settlement of
superseded-job object responses. Only explicit surface-publication capacity retains exact custody
for retry, and every retained custody retry schedules realization liveness.
Pending Select All is resolved from the completed index's bounded exact document endpoints and is
published inside that terminal surface, never by a second fallible transition.

# Interaction

Default key bindings cover character movement, word movement, vertical movement, home and end
movement, buffer start and end movement, selection extension, select all, backspace, delete, word
delete, Enter, Shift-Enter, copy, cut, paste, undo, and redo. Undo and redo operate on the local
bounded history in owned-value variants. In range-backed mode they request fixed-size host-owned
historical-root selection and never enter the ordinary mutation proposal stream.

Disabled mode retains sizing, clipping, painting, and bounded range-realization layout and prepaint,
but installs no focus, tab, key-context, action, pointer, wheel, platform-input, or scrollbar route.
It creates no input hitbox and emits no widget input event. A host may therefore use a disabled
range-backed instance as a noninteractive realization surface without bypassing ordinary GPUI
layout and prepaint.

Word movement, selection, deletion, and double-click selection share the crate-owned Unicode word
segments from `doc/design.md`. Previous-word skips immediately preceding whitespace and lands at the
start of the preceding non-whitespace segment. Next-word crosses the current segment and following
whitespace and lands at the next segment start. Word deletion consumes those same ranges, and point
selection selects the containing segment. Document edges clamp without inventing text, and hosts do
not replace this Windows-first policy with application-specific boundaries.

Enter may insert a newline or propagate to the host. In single-line mode, Up and Down may be handled or propagated to the host. Atom-aware copy and cut may write fallback plain text or propagate to the host. Rich paste may insert the plain text projection or propagate to the host.

Range-backed copy and cut capture the exact binding, revision, and composite selection, then merge
its plain UTF-8 and inline-object fallback representation through bounded text and object pages.
Fallback text follows the exact object order, including multiple objects at one source anchor. A
finite host-configured hard clipboard-byte cap applies to the complete contiguous representation.
If that representation cannot be completed within the cap, or either page source fails, conflicts,
becomes obsolete, or is cancelled, the widget reports that outcome and makes no document mutation;
its coherent caret, selection, and undo projection remains unchanged.

The range-backed host chooses either ordinary omitted provenance or the bounded provenance stream.
When streaming is selected, the widget emits one current page at a time for selected zero-width
objects. A page preserves exact source object identity, anchor, same-anchor order, selection-
qualified item cursor and ordinal, and the checked output range occupied by each fallback. Empty
fallbacks still occupy an item and advance the object cursor. The host acknowledges the exact page
before collection continues. An exact current full page key plus structural equality advances. The
same current full key with differing structure, including malformed current-key input, is a
collision that terminates and releases the stream. A different full key from a foreign operation,
previous, skipped or reordered ordinal, or stale selection is rejected as stale or wrong without
mutating or clearing current custody, so the correct current page remains acknowledgeable or
cancellable. Limit exhaustion, cancellation, rebind, unmount, or source-page failure terminates the
stream and requests no clipboard write.

The widget's exact current and peak surface accounting includes one coordinator ownership charge:
active owner storage, exact contiguous-output backing extent, queued-object capacity and deep payload
allocation capacities, and the
provenance builder or current page allocation. A current provenance allocation shared by the
coordinator and request is counted once, while final write transfer replaces coordinator-owned
output with request-owned output without overlap. Acknowledgement releases both current-page
handles before the next builder is allocated lazily for a later selected object. Exact-fit
admission succeeds, one-under admission fails safely, and cancellation or settlement removes the
whole clipboard charge.

Clipboard begin is also prepare/admit/commit. The inactive coordinator prepares an allocation-free,
nonmutating token bound to its non-reused instance, inactive generation, exact key, operation kind,
selection, and predecessor, with the exact active-owner and optional provenance-owner successor
charge. The widget admits current surface ownership plus that charge before commit. One-under byte
or item capacity leaves inactive/default ownership and allocates nothing; exact fit revalidates and
commits through the ordinary request or final-write path. Empty and nonempty selections use this
same boundary under both omitted and streamed provenance.

Clipboard text and object responses remain in response custody while the coordinator prepares one
opaque fixed-size merge step at a time. Preparation is allocation-free and nonmutating and is the
only source of merge decisions and exact destination peak charge. The widget replaces the current
coordinator charge with that prepared peak in the combined surface admission before commit. Exact
fit commits; byte- or item-one-under, stale, or overflowing admission leaves the response and its
dispatch key intact for exact retry or cancellation. Commit applies the prepared transition without
rerunning the merge and allocates no more than was admitted. Text and object response ownership is
charged from supplied text, atom, and object vector capacities and every deep fallback and
presentation allocation. Only the final prepared step consumes the response and clears its dispatch
custody. A malformed or oversized terminal preparation is committed by that same response-specific
entry point, which consumes the exact response, clears its dispatch, and returns terminal progress
atomically.
Every resident, prepared, deferred, or published object-page owner charges one page item plus every
allocated object-vector slot, including unused capacity; initialized object length remains only the
semantic object-count limit. Admission and current/high-water diagnostics use the same exact slot
charge through all geometry and publication transfers.
An inline-object display has one immutable shared backing across its source fact, GPUI fragment,
geometry target record, and coherent surface. Source-page clones receive independent backing;
geometry aliases never copy the payload. Combined response, residency, staging, publication, peak,
and release accounting recognizes the alias and charges its allocation exactly once while counting
each fixed containing record normally.

Every opaque step is bound to the coordinator instance, active clipboard key and operation,
preparation generation, exact immutable response instance when applicable, and the prepared
structure. An immutable response clone retains its instance identity, while independently created
empty or byte-equal responses receive distinct checked non-reused identities.
Commit additionally requires exact retained-charge and prepared-structure equality; shared lineage
does not admit a clone whose deep allocation capacities differ.
Same-key different-body responses, cross-coordinator use, duplicates, and tokens replayed after
cancel, rebind, finish, or reuse are rejected before mutation. The next generation is reserved
before any allocation or mutation, so exhaustion preserves all response, coordinator, and dispatch
custody.

The first nonempty append fallibly allocates one exact fixed contiguous backing for the configured
complete-output byte ceiling. Subsequent appends never grow it or copy accumulated output, and the
final request takes that allocation without copying; an all-empty output allocates none. Provenance
uses exact fixed item storage whose allocation is admitted before construction and moved into the
shared current page without a second item allocation. This lifecycle includes source-covering atom fallback continuation
across text pages, current and lookahead zero-width objects and their deep payloads, lazy provenance
builder allocation, page emission, and final write transfer. Prepared state is qualified by the
exact response and coordinator generation and becomes stale after any other transition. If a later
local prepared step is capacity-rejected after the response was transferred or released, the widget
keeps coordinator-derived runnable work scheduled and resumes it exactly once after capacity returns,
without response redispatch. The widget does not clone a retry response, predict merge output,
reserve a conservative whole-page destination, copy accumulated output, or discover capacity failure
after mutation.
If an admitted exact output or provenance allocation still cannot be established, the failure is a
typed terminal local-resource outcome rather than stale input or retryable capacity. It atomically
releases retained response, dispatch, coordinator, and prepared-work custody, schedules no retry,
and produces neither a clipboard write nor cut deletion.

After the last provenance page is acknowledged, the one contiguous clipboard write carries compact
exact page, item, fallback-byte and output-byte totals plus the final chained identity. It carries
no provenance collection and creates no second text value. Ordinary omitted provenance has no
stream or closure. Source-covering atom fallbacks remain present in plain text but are not emitted
as provenance items.

Cut requests the platform clipboard write only after the complete capped representation is ready.
It opens the staged deletion transaction against the captured binding, base revision, and selection
only after that clipboard write succeeds. Clipboard failure deletes nothing. If the later deletion
is rejected, conflicts, is cancelled, or fails, the clipboard may retain the copied representation
but that deletion transaction applies no change.

An editable owned-value host may provide one app-neutral pre-mutation edit filter. Before typing,
paste, drop, replacement, deletion, or IME commit mutates owned editor state, the widget offers the
proposed bounded replacement range and inserted UTF-8 text to that filter. Acceptance applies the
edit normally. Rejection is atomic: no prefix is inserted, and text, caret, selection, marked-text
state, scroll, and undo/redo history remain exactly as they were before the proposal. The widget
reports rejection to the host and does not retry or truncate the edit.

Pointer click focuses the input and moves the caret. Shift-click extends selection. Double-click
selects a word. Triple-click selects a logical line in multiline mode or all text in single-line
mode. Dragging extends selection. Pointer hit testing distinguishes every realized inline object
and every adjacent same-anchor gap.

Ordinary activation of a realized inline object emits one app-neutral event containing the exact
binding, source revision, stable object identity, same-anchor order key, presentation generation,
layout epoch, realized bounds, and input origin. Pointer activation includes the triggering point.
When an object is the exact active object, Enter or Space emits the same event with keyboard origin
and the current realized bounds. Estimated or stale geometry never activates an object. When an
active object is removed, replaced, unrealized, or superseded, the widget reports realization loss
and retains no offscreen geometry or hidden anchor for it.

Multiline wheel input scrolls vertically when possible and propagates when the field cannot scroll
further. Multiline scrollbar set and page requests update the vertical scroll offset. In
range-backed mode, local scrolling may extend exact layout from the current coherent checkpoint.
Absolute scrollbar set or page targeting records only a desired block offset while the geometry
index is estimated; it changes no source anchor until a complete exact sparse index starts and
finishes the bounded block-target continuation.

In range-backed multiline mode, movement, selection, reveal, wrapping, and edits may demand bounded
pages by exact binding identity and source revision. Each request key also names its purpose,
unique request identity, positive byte ceiling, and either a direction from one proven UTF-8 anchor
or a candidate offset for bounded boundary validation. The authoritative source selects the actual
UTF-8-safe returned page range inside that envelope. Until required pages arrive, the widget
preserves its last coherent surface and does not guess text or publish a partial edit. A response
with a mismatched key, binding, revision, purpose, demand kind, anchor or candidate, returned-range
constraint, edge fact, or cap is obsolete or malformed and is discarded after releasing its
capacity.

Source-zero-width inline objects use separate bounded object-page requests under the same binding,
revision, and host-owned presentation generation. Each request names one byte interval or exact
byte anchor, an optional same-anchor order cursor, direction, unique identity, and positive object-
count and retained-byte ceilings. A response is strictly ordered by anchor, host order key, and
stable object identity and proves its preceding, following, and completion facts. Both interval
edges are eligible zero-width anchors, including exact source end; a host with terminal-exclusive
storage must preserve that terminal edge through an exact bounded adapter. An object-only
continuation may make progress at one anchor without consuming UTF-8 bytes. The widget rejects
duplicate identities or order keys, non-scalar anchors, inconsistent continuations, presentation-
generation mismatches, and responses outside the exact request envelope.

At an anchor with inline objects, movement traverses the composite order through the before-all
gap, each object, every adjacent gap, and the after-all gap. Each object is one indivisible logical
step. A selection may contain one same-anchor object without containing its neighbors or source
bytes. Backspace, Delete, replacement, and cut remove exactly the adjacent or selected object
through a staged mutation; display and fallback text are never deleted from authoritative UTF-8.
The active-object state follows exact composite navigation and is cleared when selection no longer
identifies that object.

Document origin, document end, accepted source positions, prior returned page edges and object-gap
witnesses, and crate-produced geometry checkpoints are proven anchors. Restoration and other
externally supplied positions use bounded text and object validation demands and are rejected if
their byte offset is not an exact UTF-8 boundary or their adjacent-object witness is not exact; the
widget never rounds, clamps, or guesses them. A text-page ceiling of at least four payload bytes
guarantees scalar progress, while object-page ceilings guarantee same-anchor progress independently.
Grapheme, word, line, source-covering atom, and source-zero-width object semantics continue through
their own exact bounded continuations.

Grapheme, word, and logical-line operations consume the crate-owned typed boundary continuation at
each resident-page edge. An unresolved continuation requests the exact next bounded adjacent range,
consumes and releases each page into fixed segmentation state, and remains pending until a real
segment or document boundary is proven. An arbitrarily long grapheme may require arbitrarily many
bounded continuation steps, but it never grows resident page bytes or retained segment text.
Navigation, deletion, double-click selection, Home, End, and line selection never cap segment
length, treat an arbitrary page edge as a boundary, or flatten the document to find one.

Each range-backed ordinary edit proposal opens one cursor-paged staged transaction session keyed by
binding identity, base revision, and operation identity. Begin captures the exact predecessor caret
and directed selection and names one exact composite replacement range. Bounded source and proposal
pages carry exact cursors, ordinals, canonical bytes, prior and cumulative identities, positive
item and byte ceilings, and an explicit authenticated `finish-input`. The widget and host retain
only bounded current pages and fixed cumulative state; neither accumulates all fragments or treats a
hardcoded 256- or 257-fragment total as end-of-input.

Inserted UTF-8, source-covering atom changes, and source-zero-width object insertions, removals,
replacements, or moves stream through those pages. Removed objects use predecessor positions;
inserted and moved objects use authoritative successor-relative anchors and same-anchor order keys,
including anchors inside newly inserted text. Typing, paste, deletion, cut, and host-initiated
inline-object commands use this same boundary. Small keystrokes take a single-page fast path through
the same cumulative identity, finish, commit, and settlement rules. A host-initiated
command supplies its exact base, operation identity, predecessor positions, page source, and
intended successor positions; it cannot mutate the resident projection directly. While one
transaction is staging or commit-pending, the widget retains its coherent prior projection and
issues or accepts no overlapping mutation.

Text inserted before, between, or after objects sharing one source anchor preserves composite
order without giving those objects source bytes. Unchanged objects before the insertion remain
before it; unchanged objects after it move to the translated successor anchor. The host publishes
the authoritative successor anchors and order keys and preserves unchanged relative order unless
the transaction explicitly moves objects.

Range-backed undo and redo share the single live operation slot with ordinary edits. Exact command
availability is one host-supplied fact bound to the current binding identity, source revision,
logical extent, and opaque history-authority identity; stale or mismatched availability never
enables a command. A request captures only its direction, unique operation identity, that exact base
key and authority identity, and the current caret and directed selection. It contains no
replacement range, inverse or successor text, proposal page source, object collection, marker
registry, root graph, or historical content. The host authenticates the request and selects one
existing immutable historical root.

The host settles historical selection exactly once as `Committed`, `Rejected`, `Conflict`,
`Cancelled`, or `Error`. Cancellation before command admission produces `Cancelled` without
changing the current root or availability. After admission, cancellation cannot override the exact
result, and indeterminate custody remains under host reconciliation rather than becoming a sixth
outcome. The four noncommitted outcomes preserve the live binding, caret, directed selection, and
history availability. `Committed` contains only the successor binding identity, source revision and
logical extent, exact restored caret and directed selection, and successor opaque history-authority
identity with exact undo and redo availability. Every terminal settlement is fixed-size control
state and contains no historical content, inverse or replacement payload, page data, object
collection, or root graph.

The widget adopts `Committed` through one atomic live rebind that retires the old binding's pages,
requests, interaction state, and availability together and installs all successor compact facts
together. It realizes the selected root only by issuing the ordinary bounded text-page and object-
page demands under the successor binding and revision; settlement itself carries no content page or
operation-sized payload. Until those demands validate the returned positions and produce a coherent
successor surface, prior pixels may remain only as inert pending presentation and cannot accept
editing or hit testing as successor content. A position or gap mismatch leaves the committed
successor binding fail-closed and never revives the old root or reclassifies the host settlement.

`Escape` or lifecycle cancellation may cancel before commit admission. The host completes the
ordinary edit transaction exactly once as `Committed` with the successor revision and coherent extent,
`Rejected`, `Conflict`, `Cancelled`, or `Error`. `Rejected`, `Cancelled`, and `Error` prove that the
transaction made no change from its base; `Conflict` proves the base is no longer current and this
transaction applied none of its replacement. After commit admission, cancellation no longer
overrides the exact terminal result. The widget adopts `Committed` only with corresponding coherent
text and object source data plus validated successor caret and selection positions, never publishes
a fragment, retries an uncertain mutation, or treats a missing result as success. Exact replay
requires canonical page and cumulative-chain equality; conflicting reuse is a collision. Consumed
page payloads are released after acceptance, and cancellation, collision, rebind, unmount, or late
response releases all remaining session custody without creating a second terminal settlement.

Overlapping text-page and object-page demand coalesces into fixed request windows. Revision changes
cancel stale requests, and new demand waits or replaces obsolete demand when no request slot is
available; the widget never queues one request per movement, edit, logical line, source page,
object, or same-anchor gap. A geometry object response shared by a later logical geometry request,
or arriving for a superseded job while a newer job is active, settles and releases its external
request into bounded residency first. The current request consumes a matching resident payload
without a second object-page copy; the old response never fails the newer job, and capacity
rejection preserves the original response and dispatch for retry. A completed index with a
nonterminal prepared target charges presentation aliases from that target's active scanner rather
than treating the absent terminal publication as capacity exhaustion. Rebind and unmount cancel
every cancellable page, segmentation, clipboard, and geometry request, release resident
presentation and staged local capacity, and mark late results obsolete. A pre-admission ordinary
edit or historical-root selection is cancelled.
An already admitted operation still settles at the host boundary but cannot apply to the detached
or replacement widget binding.

The shared settlement coordinator is the sole allocator and admission validator for host-visible
operation identities across live and detached widget generations. It atomically validates each
allocated operation against its host-dispatch frontier, while finite slots separately retain
admitted settlement custody. The host adds no second last-seen sequence beside it.

The host exposes a configured finite settlement-custody capacity shared by live and detached widget
generations. Each ordinary commit and historical-root selection reserves one compact slot before
admission. Rebind or unmount transfers only its operation identity, exact base key, and terminal
reconciliation state into that slot; it retains no proposal, inverse, text or object page, marker
registry, root graph, or whole value. A later generation may admit work only while another slot is
available. Each late result matches and releases only its exact slot, never revives an old
generation, and cannot mutate the current binding. Repeated rebinds therefore retain no more than
the configured number of fixed-size settlement records even when several generations await late
terminal results. Custody exhaustion rejects a new operation before admission without changing
semantic undo/redo availability or the current binding.

When range-backed state is quiescent, the widget can synchronously export one compact restoration
seed containing its exact binding, source revision and logical extent, caret and selection source
positions with constant-size inline-gap witnesses, logical vertical-scroll anchor with its bounded
object continuation, and optional opaque host-supplied history-authority identity with exact undo
and redo availability bound to the same source key. Export is unavailable during composition, a
pre-admission operation, or an admitted operation. The seed contains no source text, object
presentation, page, layout, composition, undo payload, request, job, staged capacity, in-flight
operation, or detached-settlement custody and does not keep the widget mounted.

Quiescent export additionally requires no active or unpublished viewport, geometry-index,
block-target, page, segmentation, platform-range, clipboard, undo, or redo operation. An admitted
ordinary edit or historical-root selection remains nonquiescent until its exact host result settles
even if the widget can otherwise detach it during rebind or unmount.

Construction may consume a seed only with the identical host binding, revision, and extent. The
host must also confirm that any history-authority identity and availability still belong to that
same source key. The widget validates its source positions, UTF-8 boundaries, and adjacent-object
gap witnesses with bounded text and object page requests and reconstructs the coherent caret,
selection, viewport, and scroll position from newly admitted resident pages. It rejects a stale or
invalid seed without clamping or translating it and never adopts resident state, in-flight work, or
detached-settlement custody from the detached instance.

Instead of mounting first, an owner may submit that exact seed to the non-mounted prepublication
boundary defined by `doc/design.md`. The owner supplies the immutable viewport and layout
environment, exact current binding descriptor, history validation, ordinary bounded sources, and
all per-step work and retained-capacity limits. The owner repeatedly services the session, dispatches
only its bounded text-page and object-page effects, delivers exact-key responses, and schedules
later service after asynchronous input or capacity becomes available. Segmentation and geometry
advance through the same bounded continuations and request windows as mounted realization; neither
render callbacks nor a hidden disabled widget drive progress.

Ready transfers one one-shot coherent candidate into a fresh ordinary range-backed widget. The
widget atomically adopts it through the ordinary staged-publication boundary only while binding,
revision, extent, history facts, presentation generation, layout inputs, viewport environment, and
capacities and the window-affine text system still match. Focus, input routing, widget events, and
visibility remain absent until that adoption succeeds. Cancellation, staleness, malformed input, capacity termination, failed
validation, candidate rejection, and late responses release their exact detached custody and
cannot change a mounted widget. The session emits no editing, activation, scroll, focus, or
restoration event while detached.

Session or candidate destruction marks its pre-registered cleanup records ready without allocation,
host callback, reentrancy, or panic. The owner drains and acknowledges the resulting exact cancel
and release effects through bounded ordinary service before ledger slots are reusable. A full
ledger blocks new request admission; cleanup is never represented only by a discardable return
value or caller convention.

# Layout

Single-line mode horizontally scrolls to keep the caret visible with reveal padding and vertically centers the shaped line in the current line height.

Multiline mode wraps text to the available width, splits logical lines on `\n`, computes content height from visual line count, clamps vertical scroll to content bounds, and reveals the active endpoint with half-line padding.

The geometry API exposes field bounds, content height, visual line count, visible composite range,
scroll limits, caret bounds, selection bounds, realized inline-object bounds and hit regions, and
vertical reveal data with the binding identity, source revision, presentation generation, layout
epoch, and exact-or-estimated quality of each total.

Range-backed multiline layout shapes only resident text and object pages needed for the visible
range, caret, selection, and bounded overscan under configured retained-memory and per-frame work
budgets. Those budgets are independent of raw viewport pixel dimensions. Credit-gated realization
prioritizes caret, IME, and directed selection, then the active interaction or scroll anchor, then
nearby content. Its resident page, object, presentation, and shaping work limits remain fixed as
logical text and object counts grow. It never concatenates the source, selection, object
collection, undo payload, or requested range into a whole-value buffer; nonresident layout
advances through bounded source metadata and page requests.

Prepublication realization receives those viewport, overscan, allocation, shaping, presentation,
layout-epoch, and budget facts explicitly instead of reading them from element layout. It runs this
same streaming layout and geometry pipeline without prepaint or paint and produces the same bounded
coherent-surface candidate shape consumed by ordinary publication. It creates no offscreen pixels,
alternate geometry cache, or whole-value preparation path. After candidate adoption, the mounted
widget's layout, prepaint, paint, scrollbar, focus, hit-testing, and incremental realization rules
are unchanged.

When capacity cannot cover every nominally visible region, the widget coalesces the unrealized
regions into bounded filler coverage and exposes `capacity-saturated` or
`viewport-exceeds-rendering-capacity` without retaining one demand per missing line, object, or
pixel interval. Logical scroll extent remains derived from the paged sparse index. Pointer,
keyboard, caret, IME, or scroll interaction on filler re-anchors the target and admits only the
next bounded fetch and realization work. The containing shell or renderer, not `text-input`, owns
rejection or clamping of an unrepresentable drawable surface or framebuffer.

Prepaint may spend one configured bounded work quantum advancing already admitted geometry only
while every required text, object, and continuation input is resident. It stops before dispatching
a request, calling the host, or performing other external work. Reaching terminal object work in
prepaint does not publish it: the ordinary widget service path performs terminal object publication
and its observable effects through the staged-publication boundary.

Source-zero-width objects enter inline layout at their exact UTF-8 anchor and in authoritative
same-anchor order. They contribute measured inline geometry without advancing the UTF-8 cursor.
Every realized object has exact bounds and one exact hit region; every adjacent gap has exact caret
geometry. A same-anchor collection larger than one object page streams through a bounded object
continuation and may increase total work without increasing resident object or presentation caps.

When an authenticated inline-object fact produces a retained target fragment, exact geometry also
retains one matching compact presentation record under the same binding, revision, presentation
generation, epoch, and job. Target publication transfers fragments and records atomically, and the
coherent surface resolves realized painting, metadata, hit testing, and activation from that bounded
target-owned projection. Generic object-page eviction cannot invalidate it. Resident object pages
remain the only source for composite-gap proof, edit, clipboard, and continuation authority; the
target projection is bounded by retained fragments and never retains an entire same-anchor run.

A pointer or keyboard interaction retains its exact composite source gap, including its
same-anchor order witness, while a successor target is pending. Target preparation and index
continuation use that exact interaction anchor even when the prior coherent surface can map it. The
bounded exact scanner proves the gap for the successor surface through ordinary paged input; the
widget does not pin the former object page or retain the complete same-anchor collection.

Logical lines are partitioned into canonical bounded shaping segments independently of page edges.
An ordinary segment ends at the last complete grapheme or opaque-atom boundary within the
configured segment-byte cap. Each segment is an independent shaping context; bounded visual-line
placement continues across segments. Lines shorter than the cap retain ordinary whole-line shaping.
An indivisible segment beyond the cap uses the oversize layout atom from `# Look`.

The current painted viewport, pointer hit testing, caret, selection, and reveal geometry are exact
for one binding, revision, presentation generation, and layout epoch. When exact current geometry
is unavailable, the widget retains the prior coherent surface and enters pending state instead of
painting or hit-testing an estimate.

A bounded background job pages visual-line and geometry index entries for one key comprising the
binding identity, source revision, presentation generation, layout epoch, and unique job identity.
Wrapping width, shaping or font inputs, line metrics, inline-object metrics, and any other geometry-
affecting change start a new layout epoch. The job retains a fixed-capacity monotonic sparse set of
crate-produced compact checkpoints, including the origin and terminal aggregate, rather than one
entry per line or object. A checkpoint contains source, object cursor, block, visual-line, logical-
line, segment, and inline-placement continuation facts but no source text, object presentation, or
shaped glyph payload.

While that index is incomplete, total visual-line count, content height, and scrollbar extent may
be marked estimated and refined; those estimates do not drive caret, selection, reveal, hit-test,
or logical source positioning. A complete index makes those totals exact. An absolute block target
resumes from the greatest retained checkpoint at or before it and streams forward with bounded
memory until the target viewport and overscan are exact. Sparse checkpoint gaps may increase work
but not residency. Rebind, unmount, revision change, or epoch change cancels the old index and
target jobs, releases their bounded pages and continuation capacity, and rejects every late result
as obsolete.

# Variants

Default variant: editable single-line text input.

Single-line and ordinary multiline are owned-value variants. Range-backed multiline is the
host-authoritative variant defined above.

Supported variants are single-line, multiline, range-backed multiline, editable, read-only,
enabled, disabled, Enter-inserts-newline, Enter-propagates, single-line-Up/Down-handled,
single-line-Up/Down-propagates, atom-clipboard-plain-text, atom-clipboard-propagates,
rich-paste-plain-text, rich-paste-propagates, and custom themed.

# UI Roles

```css
.text-input {
  --foreground: inherit;
}

.text-input[data-variant="single-line"] {
  --reveal-padding: 12px;
}

.text-input__placeholder {
  --foreground: hsla(0, 0%, 55%, 0.72);
}

.text-input__selection {
  --background: hsla(212, 68%, 50%, 0.34);
}

.text-input__caret {
  --background: hsla(212, 82%, 62%, 1);
  --width: 2px;
}

.text-input__marked-text {
  --underline: hsla(212, 82%, 62%, 1);
}

.text-input__atom {
  --foreground: hsla(209, 78%, 72%, 1);
  --background: hsla(209, 55%, 28%, 0.85);
}
```
