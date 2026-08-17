# Name

Canonical name: text-input

Sometimes known as: text field, editor field

# Purpose

`text-input` is an app-neutral GPUI text editor widget for single-line and multiline input. Its
owned-value variants own the authoritative in-memory value, editing, selection, clipboard commands,
IME composition, bounded undo and redo, placeholder rendering, opaque atom ranges, geometry, and
multiline scrolling.

For the range-backed multiline variant, the host owns authoritative revisioned text and storage,
the opaque binding identity, bounded text and inline-object page sources, staged edit sink, and undo
or redo authority when supported. The widget owns bounded viewport, segmentation, clipboard, and
geometry requests, resident-page and inline-object realization, compact selection and caret source
positions, composition, scrolling, pending-edit coordination, and atomic adoption of exact host
results.

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
continuation, an optional exact block-target job, and one bounded staged host mutation. Request and
job records carry exact keys; host results either carry the exact committed successor or prove a
non-mutating terminal outcome.

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
page pending, object page pending, platform-range pending, coherent resident range, page failed, page
cancelled, obsolete response, segmentation pending, clipboard pending, clipboard failed, clipboard
cancelled, geometry-
index pending, geometry-index refining, geometry-index complete, geometry-index failed, geometry-
index cancelled, block-target pending, block-target failed, block-target cancelled, oversize layout
atom, overflowing, non-overflowing, scrollbar estimated, scrollbar exact, scrollbar visible,
scrollbar animating, and pointer selecting.

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
Pending Select All is resolved from the completed index's bounded exact document endpoints and is
published inside that terminal surface, never by a second fallible transition.

# Interaction

Default key bindings cover character movement, word movement, vertical movement, home and end
movement, buffer start and end movement, selection extension, select all, backspace, delete, word
delete, Enter, Shift-Enter, copy, cut, paste, undo, and redo. Undo and redo operate on the local
bounded history in owned-value variants and request the host-owned mutation in range-backed mode.

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
stable object identity and proves its preceding, following, and completion facts. An object-only
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

Each range-backed edit proposal opens one staged transaction keyed by binding identity, base
revision, and operation identity. It names one exact composite replacement range and streams
inserted UTF-8, source-covering atom changes, and source-zero-width object insertions, removals,
replacements, or moves as bounded ordered fragments. Typing, paste, deletion, cut, undo, redo, and
host-initiated inline-object commands use this same boundary. A host-initiated command supplies its
exact base, operation identity, composite range, bounded fragments, and intended successor
positions; it cannot mutate the resident projection directly. While one transaction is staging or
commit-pending, the widget retains its coherent prior projection and issues or accepts no
overlapping mutation.

Text inserted before, between, or after objects sharing one source anchor preserves composite
order without giving those objects source bytes. Unchanged objects before the insertion remain
before it; unchanged objects after it move to the translated successor anchor. The host publishes
the authoritative successor anchors and order keys and preserves unchanged relative order unless
the transaction explicitly moves objects.

`Escape` or lifecycle cancellation may cancel before commit admission. The host completes the
transaction exactly once as `Committed` with the successor revision and coherent extent,
`Rejected`, `Conflict`, `Cancelled`, or `Error`. `Rejected`, `Cancelled`, and `Error` prove that the
transaction made no change from its base; `Conflict` proves the base is no longer current and this
transaction applied none of its replacement. After commit admission, cancellation no longer
overrides the exact terminal result. The widget adopts `Committed` only with corresponding coherent
text and object source data plus validated successor caret and selection positions, never publishes
a fragment, retries an uncertain mutation, or treats a missing result as success.

Overlapping text-page and object-page demand coalesces into fixed request windows. Revision changes
cancel stale requests, and new demand waits or replaces obsolete demand when no request slot is
available; the widget never queues one request per movement, edit, logical line, source page,
object, or same-anchor gap. Rebind and unmount cancel every cancellable page, segmentation,
clipboard, and geometry request, release resident presentation and staged local capacity, and mark
late results obsolete. An already admitted edit commit still settles at the host boundary but
cannot apply to the detached or replacement widget binding.

When range-backed state is quiescent, the widget can synchronously export one compact restoration
seed containing its exact binding, source revision and logical extent, caret and selection source
positions with constant-size inline-gap witnesses, logical vertical-scroll anchor with its bounded
object continuation, and optional opaque host-owned undo/redo frontier identity and availability
fact. Export is unavailable during composition, a pre-commit edit, or an admitted edit. The seed
contains no source text, object presentation, page, layout, composition, undo payload, request, job,
or staged capacity and does not keep the widget mounted.

Quiescent export additionally requires no active or unpublished viewport, geometry-index,
block-target, page, segmentation, platform-range, clipboard, undo, or redo operation. An admitted
edit remains nonquiescent until its exact host result settles even if the widget can otherwise
detach it during rebind or unmount.

Construction may consume a seed only with the identical host binding, revision, and extent. The
widget validates its source positions, UTF-8 boundaries, and adjacent-object gap witnesses with
bounded text and object page requests and reconstructs the coherent caret, selection, viewport,
and scroll position from newly admitted resident pages. It rejects a stale or invalid seed without
clamping or translating it and never adopts resident state from the detached instance.

# Layout

Single-line mode horizontally scrolls to keep the caret visible with reveal padding and vertically centers the shaped line in the current line height.

Multiline mode wraps text to the available width, splits logical lines on `\n`, computes content height from visual line count, clamps vertical scroll to content bounds, and reveals the active endpoint with half-line padding.

The geometry API exposes field bounds, content height, visual line count, visible composite range,
scroll limits, caret bounds, selection bounds, realized inline-object bounds and hit regions, and
vertical reveal data with the binding identity, source revision, presentation generation, layout
epoch, and exact-or-estimated quality of each total.

Range-backed multiline layout shapes only resident text and object pages needed for the visible
range, caret, selection, and bounded overscan. Its resident page, object, presentation, and shaping
work limits remain fixed as logical text and object counts grow. It never concatenates the source,
selection, object collection, undo payload, or requested range into a whole-value buffer;
nonresident layout advances through bounded source metadata and page requests.

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
