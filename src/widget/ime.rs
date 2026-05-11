use super::{layout, utf16, *};

impl EntityInputHandler for TextInput {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let range = utf16::range_from_utf16(self.state.text(), &range_utf16);
        actual_range.replace(utf16::range_to_utf16(self.state.text(), &range));
        Some(self.state.text()[range].to_string())
    }

    fn selected_text_range(
        &mut self,
        ignore_disabled_input: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        if !self.enabled && !ignore_disabled_input {
            return None;
        }

        Some(UTF16Selection {
            range: utf16::range_to_utf16(self.state.text(), &self.state.selection()),
            reversed: self.state.selection_reversed(),
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.state
            .marked_range()
            .as_ref()
            .map(|range| utf16::range_to_utf16(self.state.text(), range))
    }

    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let changed = self.state.unmark_text();
        self.finish_selection_change(changed, cx);
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.should_ignore_platform_newline(new_text) {
            return;
        }

        let range = range_utf16
            .as_ref()
            .map(|range| utf16::range_from_utf16(self.state.text(), range));
        let changed = self.state.replace_text_in_range(range, new_text);
        self.finish_change(changed, cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|range| utf16::range_from_utf16(self.state.text(), range));
        let new_selected_range = new_selected_range_utf16
            .as_ref()
            .map(|range| utf16::range_from_utf16(new_text, range));
        let changed =
            self.state
                .replace_and_mark_text_in_range(range, new_text, new_selected_range);
        self.finish_change(changed, cx);
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        _: Bounds<Pixels>,
        window: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let range = utf16::range_from_utf16(self.state.text(), &range_utf16);
        layout::bounds_for_range(&self.last_layout, range, window.line_height())
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        let bounds = self.last_bounds?;
        let offset = layout::index_for_position(&self.last_layout, bounds, point);
        Some(utf16::offset_to_utf16(self.state.text(), offset))
    }
}

impl TextInput {
    fn should_ignore_platform_newline(&self, text: &str) -> bool {
        self.enter_key == TextInputEnterKey::Propagate
            && !text.is_empty()
            && text.chars().all(|ch| ch == '\r' || ch == '\n')
    }
}
