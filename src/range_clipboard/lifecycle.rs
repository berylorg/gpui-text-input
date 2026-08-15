use super::*;

impl RangeClipboardCoordinator {
    pub(crate) fn preview_rebind(&self) -> Option<ClipboardCancellation> {
        self.active.as_ref().map(|active| ClipboardCancellation {
            key: active.key,
            pending_text_page: active.pending_text,
            pending_object_page: active.pending_object,
            awaiting_write: active.state == ClipboardState::AwaitingWrite,
        })
    }

    pub(crate) fn commit_prepared_rebind(
        &mut self,
        binding: RangeBinding,
        expected: Option<ClipboardCancellation>,
    ) {
        debug_assert_eq!(self.preview_rebind(), expected);
        if let Some(cancellation) = expected {
            self.finish(cancellation.key);
        }
        self.binding = binding;
        self.highest_request = None;
        self.highest_object_request = None;
    }

    /// Cancels the exact active collection or pending write acknowledgement.
    pub fn cancel(&mut self, key: ClipboardKey) -> Result<ClipboardCompletion, ClipboardError> {
        self.active_for_key(key)?;
        self.finish(key);
        Ok(ClipboardCompletion::Cancelled)
    }

    /// Cancels active collection/write coordination and adopts a new exact binding.
    pub fn rebind(&mut self, binding: RangeBinding) -> Option<ClipboardCancellation> {
        let cancellation = self.active.as_ref().map(|active| ClipboardCancellation {
            key: active.key,
            pending_text_page: active.pending_text,
            pending_object_page: active.pending_object,
            awaiting_write: active.state == ClipboardState::AwaitingWrite,
        });
        if let Some(cancellation) = cancellation {
            self.finish(cancellation.key);
        }
        self.binding = binding;
        self.highest_request = None;
        self.highest_object_request = None;
        cancellation
    }

    /// Cancels active work and adopts a new object-presentation generation.
    pub fn set_presentation_generation(
        &mut self,
        presentation_generation: PresentationGeneration,
    ) -> Option<ClipboardCancellation> {
        let cancellation = self.active.as_ref().map(|active| ClipboardCancellation {
            key: active.key,
            pending_text_page: active.pending_text,
            pending_object_page: active.pending_object,
            awaiting_write: active.state == ClipboardState::AwaitingWrite,
        });
        if let Some(cancellation) = cancellation {
            self.finish(cancellation.key);
        }
        self.presentation_generation = presentation_generation;
        self.highest_object_request = None;
        cancellation
    }

    /// Releases every task-local page and staged-byte reservation.
    pub fn dispose(&mut self) -> Option<ClipboardCancellation> {
        let cancellation = self.active.as_ref().map(|active| ClipboardCancellation {
            key: active.key,
            pending_text_page: active.pending_text,
            pending_object_page: active.pending_object,
            awaiting_write: active.state == ClipboardState::AwaitingWrite,
        });
        if let Some(cancellation) = cancellation {
            self.finish(cancellation.key);
        }
        cancellation
    }
}
