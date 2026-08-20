use super::*;

impl LaneState {
    pub(super) const fn new(cursor: MutationCursor) -> Self {
        Self {
            next_cursor: cursor,
            next_ordinal: 0,
            cumulative_identity: MutationIdentity::ROOT,
            totals: MutationTotals {
                pages: 0,
                items: 0,
                retained_bytes: 0,
                inserted_bytes: 0,
                inserted_line_breaks: 0,
                objects: 0,
                object_bytes: 0,
                presentation_bytes: 0,
            },
            last_page: None,
        }
    }

    pub(super) const fn finish(self) -> MutationStreamFinish {
        MutationStreamFinish {
            next_cursor: self.next_cursor,
            next_ordinal: self.next_ordinal,
            cumulative_identity: self.cumulative_identity,
            totals: self.totals,
        }
    }
}

impl RangeEditCoordinator {
    pub const fn new(binding: RangeBinding, limits: MutationLimits) -> Self {
        Self {
            binding,
            limits,
            active: None,
            last_terminal: None,
            operation_high_water: None,
            high_water_begin_identity: None,
            ever_started: false,
            released: MutationCounts {
                current_pages: 0,
                retained_bytes: 0,
                objects: 0,
                object_bytes: 0,
                presentation_bytes: 0,
                proofs: 0,
                source_pages: 0,
                transactions: 0,
            },
        }
    }

    pub const fn binding(&self) -> RangeBinding {
        self.binding
    }

    pub fn state(&self) -> MutationState {
        self.active.as_ref().map_or_else(
            || {
                if self.ever_started {
                    MutationState::Settled
                } else {
                    MutationState::Idle
                }
            },
            |active| active.state,
        )
    }

    pub fn active_key(&self) -> Option<MutationKey> {
        self.active.as_ref().map(|active| active.proposal.key())
    }

    pub(crate) fn is_retired(&self, key: MutationKey) -> bool {
        self.active.is_none() && self.last_terminal == Some(key)
    }

    pub fn counts(&self) -> MutationCounts {
        self.active
            .as_ref()
            .map_or(MutationCounts::default(), ActiveMutation::counts)
    }

    pub const fn released_counts(&self) -> MutationCounts {
        self.released
    }

    pub fn stream_finish(
        &self,
        key: MutationKey,
        lane: MutationLane,
    ) -> Result<MutationStreamFinish, MutationError> {
        let active = self.active_for_key(key)?;
        Ok(match lane {
            MutationLane::Source => active.source.finish(),
            MutationLane::Proposal => active.proposal_lane.finish(),
        })
    }

    pub fn active_object_effect(&self) -> Option<ActiveObjectEffect> {
        self.active
            .as_ref()
            .and_then(|active| active.active_object_effect)
    }

    pub(super) fn check_key(&self, key: MutationKey) -> Result<(), MutationError> {
        let expected = MutationKey::new(
            self.binding.binding(),
            self.binding.revision(),
            key.operation(),
        );
        if key.binding() != expected.binding() || key.base_revision() != expected.base_revision() {
            return Err(MutationError::WrongKey {
                expected,
                actual: key,
            });
        }
        Ok(())
    }

    pub(super) fn active_for_key(
        &self,
        key: MutationKey,
    ) -> Result<&ActiveMutation, MutationError> {
        let Some(active) = &self.active else {
            return if self.last_terminal == Some(key) {
                Err(MutationError::ObsoleteOperation(key))
            } else {
                Err(MutationError::NoActive)
            };
        };
        if active.proposal.key() != key {
            return Err(MutationError::WrongKey {
                expected: active.proposal.key(),
                actual: key,
            });
        }
        Ok(active)
    }

    pub(super) fn active_mut(
        &mut self,
        key: MutationKey,
        expected: MutationState,
    ) -> Result<&mut ActiveMutation, MutationError> {
        let active = self.active_for_key(key)?;
        if active.state != expected {
            return Err(MutationError::WrongState {
                expected,
                actual: active.state,
            });
        }
        Ok(self.active.as_mut().expect("active checked"))
    }

    pub(super) fn finish(
        &mut self,
        key: MutationKey,
        outcome: MutationOutcome,
        obsolete: bool,
    ) -> MutationSettlement {
        let active = self.active.take().expect("active transaction checked");
        self.record_release(active.counts());
        self.last_terminal = Some(key);
        self.ever_started = true;
        if !obsolete {
            if let MutationOutcome::Committed(successor) = outcome {
                self.binding = successor.binding();
                self.operation_high_water = None;
                self.high_water_begin_identity = None;
            }
            MutationSettlement::Current(outcome)
        } else {
            MutationSettlement::Obsolete(outcome)
        }
    }

    pub(super) fn record_release(&mut self, released: MutationCounts) {
        self.released = self
            .released
            .checked_add(released)
            .expect("bounded mutation release accounting cannot overflow");
    }
}

impl ActiveMutation {
    pub(super) fn counts(&self) -> MutationCounts {
        MutationCounts {
            current_pages: 0,
            retained_bytes: 0,
            objects: usize::from(self.active_object_effect.is_some()),
            object_bytes: 0,
            presentation_bytes: 0,
            proofs: 0,
            source_pages: 0,
            transactions: 1,
        }
    }

    pub(super) fn lane(&self, lane: MutationLane) -> &LaneState {
        match lane {
            MutationLane::Source => &self.source,
            MutationLane::Proposal => &self.proposal_lane,
        }
    }

    pub(super) fn lane_mut(&mut self, lane: MutationLane) -> &mut LaneState {
        match lane {
            MutationLane::Source => &mut self.source,
            MutationLane::Proposal => &mut self.proposal_lane,
        }
    }
}
