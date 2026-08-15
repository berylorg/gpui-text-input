use super::*;

impl RangeEditCoordinator {
    /// Rebinds the coordinator. Pre-admission work is cancelled; an admitted commit is detached.
    pub fn rebind(&mut self, binding: RangeBinding) -> Option<MutationDisposal> {
        let mut disposal = None;
        let mut released = MutationCounts::default();
        if let Some(active) = &mut self.active {
            if active.state == MutationState::CommitPending {
                disposal = Some(MutationDisposal::Detached(active.proposal.key()));
                active.state = MutationState::DetachedCommit;
                active.detached = true;
                released = active.release_staging();
            } else if active.state != MutationState::DetachedCommit {
                let key = active.proposal.key();
                disposal = Some(MutationDisposal::Cancelled(key));
                released = active.counts();
                self.active = None;
                self.last_terminal = Some(key);
            } else {
                disposal = Some(MutationDisposal::Detached(active.proposal.key()));
            }
        }
        self.record_release(released);
        self.binding = binding;
        disposal
    }

    /// Releases all staged capacity while retaining only an admitted key for late settlement.
    pub fn dispose(&mut self) -> Option<MutationDisposal> {
        let key = self.active_key()?;
        let mut released = MutationCounts::default();
        if let Some(active) = &mut self.active {
            if matches!(
                active.state,
                MutationState::CommitPending | MutationState::DetachedCommit
            ) {
                active.state = MutationState::DetachedCommit;
                active.detached = true;
                released = active.release_staging();
                self.record_release(released);
                return Some(MutationDisposal::Detached(key));
            }
            released = active.counts();
            self.active = None;
            self.last_terminal = Some(key);
        }
        self.record_release(released);
        Some(MutationDisposal::Cancelled(key))
    }
}
