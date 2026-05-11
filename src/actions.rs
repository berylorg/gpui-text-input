use gpui::{App, Global, KeyBinding, actions};

/// Key context used by the reusable text-input widget.
pub const TEXT_INPUT_KEY_CONTEXT: &str = "GpuiTextInput";

actions!(
    gpui_text_input,
    [
        Backspace,
        Delete,
        DeleteWordBackward,
        DeleteWordForward,
        MoveLeft,
        MoveRight,
        MoveUp,
        MoveDown,
        MoveWordLeft,
        MoveWordRight,
        SelectLeft,
        SelectRight,
        SelectUp,
        SelectDown,
        SelectWordLeft,
        SelectWordRight,
        MoveHome,
        MoveEnd,
        SelectHome,
        SelectEnd,
        MoveToStart,
        MoveToEnd,
        SelectToStart,
        SelectToEnd,
        SelectAll,
        Enter,
        InsertNewline,
        Copy,
        Cut,
        Paste,
        Undo,
        Redo,
    ]
);

struct TextInputBindingsInstalled;

impl Global for TextInputBindingsInstalled {}

/// Installs the default app-neutral key bindings for text inputs.
pub fn ensure_text_input_bindings(cx: &mut App) {
    if cx.has_global::<TextInputBindingsInstalled>() {
        return;
    }

    cx.bind_keys([
        KeyBinding::new("backspace", Backspace, Some(TEXT_INPUT_KEY_CONTEXT)),
        KeyBinding::new("delete", Delete, Some(TEXT_INPUT_KEY_CONTEXT)),
        KeyBinding::new(
            "ctrl-backspace",
            DeleteWordBackward,
            Some(TEXT_INPUT_KEY_CONTEXT),
        ),
        KeyBinding::new(
            "ctrl-delete",
            DeleteWordForward,
            Some(TEXT_INPUT_KEY_CONTEXT),
        ),
        KeyBinding::new("left", MoveLeft, Some(TEXT_INPUT_KEY_CONTEXT)),
        KeyBinding::new("right", MoveRight, Some(TEXT_INPUT_KEY_CONTEXT)),
        KeyBinding::new("up", MoveUp, Some(TEXT_INPUT_KEY_CONTEXT)),
        KeyBinding::new("down", MoveDown, Some(TEXT_INPUT_KEY_CONTEXT)),
        KeyBinding::new("shift-left", SelectLeft, Some(TEXT_INPUT_KEY_CONTEXT)),
        KeyBinding::new("shift-right", SelectRight, Some(TEXT_INPUT_KEY_CONTEXT)),
        KeyBinding::new("shift-up", SelectUp, Some(TEXT_INPUT_KEY_CONTEXT)),
        KeyBinding::new("shift-down", SelectDown, Some(TEXT_INPUT_KEY_CONTEXT)),
        KeyBinding::new("ctrl-left", MoveWordLeft, Some(TEXT_INPUT_KEY_CONTEXT)),
        KeyBinding::new("ctrl-right", MoveWordRight, Some(TEXT_INPUT_KEY_CONTEXT)),
        KeyBinding::new(
            "ctrl-shift-left",
            SelectWordLeft,
            Some(TEXT_INPUT_KEY_CONTEXT),
        ),
        KeyBinding::new(
            "ctrl-shift-right",
            SelectWordRight,
            Some(TEXT_INPUT_KEY_CONTEXT),
        ),
        KeyBinding::new("home", MoveHome, Some(TEXT_INPUT_KEY_CONTEXT)),
        KeyBinding::new("end", MoveEnd, Some(TEXT_INPUT_KEY_CONTEXT)),
        KeyBinding::new("shift-home", SelectHome, Some(TEXT_INPUT_KEY_CONTEXT)),
        KeyBinding::new("shift-end", SelectEnd, Some(TEXT_INPUT_KEY_CONTEXT)),
        KeyBinding::new("ctrl-home", MoveToStart, Some(TEXT_INPUT_KEY_CONTEXT)),
        KeyBinding::new("ctrl-end", MoveToEnd, Some(TEXT_INPUT_KEY_CONTEXT)),
        KeyBinding::new(
            "ctrl-shift-home",
            SelectToStart,
            Some(TEXT_INPUT_KEY_CONTEXT),
        ),
        KeyBinding::new("ctrl-shift-end", SelectToEnd, Some(TEXT_INPUT_KEY_CONTEXT)),
        KeyBinding::new("ctrl-a", SelectAll, Some(TEXT_INPUT_KEY_CONTEXT)),
        KeyBinding::new("cmd-a", SelectAll, Some(TEXT_INPUT_KEY_CONTEXT)),
        KeyBinding::new("enter", Enter, Some(TEXT_INPUT_KEY_CONTEXT)),
        KeyBinding::new("shift-enter", InsertNewline, Some(TEXT_INPUT_KEY_CONTEXT)),
        KeyBinding::new("ctrl-c", Copy, Some(TEXT_INPUT_KEY_CONTEXT)),
        KeyBinding::new("cmd-c", Copy, Some(TEXT_INPUT_KEY_CONTEXT)),
        KeyBinding::new("ctrl-insert", Copy, Some(TEXT_INPUT_KEY_CONTEXT)),
        KeyBinding::new("ctrl-x", Cut, Some(TEXT_INPUT_KEY_CONTEXT)),
        KeyBinding::new("cmd-x", Cut, Some(TEXT_INPUT_KEY_CONTEXT)),
        KeyBinding::new("shift-delete", Cut, Some(TEXT_INPUT_KEY_CONTEXT)),
        KeyBinding::new("ctrl-v", Paste, Some(TEXT_INPUT_KEY_CONTEXT)),
        KeyBinding::new("cmd-v", Paste, Some(TEXT_INPUT_KEY_CONTEXT)),
        KeyBinding::new("shift-insert", Paste, Some(TEXT_INPUT_KEY_CONTEXT)),
        KeyBinding::new("ctrl-z", Undo, Some(TEXT_INPUT_KEY_CONTEXT)),
        KeyBinding::new("cmd-z", Undo, Some(TEXT_INPUT_KEY_CONTEXT)),
        KeyBinding::new("ctrl-y", Redo, Some(TEXT_INPUT_KEY_CONTEXT)),
        KeyBinding::new("ctrl-shift-z", Redo, Some(TEXT_INPUT_KEY_CONTEXT)),
        KeyBinding::new("cmd-shift-z", Redo, Some(TEXT_INPUT_KEY_CONTEXT)),
    ]);
    cx.set_global(TextInputBindingsInstalled);
}
