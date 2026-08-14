use super::*;

impl RangeClipboardCoordinator {
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
            pending_page: active.pending,
        });
        if let Some(cancellation) = cancellation {
            self.finish(cancellation.key);
        }
        self.binding = binding;
        cancellation
    }

    /// Releases every task-local page and staged-byte reservation.
    pub fn dispose(&mut self) -> Option<ClipboardCancellation> {
        let cancellation = self.active.as_ref().map(|active| ClipboardCancellation {
            key: active.key,
            pending_page: active.pending,
        });
        if let Some(cancellation) = cancellation {
            self.finish(cancellation.key);
        }
        cancellation
    }
}
