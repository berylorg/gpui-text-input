# Goals

Provide reusable GPUI text-input primitives for applications that already build their UI on `gpui`.

The crate exists to supply app-neutral text editing behavior and presentation for single-line and multiline text fields. Host applications remain responsible for their own settings schemas, command semantics, rich draft content, persistence, and backend integration.

## Non-goals

This crate does not define application-specific settings schemas, validation rules, persistence paths, apply or cancel policy, or preference semantics.

This crate does not depend on Beryl, `gpui-settings-window`, Myrrh, Codex, or any other host application crate.

This crate does not own Beryl composer concepts, including image atoms, image marker rendering or context menus, submit-to-Codex behavior, transcript quote insertion, backend input serialization, image-label allocation, workspace image assets, or Beryl-private clipboard metadata.

This crate does not support non-GPUI UI frameworks.

# Decisions

## Standalone Crate

The crate is a standalone Cargo package named `gpui-text-input`.

It depends directly on `gpui` and exposes an app-neutral public boundary. Consumers that need a forked GPUI package can align or patch `gpui` from their own workspace.

## Ownership Boundary

The ordinary owned-value single-line and multiline variants own their authoritative in-memory UTF-8
value, cursor and selection state, IME composition, generic editing and clipboard mechanics, bounded
undo and redo history, focus integration, layout, scrolling, and app-neutral events.

The range-backed multiline variant does not own authoritative text or storage. Its host owns the
authoritative revisioned UTF-8 source, opaque binding identity, logical extent, bounded page source,
staged edit sink, and any supported undo and redo authority. The widget owns only its fixed resident
page window, revision-bound viewport and range requests, compact cursor and selection offsets,
composition state, scroll state, and bounded pending-edit coordination. It never concatenates those
pages into an authoritative whole value or reconstructs host undo history.

Every range-backed request names the exact binding identity and source revision. A response applies
only to that binding and revision; rebinding never lets an old response become current by offset
coincidence. The public boundary gives page requests, staged mutations, clipboard reads, and layout
jobs distinct request or job identities so callers can cancel, reject, and release them exactly.
Partial application, result-free adoption, and cleanup that mutates authoritative text are invalid
host behavior.

Multiline text-input widgets use app-neutral `gpui-scrollbar` primitives for scrollbar chrome,
managed visibility and fade behavior, and pointer direct manipulation when measured content
overflows vertically. Text-input state owns editing-interaction coordination, wheel scrolling,
keyboard-driven reveal behavior, vertical scroll offset, and scroll-limit clamping in both ownership
variants; `gpui-scrollbar` receives callback-backed geometry and reports page or drag requests
without owning text-input policy or host text authority.

Scrollbar geometry supplied by multiline text-input widgets is derived from the current authoritative text-input scroll offset and the latest measured scroll limits. Painted geometry may provide bounds and limits, but stale painted offsets must not drive scrollbar thumb position after wheel scrolling or keyboard reveal changes.

The crate supports opaque inline atoms as app-neutral text ranges. Atoms have stable host-owned ids,
visible display ranges, and fallback copy text. Owned-value variants keep atom ranges valid across
generic edits, selection, navigation, deletion, undo, redo, and plain clipboard export. Range-backed
hosts return authoritative atom ranges with their revisioned pages and mutation results, while the
widget preserves those ranges within its coherent resident projection. The crate does not interpret
what an atom means or serialize any host domain payload.

Host applications own domain meaning for the text, field validation, settings apply behavior,
command submission, transcript quoting, non-text attachments, backend input serialization,
persistence, and any application-specific clipboard metadata. The range-backed host additionally
owns the authoritative source and mutation boundary above.

## Range-Backed Source Boundary

A range-backed binding is the pair of one opaque host binding identity and one exact source
revision. Its source describes the checked logical UTF-8 extent and serves only bounded absolute
ranges. Each page carries the binding and revision, its source-selected exact logical range, valid
UTF-8 bytes, authoritative atom facts for that range, and checked preceding, following, and
end-of-source facts. A page edge is a proven UTF-8 scalar boundary but is not implicitly a grapheme,
word, atom, or logical-line boundary.

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

## Range-Backed Edit Transactions

An edit, undo, or redo uses one host-owned staged transaction keyed by binding identity, base
revision, and unique operation identity. The transaction names one exact replacement range and
accepts inserted UTF-8 and atom changes only as bounded ordered fragments with checked offsets and a
terminal fragment. Staging does not change authoritative text, and the host may reject the proposal
or a fragment before commit without exposing a partial replacement.

Cancellation observed before commit admission terminates the transaction as `Cancelled` and leaves
the base revision unchanged. Once commit is admitted, later cancellation cannot retract it or
replace its exact result. The host settles every transaction exactly once as `Committed` with the
new revision and coherent extent, `Rejected`, `Conflict`, `Cancelled`, or `Error`. `Rejected`,
`Cancelled`, and `Error` prove that this transaction made no change from its base; `Conflict` proves
that the base is no longer current and that this transaction applied none of its replacement. A host
with an internally uncertain commit must reconcile it before returning one of these terminal
outcomes; the widget never retries or infers the outcome.

`Committed` atomically publishes the complete replacement and no other outcome publishes any of
it. Terminal settlement releases every staged fragment and transaction reservation as one cleanup
operation; cancellation, failure, or cleanup cannot delete a prefix, retain a suffix as visible
text, or partially apply compensating work. The widget adopts the successor revision only with a
coherent source projection for that exact committed result.

## Range-Backed Geometry And Clipboard

Range-backed multiline layout maintains a bounded background visual-line and geometry index. Every
index job and result is keyed by binding identity, source revision, layout epoch, and unique job
identity. The layout epoch changes whenever wrapping width, font or shaping inputs, line metrics,
atom geometry, or another geometry-affecting input changes. Jobs page through source ranges without
retaining shaped output for every visited line.

The geometry owner admits byte and semantic-item residency independently. It consumes GPUI's exact
returned fragment-graph item charge and session continuation charge, then combines them with exact
crate-owned owner, immutable input, desired and active job, borrowed page, cursor and atom, segment,
checkpoint, candidate, fragment, and publication records. Admission includes their peak
coexistence with the prior coherent publication; the exact cap is accepted and one under is rejected
atomically without replacing that publication.

Layout uses GPUI's canonical bounded streaming text-layout boundary. The crate derives shaping
segments from its exact grapheme, logical-line, and opaque-atom cursors independently of page edges
and request timing. An ordinary segment ends at the last complete grapheme or atom boundary within
the configured segment-byte cap. GPUI shapes it as an independent shaping context and carries only
bounded visual-line placement state into the next segment. A single indivisible grapheme or atom
that exceeds the cap is scanned to its exact logical end without retaining its complete bytes, then
represented by one compact oversize-layout atom. Its source range remains authoritative for
selection, editing, copy, and replacement.

Each index job begins from one crate-produced compact layout checkpoint and advances through exact
source pages and canonical shaping segments. A checkpoint contains no source text or shaped glyph
payload; it carries the exact source offset, block offset, visual-line count, logical-line and
segment position, inline placement continuation, and shaping-input identity needed to resume.
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

The painted viewport, pointer hit testing, caret geometry, selection geometry, and reveal target are
exact for the current binding, revision, and layout epoch. Until the background index is complete,
only the total visual-line count, total content height, and scrollbar extent may be explicitly
estimated and refined. Estimated totals never authorize caret placement or selection geometry and
become exact only when the complete index for that key is proven. Results from an older binding,
revision, epoch, or job identity are obsolete and discarded.

Viewport publication freezes its logical selection, caret, composition, scroll anchor, exact
checkpoint, resident pages, shaped fragments, atoms, caret and selection geometry, hit regions, and
reveal facts under one key and one actual-value capacity admission. Desired interaction state is not
current presentation state. Pending, failed, cancelled, stale, or over-cap replacement work leaves
the complete prior coherent publication unchanged.

Restoration validates the seed's binding, revision, extent, bounded intra-anchor offset, and every
distinct caret, selection, scroll, viewport, and overscan range through bounded source and layout
continuations. Off-viewport endpoints retain exact logical facts and an exact resolved block
relationship; an absent rectangle is valid only when that relationship proves the endpoint is
outside the published viewport. No restored surface publishes until these facts and the visible
window are coherent.

Range-backed copy and cut bind to the exact current binding, revision, and selected logical range.
They read that representation through bounded pages under one host-configured hard clipboard-byte
cap; the widget does not start a contiguous clipboard write unless it has the
complete exact representation within that cap. Page failure, revision conflict, cancellation,
rebind, unmount, or a representation exceeding the cap makes no document mutation.

Cut writes the complete clipboard representation first. Only a successful clipboard write may
start the staged deletion transaction against the same binding, base revision, and selected range.
If that later deletion conflicts, is rejected, is cancelled, or fails, the copied clipboard value
may remain but that deletion transaction applies no change. The crate provides no cut path that
deletes before or independently of successful copy.

## Text Model

The owned-value text model stores plain UTF-8 text and exposes caret, selection, marked-text, and
edit operations in terms of valid text boundaries. The range-backed model exposes the same compact
editor coordinates against one exact host revision while retaining only bounded resident pages;
nonresident boundary decisions request the required revision-bound range instead of guessing.

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

## Widget Layer

The GPUI widget layer owns focus handling, platform text-input integration, keyboard action routing
for baseline text editing, pointer hit testing, selection painting, caret painting, placeholder
rendering, and visible-range behavior. In the range-backed variant these mechanics operate only on
the current coherent resident projection and the bounded coordination state defined above.

The widget exposes app-neutral callbacks, events, and key-propagation policies for text-input activity. Those hooks report or delegate baseline text-input activity; they do not encode host commands such as settings apply, conversation submission, color-picker opening, numeric stepping, or backend steering.

Rebinding or unmounting cancels every cancellable page, segmentation, clipboard, and geometry job
owned by that widget instance and releases its resident pages, staged clipboard bytes, job slots,
and other local capacity. A pre-commit edit transaction receives cancellation; an already admitted
commit still settles exactly at the host boundary, but its result is obsolete for the detached
widget and cannot mutate a later binding. Late responses are rejected by their request or job key.

At a quiescent range-backed cut with no active composition, pre-commit edit, or admitted edit, the
host may synchronously request one compact restoration seed before rebind or unmount. The seed
contains only the exact binding identity, source revision and logical extent, logical caret and
selection offsets, a logical vertical-scroll anchor with its bounded intra-anchor offset, and an
optional opaque host-owned undo/redo frontier identity and availability fact. The widget does not
inspect that host-owned frontier. The seed contains no text, atom, resident page, shaped layout,
composition, undo payload, clipboard payload, pending request, job, or staged mutation. A host that
requires exact restoration must first fence
new edits and settle or explicitly cancel every nonquiescent state through its ordinary exact
boundary; an already admitted commit cannot be covered by a seed captured at its prior revision.

Quiescent additionally means that the widget owns no active or unpublished viewport, index,
block-target, page, segmentation, platform-range, clipboard, undo, or redo operation. Cancellable
work must settle or cancel and release normally. An admitted edit remains nonquiescent until its
exact host settlement arrives; rebind or unmount may detach it, but seed export cannot cancel it or
describe its base revision as current.

A new range-backed widget may accept that seed only for the identical binding, revision, and logical
extent. It validates the compact offsets and required UTF-8 boundaries through bounded source pages,
re-requests the ranges needed for caret, selection, viewport, and overscan, and publishes no
restored surface until those facts are coherent. A mismatched, malformed, or boundary-invalid seed
is rejected rather than clamped, translated to another revision, or used to retain old widget
state. Restoration transfers no resident or staged capacity from the unmounted instance.

Owned-value undo and redo histories are retained editor mechanics, not host application state. Each
stack is bounded by both snapshot count and retained UTF-8 byte budget, and hosts may clear that
history explicitly without changing the current buffer. Range-backed undo and redo, when supported,
are host-owned mutations using the same exact revision and atomic-result boundary as other
range-backed edits; the widget retains no authoritative undo snapshot stack for that variant.

## Application Neutrality

The public API uses generic text-input names and value types. It must not expose host application nouns, Codex protocol types, Beryl workspace types, settings-window row types, image-label concepts, or persistence concepts.

Inline non-text hooks must be modeled as opaque editor primitives with host-owned semantics. The crate must not special-case Beryl images, settings fields, or backend payloads.
