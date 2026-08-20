use gpui::Context;

use crate::{
    MutationFinishInput, MutationKey, MutationKind, MutationPage, ObjectResidency, OperationId,
    RangeHistoryIntent, RangeHistorySession, RangeResidency, RangeTextInput, RangeTextInputError,
    RangeTextInputRequest, SourcePosition,
};

#[derive(Clone, Copy, Debug)]
pub(super) struct PendingHistory {
    intent: RangeHistoryIntent,
    begun: bool,
}

impl PendingHistory {
    pub const fn intent(self) -> RangeHistoryIntent {
        self.intent
    }

    pub const fn is_begun(self) -> bool {
        self.begun
    }
}

impl RangeTextInput {
    pub(super) fn request_history(&mut self, kind: MutationKind, cx: &mut Context<Self>) {
        if !self.enabled
            || self.read_only
            || self.interactive_surface().is_none()
            || self.pending_history.is_some()
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
            OperationId::new(self.next_id()),
        );
        let intent = RangeHistoryIntent::new(key, kind);
        self.pending_history = Some(PendingHistory {
            intent,
            begun: false,
        });
        self.push_request(RangeTextInputRequest::HistoryIntent(intent), cx);
    }

    pub fn submit_history_session(
        &mut self,
        session: RangeHistorySession,
        base_positions: &[SourcePosition],
        text: &RangeResidency,
        objects: &ObjectResidency,
        cx: &mut Context<Self>,
    ) -> Result<MutationKey, RangeTextInputError> {
        let pending = self.pending_history.ok_or(RangeTextInputError::Stale)?;
        let intent = session.intent();
        let begin = session.begin();
        let proposal = begin.proposal();
        if pending.begun
            || pending.intent != intent
            || proposal.key() != intent.key()
            || proposal.kind() != intent.kind()
            || !matches!(intent.kind(), MutationKind::Undo | MutationKind::Redo)
        {
            return Err(RangeTextInputError::Stale);
        }
        let key = self.begin_host_mutation(begin, base_positions, text, objects, cx)?;
        self.pending_history = Some(PendingHistory {
            intent,
            begun: true,
        });
        Ok(key)
    }

    pub fn submit_history_page(
        &mut self,
        page: MutationPage,
        cx: &mut Context<Self>,
    ) -> Result<crate::MutationPageAcceptance, RangeTextInputError> {
        let pending = self.pending_history.ok_or(RangeTextInputError::Stale)?;
        if !pending.begun || page.key().key() != pending.intent.key() {
            return Err(RangeTextInputError::Stale);
        }
        self.submit_mutation_page(page, cx)
    }

    pub fn finish_history_input(
        &mut self,
        finish: MutationFinishInput,
        cx: &mut Context<Self>,
    ) -> Result<(), RangeTextInputError> {
        let pending = self.pending_history.ok_or(RangeTextInputError::Stale)?;
        if !pending.begun || finish.key() != pending.intent.key() {
            return Err(RangeTextInputError::Stale);
        }
        self.submit_mutation_finish(finish, cx)
    }
}
