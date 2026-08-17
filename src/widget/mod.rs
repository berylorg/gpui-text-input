use std::{cell::Cell, ops::Range, rc::Rc};

use gpui::{
    App, Bounds, Context, EntityInputHandler, EventEmitter, FocusHandle, Focusable, Pixels, Point,
    SharedString, UTF16Selection, Window, point, px,
};
use gpui_scrollbar::{
    ScrollDirection, ScrollbarInteraction, ScrollbarMountGeneration, ScrollbarOwnerId,
    ScrollbarOwnerKey, ScrollbarScrollState, ScrollbarState, ScrollbarVisibilityUpdateCallback,
};

use crate::{
    TextInputAtom, TextInputAtomError, TextInputChange, TextInputMode, TextInputOptions,
    TextInputRetainedCounts, TextInputSelectionAtom, TextInputSelectionExport, TextInputState,
};

mod construction;
mod events;
mod geometry_api;
mod ime;
mod keyboard;
pub(crate) mod layout;
mod render;
mod theme;
mod utf16;

pub use events::{TextInputCommand, TextInputEvent, TextInputSelection};
pub use geometry_api::{TextInputGeometry, TextInputScrollLimits, TextInputVerticalReveal};
pub use layout::wrapped_visual_line_count_for_width;
pub use theme::TextInputTheme;

use layout::InputLineLayout;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TextInputEnterKey {
    InsertNewline,
    Propagate,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TextInputSingleLineVerticalKey {
    Handle,
    Propagate,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TextInputAtomClipboardPolicy {
    PlainText,
    Propagate,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TextInputRichPastePolicy {
    PlainText,
    Propagate,
}

pub struct TextInput {
    focus_handle: FocusHandle,
    state: TextInputState,
    placeholder: SharedString,
    theme: TextInputTheme,
    enabled: bool,
    enter_key: TextInputEnterKey,
    single_line_vertical_key: TextInputSingleLineVerticalKey,
    atom_clipboard_policy: TextInputAtomClipboardPolicy,
    rich_paste_policy: TextInputRichPastePolicy,
    last_layout: Vec<InputLineLayout>,
    last_bounds: Option<Bounds<Pixels>>,
    last_geometry: Option<TextInputGeometry>,
    scroll_x: Pixels,
    scroll_y: Pixels,
    vertical_scrollbar: Option<VerticalScrollbar>,
    content_height: Pixels,
    visible_range: Range<usize>,
    reveal_cursor: bool,
    is_selecting: bool,
}

struct VerticalScrollbar {
    owner: ScrollbarOwnerKey,
    state: ScrollbarState,
    model: Rc<Cell<Option<ScrollbarScrollState>>>,
    interaction: ScrollbarInteraction,
    on_visibility_update: ScrollbarVisibilityUpdateCallback,
}

#[derive(Clone, Copy)]
enum PendingScrollbarRequest {
    Set(Pixels),
    Page(ScrollDirection, Pixels),
}

impl TextInput {
    pub fn text(&self) -> &str {
        self.state.text()
    }

    pub fn cursor_offset(&self) -> usize {
        self.state.cursor_offset()
    }

    pub fn selection(&self) -> Range<usize> {
        self.state.selection()
    }

    pub fn atoms(&self) -> &[TextInputAtom] {
        self.state.atoms()
    }

    pub fn has_marked_text(&self) -> bool {
        self.state.marked_range().is_some()
    }

    pub fn selection_export(&self) -> Option<TextInputSelectionExport> {
        self.state.selection_export()
    }

    pub fn state(&self) -> &TextInputState {
        &self.state
    }

    pub fn retained_counts(&self) -> TextInputRetainedCounts {
        let mut counts = self.state.retained_counts();
        counts.widget_layout_line_count = Some(self.last_layout.len());
        counts.widget_visual_line_count = Some(
            self.last_layout
                .iter()
                .map(|line| line.line.wrap_boundaries().len() + 1)
                .sum(),
        );
        counts.widget_visible_text_bytes =
            Some(visible_text_bytes(self.state.text(), &self.visible_range));
        counts
    }

    pub fn clear_edit_history(&mut self) {
        self.state.clear_edit_history();
    }

    pub fn visible_range(&self) -> Range<usize> {
        self.visible_range.clone()
    }

    pub fn scroll_offset(&self) -> Point<Pixels> {
        point(self.scroll_x, self.scroll_y)
    }

    #[doc(hidden)]
    pub fn has_vertical_scrollbar_visibility_state_for_test(&self) -> bool {
        self.vertical_scrollbar.is_some()
    }

    #[doc(hidden)]
    pub fn vertical_scrollbar_active_for_test(&self) -> bool {
        self.vertical_scrollbar
            .as_ref()
            .and_then(|scrollbar| {
                scrollbar
                    .state
                    .opacity_at(scrollbar.owner, std::time::Instant::now())
            })
            .is_some_and(|opacity| opacity > 0.0)
    }

    #[doc(hidden)]
    pub fn vertical_scrollbar_scroll_y_for_test(&self) -> Option<Pixels> {
        self.vertical_scrollbar_state()
            .map(|state| state.scroll_offset.y)
    }

    #[doc(hidden)]
    pub fn record_vertical_scrollbar_activity_for_test(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.note_vertical_scrollbar_activity(window, cx);
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub fn set_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if self.enabled == enabled {
            return;
        }

        self.enabled = enabled;
        cx.notify();
    }

    pub fn set_theme(&mut self, theme: TextInputTheme, cx: &mut Context<Self>) {
        if self.theme == theme {
            return;
        }

        self.theme = theme;
        cx.notify();
    }

    pub fn set_enter_key(&mut self, enter_key: TextInputEnterKey) {
        self.enter_key = enter_key;
    }

    pub fn set_single_line_vertical_key(&mut self, key: TextInputSingleLineVerticalKey) {
        self.single_line_vertical_key = key;
    }

    pub fn set_atom_clipboard_policy(&mut self, policy: TextInputAtomClipboardPolicy) {
        self.atom_clipboard_policy = policy;
    }

    pub fn set_rich_paste_policy(&mut self, policy: TextInputRichPastePolicy) {
        self.rich_paste_policy = policy;
    }

    pub fn tab_focus_handle(&self) -> FocusHandle {
        self.focus_handle.clone()
    }

    pub fn focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.enabled {
            window.focus(&self.focus_handle);
            cx.notify();
        }
    }

    pub fn set_text(&mut self, text: impl Into<String>, cx: &mut Context<Self>) -> bool {
        let changed = self.state.reset_text(text);
        self.scroll_x = px(0.0);
        self.scroll_y = px(0.0);
        self.reveal_cursor = true;
        self.finish_selection_change(changed, cx);
        changed
    }

    pub fn set_text_and_select(&mut self, text: impl Into<String>, cx: &mut Context<Self>) -> bool {
        let changed = self.set_text(text, cx) | self.state.select_all();
        self.finish_selection_change(changed, cx);
        changed
    }

    pub fn set_atoms(
        &mut self,
        atoms: impl IntoIterator<Item = TextInputAtom>,
        cx: &mut Context<Self>,
    ) -> Result<bool, TextInputAtomError> {
        let changed = self.state.set_atoms(atoms)?;
        self.finish_selection_change(changed, cx);
        Ok(changed)
    }

    pub fn select_all_text(&mut self, cx: &mut Context<Self>) -> bool {
        let changed = self.state.select_all();
        self.finish_selection_change(changed, cx);
        changed
    }

    pub fn set_selection(
        &mut self,
        range: Range<usize>,
        reversed: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        let changed = self.state.set_selection(range, reversed);
        self.finish_selection_change(changed, cx);
        changed
    }

    pub fn replace_selected_text(&mut self, text: &str, cx: &mut Context<Self>) -> bool {
        let changed = self.state.paste(text);
        self.finish_change(changed, cx)
    }

    pub fn insert_text_at_offset(
        &mut self,
        offset: usize,
        text: &str,
        cx: &mut Context<Self>,
    ) -> bool {
        let changed = self.state.insert_text_at_offset(offset, text);
        self.finish_change(changed, cx)
    }

    pub fn insert_newline(&mut self, cx: &mut Context<Self>) -> bool {
        let changed = self.state.insert_newline();
        self.finish_change(changed, cx)
    }

    pub fn replace_text_range_with_atom(
        &mut self,
        range: Range<usize>,
        atom_text: &str,
        atom_id: impl Into<String>,
        atom_copy_text: impl Into<String>,
        cx: &mut Context<Self>,
    ) -> Result<bool, TextInputAtomError> {
        let changed = self.state.replace_text_in_range_with_atom(
            Some(range),
            atom_text,
            atom_id,
            atom_copy_text,
        )?;
        Ok(self.finish_change(changed, cx))
    }

    pub fn replace_selected_text_with_atoms(
        &mut self,
        display_text: &str,
        atoms: impl IntoIterator<Item = TextInputSelectionAtom>,
        cx: &mut Context<Self>,
    ) -> Result<bool, TextInputAtomError> {
        let changed = self
            .state
            .replace_text_in_range_with_atoms(None, display_text, atoms)?;
        Ok(self.finish_change(changed, cx))
    }

    pub fn cut_selection_export(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Option<TextInputSelectionExport> {
        let (selection, change) = self.state.cut_selection_export()?;
        let _ = self.finish_change(Some(change), cx);
        Some(selection)
    }

    pub fn remove_atom_by_id(&mut self, atom_id: &str, cx: &mut Context<Self>) -> bool {
        let changed = self.state.remove_atom_by_id(atom_id);
        self.finish_change(changed, cx)
    }

    fn finish_change(&mut self, change: Option<TextInputChange>, cx: &mut Context<Self>) -> bool {
        let Some(change) = change else {
            return false;
        };

        self.reveal_cursor = true;
        self.emit_selection_changed(cx);
        cx.emit(TextInputEvent::Changed(change));
        cx.notify();
        true
    }

    fn finish_selection_change(&mut self, changed: bool, cx: &mut Context<Self>) {
        if changed {
            self.reveal_cursor = true;
            self.emit_selection_changed(cx);
            cx.notify();
        }
    }

    fn should_reveal_cursor_for_bounds(&self, bounds: Bounds<Pixels>) -> bool {
        self.reveal_cursor
            || self
                .last_bounds
                .is_none_or(|last_bounds| last_bounds.size != bounds.size)
    }

    fn emit_selection_changed(&self, cx: &mut Context<Self>) {
        cx.emit(TextInputEvent::SelectionChanged(TextInputSelection {
            range: self.state.selection(),
            reversed: self.state.selection_reversed(),
        }));
    }
}

impl EventEmitter<TextInputEvent> for TextInput {}

impl Focusable for TextInput {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

fn visible_text_bytes(text: &str, range: &Range<usize>) -> usize {
    if range.start > range.end {
        return 0;
    }

    text.get(range.clone()).map_or(0, str::len)
}
