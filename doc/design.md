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

The crate owns generic text-input UI mechanics: text storage, cursor and selection state, keyboard editing primitives, pointer selection, IME text range handling, plain-text clipboard behavior, undo and redo state, focus integration, placeholder presentation, single-line layout, multiline layout, scrolling needed for multiline editing, and app-neutral events for text changes or key handling.

Multiline text-input widgets use app-neutral `gpui-scrollbar` primitives for scrollbar chrome, managed visibility and fade behavior, and pointer direct manipulation when measured content overflows vertically. Text-input state remains the owner of text editing, wheel scrolling, keyboard-driven reveal behavior, vertical scroll offset, and scroll-limit clamping; `gpui-scrollbar` receives callback-backed geometry and reports page or drag requests without owning text-input policy.

Scrollbar geometry supplied by multiline text-input widgets is derived from the current authoritative text-input scroll offset and the latest measured scroll limits. Painted geometry may provide bounds and limits, but stale painted offsets must not drive scrollbar thumb position after wheel scrolling or keyboard reveal changes.

The crate supports opaque inline atoms as app-neutral text ranges. Atoms have stable host-owned ids, visible display ranges, and fallback copy text. The crate keeps atom ranges valid across generic text edits, selection, navigation, deletion, undo, redo, and plain clipboard export, but it does not interpret what an atom means or serialize any host domain payload.

Host applications own domain meaning for the text, field validation, settings apply behavior, command submission, transcript quoting, non-text attachments, backend input serialization, storage, and any application-specific clipboard metadata.

## Text Model

The text model stores plain UTF-8 text and exposes caret, selection, marked-text, and edit operations in terms of valid text boundaries.

Character-wise movement and deletion operate on Unicode grapheme boundaries. Word-wise movement and deletion use one crate-owned word-boundary policy so consumers receive consistent behavior.

Single-line fields normalize inserted newline characters into non-line-breaking spacing. Multiline fields normalize line endings to `\n` and preserve newline insertion.

Read-only mode preserves focus, caret movement, selection, copy, and text-range queries while rejecting destructive edits, cut, paste, undo, redo, and IME replacements that would mutate text.

## Widget Layer

The GPUI widget layer owns focus handling, platform text-input integration, keyboard action routing for baseline text editing, pointer hit testing, selection painting, caret painting, placeholder rendering, and visible-range behavior.

The widget exposes app-neutral callbacks, events, and key-propagation policies for text-input activity. Those hooks report or delegate baseline text-input activity; they do not encode host commands such as settings apply, conversation submission, color-picker opening, numeric stepping, or backend steering.

Undo and redo history are retained editor mechanics, not host application state. Each stack is bounded by both snapshot count and retained UTF-8 byte budget, and hosts may clear edit history explicitly after accepting or persisting field contents without changing the current buffer.

## Application Neutrality

The public API uses generic text-input names and value types. It must not expose host application nouns, Codex protocol types, Beryl workspace types, settings-window row types, image-label concepts, or persistence concepts.

Inline non-text hooks must be modeled as opaque editor primitives with host-owned semantics. The crate must not special-case Beryl images, settings fields, or backend payloads.
