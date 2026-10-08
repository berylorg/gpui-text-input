use super::*;

impl TextInput {
    pub(in crate::widget) fn move_left(
        &mut self,
        _: &MoveLeft,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.enabled {
            return;
        }

        let changed = self.state.move_left();
        self.finish_selection_command(TextInputCommand::MoveLeft, changed, cx);
    }

    pub(in crate::widget) fn move_right(
        &mut self,
        _: &MoveRight,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.enabled {
            return;
        }

        let changed = self.state.move_right();
        self.finish_selection_command(TextInputCommand::MoveRight, changed, cx);
    }

    pub(in crate::widget) fn move_up(
        &mut self,
        _: &MoveUp,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.enabled {
            return;
        }

        if self.state.mode() == TextInputMode::SingleLine
            && self.single_line_vertical_key == TextInputSingleLineVerticalKey::Propagate
        {
            cx.propagate();
            return;
        }

        let changed = self.move_vertically(VerticalDirection::Up, false);
        self.finish_selection_command(TextInputCommand::MoveUp, changed, cx);
    }

    pub(in crate::widget) fn move_down(
        &mut self,
        _: &MoveDown,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.enabled {
            return;
        }

        if self.state.mode() == TextInputMode::SingleLine
            && self.single_line_vertical_key == TextInputSingleLineVerticalKey::Propagate
        {
            cx.propagate();
            return;
        }

        let changed = self.move_vertically(VerticalDirection::Down, false);
        self.finish_selection_command(TextInputCommand::MoveDown, changed, cx);
    }

    pub(in crate::widget) fn move_word_left(
        &mut self,
        _: &MoveWordLeft,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.enabled {
            return;
        }

        let changed = self.state.move_word_left();
        self.finish_selection_command(TextInputCommand::MoveWordLeft, changed, cx);
    }

    pub(in crate::widget) fn move_word_right(
        &mut self,
        _: &MoveWordRight,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.enabled {
            return;
        }

        let changed = self.state.move_word_right();
        self.finish_selection_command(TextInputCommand::MoveWordRight, changed, cx);
    }

    pub(in crate::widget) fn select_left(
        &mut self,
        _: &SelectLeft,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.enabled {
            return;
        }

        let changed = self.state.select_left();
        self.finish_selection_command(TextInputCommand::SelectLeft, changed, cx);
    }

    pub(in crate::widget) fn select_right(
        &mut self,
        _: &SelectRight,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.enabled {
            return;
        }

        let changed = self.state.select_right();
        self.finish_selection_command(TextInputCommand::SelectRight, changed, cx);
    }

    pub(in crate::widget) fn select_up(
        &mut self,
        _: &SelectUp,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.enabled {
            return;
        }

        let changed = self.move_vertically(VerticalDirection::Up, true);
        self.finish_selection_command(TextInputCommand::SelectUp, changed, cx);
    }

    pub(in crate::widget) fn select_down(
        &mut self,
        _: &SelectDown,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.enabled {
            return;
        }

        let changed = self.move_vertically(VerticalDirection::Down, true);
        self.finish_selection_command(TextInputCommand::SelectDown, changed, cx);
    }

    pub(in crate::widget) fn select_word_left(
        &mut self,
        _: &SelectWordLeft,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.enabled {
            return;
        }

        let changed = self.state.select_word_left();
        self.finish_selection_command(TextInputCommand::SelectWordLeft, changed, cx);
    }

    pub(in crate::widget) fn select_word_right(
        &mut self,
        _: &SelectWordRight,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.enabled {
            return;
        }

        let changed = self.state.select_word_right();
        self.finish_selection_command(TextInputCommand::SelectWordRight, changed, cx);
    }

    pub(in crate::widget) fn move_home(
        &mut self,
        _: &MoveHome,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.enabled {
            return;
        }

        let changed = self.state.move_home();
        self.finish_selection_command(TextInputCommand::MoveHome, changed, cx);
    }

    pub(in crate::widget) fn move_end(
        &mut self,
        _: &MoveEnd,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.enabled {
            return;
        }

        let changed = self.state.move_end();
        self.finish_selection_command(TextInputCommand::MoveEnd, changed, cx);
    }

    pub(in crate::widget) fn select_home(
        &mut self,
        _: &SelectHome,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.enabled {
            return;
        }

        let changed = self.state.select_home();
        self.finish_selection_command(TextInputCommand::SelectHome, changed, cx);
    }

    pub(in crate::widget) fn select_end(
        &mut self,
        _: &SelectEnd,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.enabled {
            return;
        }

        let changed = self.state.select_end();
        self.finish_selection_command(TextInputCommand::SelectEnd, changed, cx);
    }

    pub(in crate::widget) fn move_to_start(
        &mut self,
        _: &MoveToStart,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.enabled {
            return;
        }

        let changed = self.state.move_to_start();
        self.finish_selection_command(TextInputCommand::MoveToStart, changed, cx);
    }

    pub(in crate::widget) fn move_to_end(
        &mut self,
        _: &MoveToEnd,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.enabled {
            return;
        }

        let changed = self.state.move_to_end();
        self.finish_selection_command(TextInputCommand::MoveToEnd, changed, cx);
    }

    pub(in crate::widget) fn select_to_start(
        &mut self,
        _: &SelectToStart,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.enabled {
            return;
        }

        let changed = self.state.select_to_start();
        self.finish_selection_command(TextInputCommand::SelectToStart, changed, cx);
    }

    pub(in crate::widget) fn select_to_end(
        &mut self,
        _: &SelectToEnd,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.enabled {
            return;
        }

        let changed = self.state.select_to_end();
        self.finish_selection_command(TextInputCommand::SelectToEnd, changed, cx);
    }

    pub(in crate::widget) fn select_all(
        &mut self,
        _: &SelectAll,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.enabled {
            return;
        }

        let changed = self.state.select_all();
        self.finish_selection_command(TextInputCommand::SelectAll, changed, cx);
    }
}
