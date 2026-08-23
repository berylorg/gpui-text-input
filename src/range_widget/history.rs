use gpui::Context;

use crate::{
    MutationKey, MutationKind, RangeHistoryFrontier, RangeHistoryIntent, RangeHistorySession,
    RangeTextInput, RangeTextInputError, RangeTextInputRequest,
};

#[derive(Clone, Copy, Debug)]
pub(super) struct PendingHistory {
    intent: RangeHistoryIntent,
    admitted: bool,
}

impl PendingHistory {
    pub const fn intent(self) -> RangeHistoryIntent {
        self.intent
    }

    pub const fn is_admitted(self) -> bool {
        self.admitted
    }
}

impl RangeTextInput {
    pub fn history_frontier(&self) -> RangeHistoryFrontier {
        if let Some(seed) = self.published_restoration {
            return seed
                .history
                .unwrap_or_else(|| RangeHistoryFrontier::unavailable(seed.binding));
        }
        self.history_frontier
    }

    pub fn set_history_frontier(
        &mut self,
        expected: RangeHistoryFrontier,
        replacement: RangeHistoryFrontier,
    ) -> Result<(), RangeTextInputError> {
        if !self.mounted {
            return Err(RangeTextInputError::NotMounted);
        }
        if self.restoration.is_some() || self.restoration_seed.is_some() {
            return Err(RangeTextInputError::Busy);
        }
        if self.pending_history.is_some() {
            return Err(RangeTextInputError::Busy);
        }
        if expected != self.history_frontier()
            || expected.binding() != self.config.binding
            || replacement.binding() != self.config.binding
        {
            return Err(RangeTextInputError::Stale);
        }
        self.history_frontier = replacement;
        if let Some(seed) = self.published_restoration.as_mut() {
            seed.history = Some(replacement);
        }
        Ok(())
    }

    pub(super) fn request_history(&mut self, kind: MutationKind, cx: &mut Context<Self>) {
        let Some(surface) = self.interactive_surface() else {
            return;
        };
        let caret = surface.caret();
        let selection = surface.selection();
        let history_frontier = self.history_frontier();
        if !self.enabled
            || self.read_only
            || self.pending_history.is_some()
            || self.replacement.is_some()
            || !matches!(self.clipboard.state(), crate::ClipboardState::Idle)
            || history_frontier.binding() != self.config.binding
            || !history_frontier.allows(kind)
            || !matches!(
                self.edits.state(),
                crate::MutationState::Idle | crate::MutationState::Settled
            )
        {
            return;
        }
        let key = MutationKey::new(
            self.config.binding.binding(),
            self.config.binding.revision(),
            match self.config.settlement_coordinator.allocate_operation() {
                Ok(operation) => operation,
                Err(_) => return,
            },
        );
        let intent = RangeHistoryIntent::new(
            key,
            self.config.binding,
            kind,
            history_frontier,
            caret,
            selection,
        );
        self.pending_history = Some(PendingHistory {
            intent,
            admitted: false,
        });
        self.push_request(RangeTextInputRequest::HistoryIntent(intent), cx);
    }

    pub fn submit_history_session(
        &mut self,
        session: RangeHistorySession,
    ) -> Result<MutationKey, RangeTextInputError> {
        let pending = self.pending_history.ok_or(RangeTextInputError::Stale)?;
        let intent = session.intent();
        if pending.admitted
            || pending.intent != intent
            || intent.binding() != self.config.binding
            || intent.frontier().binding() != intent.binding()
            || intent.frontier() != self.history_frontier()
            || !matches!(intent.kind(), MutationKind::Undo | MutationKind::Redo)
        {
            return Err(RangeTextInputError::Stale);
        }
        self.config.settlement_coordinator.reserve_history(intent)?;
        self.pending_history = Some(PendingHistory {
            intent,
            admitted: true,
        });
        Ok(intent.key())
    }
}
