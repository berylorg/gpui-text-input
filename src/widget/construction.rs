use super::*;

impl TextInput {
    /// Creates a single-line text input.
    pub fn new(
        initial_value: impl Into<String>,
        placeholder: impl Into<SharedString>,
        cx: &mut Context<Self>,
    ) -> Self {
        Self::new_with_options(
            initial_value,
            placeholder,
            TextInputOptions::single_line(),
            cx,
        )
    }

    /// Creates a multiline text input.
    pub fn multiline(
        initial_value: impl Into<String>,
        placeholder: impl Into<SharedString>,
        cx: &mut Context<Self>,
    ) -> Self {
        Self::new_with_options(
            initial_value,
            placeholder,
            TextInputOptions::multiline(),
            cx,
        )
    }

    /// Creates a text input with explicit model options.
    pub fn new_with_options(
        initial_value: impl Into<String>,
        placeholder: impl Into<SharedString>,
        options: TextInputOptions,
        cx: &mut Context<Self>,
    ) -> Self {
        let state = TextInputState::new(initial_value, options);
        let cursor = state.cursor_offset();
        let mode = state.mode();
        let vertical_scrollbar = (mode == TextInputMode::Multiline).then(|| {
            let owner = ScrollbarOwnerKey::new(
                ScrollbarOwnerId::new(cx.entity_id().as_u64()),
                ScrollbarMountGeneration::new(1),
            );
            let model = Rc::new(Cell::new(None));
            let pending = Rc::new(Cell::new(None));
            let weak = cx.weak_entity();
            let interaction = ScrollbarInteraction::new(
                {
                    let model = model.clone();
                    move || model.get()
                },
                {
                    let pending = pending.clone();
                    move |_, offset| pending.set(Some(PendingScrollbarRequest::Set(offset)))
                },
                {
                    let pending = pending.clone();
                    move |_, direction, distance| {
                        pending.set(Some(PendingScrollbarRequest::Page(direction, distance)));
                    }
                },
                |_| {},
                |_| {},
                {
                    let weak = weak.clone();
                    move |_, _, cx| {
                        let Some(request) = pending.take() else {
                            return;
                        };
                        let _ = weak.update(cx, |input, cx| {
                            input.apply_scrollbar_request(request, cx);
                        });
                    }
                },
            );
            let on_visibility_update = Rc::new(move |_, _: &mut Window, cx: &mut App| {
                let _ = weak.update(cx, |_, cx| cx.notify());
            });
            VerticalScrollbar {
                owner,
                state: ScrollbarState::new(owner),
                model,
                interaction,
                on_visibility_update,
            }
        });
        Self {
            focus_handle: cx.focus_handle(),
            state,
            placeholder: placeholder.into(),
            theme: TextInputTheme::default(),
            enabled: true,
            enter_key: TextInputEnterKey::InsertNewline,
            single_line_vertical_key: TextInputSingleLineVerticalKey::Handle,
            atom_clipboard_policy: TextInputAtomClipboardPolicy::PlainText,
            rich_paste_policy: TextInputRichPastePolicy::PlainText,
            last_layout: Vec::new(),
            last_bounds: None,
            last_geometry: None,
            scroll_x: px(0.0),
            scroll_y: px(0.0),
            vertical_scrollbar,
            content_height: px(0.0),
            visible_range: cursor..cursor,
            reveal_cursor: true,
            is_selecting: false,
        }
    }
}
