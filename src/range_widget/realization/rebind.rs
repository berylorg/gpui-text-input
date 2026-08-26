use super::*;

#[derive(Clone)]
pub(in crate::range_widget) enum PendingRebindIntent {
    Direct {
        binding: crate::RangeBinding,
        selection: Option<RangeSourceSelection>,
    },
    History {
        intent: RangeHistoryIntent,
        commit: RangeHistoryCommit,
    },
    Mutation {
        key: crate::MutationKey,
        outcome: crate::MutationOutcome,
        binding: crate::RangeBinding,
        selection: RangeSourceSelection,
        positions: crate::MutationPositions,
        proofs: Vec<crate::range_edit::SourcePositionProof>,
        composition: Option<crate::ByteRange>,
        active_loss_reason: crate::InlineObjectRealizationLossReason,
    },
}

impl PendingRebindIntent {
    pub(in crate::range_widget) fn charge(&self) -> RangeSurfaceCharge {
        match self {
            Self::Mutation { proofs, .. } => RangeSurfaceCharge {
                bytes: proofs.capacity()
                    * std::mem::size_of::<crate::range_edit::SourcePositionProof>(),
                items: proofs.capacity(),
            },
            Self::Direct { .. } | Self::History { .. } => RangeSurfaceCharge::default(),
        }
    }
}

impl RangeTextInput {
    pub(in crate::range_widget) fn retain_pending_rebind_intent(
        &mut self,
        intent: PendingRebindIntent,
    ) -> Result<(), RangeTextInputError> {
        if self.pending_rebind_intent.as_ref().is_some_and(|pending| {
            !matches!(pending, PendingRebindIntent::Direct { .. })
                && matches!(intent, PendingRebindIntent::Direct { .. })
        }) {
            return Err(RangeTextInputError::Busy);
        }
        let current = self.current_realization_ownership();
        let charge = intent.charge();
        let replaced = self
            .pending_rebind_intent
            .as_ref()
            .map_or(RangeSurfaceCharge::default(), PendingRebindIntent::charge);
        let retained_bytes = current
            .owned_bytes
            .checked_sub(replaced.bytes)
            .ok_or(RangeTextInputError::SurfaceCapacity)?;
        let retained_items = current
            .owned_items
            .checked_sub(replaced.items)
            .ok_or(RangeTextInputError::SurfaceCapacity)?;
        let peak = RangeSurfaceCharge {
            bytes: current
                .owned_bytes
                .checked_add(charge.bytes)
                .zip(
                    retained_bytes
                        .checked_add(charge.bytes)
                        .and_then(|bytes| bytes.checked_add(charge.bytes)),
                )
                .map(|(replacement, service)| replacement.max(service))
                .ok_or(RangeTextInputError::SurfaceCapacity)?,
            items: current
                .owned_items
                .checked_add(charge.items)
                .zip(
                    retained_items
                        .checked_add(charge.items)
                        .and_then(|items| items.checked_add(charge.items)),
                )
                .map(|(replacement, service)| replacement.max(service))
                .ok_or(RangeTextInputError::SurfaceCapacity)?,
        };
        if peak.bytes > self.config.limits.max_surface_bytes
            || peak.items > self.config.limits.max_surface_items
        {
            return Err(RangeTextInputError::SurfaceCapacity);
        }
        self.observe_surface_admission_peak(peak);
        self.pending_rebind_intent = Some(intent);
        self.observe_realization_ownership();
        Ok(())
    }

    pub(in crate::range_widget) fn service_pending_rebind_intent(
        &mut self,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) -> Result<bool, RangeTextInputError> {
        let Some(intent) = self.pending_rebind_intent.clone() else {
            return Ok(false);
        };
        if self.pending_layout_intent.is_some() || self.pending_presentation_intent.is_some() {
            self.schedule_realization_continuation(cx);
            return Ok(false);
        }
        if !self.try_spend_realization_credit(cx) {
            return Ok(false);
        }
        let result = match intent {
            PendingRebindIntent::Direct { binding, selection } => self
                .commit_pending_direct_rebind(binding, selection, window, cx)
                .map(|_| ()),
            PendingRebindIntent::History { intent, commit } => self
                .commit_pending_history_rebind(intent, commit, window, cx)
                .map(|_| ()),
            PendingRebindIntent::Mutation {
                key,
                outcome,
                binding,
                selection,
                positions,
                proofs,
                composition,
                active_loss_reason,
            } => self
                .commit_pending_mutation_rebind(
                    key,
                    outcome,
                    binding,
                    selection,
                    positions,
                    proofs,
                    composition,
                    active_loss_reason,
                    window,
                    cx,
                )
                .map(|_| ()),
        };
        match result {
            Ok(()) => {
                self.pending_rebind_intent = None;
                self.observe_realization_ownership();
                Ok(true)
            }
            Err(error) => {
                self.refund_realization_credit();
                self.schedule_realization_continuation(cx);
                Err(error)
            }
        }
    }
}
